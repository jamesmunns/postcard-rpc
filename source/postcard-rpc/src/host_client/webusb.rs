//! Implementation of transport using webusb

use gloo::utils::format::JsValueSerdeExt;
use postcard_schema::Schema;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::info;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    UsbDevice, UsbDirection, UsbEndpoint, UsbInTransferResult, UsbInterface, UsbTransferStatus,
};

use crate::header::VarSeqKind;
use crate::host_client::{HostClient, WireRx, WireSpawn, WireTx};

/// Implementation of the wire interface for WebUsb
#[derive(Clone)]
pub struct WebUsbWire {
    device: UsbDevice,
    transfer_max_length: u32,
    ep_in: u8,
    ep_out: u8,
}

/// WebUsb Error type
#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    /// Error originating from the browser
    #[error("Browser error: {0}")]
    Browser(String),
    /// Error originating from the USB stack
    #[error("USB transfer error: {0}")]
    UsbTransfer(&'static str),
}

impl From<JsValue> for Error {
    fn from(e: JsValue) -> Self {
        let error_s = format!("{e:?}");
        Self::Browser(error_s)
    }
}

impl From<UsbTransferStatus> for Error {
    fn from(status: UsbTransferStatus) -> Self {
        let cause = match status {
            UsbTransferStatus::Ok => unreachable!(),
            UsbTransferStatus::Stall => "stall",
            UsbTransferStatus::Babble => "babble",
            _ => "unknown",
        };

        Self::UsbTransfer(cause)
    }
}

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Create a new webusb connection instance
    pub async fn try_new_webusb(
        vendor_id: u16,
        interface: u8,
        transfer_max_length: u32,
        ep_in: u8,
        ep_out: u8,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_len: VarSeqKind,
    ) -> Result<Self, Error> {
        let wire =
            WebUsbWire::new(vendor_id, interface, transfer_max_length, ep_in, ep_out).await?;
        Ok(HostClient::new_with_wire(
            wire.clone(),
            wire.clone(),
            wire,
            seq_no_len,
            err_uri_path,
            outgoing_depth,
        ))
    }

    /// Create a new webusb connection instance from an existing UsbDevice
    ///
    /// This will use the first device interface with class 0xFF
    pub async fn try_new_webusb_from_device(
        device: UsbDevice,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_len: VarSeqKind,
    ) -> Result<Self, Error> {
        let wire = WebUsbWire::from_device(device).await?;
        Ok(HostClient::new_with_wire(
            wire.clone(),
            wire.clone(),
            wire,
            seq_no_len,
            err_uri_path,
            outgoing_depth,
        ))
    }

    /// Create a new webusb connection instance from an existing UsbDevice and UsbInterface
    ///
    /// This will use the first device interface with class 0xFF
    pub async fn try_new_webusb_from_device_and_interface(
        device: UsbDevice,
        interface: &UsbInterface,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_len: VarSeqKind,
    ) -> Result<Self, Error> {
        let wire = WebUsbWire::from_device_and_interface(device, interface).await?;
        Ok(HostClient::new_with_wire(
            wire.clone(),
            wire.clone(),
            wire,
            seq_no_len,
            err_uri_path,
            outgoing_depth,
        ))
    }
}

/// # Example usage ()
///
/// ```no_run
/// let wire = WebUsbWire::new(0x16c0, 0, 1000, 1, 1)
///         .await
///         .expect("WebUSB error");
///
/// let client = HostClient::<WireError>::new_with_wire(
///     wire.clone(),
///     wire.clone(),
///     wire,
///     crate::standard_icd::ERROR_PATH,
///     8,
///     VarSeqKind::Seq1,
/// )
/// .expect("could not create HostClient");
/// ```
impl WebUsbWire {
    /// Create a new instance of [`WebUsbWire`]
    pub async fn new(
        vendor_id: u16,
        interface: u8,
        transfer_max_length: u32,
        ep_in: u8,
        ep_out: u8,
    ) -> Result<Self, Error> {
        let window = gloo::utils::window();
        let navigator = window.navigator();
        let usb = navigator.usb();

        // TODO probably better filter API: Option<Value> parameter in constructor instead of vendor_id
        // however: that doesn't allow the manual ensure-filtering below 🤔
        let filter = json!({
            "filters": [{"vendorId": vendor_id}]
        });
        let filter = JsValue::from_serde(&filter).unwrap();
        // try to get device from already paired list
        let devices: js_sys::Array = JsFuture::from(usb.get_devices()).await?.into();
        let device = if devices.length() > 0 {
            info!("found {} existing devices", devices.length());
            // need to ensure we get the right one because others might've been paired in the past
            devices
                .into_iter()
                .map(|dev| dev.dyn_into::<UsbDevice>().expect("WebUSB api broke"))
                .filter(|device| device.vendor_id() == vendor_id)
                .nth(0)
        } else {
            None
        };

        let device = match device {
            Some(device) => device,
            None => JsFuture::from(usb.request_device(&filter.into()))
                .await?
                .dyn_into()
                .map_err(|e| Error::from(e))?,
        };

        Self::open_webusb_interface(device, interface, transfer_max_length, ep_in, ep_out).await
    }

