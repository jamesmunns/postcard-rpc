use std::{time::Duration, sync::{Arc, atomic::AtomicBool}, hash::{Hash, Hasher}};

use host_client::{serial, io_thread};
use james_icd::{SleepDone, Sleep};
use pd_core::{Dispatch, WireHeader, headered::{to_slice_cobs, to_slice_cobs_keyed, to_slice}, Key};
use postcard::experimental::schema::Schema;
use tokio::time::sleep;

struct Context {

}

#[derive(Debug)]
enum CommsError {

}

const SLEEP_PATH: &str = "sleep";

fn sleep_resp_handler(hdr: &WireHeader, _c: &mut Context, buf: &[u8]) -> Result<(), CommsError> {
    match postcard::from_bytes::<SleepDone>(buf) {
        Ok(m) => println!("Got({}:{:?}): {m:?}", hdr.seq_no, hdr.key),
        Err(_) => println!("sleep done fail"),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let (tx_pc, rx_pc) = tokio::sync::mpsc::channel(8);
    let (tx_fw, rx_fw) = tokio::sync::mpsc::channel(8);

    let port = serial::new("/dev/tty.usbmodem123456781", 115_200)
        .timeout(Duration::from_millis(10))
        .open()
        .unwrap();

    let halt = Arc::new(AtomicBool::new(false));

    let _jh = Some(std::thread::spawn({
        let halt = halt.clone();
        move || io_thread(port, tx_fw, rx_pc, halt)
    }));

    tokio::task::spawn(async move {
        let mut rx_fw = rx_fw;
        let mut dispatch = Dispatch::<Context, CommsError, 8>::new(Context { });
        dispatch
            .add_handler::<SleepDone>(SLEEP_PATH, sleep_resp_handler)
            .unwrap();

        loop {
            let msg = rx_fw.recv().await.unwrap();
            dispatch.dispatch(&msg).unwrap();
        }
    });

    let mut ctr = 0;
    loop {
        let mut buf = [0u8; 128];
        let msg = Sleep { seconds: 3, micros: 500_000 };
        let used = to_slice_cobs(ctr, SLEEP_PATH, &msg, &mut buf).unwrap();
        ctr += 1;
        tx_pc.send(used.to_vec()).await.unwrap();
        sleep(Duration::from_secs(1)).await;
    }
}
