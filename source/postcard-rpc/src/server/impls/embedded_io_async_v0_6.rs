//! Implementation using `embedded-io-async`
use core::{fmt::Arguments, marker::PhantomData, ops::DerefMut};

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq},
    server::{WireRx, WireRxErrorKind, WireTx, WireTxErrorKind},
    standard_icd::LoggingTopic,
    Topic,
};
use cobs::decode;
use embassy_sync_0_7::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embedded_io_async_0_6::{Read, Write};
use postcard::{
    ser_flavors::{Flavor, Slice},
    Serializer,
};
use serde::Serialize;
use static_cell::{ConstStaticCell, StaticCell};

/// A collection of types and aliases useful for importing the correct types
pub mod dispatch_impl {
    pub use crate::server::impls::embassy_shared::embassy_spawn as spawn_fn;

    /// Type alias for `WireTx` impl
    pub type WireTxImpl<M, D> = super::EioWireTx<M, D>;
    /// Type alias for `WireRx` impl
    pub type WireRxImpl<D> = super::EioWireRx<D>;
    /// Type alias for `WireSpawn` impl
    pub type WireSpawnImpl = crate::server::impls::embassy_shared::EmbassyWireSpawn;
    /// Type alias for the receive buffer
    pub type WireRxBuf = &'static mut [u8];
}

pub use super::embassy_shared::embassy_spawn;
pub use super::embassy_shared::EmbassyWireSpawn as EioWireSpawn;

/// A handy type for storing buffers and the RX/TX impls
pub struct WireStorage<
    Rx: Read,
    Tx: Write,
    M: RawMutex + 'static,
    const RXB: usize,
    const TXB: usize,
> {
    bufs: ConstStaticCell<([u8; RXB], [u8; TXB])>,
    tx: StaticCell<Mutex<M, EioWireTxInner<Tx>>>,
    _rx: PhantomData<Rx>,
}

/// The WireTX impl for embedded-io-async
pub struct EioWireTx<R, Tx>
where
    R: RawMutex + 'static,
    Tx: Write + 'static,
{
    t: &'static Mutex<R, EioWireTxInner<Tx>>,
}

/// Embedded-IO Wire RX implementation
pub struct EioWireRx<R: Read> {
    /// A buffer for storing accumulated data
    remain: &'static mut [u8],
    /// the current offset into remain
    offset: usize,
    /// the Embedded-IO RX impl'r
    rx: R,
}

struct EioWireTxInner<Tx: Write> {
    t: Tx,
    tx_buf: &'static mut [u8],
    log_seq: u16,
}

// ----- IMPLS -----

// impl WireStorage

impl<Rx: Read, Tx: Write, M: RawMutex + 'static, const RXB: usize, const TXB: usize>
    WireStorage<Rx, Tx, M, RXB, TXB>
{
    /// Create a new wire storage
    pub const fn new() -> Self {
        Self {
            bufs: ConstStaticCell::new(([0u8; RXB], [0u8; TXB])),
            tx: StaticCell::new(),
            _rx: PhantomData,
        }
    }

    /// Create a new Wire pair using this storage
    pub fn init(&'static self, r: Rx, t: Tx) -> Option<(EioWireRx<Rx>, EioWireTx<M, Tx>)> {
        let (rxb, txb) = self.bufs.try_take()?;
        let txi = self.tx.try_init(Mutex::new(EioWireTxInner {
            t,
            tx_buf: txb,
            log_seq: 0,
        }))?;
        let rx = EioWireRx {
            remain: rxb,
            offset: 0,
            rx: r,
        };
        let tx = EioWireTx { t: txi };
        Some((rx, tx))
    }
}

impl<Rx: Read, Tx: Write, M: RawMutex + 'static, const RXB: usize, const TXB: usize> Default
    for WireStorage<Rx, Tx, M, RXB, TXB>
{
    fn default() -> Self {
        Self::new()
    }
}

// impl EioWireTx

impl<R, Tx> Clone for EioWireTx<R, Tx>
where
    R: RawMutex + 'static,
    Tx: Write + 'static,
{
    fn clone(&self) -> Self {
        Self { t: self.t }
    }
}

