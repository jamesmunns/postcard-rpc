#![allow(async_fn_in_trait)]

use crate::{
    headered::extract_header_from_bytes,
    standard_icd::{FrameTooLong, FrameTooShort, WireError},
    WireHeader,
};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_usb::{
    driver::Driver,
    msos::{self, windows_version},
    Builder, UsbDevice,
};
use embassy_usb_driver::{Endpoint, EndpointError, EndpointOut};
use sender::Sender;

pub mod buffers;
pub mod dispatch_macro;
pub mod sender;

const DEVICE_INTERFACE_GUIDS: &[&str] = &["{AFB9A6FB-30BA-44BC-9232-806CFC875321}"];

/// A trait that defines the postcard-rpc message dispatching behavior
///
/// This is normally generated automatically by the [`define_dispatch!()`][crate::define_dispatch]
/// macro.
pub trait Dispatch {
    type Mutex: RawMutex;
    type Driver: Driver<'static>;

    /// Handle a single message, with the header deserialized and the
    /// body not yet deserialized.
    ///
    /// This function must handle replying (either immediately or
    /// in the future, for example if spawning a task)
    async fn dispatch(&mut self, hdr: WireHeader, body: &[u8]);

    /// Send an error message, of the path and key defined for
    /// the connection
    async fn error(&self, seq_no: u32, error: WireError);

    /// Obtain an owned sender
    fn sender(&self) -> Sender<Self::Mutex, Self::Driver>;
}

/// A conversion trait for taking the Context and making a SpawnContext
///
/// This is necessary if you use the `spawn` variant of `define_dispatch!`.
pub trait SpawnContext {
    type SpawnCtxt: 'static;
    fn spawn_ctxt(&mut self) -> Self::SpawnCtxt;
}

/// A basic example of embassy_usb configuration values
pub fn example_config() -> embassy_usb::Config<'static> {
    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("Embassy");
    config.product = Some("postcard-rpc example");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
}

/// Configure the USB driver for use with postcard-rpc
///
/// At the moment this is very geared towards USB FS.
pub fn configure_usb<D: embassy_usb::driver::Driver<'static>>(
    driver: D,
    bufs: &'static mut buffers::UsbDeviceBuffers,
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

/// Handle RPC Dispatching
pub async fn rpc_dispatch<M, D, T>(
    mut ep_out: D::EndpointOut,
    mut dispatch: T,
    rx_buf: &'static mut [u8],
) -> !
where
    M: RawMutex + 'static,
    D: Driver<'static> + 'static,
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
                                dispatch.error(0, err).await;
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
                dispatch.dispatch(hdr, body).await;
            } else {
                let err = WireError::FrameTooShort(FrameTooShort {
                    len: u32::try_from(frame.len()).unwrap_or(u32::MAX),
                });
                dispatch.error(0, err).await;
            }
        }
    }
}
