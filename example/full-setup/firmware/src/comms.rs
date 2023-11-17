use embassy_time::Duration;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender};

use james_icd::{
    sleep::{Sleep, SleepDone, SleepEndpoint},
    wire_error::{FatalError, ERROR_KEY},
};
use postcard::experimental::schema::Schema;
use postcard_rpc::{
    accumulator::dispatch::{CobsDispatch, FeedError},
    Key, WireHeader, Endpoint,
};
use serde::Serialize;
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
}

impl Context {
    async fn respond_keyed<T: Serialize + Schema>(&mut self, key: Key, seq_no: u32, msg: &T) {
        // Lock the sender mutex to get access to the outgoing serial port
        // as well as the shared scratch buffer.
        let SendContents {
            ref mut tx,
            ref mut scratch,
        } = &mut *self.send.lock().await;

        if let Ok(used) = postcard_rpc::headered::to_slice_cobs_keyed(seq_no, key, &msg, scratch) {
            let max: usize = tx.max_packet_size().into();
            for ch in used.chunks(max - 1) {
                if tx.write_packet(ch).await.is_err() {
                    break;
                }
            }
        }
    }
}

enum CommsError {
    PoolFull(u32),
    Postcard,
}

static SENDER: StaticCell<Mutex<ThreadModeRawMutex, SendContents>> = StaticCell::new();

#[embassy_executor::task]
pub async fn comms_task(class: CdcAcmClass<'static, OtgDriver>) {
    let (tx, mut rx) = class.split();
    let mut in_buf = [0u8; 128];
    let send = SENDER.init(Mutex::new(SendContents {
        tx,
        scratch: [0u8; 128],
    }));

    let mut cobs_dispatch = CobsDispatch::<Context, CommsError, 8, 128>::new(Context {
        send,
        spawner: Spawner::for_current_executor().await,
    });
    cobs_dispatch
        .dispatcher()
        .add_handler::<SleepEndpoint>(sleep_handler)
        .unwrap();

    loop {
        rx.wait_connection().await;

        info!("Connected");
        let _ = incoming(&mut rx, &mut in_buf, &mut cobs_dispatch).await;
        info!("Disconnected");
    }
}

async fn incoming(
    rx: &mut Receiver<'static, OtgDriver>,
    buf: &mut [u8],
    cobs_dispatch: &mut CobsDispatch<Context, CommsError, 8, 128>,
) -> Result<(), Disconnected> {
    loop {
        let ct = rx.read_packet(buf).await?;
        info!("got frame");

        let mut window = &buf[..ct];
        while let Err(FeedError { err, remainder }) = cobs_dispatch.feed(window) {
            let (seq_no, resp) = match err {
                postcard_rpc::Error::NoMatchingHandler { key: _, seq_no } => {
                    info!("NMH");
                    (seq_no, FatalError::UnknownEndpoint)
                }
                postcard_rpc::Error::DispatchFailure(CommsError::PoolFull(seq)) => {
                    info!("DFPF");
                    (seq, FatalError::NotEnoughSenders)
                }
                postcard_rpc::Error::DispatchFailure(CommsError::Postcard) => {
                    info!("PFPo");
                    (0, FatalError::WireFailure)
                }
                postcard_rpc::Error::Postcard(_) => (0, FatalError::WireFailure),
            };
            let context = cobs_dispatch.dispatcher().context();
            context.respond_keyed(ERROR_KEY, seq_no, &resp).await;
            window = remainder;
        }
        info!("done frame");
    }
}

fn sleep_handler(hdr: &WireHeader, c: &mut Context, bytes: &[u8]) -> Result<(), CommsError> {
    info!("dispatching sleep...");
    let new_c = c.clone();
    if let Ok(msg) = postcard::from_bytes::<Sleep>(bytes) {
        if c.spawner.spawn(sleep_task(hdr.seq_no, new_c, msg)).is_ok() {
            Ok(())
        } else {
            Err(CommsError::PoolFull(hdr.seq_no))
        }
    } else {
        warn!("Out of senders!");
        Err(CommsError::Postcard)
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
    if let Ok(used) =
        postcard_rpc::headered::to_slice_cobs_keyed(seq_no, SleepEndpoint::RESP_KEY, &msg, scratch)
    {
        let max: usize = tx.max_packet_size().into();
        for ch in used.chunks(max - 1) {
            if tx.write_packet(ch).await.is_err() {
                break;
            }
        }
    }
}
