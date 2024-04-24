#![allow(async_fn_in_trait)]

use crate::{headered::extract_header_from_bytes, Key, WireHeader};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb::{
    driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut},
    msos::{self, windows_version},
    Builder, UsbDevice,
};
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};
use static_cell::StaticCell;

mod dispatch_macro;

const DEVICE_INTERFACE_GUIDS: &[&str] = &["{AFB9A6FB-30BA-44BC-9232-806CFC875321}"];

pub struct UsbBuffers {
    pub config_descriptor: [u8; 256],
    pub bos_descriptor: [u8; 256],
    pub control_buf: [u8; 64],
    pub msos_descriptor: [u8; 256],
}

impl UsbBuffers {
    pub const fn new() -> Self {
        Self {
            config_descriptor: [0u8; 256],
            bos_descriptor: [0u8; 256],
            msos_descriptor: [0u8; 256],
            control_buf: [0u8; 64],
        }
    }
}

pub fn example_config() -> embassy_usb::Config<'static> {
    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-serial example");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
}

pub fn configure_usb<D: embassy_usb::driver::Driver<'static>>(
    driver: D,
    bufs: &'static mut UsbBuffers,
    config: embassy_usb::Config<'static>,
) -> (UsbDevice<'static, D>, D::EndpointIn, D::EndpointOut) {
    let mut builder = Builder::new(
        driver,
        config,
        &mut bufs.config_descriptor,
        &mut bufs.bos_descriptor,
        &mut bufs.msos_descriptor,
        &mut bufs.control_buf,
    );

    // Add the Microsoft OS Descriptor (MSOS/MOD) descriptor.
    // We tell Windows that this entire device is compatible with the "WINUSB" feature,
    // which causes it to use the built-in WinUSB driver automatically, which in turn
    // can be used by libusb/rusb software without needing a custom driver or INF file.
    // In principle you might want to call msos_feature() just on a specific function,
    // if your device also has other functions that still use standard class drivers.
    builder.msos_descriptor(windows_version::WIN8_1, 0);
    builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    // Add a vendor-specific function (class 0xFF), and corresponding interface,
    // that uses our custom handler.
    let mut function = builder.function(0xFF, 0, 0);
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(0xFF, 0, 0, None);
    let ep_out = alt.endpoint_bulk_out(64);
    let ep_in = alt.endpoint_bulk_in(64);
    drop(function);

    // Build the builder.
    let usb = builder.build();

    (usb, ep_in, ep_out)
}

// comms

pub trait Dispatch {
    type Mutex: RawMutex;
    type Driver: Driver<'static>;

    async fn dispatch(
        &self,
        hdr: WireHeader,
        body: &[u8],
        sender: Sender<Self::Mutex, Self::Driver>,
    );
    async fn error(
        &self,
        seq_no: u32,
        error: WireError,
        sender: Sender<Self::Mutex, Self::Driver>,
    );
}

#[derive(Copy)]
pub struct Sender<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, SenderInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Sender<M, D> {
    #[inline]
    pub async fn reply<E>(&self, seq_no: u32, resp: &E::Response) -> Result<(), ()>
    where
        E: crate::Endpoint,
        E::Response: Serialize,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        reply::<D, E>(ep_in, seq_no, resp, tx_buf).await
    }

    #[inline]
    pub async fn reply_keyed<T>(&self, seq_no: u32, key: Key, resp: &T) -> Result<(), ()>
    where
        T: Serialize + Schema,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        reply_keyed::<D, T>(ep_in, key, seq_no, resp, tx_buf).await
    }
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Clone for Sender<M, D> {
    fn clone(&self) -> Self {
        Sender { inner: self.inner }
    }
}

pub struct SenderInner<D: Driver<'static>> {
    ep_in: D::EndpointIn,
    tx_buf: &'static mut [u8],
}

async fn reply<D, E>(ep_in: &mut D::EndpointIn, seq_no: u32, resp: &E::Response, out: &mut [u8]) -> Result<(), ()>
where
    D: Driver<'static>,
    E: crate::Endpoint,
    E::Response: Serialize,
{
    if let Ok(used) = crate::headered::to_slice_keyed(seq_no, E::RESP_KEY, resp, out) {
        send_all::<D>(ep_in, used).await
    } else {
        Err(())
    }
}

async fn reply_keyed<D, T>(ep_in: &mut D::EndpointIn, key: Key, seq_no: u32, resp: &T, out: &mut [u8]) -> Result<(), ()>
where
    D: Driver<'static>,
    T: Serialize + Schema,
{
    if let Ok(used) = crate::headered::to_slice_keyed(seq_no, key, resp, out) {
        send_all::<D>(ep_in, used).await
    } else {
        Err(())
    }
}

