//! A post-dispatch host client
//!
//! This library is meant to be used with the `Dispatch` type and the
//! post-dispatch wire protocol.

use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use crate::{
    accumulator::raw::{CobsAccumulator, FeedResult},
    headered::{extract_header_from_bytes, to_stdvec_keyed},
    Endpoint, Key, WireHeader,
};
use cobs::encode_vec;
use maitake_sync::{
    wait_map::{WaitError, WakeOutcome},
    WaitMap,
};
use postcard::experimental::schema::Schema;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    select,
    sync::mpsc::{Receiver, Sender},
};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// Host Error Kind
#[derive(Debug, PartialEq)]
pub enum HostErr<WireErr> {
    /// An error of the user-specified wire error type
    Wire(WireErr),
    /// We got a response that didn't match the expected value or the
    /// user specified wire error type
    BadResponse,
    /// Deserialization of the message failed
    Postcard(postcard::Error),
    /// The interface has been closed, and no further messages are possible
    Closed,
}

impl<T> From<postcard::Error> for HostErr<T> {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

impl<T> From<WaitError> for HostErr<T> {
    fn from(_: WaitError) -> Self {
        Self::Closed
    }
}

async fn wire_worker(
    mut port: SerialStream,
    mut outgoing: Receiver<Vec<u8>>,
    ctx: Arc<HostContext>,
) {
    let mut buf = [0u8; 1024];
    let mut acc = CobsAccumulator::<1024>::new();

    loop {
        // Wait for EITHER a serialized request, OR some data from the embedded device
        select! {
            out = outgoing.recv() => {
                // Receiver returns None when all Senders have hung up
                let Some(msg) = out else {
                    return;
                };

                // Turn the serialized message into a COBS encoded message
                let mut msg = encode_vec(&msg);
                msg.push(0);

                // And send it!
                if port.write_all(&msg).await.is_err() {
                    // I guess the serial port hung up.
                    return;
                }
            }
            inc = port.read(&mut buf) => {
                // if read errored, we're done
                let Ok(used) = inc else {
                    return;
                };
                let mut window = &buf[..used];

                'cobs: while !window.is_empty() {
                    window = match acc.feed(window) {
                        // Consumed the whole USB frame
                        FeedResult::Consumed => break 'cobs,
                        // Silently ignore line errors
                        // TODO: probably add tracing here
                        FeedResult::OverFull(new_wind) => new_wind,
                        FeedResult::DeserError(new_wind) => new_wind,
                        // We got a message! Attempt to dispatch it
                        FeedResult::Success { data, remaining } => {
                            // Attempt to extract a header so we can get the sequence number
                            if let Ok((hdr, body)) = extract_header_from_bytes(data) {
                                // Wake the given sequence number. If the WaitMap is closed, we're done here
                                if let WakeOutcome::Closed(_) = ctx.map.wake(&hdr, body.to_vec()) {
                                    return;
                                }
                            }

                            remaining
                        }
                    };
                }
            }
        }
    }
}

/// The [HostClient] is the primary PC-side interface.
///
/// It is generic over a single type, `WireErr`, which can be used by the
/// embedded system when a request was not understood, or some other error
/// has occurred.
///
/// [HostClient]s can be cloned, and used across multiple tasks/threads.
pub struct HostClient<WireErr> {
    ctx: Arc<HostContext>,
    out: Sender<Vec<u8>>,
    err_key: Key,
    _pd: PhantomData<fn() -> WireErr>,
}

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Create a new [HostClient]
    ///
    /// `serial_path` is the path to the serial port used. `err_uri_path` is
    /// the path associated with the `WireErr` message type.
    ///
    /// Panics if we couldn't open the serial port
    pub fn new(serial_path: &str, err_uri_path: &str) -> Self {
        // TODO: queue depth as a config?
        let (tx_pc, rx_pc) = tokio::sync::mpsc::channel(8);

        // TODO: baud rate as a config?
        // TODO: poll interval as a config?
        let port = tokio_serial::new(serial_path, 115_200)
            .open_native_async()
            .unwrap();

        let ctx = Arc::new(HostContext {
            map: WaitMap::new(),
            seq: AtomicU32::new(0),
        });
        tokio::task::spawn({
            let ctx = ctx.clone();
            async move { wire_worker(port, rx_pc, ctx).await }
        });

        let err_key = Key::for_path::<WireErr>(err_uri_path);

        HostClient {
            ctx,
            out: tx_pc,
            err_key,
            _pd: PhantomData,
        }
    }

    /// Send a message of type [Endpoint::Request][Endpoint] to `path`, and await
    /// a response of type [Endpoint::Response][Endpoint] (or WireErr) to `path`.
    ///
    /// This function will wait potentially forever. Consider using with a timeout.
    pub async fn send_resp<E: Endpoint>(
        &self,
        t: &E::Request,
    ) -> Result<E::Response, HostErr<WireErr>>
    where
        E::Request: Serialize + Schema,
        E::Response: DeserializeOwned + Schema,
    {
        let seq_no = self.ctx.seq.fetch_add(1, Ordering::Relaxed);
        let msg =
            to_stdvec_keyed(seq_no, E::REQ_KEY, &t).expect("Allocations should not ever fail");
        self.out.send(msg).await.map_err(|_| HostErr::Closed)?;
        let ok_resp = self.ctx.map.wait(WireHeader {
            seq_no,
            key: E::RESP_KEY,
        });
        let err_resp = self.ctx.map.wait(WireHeader {
            seq_no,
            key: self.err_key,
        });

        select! {
            o = ok_resp => {
                let resp = o?;
                let r = postcard::from_bytes::<E::Response>(&resp)?;
                Ok(r)
            },
            e = err_resp => {
                let resp = e?;
                let r = postcard::from_bytes::<WireErr>(&resp)?;
                Err(HostErr::Wire(r))
            },
        }
    }
}

// Manual Clone impl because WireErr may not impl Clone
impl<WireErr> Clone for HostClient<WireErr> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            out: self.out.clone(),
            err_key: self.err_key,
            _pd: PhantomData,
        }
    }
}

/// Shared context between [HostClient] and [wire_worker]
struct HostContext {
    map: WaitMap<WireHeader, Vec<u8>>,
    seq: AtomicU32,
}
