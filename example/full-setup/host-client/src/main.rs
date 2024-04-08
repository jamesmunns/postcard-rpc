use std::time::Duration;

use james_icd::{
    sleep::{Sleep, SleepEndpoint},
    wire_error::{FatalError, ERROR_PATH},
};
use postcard_rpc::host_client::HostClient;

#[tokio::main]
async fn main() {
    let client = HostClient::<FatalError>::new_raw_nusb(|d| {
        d.serial_number() == Some("12345678")
    }, ERROR_PATH, 8);

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
                    let res = client.send_resp::<SleepEndpoint>(&msg).await;

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