async fn send_all<D>(ep_in: &mut D::EndpointIn, out: &[u8]) -> Result<(), ()>
where
    D: Driver<'static>,
{
    if out.is_empty() {
        return Ok(());
    }
    ep_in.wait_enabled().await;
    // write in segments of 64. The last chunk may
    // be 0 < len <= 64.
    for ch in out.chunks(64) {
        if ep_in.write(ch).await.is_err() {
            return Err(());
        }
    }
    // If the total we sent was a multiple of 64, send an
    // empty message to "flush" the transaction
    if (out.len() & (64 - 1)) == 0 && ep_in.write(&[]).await.is_err() {
        return Err(());
    }

    Ok(())
}

impl<M, D> Sender<M, D>
where
    M: RawMutex + 'static,
    D: Driver<'static> + 'static,
{
    pub fn init_sender(
        sc: &'static StaticCell<Mutex<M, SenderInner<D>>>,
        tx_buf: &'static mut [u8],
        ep_in: D::EndpointIn,
    ) -> Self {
        let x = sc.init(Mutex::new(SenderInner { ep_in, tx_buf }));
        Sender { inner: x }
    }
}

pub async fn rpc_dispatch<M, D, T>(
    mut ep_out: D::EndpointOut,
    sender: Sender<M, D>,
    dispatch: T,
    rx_buf: &mut [u8],
) -> !
where
    M: RawMutex + 'static,
    D: Driver<'static>,
    T: Dispatch<Mutex = M, Driver = D>,
{
    'connect: loop {
        // Wait for connection
        ep_out.wait_enabled().await;

        // For each packet...
        'packet: loop {
            // Accumulate a whole frame
            let mut window = &mut rx_buf[..];
            'buffer: loop {
                if window.is_empty() {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("Overflow!");
                    let mut bonus: usize = 0;
                    loop {
                        // Just drain until the end of the overflow frame
                        match ep_out.read(rx_buf).await {
                            Ok(n) if n < 64 => {
                                bonus = bonus.saturating_add(n);
                                let err = WireError::FrameTooLong(FrameTooLong {
                                    len: u32::try_from(bonus.saturating_add(rx_buf.len()))
                                        .unwrap_or(u32::MAX),
                                    max: u32::try_from(rx_buf.len()).unwrap_or(u32::MAX),
                                });
                                dispatch.error(0, err, sender.clone()).await;
                                continue 'packet;
                            }
                            Ok(n) => {
                                bonus = bonus.saturating_add(n);
                            }
                            Err(EndpointError::BufferOverflow) => panic!(),
                            Err(EndpointError::Disabled) => continue 'connect,
                        };
                    }
                }

                let n = match ep_out.read(window).await {
                    Ok(n) => n,
                    Err(EndpointError::BufferOverflow) => panic!(),
                    Err(EndpointError::Disabled) => continue 'connect,
                };

                let (_now, later) = window.split_at_mut(n);
                window = later;
                if n != 64 {
                    break 'buffer;
                }
            }

            // We now have a full frame! Great!
            let wlen = window.len();
            let len = rx_buf.len() - wlen;
            let frame = &rx_buf[..len];

            #[cfg(feature = "defmt")]
            defmt::debug!("got frame: {=usize}", frame.len());

            // If it's for us, process it
            if let Ok((hdr, body)) = extract_header_from_bytes(frame) {
                dispatch.dispatch(hdr, body, sender.clone()).await;
            } else {
                let err = WireError::FrameTooShort(FrameTooShort {
                    len: u32::try_from(frame.len()).unwrap_or(u32::MAX),
                });
                dispatch.error(0, err, sender.clone()).await;
            }
        }
    }
}

pub const ERROR_KEY: Key = Key::for_path::<WireError>(ERROR_PATH);
pub const ERROR_PATH: &str = "error";

#[derive(Serialize, Deserialize, Schema, Debug)]
pub struct FrameTooLong {
    pub len: u32,
    pub max: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug)]
pub struct FrameTooShort {
    pub len: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug)]
pub enum WireError {
    FrameTooLong(FrameTooLong),
    FrameTooShort(FrameTooShort),
    DeserFailed,
    SerFailed,
    UnknownKey([u8; 8]),
    FailedToSpawn,
}

pub enum Outcome<T> {
    Reply(T),
    SpawnSuccess,
    SpawnFailure,
}
