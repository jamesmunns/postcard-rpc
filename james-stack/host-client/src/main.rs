use std::time::Duration;

use post_dispatch::host_client::HostClient;
use james_icd::{sleep::{Sleep, SleepDone, SLEEP_PATH}, wire_error::{FatalError, ERROR_PATH}};

#[tokio::main]
async fn main() {
    let client = HostClient::<FatalError>::new(
        "/dev/tty.usbmodem123456781",
        ERROR_PATH,
    );

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
                    let res = client.send_resp::<Sleep, SleepDone>(SLEEP_PATH, msg).await;

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
