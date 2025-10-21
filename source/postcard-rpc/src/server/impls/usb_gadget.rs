#![allow(missing_docs)]

use core::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use std::sync::Arc;

use crate::server::{WireRx, WireRxErrorKind, WireTx, WireTxErrorKind};

use bytes::Bytes;
use tokio::sync::Mutex;
use usb_gadget::function::custom::{EndpointReceiver, EndpointSender};

/// Default time in milliseconds to wait for the completion of sending
pub const DEFAULT_TIMEOUT_MS_PER_FRAME: usize = 2;

/// A collection of types and aliases useful for importing the correct types
pub mod dispatch_impl {
    use core::sync::atomic::{AtomicBool, Ordering};

    use std::{
        io::{self, Error, ErrorKind},
        sync::Arc,
    };

    use usb_gadget::{
        function::{
            custom::{Endpoint, EndpointDirection, Event, Interface},
            Handle,
        },
        Class, Config, OsDescriptor, WebUsb,
    };

    /// Type alias for `WireTx` impl
    pub type WireTxImpl = super::UsbGadgetWireTx;
    /// Type alias for `WireRx` impl
    pub type WireRxImpl = super::UsbGadgetWireRx;
    /// Type alias for `WireSpawn` impl
    pub type WireSpawnImpl = crate::server::impls::tokio_shared::TokioWireSpawn;
    /// Type alias for the receive buffer
    pub type WireRxBuf = &'static mut [u8];

    pub use crate::server::impls::tokio_shared::tokio_spawn as spawn_fn;

    use usb_gadget::function::custom::Custom;
    use usb_gadget::{Gadget, RegGadget};

    use crate::server::impls::usb_gadget::{UsbGadgetWireRx, UsbGadgetWireTx};

    /// A handy type for storing buffers and the RX/TX impls
    pub struct WireStorage {}

    impl WireStorage {
        pub const fn new() -> Self {
            Self {}
        }

        pub fn init(
            &'static self,
            gadget: Gadget,
            tx_buf: &'static mut [u8],
        ) -> (RegGadget, WireTxImpl, WireRxImpl) {
            let udc = usb_gadget::default_udc().expect("cannot get UDC");

            let ((gadget, handle), wtx, wrx) = self.init_without_build(gadget, tx_buf);
            let reg = gadget
                .with_config(Config::new("config").with_function(handle))
                .bind(&udc)
                .expect("cannot bind to UDC");

            (reg, wtx, wrx)
        }

        pub fn init_without_build(
            &'static self,
            gadget: Gadget,
            tx_buf: &'static mut [u8],
        ) -> ((Gadget, Handle), WireTxImpl, WireRxImpl) {
            let (ep_tx, ep_tx_dir) = EndpointDirection::device_to_host();
            let (ep_rx, ep_rx_dir) = EndpointDirection::host_to_device();

            let (mut custom, handle) = Custom::builder()
                .with_interface(
                    Interface::new(Class::vendor_specific(0, 0), "postcard-rpc")
                        .with_endpoint({
                            let mut ep = Endpoint::bulk(ep_tx_dir);
                            ep.max_packet_size_hs = 64;
                            ep
                        })
                        .with_endpoint({
                            let mut ep = Endpoint::bulk(ep_rx_dir);
                            ep.max_packet_size_hs = 64;
                            ep
                        }),
                )
                .build();

            let gadget = gadget
                .with_os_descriptor(OsDescriptor::microsoft())
                .with_web_usb(WebUsb::new(0xf1, "http://webusb.org"));

            let rx_enabled = Arc::new(AtomicBool::new(false));
            let tx_enabled = Arc::new(AtomicBool::new(false));

            {
                let rx_enabled = rx_enabled.clone();
                let tx_enabled = tx_enabled.clone();

                // Listen to events on the custom function
                // The device will be unbound/removed when the `custom` interface is dropped
                tokio::spawn(async move {
                    while let Ok(_) = custom.wait_event().await {
                        match custom.event()? {
                            Event::Enable => {
                                tx_enabled.store(true, Ordering::Release);
                                rx_enabled.store(true, Ordering::Release);
                            }
                            _ => {}
                        }
                    }

                    Err::<(), io::Error>(Error::from(ErrorKind::BrokenPipe))
                });
            }

            let wtx = UsbGadgetWireTx::new(ep_tx, tx_enabled, tx_buf);
            let wrx = UsbGadgetWireRx::new(ep_rx, rx_enabled);

            ((gadget, handle), wtx, wrx)
        }
    }
}

/// The WireTX impl for usb-gadget
#[derive(Debug, Clone)]
pub struct UsbGadgetWireTx {
    inner: Arc<Mutex<UsbGadgetWireTxInner>>,
}

