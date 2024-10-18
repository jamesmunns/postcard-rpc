use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointIn};
use futures_util::FutureExt;
use postcard_schema::Schema;
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
        let max_log_len = actual_varint_max_len(tx_buf.len());
        let x = sc.init(Mutex::new(SenderInner {
            ep_in,
            tx_buf,
            log_seq: 0,
            max_log_len,
        }));
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
        let SenderInner {
            ep_in,
            tx_buf,
            log_seq: _,
            max_log_len: _,
        } = &mut *inner;
        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, E::RESP_KEY, resp, tx_buf) {
            send_all::<D>(ep_in, used, true).await
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
        let SenderInner {
            ep_in,
            tx_buf,
            log_seq: _,
            max_log_len: _,
        } = &mut *inner;
        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, key, resp, tx_buf) {
            send_all::<D>(ep_in, used, true).await
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
        let SenderInner {
            ep_in,
            tx_buf,
            log_seq: _,
            max_log_len: _,
        } = &mut *inner;

        if let Ok(used) = crate::headered::to_slice_keyed(seq_no, T::TOPIC_KEY, msg, tx_buf) {
            send_all::<D>(ep_in, used, true).await
        } else {
            Err(())
        }
    }

    pub async fn str_publish<'a, T>(&self, s: &'a str)
    where
        T: crate::Topic<Message = [u8]>,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner {
            ep_in,
            tx_buf,
            log_seq,
            max_log_len: _,
        } = &mut *inner;
        let seq_no = *log_seq;
        *log_seq = log_seq.wrapping_add(1);
        if let Ok(used) =
            crate::headered::to_slice_keyed(seq_no, T::TOPIC_KEY, s.as_bytes(), tx_buf)
        {
            let _ = send_all::<D>(ep_in, used, false).await;
        }
    }

    pub async fn fmt_publish<'a, T>(&self, args: core::fmt::Arguments<'a>)
    where
        T: crate::Topic<Message = [u8]>,
    {
        let mut inner = self.inner.lock().await;
        let SenderInner {
            ep_in,
            tx_buf,
            log_seq,
            max_log_len,
        } = &mut *inner;
        let ttl_len = tx_buf.len();

        // First, populate the header
        let hdr = crate::WireHeader {
            key: T::TOPIC_KEY,
            seq_no: *log_seq,
        };
        *log_seq = log_seq.wrapping_add(1);
        let Ok(hdr_used) = postcard::to_slice(&hdr, tx_buf) else {
            return;
        };
        let hdr_used = hdr_used.len();

        // Then, reserve space for non-canonical length fields
        // We also set all but the last bytes to be "continuation"
        // bytes
        let (_, remaining) = tx_buf.split_at_mut(hdr_used);
        if remaining.len() < *max_log_len {
            return;
        }
        let (len_field, body) = remaining.split_at_mut(*max_log_len);
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

        let _ = send_all::<D>(ep_in, &tx_buf[..act_used], false).await;
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
    log_seq: u32,
    max_log_len: usize,
}

/// Helper function for sending a single frame.
///
/// If an empty slice is provided, no bytes will be sent.
#[inline]
async fn send_all<D>(
    ep_in: &mut D::EndpointIn,
    out: &[u8],
    wait_for_enabled: bool,
) -> Result<(), ()>
where
    D: Driver<'static>,
{
    if out.is_empty() {
        return Ok(());
    }
    if wait_for_enabled {
        ep_in.wait_enabled().await;
    } else if ep_in.wait_enabled().now_or_never().is_none() {
        return Ok(());
    }

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
