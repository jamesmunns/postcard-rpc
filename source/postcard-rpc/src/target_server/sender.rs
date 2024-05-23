use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointIn};
use postcard::experimental::schema::Schema;
use serde::Serialize;
use static_cell::StaticCell;

use crate::Key;

/// This is the interface for sending information to the client.
///
/// This is normally used by postcard-rpc itself, as well as for cases where
/// you have to manually send data, like publishing on a topic or delayed
/// replies (e.g. when spawning a task).
#[derive(Copy)]
pub struct Sender<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, SenderInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Sender<M, D> {
    /// Initialize the Sender, giving it the pieces it needs
    ///
    /// Panics if called more than once.
    pub fn init_sender(
        sc: &'static StaticCell<Mutex<M, SenderInner<D>>>,
        tx_buf: &'static mut [u8],
        ep_in: D::EndpointIn,
    ) -> Self {
        let x = sc.init(Mutex::new(SenderInner { ep_in, tx_buf }));
        Sender { inner: x }
    }

    /// Send a reply for the given endpoint
    #[inline]
    pub async fn reply<E>(&self, seq_no: u32, resp: &E::Response) -> Result<(), ()>
    where
        E: crate::Endpoint,
        E::Response: Serialize + Schema,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, E::RESP_KEY, resp, tx_buf) {
            send_all::<D>(ep_in, used).await
        } else {
            Err(())
        }
    }

    /// Send a reply with the given Key
    ///
    /// This is useful when replying with "unusual" keys, for example Error responses
    /// not tied to any specific Endpoint.
    #[inline]
    pub async fn reply_keyed<T>(&self, seq_no: u32, key: Key, resp: &T) -> Result<(), ()>
    where
        T: Serialize + Schema,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, key, resp, tx_buf) {
            send_all::<D>(ep_in, used).await
        } else {
            Err(())
        }
    }

    /// Publish a Topic message
    #[inline]
    pub async fn publish<T>(&self, seq_no: u32, msg: &T::Message) -> Result<(), ()>
    where
        T: crate::Topic,
        T::Message: Serialize + Schema,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, T::TOPIC_KEY, msg, tx_buf) {
            send_all::<D>(ep_in, used).await
        } else {
            Err(())
        }
    }
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Clone for Sender<M, D> {
    fn clone(&self) -> Self {
        Sender { inner: self.inner }
    }
}

/// Implementation detail, holding the endpoint and scratch buffer used for sending
pub struct SenderInner<D: Driver<'static>> {
    ep_in: D::EndpointIn,
    tx_buf: &'static mut [u8],
}

/// Helper function for sending a single frame.
///
/// If an empty slice is provided, no bytes will be sent.
#[inline]
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
    // empty message to "flush" the transaction. We already checked
    // above that the len != 0.
    if (out.len() & (64 - 1)) == 0 && ep_in.write(&[]).await.is_err() {
        return Err(());
    }

    Ok(())
}
