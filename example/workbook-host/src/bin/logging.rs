use postcard_rpc::standard_icd::LoggingTopic;
use workbook_host_client::client::WorkbookClient;

#[tokio::main]
async fn main() {
    println!("Connecting to USB device...");
    let client = WorkbookClient::new();
    println!("Connected! Pinging 42");
    let ping = client.ping(42).await.unwrap();
    println!("Got: {ping}.");
    println!();

    let mut logsub = client.client.subscribe::<LoggingTopic>(64).await.unwrap();

    while let Some(msg) = logsub.recv().await {
        println!("LOG: {msg}");
    }
    println!("Device disconnected");
}
