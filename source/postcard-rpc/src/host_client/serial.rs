
use crate::{accumulator::raw::{CobsAccumulator, FeedResult}, headered::extract_header_from_bytes, Key};
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, select, sync::mpsc::Sender};
use cobs::encode_vec;
use std::collections::HashMap;
use super::{HostClient, RpcFrame, WireContext, ProcessError};

async fn cobs_wire_worker(mut port: SerialStream, ctx: WireContext) {
    let mut buf = [0u8; 1024];
    let mut acc = CobsAccumulator::<1024>::new();
    let mut subs: HashMap<Key, Sender<RpcFrame>> = HashMap::new();

    let WireContext {
        mut outgoing,
        incoming,
        mut new_subs,
    } = ctx;

    loop {
        // Wait for EITHER a serialized request, OR some data from the embedded device
        select! {
            sub = new_subs.recv() => {
                let Some(si) = sub else {
                    return;
                };

                subs.insert(si.key, si.tx);
            }
            out = outgoing.recv() => {
                // Receiver returns None when all Senders have hung up
                let Some(msg) = out else {
                    return;
                };

                // Turn the serialized message into a COBS encoded message
                //
                // TODO: this is a little wasteful, payload is already a vec,
                // then we serialize it to a second vec, then encode that to
                // a third cobs-encoded vec. Oh well.
                let msg = msg.to_bytes();
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
                                // Got a header, turn it into a frame
                                let frame = RpcFrame { header: hdr.clone(), body: body.to_vec() };

                                // Give priority to subscriptions. TBH I only do this because I know a hashmap
                                // lookup is cheaper than a waitmap search.
                                if let Some(tx) = subs.get_mut(&hdr.key) {
                                    // Yup, we have a subscription
                                    if tx.send(frame).await.is_err() {
                                        // But if sending failed, the listener is gone, so drop it
                                        subs.remove(&hdr.key);
                                    }
                                } else {
                                    // Wake the given sequence number. If the WaitMap is closed, we're done here
                                    if let Err(ProcessError::Closed) = incoming.process(frame) {
                                        return;
                                    }
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

/// # Constructor Methods
///
/// These methods are used to create a new [HostClient] instance for use with tokio serial and cobs encoding.
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
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use serde::{Serialize, Deserialize};
    /// use postcard::experimental::schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// let client = HostClient::<Error>::new_serial_cobs(
    ///     // the serial port path
    ///     "/dev/ttyACM0",
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Baud rate of serial (does not generally matter for
    ///     //  USB UART/CDC-ACM serial connections)
    ///     115_200,
    /// );
    /// ```
    ///
    pub fn new_serial_cobs(
        serial_path: &str,
        err_uri_path: &str,
        outgoing_depth: usize,
        baud: u32,
    ) -> Self {
        let (me, wire) = Self::new_manual(err_uri_path, outgoing_depth);

        let port = tokio_serial::new(serial_path, baud)
            .open_native_async()
            .unwrap();

        tokio::task::spawn(async move { cobs_wire_worker(port, wire).await });

        me
    }
}