impl<R, Tx> WireTx for EioWireTx<R, Tx>
where
    R: RawMutex + 'static,
    Tx: Write + 'static,
{
    type Error = WireTxErrorKind;

    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut guard = self.t.lock().await;
        let EioWireTxInner { t, tx_buf, .. } = guard.deref_mut();

        // Create a cobs-encoding flavor using our temp buffer
        let mut flavor = flava_flav(tx_buf)?;

        // Put the header into the buffer, which will cobs encode it
        header_to_flavor(&hdr, &mut flavor)?;

        // Now do normal serialization (and cobs encoding)
        let used = body_to_flavor(msg, flavor)?;

        // Write it all to the serial port now
        //
        // TODO: Sending should really never fail, if in practice it does,
        // we might want to do a more clever mapping of errors to differentiate
        // between fatal and non-fatal errors.
        t.write_all(used)
            .await
            .map_err(|_| WireTxErrorKind::ConnectionClosed)?;

        // defmt::println!("SENT NORMAL {=usize} {=[u8]}", used.len(), used);
        // We did it! yaaaay!
        Ok(())
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut guard = self.t.lock().await;
        let EioWireTxInner { t, tx_buf, .. } = guard.deref_mut();

        // Create a cobs-encoding flavor using our temp buffer
        let mut flavor = flava_flav(tx_buf)?;

        flavor.try_extend(buf).map_err(|_| WireTxErrorKind::Other)?;
        let used = flavor.finalize().map_err(|_| WireTxErrorKind::Other)?;

        // Write it all to the serial port now
        //
        // TODO: Sending should really never fail, if in practice it does,
        // we might want to do a more clever mapping of errors to differentiate
        // between fatal and non-fatal errors.
        t.write_all(used)
            .await
            .map_err(|_| WireTxErrorKind::ConnectionClosed)?;

        // We did it! yaaaay!
        Ok(())
    }

    async fn send_log_str(&self, kkind: VarKeyKind, s: &str) -> Result<(), Self::Error> {
        let mut guard = self.t.lock().await;
        let EioWireTxInner { t, tx_buf, log_seq } = guard.deref_mut();

        // Create a cobs-encoding flavor using our temp buffer
        let mut flavor = flava_flav(tx_buf)?;

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

        header_to_flavor(&wh, &mut flavor)?;
        let used = body_to_flavor(s, flavor)?;

        // Write it all to the serial port now
        //
        // TODO: Sending should really never fail, if in practice it does,
        // we might want to do a more clever mapping of errors to differentiate
        // between fatal and non-fatal errors.
        t.write_all(used)
            .await
            .map_err(|_| WireTxErrorKind::ConnectionClosed)?;

        // We did it! yaaaay!
        Ok(())
    }

    async fn send_log_fmt<'a>(
        &self,
        _kkind: VarKeyKind,
        _a: Arguments<'a>,
    ) -> Result<(), Self::Error> {
        todo!()
    }
}

// impl EioWireRx

impl<R: Read> EioWireRx<R> {
    /// Create a new EioWireRx impl
    pub fn new(rx: R, buffer: &'static mut [u8]) -> Self {
        Self {
            remain: buffer,
            rx,
            offset: 0,
        }
    }
}

impl<R: Read> WireRx for EioWireRx<R> {
    type Error = WireRxErrorKind;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        // Robustness: enforce offset is always inbounds to hopefully please the optimizer.
        if self.offset >= self.remain.len() {
            self.offset = 0;
        }

        // If offset is non-zero, we could potentially get a "free" packet without receiving
        // anything from our input
        if self.offset != 0 {
            let r = &mut self.remain[..self.offset];
            // Do we have a zero? If so, we could have a packet!
            if let Some(pos) = r.iter().position(|b| *b == 0) {
                let (now, later) = r.split_at(pos + 1);
                let res = decode(now, buf);

                let after_len = later.len();
                copy_backwards(r, pos + 1);
                self.offset = after_len;

                return match res {
                    // decoded successfully
                    Ok(rpt) => Ok(&mut buf[..rpt.frame_size()]),
                    // the destination buf was too small
                    Err(cobs::DecodeError::TargetBufTooSmall) => {
                        Err(WireRxErrorKind::ReceivedMessageTooLarge)
                    }
                    // some kind of decode error
                    Err(_) => Err(WireRxErrorKind::Other),
                };
            }
            // else: We have data but no zero
        }

        // Nope, no free packets, we're going to have to receive some data.
        let Self { remain, offset, rx } = self;
        loop {
            // Do we have any room left in `remain`?
            if *offset >= remain.len() {
                // if we're full, we shouldn't be receiving more data.
                *offset = 0;
                return Err(WireRxErrorKind::ReceivedMessageTooLarge);
            }

            // Here's the surgery we're gunna do on the buffer:
            //
            //           v----- offset                  < PART A
            // | old     | new                      |   < PART A
            //           |---got--------|               < PART B
            // | old     | left         | _right    |   < PART B
            // | old     | left  0| lr  | _right    |   < PART C
            //      pos----------^                      < PART C
            //      old_left_len--------^           |   < PART C
            // |    DECODE        | lr  | right     |   < PART C
            // | lr  |                              |   < PART D
            //
            // First, we break off the current contents from the about-to-be-received
            // contents - PART A
            let (old, new) = remain.split_at_mut(*offset);
            let got = rx.read(new).await;
            let got = match got {
                Ok(0) => {
                    *offset = 0;
                    return Err(WireRxErrorKind::ConnectionClosed);
                }
                Ok(n) => n,
                Err(_) => {
                    *offset = 0;
                    return Err(WireRxErrorKind::Other);
                }
            };

            // At this point, we have some data in the `new` region. Let's split this
            // down to the part we actually received - PART B
            let (left, _right) = new.split_at(got);

            // Now: If there's no zero in the new region, we're done with this
            // new data. Go back around to wait for some more.
            let Some(pos) = left.iter().position(|b| *b == 0) else {
                *offset += got;
                continue;
            };

            // Okay! We have some data, and there's a zero. We want to merge together
            // the old data, PLUS UP TO the first 0 in the new data. We also need to
            // remember where the new data stops, so we can hold on to that. - PART C
            let old_left_len = old.len() + left.len();
            let lrstart = old.len() + (pos + 1);
            let lrlen = left.len() - (pos + 1);
            // We decode into the output buffer
            let res = decode(&remain[..lrstart], buf);

            // Now: regardless of whether that was SUCCESSFUL or not, we need to move
            // the new data AFTER the first zero BACK to the beginning of the array.
            // PART D
            copy_backwards(&mut remain[..old_left_len], lrstart);
            self.offset = lrlen;

            return match res {
                Ok(rpt) => Ok(&mut buf[..rpt.frame_size()]),
                Err(_e) => Err(WireRxErrorKind::ReceivedMessageTooLarge),
            };
        }
    }
}

