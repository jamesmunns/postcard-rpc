use embassy_time::Duration;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender};

use pd_core::accumulator::{CobsAccumulator, FeedResult};
use james_icd::{Sleep, SleepDone};
use pd_core::{Dispatch, Key, WireHeader};
use static_cell::StaticCell;

use crate::usb::{Disconnected, OtgDriver};

struct SendContents {
    tx: Sender<'static, OtgDriver>,
    scratch: [u8; 128],
}

#[derive(Clone)]
struct Context {
    send: &'static Mutex<ThreadModeRawMutex, SendContents>,
    spawner: Spawner,
    sleep_done_key: Key,
}

enum CommsError {
    Oops,
}

const SLEEP_PATH: &str = "sleep";
static SENDER: StaticCell<Mutex<ThreadModeRawMutex, SendContents>> = StaticCell::new();

#[embassy_executor::task]
pub async fn comms_task(class: CdcAcmClass<'static, OtgDriver>) {
    let (tx, mut rx) = class.split();
    let mut in_buf = [0u8; 128];
    let mut acc = CobsAccumulator::<128>::new();
    let send = SENDER.init(Mutex::new(SendContents {
        tx,
        scratch: [0u8; 128],
    }));

    // Pre-hash keys for responses
    let sleep_key = Key::for_path::<Sleep>(SLEEP_PATH);
    info!("sleep_key: {:?}", sleep_key);
    let sleep_done_key = Key::for_path::<SleepDone>(SLEEP_PATH);

    let mut dispatch = Dispatch::<Context, CommsError, 8>::new(Context {
        send,
        spawner: Spawner::for_current_executor().await,
        sleep_done_key,
    });
    dispatch
        .add_handler::<Sleep>(SLEEP_PATH, sleep_handler)
        .unwrap();

    loop {
        rx.wait_connection().await;

        info!("Connected");
        let _ = incoming(&mut rx, &mut in_buf, &mut acc, &mut dispatch).await;
        info!("Disconnected");
    }
}

async fn incoming(
    rx: &mut Receiver<'static, OtgDriver>,
    buf: &mut [u8],
    acc: &mut CobsAccumulator<128>,
    disp: &mut Dispatch<Context, CommsError, 8>,
) -> Result<(), Disconnected> {
    loop {
        let ct = rx.read_packet(buf).await?;
        info!("got frame");

        let mut window = &buf[..ct];

        'cobs: while !window.is_empty() {
            window = match acc.feed(window) {
                FeedResult::Consumed => break 'cobs,
                FeedResult::OverFull(new_wind) => new_wind,
                FeedResult::DeserError(new_wind) => new_wind,
                FeedResult::Success { data, remaining } => {
                    info!("decobsed! {:?}", data);
                    match disp.dispatch(data) {
                        Ok(_) => info!("good disp!"),
                        Err(e) => match e {
                            pd_core::Error::NoMatchingHandler => info!("NMH"),
                            pd_core::Error::DispatchFailure(_) => info!("DF"),
                            pd_core::Error::Postcard(_) => info!("PC"),
                        },
                    }
                    remaining
                }
            };
        }
    }
}

fn sleep_handler(hdr: &WireHeader, c: &mut Context, bytes: &[u8]) -> Result<(), CommsError> {
    info!("dispatching sleep...");
    let new_c = c.clone();
    if let Ok(msg) = postcard::from_bytes::<Sleep>(bytes) {
        if c.spawner.spawn(sleep_task(hdr.seq_no, new_c, msg)).is_ok() {
            Ok(())
        } else {
            Err(CommsError::Oops)
        }
    } else {
        warn!("Out of senders!");
        Err(CommsError::Oops)
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn sleep_task(seq_no: u32, c: Context, s: Sleep) {
    info!("Sleep spawned");
    Timer::after(Duration::from_secs(s.seconds.into())).await;
    Timer::after(Duration::from_micros(s.micros.into())).await;
    info!("Sleep complete");
    let SendContents {
        ref mut tx,
        ref mut scratch,
    } = &mut *c.send.lock().await;
    let msg = SleepDone { slept_for: s };
    if let Ok(used) = pd_core::headered::to_slice_cobs_keyed(seq_no, c.sleep_done_key, &msg, scratch) {
        let max: usize = tx.max_packet_size().into();
        for ch in used.chunks(max - 1) {
            if tx.write_packet(ch).await.is_err() {
                break;
            }
        }
    }
}
