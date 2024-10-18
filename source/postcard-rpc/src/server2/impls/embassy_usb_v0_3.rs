use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointIn};
use futures_util::FutureExt;
use postcard::ser_flavors::Slice;
use postcard_schema::Schema;
use serde::Serialize;
use static_cell::StaticCell;

use crate::{headered::Headered, Key};

use crate::server2::{WireTx, WireTxErrorKind};

/// This is the interface for sending information to the client.
///
/// This is normally used by postcard-rpc itself, as well as for cases where
/// you have to manually send data, like publishing on a topic or delayed
/// replies (e.g. when spawning a task).
#[derive(Copy)]
pub struct Sender<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, SenderInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Clone for Sender<M, D> {
    fn clone(&self) -> Self {
        Sender { inner: self.inner }
    }
}

/// Implementation detail, holding the endpoint and scratch buffer used for sending
pub struct SenderInner<D: Driver<'static>> {
    ep_in: D::EndpointIn,
    log_seq: u32,
    tx_buf: &'static mut [u8],
    max_log_len: usize,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> WireTx for Sender<M, D> {
    type Error = WireTxErrorKind;

    async fn send<T: Serialize + ?Sized>(&self, hdr: crate::WireHeader, msg: &T) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let SenderInner { ep_in, log_seq, tx_buf, max_log_len }: &mut SenderInner<D> = &mut inner;

        let flavor = Headered::try_new_keyed(Slice::new(*tx_buf), hdr.seq_no, hdr.key).map_err(|_| WireTxErrorKind::Other)?;
        let res = postcard::serialize_with_flavor(msg, flavor);

        if let Ok(used) = res {
            send_all::<D>(ep_in, used).await
        } else {
            Err(WireTxErrorKind::Other)
        }
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, log_seq, tx_buf, max_log_len }: &mut SenderInner<D> = &mut inner;
        send_all::<D>(ep_in, buf).await
    }
}

#[inline]
async fn send_all<D>(
    ep_in: &mut D::EndpointIn,
    out: &[u8],
) -> Result<(), WireTxErrorKind>
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
