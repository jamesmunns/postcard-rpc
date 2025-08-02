use std::time::Duration;

use postcard_rpc::{header::VarSeqKind, host_client::HostClient, standard_icd::{PingEndpoint, WireError}};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    use tracing_subscriber;
    tracing_subscriber::fmt::init();

    let dev = "/dev/tty.usbmodem0006830188961";

    println!("Connecting to {dev}...");

    let client = HostClient::<WireError>::new_serial_cobs(
        dev,
        "error",
        64,
        115_200,
        VarSeqKind::Seq2,
    );

    println!("Connected :)");

    for _ in 0..3 {
        let val = client.send_resp::<PingEndpoint>(&123).await.unwrap();
        println!("Ping said {val}");
        sleep(Duration::from_secs(1)).await;
    }
}
