use std::time::Duration;

use host_client::HostClient;
use james_icd::{Sleep, SleepDone};

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
