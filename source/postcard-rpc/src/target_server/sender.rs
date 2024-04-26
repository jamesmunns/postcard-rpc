use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointIn};
use postcard::experimental::schema::Schema;
use serde::Serialize;
use static_cell::StaticCell;

use crate::Key;

#[derive(Copy)]
pub struct Sender<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, SenderInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Sender<M, D> {
    pub fn init_sender(
        sc: &'static StaticCell<Mutex<M, SenderInner<D>>>,
        tx_buf: &'static mut [u8],
        ep_in: D::EndpointIn,
    ) -> Self {
        let x = sc.init(Mutex::new(SenderInner { ep_in, tx_buf }));
        Sender { inner: x }
    }

    #[inline]
    pub async fn reply<E>(&self, seq_no: u32, resp: &E::Response) -> Result<(), ()>
    where
        E: crate::Endpoint,
        E::Response: Serialize + Schema,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner { ep_in, tx_buf } = &mut *inner;
        reply_keyed::<D, E::Response>(ep_in, E::RESP_KEY, seq_no, resp, tx_buf).await
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

#[inline]
async fn reply_keyed<D, T>(
    ep_in: &mut D::EndpointIn,
    key: Key,
    seq_no: u32,
    resp: &T,
    out: &mut [u8],
) -> Result<(), ()>
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
    // empty message to "flush" the transaction
    if (out.len() & (64 - 1)) == 0 && ep_in.write(&[]).await.is_err() {
        return Err(());
    }

    Ok(())
}
