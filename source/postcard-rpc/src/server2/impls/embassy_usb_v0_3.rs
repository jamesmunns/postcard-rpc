use embassy_executor::{SpawnError, SpawnToken, Spawner};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use futures_util::FutureExt;
use postcard::ser_flavors::Slice;
use serde::Serialize;

use crate::{
    headered::Headered,
    server2::{WireRx, WireRxErrorKind, WireSpawn, WireTx, WireTxErrorKind},
};

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

/// Implementation detail, holding the endpoint and scratch buffer used for sending
pub struct EUsbWireTxInner<D: Driver<'static>> {
    ep_in: D::EndpointIn,
    _log_seq: u32,
    tx_buf: &'static mut [u8],
    _max_log_len: usize,
}

#[derive(Copy)]
pub struct EUsbWireTx<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, EUsbWireTxInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Clone for EUsbWireTx<M, D> {
    fn clone(&self) -> Self {
        EUsbWireTx { inner: self.inner }
    }
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> WireTx for EUsbWireTx<M, D> {
    type Error = WireTxErrorKind;

    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: crate::WireHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let EUsbWireTxInner {
            ep_in,
            _log_seq: _,
            tx_buf,
            _max_log_len: _,
        }: &mut EUsbWireTxInner<D> = &mut inner;

        let flavor = Headered::try_new_keyed(Slice::new(tx_buf), hdr.seq_no, hdr.key)
            .map_err(|_| WireTxErrorKind::Other)?;
        let res = postcard::serialize_with_flavor(msg, flavor);

        if let Ok(used) = res {
            send_all::<D>(ep_in, used).await
        } else {
            Err(WireTxErrorKind::Other)
        }
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;
        send_all::<D>(&mut inner.ep_in, buf).await
    }
}

#[inline]
async fn send_all<D>(ep_in: &mut D::EndpointIn, out: &[u8]) -> Result<(), WireTxErrorKind>
where
    D: Driver<'static>,
{
    if out.is_empty() {
        return Ok(());
    }
    // TODO: Timeout?
    if ep_in.wait_enabled().now_or_never().is_none() {
        return Ok(());
    }

    // write in segments of 64. The last chunk may
    // be 0 < len <= 64.
    for ch in out.chunks(64) {
        if ep_in.write(ch).await.is_err() {
            return Err(WireTxErrorKind::ConnectionClosed);
        }
    }
    // If the total we sent was a multiple of 64, send an
    // empty message to "flush" the transaction. We already checked
    // above that the len != 0.
    if (out.len() & (64 - 1)) == 0 && ep_in.write(&[]).await.is_err() {
        return Err(WireTxErrorKind::ConnectionClosed);
    }

    Ok(())
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

pub struct EUsbWireRx<D: Driver<'static>> {
    ep_out: D::EndpointOut,
}

impl<D: Driver<'static>> WireRx for EUsbWireRx<D> {
    type Error = WireRxErrorKind;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let buflen = buf.len();
        let mut window = &mut buf[..];
        while !window.is_empty() {
            let n = match self.ep_out.read(window).await {
                Ok(n) => n,
                Err(EndpointError::BufferOverflow) => {
                    return Err(WireRxErrorKind::ReceivedMessageTooLarge)
                }
                Err(EndpointError::Disabled) => return Err(WireRxErrorKind::ConnectionClosed),
            };

            let (_now, later) = window.split_at_mut(n);
            window = later;
            if n != 64 {
                // We now have a full frame! Great!
                let wlen = window.len();
                let len = buflen - wlen;
                let frame = &mut buf[..len];

                return Ok(frame);
            }
        }

        // If we got here, we've run out of space. That's disappointing. Accumulate to the
        // end of this packet
        loop {
            match self.ep_out.read(buf).await {
                Ok(64) => {}
                Ok(_) => return Err(WireRxErrorKind::ReceivedMessageTooLarge),
                Err(EndpointError::BufferOverflow) => {
                    return Err(WireRxErrorKind::ReceivedMessageTooLarge)
                }
                Err(EndpointError::Disabled) => return Err(WireRxErrorKind::ConnectionClosed),
            };
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

// todo: just use a standard tokio impl?
#[derive(Clone)]
pub struct EUsbWireSpawn {
    spawner: Spawner,
}

impl WireSpawn for EUsbWireSpawn {
    type Error = SpawnError;

    type Info = Spawner;

    fn info(&self) -> &Self::Info {
        &self.spawner
    }
}

pub fn embassy_spawn<Sp, S>(sp: &Sp, tok: SpawnToken<S>) -> Result<(), Sp::Error>
where
    Sp: WireSpawn<Error = SpawnError, Info = Spawner>,
{
    let info = sp.info();
    info.spawn(tok)
}
