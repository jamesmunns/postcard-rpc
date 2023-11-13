use std::{sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
}, time::Duration};

use cobs::encode_vec;
use icd::FatalError;
pub use james_icd as icd;
use maitake_sync::{wait_map::WakeOutcome, WaitMap};
use pd_core::{
    accumulator::raw::{CobsAccumulator, FeedResult},
    headered::{extract_header_from_bytes, Headered},
    Key, WireHeader,
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

struct HostClientWorker {
    map: Arc<WaitMap<u32, Vec<u8>>>,
    inc: Receiver<Vec<u8>>,
}

impl HostClientWorker {
    async fn work(mut self) {
        while let Some(msg) = self.inc.recv().await {
            if let Ok((hdr, _body)) = extract_header_from_bytes(&msg) {
                if let WakeOutcome::Closed(_) = self.map.wake(&hdr.seq_no, msg) {
                    return;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct HostClient {
    map: Arc<WaitMap<u32, Vec<u8>>>,
    out: Sender<Vec<u8>>,
    seq: Arc<AtomicU32>,
}

impl HostClient {
    pub fn new(path: &str) -> Self {
        let (tx_pc, rx_pc) = tokio::sync::mpsc::channel(8);
        let (tx_fw, rx_fw) = tokio::sync::mpsc::channel(8);

        let port = serial::new(path, 115_200)
            .timeout(Duration::from_millis(10))
            .open()
            .unwrap();

        let halt = Arc::new(AtomicBool::new(false));

        let _jh = Some(std::thread::spawn({
            let halt = halt.clone();
            move || io_thread(port, tx_fw, rx_pc, halt)
        }));

        let map = Arc::new(WaitMap::new());
        let seq_no = Arc::new(AtomicU32::new(0));
        tokio::task::spawn({
            let map = map.clone();
            async move {
                HostClientWorker { map, inc: rx_fw }.work().await;
            }
        });

        HostClient { map, out: tx_pc, seq: seq_no }
    }

    pub async fn send_resp<TX, RX>(&self, path: &str, t: TX) -> Result<RX, FatalError>
    where
        TX: Serialize + Schema,
        RX: DeserializeOwned + Schema,
    {
        let seq_no = self.seq.fetch_add(1, Ordering::Relaxed);
        let msg = to_stdvec(seq_no, path, &t).unwrap();
        self.out.send(msg).await.unwrap();
        let resp = self.map.wait(seq_no).await.unwrap();
        let (hdr, body) = extract_header_from_bytes(&resp).unwrap();

        if hdr.key == Key::for_path::<RX>(path) {
            let r = postcard::from_bytes::<RX>(body).unwrap();
            Ok(r)
        } else if hdr.key == Key::for_path::<FatalError>("error") {
            let r = postcard::from_bytes::<FatalError>(body).unwrap();
            Err(r)
        } else {
            panic!()
        }
    }
}

pub fn io_thread(
    mut port: Box<dyn serial::SerialPort>,
    to_pc: Sender<Vec<u8>>,
    mut to_fw: Receiver<Vec<u8>>,
    halt: Arc<AtomicBool>,
) {
    let mut scratch = [0u8; 256];
    let mut acc = CobsAccumulator::<256>::new();

    loop {
        if halt.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(out) = to_fw.try_recv() {
            let mut val = encode_vec(&out);
            val.push(0);
            port.write_all(&val).unwrap();
        }

        match port.read(&mut scratch) {
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(0) => {
                halt.store(true, Ordering::Relaxed);
                return;
            }
            Err(_e) => {
                halt.store(true, Ordering::Relaxed);
                return;
            }
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
                                halt.store(true, Ordering::Relaxed);
                                return;
                            }

                            remaining
                        }
                    };
                }
            }
        }
    }
}

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
