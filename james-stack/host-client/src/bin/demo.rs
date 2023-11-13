use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use host_client::{io_thread, serial, HostClient};
use james_icd::{FatalError, Sleep, SleepDone};
use pd_core::{headered::to_slice_cobs, Dispatch, WireHeader};
use tokio::time::sleep;

struct Context {}

#[derive(Debug)]
enum CommsError {}

const SLEEP_PATH: &str = "sleep";
const ERROR_PATH: &str = "error";

fn sleep_resp_handler(hdr: &WireHeader, _c: &mut Context, buf: &[u8]) -> Result<(), CommsError> {
    match postcard::from_bytes::<SleepDone>(buf) {
        Ok(m) => println!(" -> Got({}:{:?}): {m:?}", hdr.seq_no, hdr.key),
        Err(_) => println!("sleep done fail"),
    }
    Ok(())
}

fn error_resp_handler(hdr: &WireHeader, _c: &mut Context, buf: &[u8]) -> Result<(), CommsError> {
    match postcard::from_bytes::<FatalError>(buf) {
        Ok(m) => println!(" -> Got({}:{:?}): {m:?}", hdr.seq_no, hdr.key),
        Err(_) => println!("sleep done fail"),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let client = HostClient::new("/dev/tty.usbmodem123456781");

    for i in 0..5 {
        tokio::spawn({
            let client = client.clone();
            async move {
                let mut ttl = 0;
                let mut win = 0;
                loop {
                    ttl += 1;
                    let msg = Sleep {
                        seconds: 3,
                        micros: 500_000,
                    };
                    println!("task {i} sending sleep");
                    let res = client.send_resp::<Sleep, SleepDone>("sleep", msg).await;
                    if res.is_ok() {
                        win += 1;
                    }
                    println!("task {i} ({win}/{ttl}) got {res:?}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