// ---- HELPER FUNCTIONS -----

// This is basically `rotate_left`, but we don't copy the data at the
// start back to the end, allowing for duplicate data
fn copy_backwards(buf: &mut [u8], start: usize) {
    if buf.len() <= start {
        return;
    }
    let count = buf.len() - start;
    let base = buf.as_mut_ptr();

    // Safety: We've checked that base + start is inbounds for buf, and that
    // count + start is inbounds of buf.
    unsafe {
        core::ptr::copy(base.add(start).cast_const(), base, count);
    }
}

fn flava_flav(buf: &'_ mut [u8]) -> Result<Cobs<Slice<'_>>, WireTxErrorKind> {
    Cobs::try_new(Slice::new(buf)).map_err(|_| WireTxErrorKind::Other)
}

fn header_to_flavor(hdr: &VarHeader, flava: &mut Cobs<Slice<'_>>) -> Result<(), WireTxErrorKind> {
    // Serialize the header to a side buffer, since it doesn't use Serde
    let mut hdr_buf = [0u8; 1 + 4 + 8];
    let (used, _unused) = hdr
        .write_to_slice(&mut hdr_buf)
        .ok_or(WireTxErrorKind::Other)?;

    // Put the header into the buffer, which will cobs encode it
    flava.try_extend(used).map_err(|_| WireTxErrorKind::Other)?;

    Ok(())
}

fn body_to_flavor<'a, T: Serialize + ?Sized>(
    msg: &T,
    flava: Cobs<Slice<'a>>,
) -> Result<&'a [u8], WireTxErrorKind> {
    let mut serializer = Serializer { output: flava };
    msg.serialize(&mut serializer)
        .map_err(|_| WireTxErrorKind::Other)?;
    let used = serializer
        .output
        .finalize()
        .map_err(|_| WireTxErrorKind::Other)?;
    Ok(used)
}

// ---- COPY AND PASTE SHAME ----

use core::ops::IndexMut;

/// The `Cobs` flavor implements [Consistent Overhead Byte Stuffing] on
/// the serialized data. The output of this flavor includes the termination/sentinel
/// byte of `0x00`.
///
/// This protocol is useful when sending data over a serial interface without framing such as a UART
///
/// [Consistent Overhead Byte Stuffing]: https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing
///
/// Copy and pasted from postcard, but with cobs v0.4 instead of v0.2.
struct Cobs<B>
where
    B: Flavor + IndexMut<usize, Output = u8>,
{
    flav: B,
    cobs: cobs::EncoderState,
}

impl<B> Cobs<B>
where
    B: Flavor + IndexMut<usize, Output = u8>,
{
    /// Create a new Cobs modifier Flavor. If there is insufficient space
    /// to push the leading header byte, the method will return an Error
    fn try_new(mut bee: B) -> postcard::Result<Self> {
        bee.try_push(0)
            .map_err(|_| postcard::Error::SerializeBufferFull)?;
        Ok(Self {
            flav: bee,
            cobs: cobs::EncoderState::default(),
        })
    }
}

impl<B> Flavor for Cobs<B>
where
    B: Flavor + IndexMut<usize, Output = u8>,
{
    type Output = <B as Flavor>::Output;

    #[inline(always)]
    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        use cobs::PushResult::*;
        match self.cobs.push(data) {
            AddSingle(n) => self.flav.try_push(n),
            ModifyFromStartAndSkip((idx, mval)) => {
                self.flav[idx] = mval;
                self.flav.try_push(0)
            }
            ModifyFromStartAndPushAndSkip((idx, mval, nval)) => {
                self.flav[idx] = mval;
                self.flav.try_push(nval)?;
                self.flav.try_push(0)
            }
        }
    }

    fn finalize(mut self) -> postcard::Result<Self::Output> {
        let (idx, mval) = self.cobs.finalize();
        self.flav[idx] = mval;
        self.flav.try_push(0)?;
        self.flav.finalize()
    }
}
