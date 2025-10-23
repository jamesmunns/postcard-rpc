#![allow(missing_docs)]

use core::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use std::sync::Arc;

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq},
    server::{WireRx, WireRxErrorKind, WireTx, WireTxErrorKind},
    standard_icd::LoggingTopic,
    Topic,
};

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
        ) -> Result<(RegGadget, WireTxImpl, WireRxImpl), io::Error> {
            let udc = usb_gadget::default_udc()?;

            let ((gadget, handle), wtx, wrx) = self.init_without_build(gadget, tx_buf);
            let reg = gadget
                .with_config(Config::new("config").with_function(handle))
                .bind(&udc)?;

            Ok((reg, wtx, wrx))
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

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

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
            log_seq: 0,
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
    log_seq: u16,
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
        kkind: crate::header::VarKeyKind,
        s: &str,
    ) -> Result<(), Self::Error> {
        let bytes = {
            let mut inner = self.inner.lock().await;
            let UsbGadgetWireTxInner {
                log_seq, tx_buf, ..
            } = &mut *inner;

            let key = match kkind {
                VarKeyKind::Key1 => VarKey::Key1(LoggingTopic::TOPIC_KEY1),
                VarKeyKind::Key2 => VarKey::Key2(LoggingTopic::TOPIC_KEY2),
                VarKeyKind::Key4 => VarKey::Key4(LoggingTopic::TOPIC_KEY4),
                VarKeyKind::Key8 => VarKey::Key8(LoggingTopic::TOPIC_KEY),
            };
            let ctr = *log_seq;
            *log_seq = log_seq.wrapping_add(1);
            let wh = VarHeader {
                key,
                seq_no: VarSeq::Seq2(ctr),
            };

            let (hdr_used, remain) = wh.write_to_slice(tx_buf).ok_or(WireTxErrorKind::Other)?;
            let bdy_used = postcard::to_slice(s, remain).map_err(|_| WireTxErrorKind::Other)?;
            let used_total = hdr_used.len() + bdy_used.len();

            tx_buf
                .get(..used_total)
                .map(|used| Bytes::copy_from_slice(used))
        };

        match &bytes {
            Some(bytes) => self.send_raw(bytes).await,
            None => Err(WireTxErrorKind::Other),
        }
    }

    async fn send_log_fmt<'a>(
        &self,
        kkind: crate::header::VarKeyKind,
        args: core::fmt::Arguments<'a>,
    ) -> Result<(), Self::Error> {
        let bytes = {
            let mut inner = self.inner.lock().await;
            let UsbGadgetWireTxInner {
                log_seq, tx_buf, ..
            } = &mut *inner;

            let ttl_len = tx_buf.len();
            let key = match kkind {
                VarKeyKind::Key1 => VarKey::Key1(LoggingTopic::TOPIC_KEY1),
                VarKeyKind::Key2 => VarKey::Key2(LoggingTopic::TOPIC_KEY2),
                VarKeyKind::Key4 => VarKey::Key4(LoggingTopic::TOPIC_KEY4),
                VarKeyKind::Key8 => VarKey::Key8(LoggingTopic::TOPIC_KEY),
            };
            let ctr = *log_seq;
            *log_seq = log_seq.wrapping_add(1);
            let wh = VarHeader {
                key,
                seq_no: VarSeq::Seq2(ctr),
            };

            let Some((_hdr, remaining)) = wh.write_to_slice(tx_buf) else {
                return Err(WireTxErrorKind::Other);
            };
            let max_log_len = actual_varint_max_len(remaining.len());

            // Then, reserve space for non-canonical length fields
            // We also set all but the last bytes to be "continuation"
            // bytes
            if remaining.len() < max_log_len {
                return Err(WireTxErrorKind::Other);
            }

            let (len_field, body) = remaining.split_at_mut(max_log_len);
            for b in len_field.iter_mut() {
                *b = 0x80;
            }
            if let Some(b) = len_field.last_mut() {
                *b = 0x00;
            }

            // Then, do the formatting
            let body_len = body.len();
            let mut sw = SliceWriter(body);
            let res = core::fmt::write(&mut sw, args);

            // Calculate the number of bytes used *for formatting*.
            let remain = sw.0.len();
            let used = body_len - remain;

            // If we had an error, that's probably because we ran out
            // of room. If we had an error, AND there is at least three
            // bytes, then replace those with '.'s like ...
            if res.is_err() && (body.len() >= 3) {
                let start = body.len() - 3;
                body[start..].iter_mut().for_each(|b| *b = b'.');
            }

            // then go back and fill in the len - we write the len
            // directly to the reserved bytes, and if we DIDN'T use
            // the full space, we mark the end of the real length as
            // a continuation field. This will result in a non-canonical
            // "extended" length in postcard, and will "spill into" the
            // bytes we wrote previously above
            let mut len_bytes = [0u8; varint_max::<usize>()];
            let len_used = varint_usize(used, &mut len_bytes);
            if len_used.len() != len_field.len() {
                if let Some(b) = len_used.last_mut() {
                    *b |= 0x80;
                }
            }
            len_field[..len_used.len()].copy_from_slice(len_used);

            // Calculate the TOTAL amount
            let act_used = ttl_len - remain;

            tx_buf
                .get(..act_used)
                .map(|used| Bytes::copy_from_slice(used))
        };

        match &bytes {
            Some(bytes) => self.send_raw(bytes).await,
            None => Err(WireTxErrorKind::Other),
        }
    }
}

struct SliceWriter<'a>(&'a mut [u8]);

impl<'a> core::fmt::Write for SliceWriter<'a> {
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        let sli = core::mem::take(&mut self.0);

        // If this write would overflow us, note that, but still take
        // as much as we possibly can here
        let bad = s.len() > sli.len();
        let to_write = s.len().min(sli.len());
        let (now, later) = sli.split_at_mut(to_write);
        now.copy_from_slice(s.as_bytes());
        self.0 = later;

        // Now, report whether we overflowed or not
        if bad {
            Err(core::fmt::Error)
        } else {
            Ok(())
        }
    }
}

/// Returns the maximum number of bytes required to encode T.
const fn varint_max<T: Sized>() -> usize {
    const BITS_PER_BYTE: usize = 8;
    const BITS_PER_VARINT_BYTE: usize = 7;

    // How many data bits do we need for this type?
    let bits = core::mem::size_of::<T>() * BITS_PER_BYTE;

    // We add (BITS_PER_VARINT_BYTE - 1), to ensure any integer divisions
    // with a remainder will always add exactly one full byte, but
    // an evenly divided number of bits will be the same
    let roundup_bits = bits + (BITS_PER_VARINT_BYTE - 1);

    // Apply division, using normal "round down" integer division
    roundup_bits / BITS_PER_VARINT_BYTE
}

#[inline]
fn varint_usize(n: usize, out: &mut [u8; varint_max::<usize>()]) -> &mut [u8] {
    let mut value = n;
    for i in 0..varint_max::<usize>() {
        out[i] = value.to_le_bytes()[0];
        if value < 128 {
            return &mut out[..=i];
        }

        out[i] |= 0x80;
        value >>= 7;
    }
    debug_assert_eq!(value, 0);
    &mut out[..]
}

fn actual_varint_max_len(largest: usize) -> usize {
    if largest < (2 << 7) {
        1
    } else if largest < (2 << 14) {
        2
    } else if largest < (2 << 21) {
        3
    } else if largest < (2 << 28) {
        4
    } else {
        varint_max::<usize>()
    }
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

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
        Err(WireRxErrorKind::Other)
    }
}
