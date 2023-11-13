use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub use james_icd as icd;
use pd_core::accumulator::raw::{CobsAccumulator, FeedResult};
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
            port.write_all(&out).unwrap();
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
