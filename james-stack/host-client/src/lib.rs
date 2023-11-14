//! A post-dispatch host client
//!
//! This library is meant to be used with the `Dispatch` type and the
//! post-dispatch wire protocol.

use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use cobs::encode_vec;

use maitake_sync::{
    wait_map::{WaitError, WakeOutcome},
    WaitMap,
};
use pd_core::{
    accumulator::raw::{CobsAccumulator, FeedResult},
    headered::{extract_header_from_bytes, Headered},
    Key,
};
use postcard::{experimental::schema::Schema, ser_flavors::StdVec};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

/// Unfortunately, the `serialport` crate seems to have some issues on M-series Macs.
///
/// For these hosts, we use a patched version of the crate that has some hacky
/// fixes applied that seem to resolve the issue.
///
/// Context: <https://github.com/serialport/serialport-rs/issues/49>
pub mod serial {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    pub use serialport_macos_hack::*;

    #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
    pub use serialport_regular::*;
}

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
        let (tx_fw, rx_fw) = tokio::sync::mpsc::channel(8);

        // TODO: baud rate as a config?
        // TODO: poll interval as a config?
        let port = serial::new(serial_path, 115_200)
            .timeout(Duration::from_millis(10))
            .open()
            .unwrap();

        let halt = Arc::new(AtomicBool::new(false));

        let _jh = Some(std::thread::spawn({
            let halt = halt.clone();
            move || io_thread(port, tx_fw, rx_pc, halt)
        }));

        let ctx = Arc::new(HostContext {
            map: WaitMap::new(),
            seq: AtomicU32::new(0),
            _jh,
            io_halt: halt,
        });
        tokio::task::spawn({
            let ctx = ctx.clone();
            async move {
                HostClientWorker { ctx, inc: rx_fw }.work().await;
            }
        });

        let err_key = Key::for_path::<WireErr>(err_uri_path);

        HostClient {
            ctx,
            out: tx_pc,
            err_key,
            _pd: PhantomData,
        }
    }

    /// Send a message of type TX to `path`, and await a response of type
    /// RX (or WireErr) to `path`.
    ///
    /// This function will wait potentially forever. Consider using with a timeout.
    pub async fn send_resp<TX, RX>(&self, path: &str, t: TX) -> Result<RX, HostErr<WireErr>>
    where
        TX: Serialize + Schema,
        RX: DeserializeOwned + Schema,
    {
        let seq_no = self.ctx.seq.fetch_add(1, Ordering::Relaxed);
        let msg = to_stdvec(seq_no, path, &t).expect("Allocations should not ever fail");
        self.out.send(msg).await.map_err(|_| HostErr::Closed)?;
        let resp = self.ctx.map.wait(seq_no).await?;
        let (hdr, body) = extract_header_from_bytes(&resp)?;

        if hdr.key == Key::for_path::<RX>(path) {
            let r = postcard::from_bytes::<RX>(body)?;
            Ok(r)
        } else if hdr.key == self.err_key {
            let r = postcard::from_bytes::<WireErr>(body)?;
            Err(HostErr::Wire(r))
        } else {
            Err(HostErr::BadResponse)
        }
    }
}

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

/// Shared context between [HostClient] and [HostClientWorker]
struct HostContext {
    map: WaitMap<u32, Vec<u8>>,
    seq: AtomicU32,
    io_halt: Arc<AtomicBool>,
    _jh: Option<JoinHandle<()>>,
}

impl Drop for HostContext {
    fn drop(&mut self) {
        // On the drop of the last HostClient, halt the IO thread
        self.io_halt.store(true, Ordering::Relaxed);

        // And wait for the thread to join
        if let Some(jh) = self._jh.take() {
            let _ = jh.join();
        }
    }
}

/// A helper type for processing incoming messages into the WaitMap
struct HostClientWorker {
    ctx: Arc<HostContext>,
    inc: Receiver<Vec<u8>>,
}

impl HostClientWorker {
    /// Process incoming messages
    async fn work(mut self) {
        // While the channel from the IO worker is still open, for each message:
        while let Some(msg) = self.inc.recv().await {
            // Attempt to extract a header so we can get the sequence number
            if let Ok((hdr, _body)) = extract_header_from_bytes(&msg) {
                // Wake the given sequence number. If the WaitMap is closed, we're done here
                if let WakeOutcome::Closed(_) = self.ctx.map.wake(&hdr.seq_no, msg) {
                    break;
                }
            }
        }
        // tell everyone we're done here
        self.ctx.io_halt.store(true, Ordering::Relaxed);
        self.ctx.map.close();
    }
}

// This is silly and I should switch to `nusb` or `tokio-serial`.
fn io_thread(
    mut port: Box<dyn serial::SerialPort>,
    to_pc: Sender<Vec<u8>>,
    mut to_fw: Receiver<Vec<u8>>,
    halt: Arc<AtomicBool>,
) {
    let mut scratch = [0u8; 256];
    let mut acc = CobsAccumulator::<256>::new();

    'serve: loop {
        if halt.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(out) = to_fw.try_recv() {
            let mut val = encode_vec(&out);
            val.push(0);
            if port.write_all(&val).is_err() {
                break 'serve;
            }
        }

        match port.read(&mut scratch) {
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(0) => break 'serve,
            Err(_e) => break 'serve,
            Ok(n) => {
                let mut window = &scratch[..n];

                'cobs: while !window.is_empty() {
                    window = match acc.feed(window) {
                        FeedResult::Consumed => break 'cobs,
                        FeedResult::OverFull(new_wind) => new_wind,
                        FeedResult::DeserError(new_wind) => {
                            println!("Deser Error!");
                            new_wind
                        }
                        FeedResult::Success { data, remaining } => {
                            if to_pc.try_send(data.to_vec()).is_err() {
                                break 'serve;
                            }

                            remaining
                        }
                    };
                }
            }
        }
    }
    halt.store(true, Ordering::Relaxed);
    // We also drop the channels here, which will notify the
    // HostClientWorker
}

// NOTE: These shouldn't live here, and should be in pd-core and behind a
// feature flag or something.

/// WARNING: This rehashes the schema! Prefer [to_slice_keyed]!
pub fn to_stdvec<T: Serialize + ?Sized + Schema>(
    seq_no: u32,
    path: &str,
    value: &T,
) -> Result<Vec<u8>, postcard::Error> {
    let flavor = Headered::try_new::<T>(StdVec::new(), seq_no, path)?;
    postcard::serialize_with_flavor(value, flavor)
}

pub fn to_stdvec_keyed<T: Serialize + ?Sized + Schema>(
    seq_no: u32,
    key: Key,
    value: &T,
) -> Result<Vec<u8>, postcard::Error> {
    let flavor = Headered::try_new_keyed(StdVec::new(), seq_no, key)?;
    postcard::serialize_with_flavor(value, flavor)
}
