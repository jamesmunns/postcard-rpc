use dioxus_core::prelude::spawn;
use gloo::utils::format::JsValueSerdeExt;
use serde_json::json;
use tracing::info;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{UsbDevice, UsbInTransferResult, UsbTransferStatus};

use super::Client;

#[derive(Clone)]
pub struct WebUsbClient {
    device: UsbDevice,
    transfer_max_length: u32,
    ep_in: u8,
    ep_out: u8,
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    #[error("Browser error: {0}")]
    Browser(String),
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

/// # Example usage ()
/// ```no_run
/// let client = HostClient::<WireError>::new_with_client(
///     WebUsbClient::new(0x16c0, 0, 1000, 1, 1)
///         .await
///         .expect("WebUSB error"),
///     ERROR_PATH,
///     MAGIC_EIGHTBALL,
/// )
/// .expect("could not create HostClient");
/// ```
impl WebUsbClient {
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
        JsFuture::from(device.open()).await?;
        tracing::info!("deveie openc, claiming interface {interface}");
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

impl Client for WebUsbClient {
    type Error = Error;

    async fn receive(&self) -> Result<Vec<u8>, Self::Error> {
        tracing::info!("receive…");
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

    async fn send(&self, mut data: Vec<u8>) -> Result<(), Self::Error> {
        tracing::info!("send…");
        // TODO for reasons unknown, web-sys wants mutable access to the send buffer.
        // tracking issue: https://github.com/rustwasm/wasm-bindgen/issues/3963
        JsFuture::from(
            self.device
                .transfer_out_with_u8_array(self.ep_out, &mut data),
        )
        .await?;
        Ok(())
    }

    fn spawn(&self, fut: impl core::future::Future<Output = ()> + 'static) {
        spawn(fut);
    }
}