impl UsbGadgetWireTx {
    pub fn new(
        ep_tx: EndpointSender,
        ep_enabled: Arc<AtomicBool>,
        tx_buf: &'static mut [u8],
    ) -> Self {
        let inner = UsbGadgetWireTxInner {
            ep_tx,
            ep_enabled,
            tx_buf,
            pending_frame: false,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

#[derive(Debug)]
struct UsbGadgetWireTxInner {
    ep_tx: EndpointSender,
    ep_enabled: Arc<AtomicBool>,
    tx_buf: &'static mut [u8],
    pending_frame: bool,
}

impl WireTx for UsbGadgetWireTx {
    type Error = WireTxErrorKind;

    async fn wait_connection(&self) {
        let inner = self.inner.lock().await;

        while !inner.ep_enabled.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    async fn send<T: serde::Serialize + ?Sized>(
        &self,
        hdr: crate::header::VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let bytes = {
            let mut inner = self.inner.lock().await;

            let (hdr_used, remain) = hdr
                .write_to_slice(&mut inner.tx_buf)
                .ok_or(WireTxErrorKind::Other)?;

            let bdy_used = postcard::to_slice(msg, remain).map_err(|_| WireTxErrorKind::Other)?;
            let used_total = hdr_used.len() + bdy_used.len();

            Bytes::copy_from_slice(&inner.tx_buf[0..used_total])
        };

        self.send_raw(&bytes).await
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;
        let UsbGadgetWireTxInner {
            ep_tx,
            pending_frame,
            ..
        } = &mut *inner;

        let chunk_size = ep_tx
            .max_packet_size()
            .or(Err(WireTxErrorKind::ConnectionClosed))?;

        let timeout_ms_per_frame = DEFAULT_TIMEOUT_MS_PER_FRAME;

        // Calculate an estimated timeout based on the number of frames we need to send
        // For now, we use 2ms/frame by default, rounded UP
        let frames = (buf.len() + (chunk_size - 1)) / chunk_size;
        let timeout = Duration::from_millis((frames * timeout_ms_per_frame) as u64);

        let send = async {
            // If we left off a pending frame, send one now so we don't leave an unterminated message
            if *pending_frame {
                ep_tx
                    .send_async(Bytes::new())
                    .await
                    .or(Err(WireTxErrorKind::ConnectionClosed))?
            }

            *pending_frame = true;

            let mut bytes = Bytes::copy_from_slice(buf);

            while !bytes.is_empty() {
                let ch = bytes.split_to(chunk_size.min(bytes.len()));

                ep_tx
                    .send_async(ch)
                    .await
                    .or(Err(WireTxErrorKind::ConnectionClosed))?;
            }

            // If the total we sent was a multiple of packet size, send an
            // empty message to "flush" the transaction. We already checked
            // above that the len != 0.
            if (buf.len() & (chunk_size - 1)) == 0 {
                ep_tx
                    .send_async(Bytes::new())
                    .await
                    .or(Err(WireTxErrorKind::ConnectionClosed))?
            }

            *pending_frame = false;

            Ok::<(), WireTxErrorKind>(())
        };

        tokio::time::timeout(timeout, send)
            .await
            .or(Err(WireTxErrorKind::Timeout))?
    }

    async fn send_log_str(
        &self,
        _kkind: crate::header::VarKeyKind,
        _s: &str,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }

    async fn send_log_fmt<'a>(
        &self,
        _kkind: crate::header::VarKeyKind,
        _a: core::fmt::Arguments<'a>,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }
}

/// The WireRx impl for usb-gadget
#[derive(Debug, Clone)]
pub struct UsbGadgetWireRx {
    ep_rx: Arc<Mutex<EndpointReceiver>>,
    ep_enabled: Arc<AtomicBool>,
}

impl UsbGadgetWireRx {
    pub fn new(ep_rx: EndpointReceiver, ep_enabled: Arc<AtomicBool>) -> Self {
        Self {
            ep_rx: Arc::new(Mutex::new(ep_rx)),
            ep_enabled,
        }
    }
}

impl WireRx for UsbGadgetWireRx {
    type Error = WireRxErrorKind;

    async fn wait_connection(&mut self) {
        let Self { ep_enabled, .. } = self;

        while !ep_enabled.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let mut ep_rx = self.ep_rx.lock().await;

        let packet_size = ep_rx
            .max_packet_size()
            .or(Err(WireRxErrorKind::ConnectionClosed))?;

        let buflen = buf.len();
        let mut window = &mut buf[..];

        while !window.is_empty() {
            let data = ep_rx
                .recv_async(bytes::BytesMut::with_capacity(packet_size))
                .await
                .or(Err(WireRxErrorKind::ConnectionClosed))?;

            match data {
                Some(data) => {
                    let n = data.len();
                    window[0..n].copy_from_slice(&data);

                    let (_now, later) = window.split_at_mut(n);
                    window = later;
                    if n != packet_size {
                        // We now have a full frame! Great!
                        let wlen = window.len();
                        let len = buflen - wlen;
                        let frame = &mut buf[..len];

                        return Ok(frame);
                    }
                }
                None => return Ok(&mut buf[0..0]),
            }
        }

        // Ran out of space...?
        Err(WireRxErrorKind::Other) // TODO
    }
}