    /// Create a webusb connection instance from an existing UsbDevice
    ///
    /// This will use the first device interface with class 0xFF
    pub async fn from_device(device: UsbDevice) -> Result<Self, Error> {
        let configuration = device
            .configuration()
            // TODO: API Convert to NotFound Error
            .ok_or_else(|| Error::Browser("Notfound: No device configuration".to_string()))?;
        let interface = configuration
            .interfaces()
            .iter()
            .find_map(|i| {
                let i = i.dyn_into::<UsbInterface>().ok()?;
                if i.alternate().interface_class() == 0xFF {
                    Some(i)
                } else {
                    None
                }
            })
            // TODO: API Convert to NotFound Error
            .ok_or_else(|| {
                Error::Browser("Notfound: Failed to find usable interface".to_string())
            })?;

        Self::from_device_and_interface(device, &interface).await
    }

    /// Create a webusb connection instance from an existing UsbDevice and UsbInterface
    pub async fn from_device_and_interface(
        device: UsbDevice,
        interface: &UsbInterface,
    ) -> Result<Self, Error> {
        let alternate = interface.alternate();
        let endpoints = alternate.endpoints();
        let ep_in = endpoints
            .iter()
            .find_map(|e| {
                let e = e.dyn_into::<UsbEndpoint>().ok()?;
                if e.direction() == UsbDirection::In {
                    Some(e)
                } else {
                    None
                }
            })
            // TODO: API Convert to NotFound Error
            .ok_or_else(|| Error::Browser("Notfound: Failed to find IN endpoint".to_string()))?;
        let ep_out = endpoints
            .iter()
            .find_map(|e| {
                let e = e.dyn_into::<UsbEndpoint>().ok()?;
                if e.direction() == UsbDirection::Out {
                    Some(e)
                } else {
                    None
                }
            })
            // TODO: API Convert to NotFound Error
            .ok_or_else(|| Error::Browser("Notfound: Failed to find OUT endpoint".to_string()))?;

        Self::open_webusb_interface(
            device,
            interface.interface_number(),
            ep_out.packet_size(),
            ep_in.endpoint_number(),
            ep_out.endpoint_number(),
        )
        .await
    }

    async fn open_webusb_interface(
        device: UsbDevice,
        interface: u8,
        transfer_max_length: u32,
        ep_in: u8,
        ep_out: u8,
    ) -> Result<Self, Error> {
        JsFuture::from(device.open()).await?;
        tracing::info!("device open, claiming interface {interface}");
        JsFuture::from(device.claim_interface(interface)).await?;
        info!("done");

        Ok(Self {
            device,
            transfer_max_length,
            ep_in,
            ep_out,
        })
    }
}

impl WireTx for WebUsbWire {
    type Error = Error;

    async fn send(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        tracing::trace!("send…");
        let data: js_sys::Uint8Array = data.as_slice().into();
        // TODO for reasons unknown, web-sys wants mutable access to the send buffer.
        // tracking issue: https://github.com/rustwasm/wasm-bindgen/issues/3963
        JsFuture::from(self.device.transfer_out_with_u8_array(self.ep_out, &data)?).await?;
        Ok(())
    }
}

impl WireRx for WebUsbWire {
    type Error = Error;

    async fn receive(&mut self) -> Result<Vec<u8>, Self::Error> {
        tracing::trace!("receive…");
        let res: UsbInTransferResult = JsFuture::from(
            self.device
                .transfer_in(self.ep_in, self.transfer_max_length),
        )
        .await?
        .into();

        let status = res.status();
        if status == UsbTransferStatus::Ok {
            match res.data() {
                Some(view) => {
                    let arr = js_sys::Uint8Array::new(&view.buffer());
                    let data = arr.to_vec();
                    Ok(data)
                }
                None => Ok(vec![]),
            }
        } else {
            Err(status.into())
        }
    }
}

impl WireSpawn for WebUsbWire {
    fn spawn(&mut self, fut: impl core::future::Future<Output = ()> + 'static) {
        spawn_local(fut);
    }
}
