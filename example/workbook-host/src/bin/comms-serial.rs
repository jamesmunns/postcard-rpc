use std::{
    io::{stdout, Write},
    time::{Duration, Instant},
};

use workbook_host_client::{client::WorkbookClient, icd, read_line};

#[tokio::main]
async fn main() {
    let mut args = std::env::args_os();
    let _ = args.next(); // skip the program name
    let Some(port) = args.next() else {
        println!("Usage: cargo run --bin comms-serial <port>");
        println!("Available ports:");
        for port in tokio_serial::available_ports().unwrap() {
            println!(" * {}", port.port_name)
        }
        return;
    };
    let port = port.to_str().unwrap();

    println!("Connecting to USB device...");
    let client = WorkbookClient::new_serial(port);
    println!("Connected! Pinging 42");
    let ping = client.ping(42).await.unwrap();
    println!("Got: {ping}.");
    let uid = client.get_id().await.unwrap();
    println!("ID: {uid:016X}");
    println!();

    // Begin repl...
    loop {
        print!("> ");
        stdout().flush().unwrap();
        let line = read_line().await;
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["ping"] => {
                let ping = client.ping(42).await.unwrap();
                println!("Got: {ping}.");
            }
            ["ping", n] => {
                let Ok(idx) = n.parse::<u32>() else {
                    println!("Bad u32: '{n}'");
                    continue;
                };
                let ping = client.ping(idx).await.unwrap();
                println!("Got: {ping}.");
            }
            ["rgb", pos, r, g, b] => {
                let (Ok(pos), Ok(r), Ok(g), Ok(b)) = (pos.parse(), r.parse(), g.parse(), b.parse())
                else {
                    panic!();
                };
                client.set_rgb_single(pos, r, g, b).await.unwrap();
            }
            ["rgball", r, g, b] => {
                let (Ok(r), Ok(g), Ok(b)) = (r.parse(), g.parse(), b.parse()) else {
                    panic!();
                };
                client.set_all_rgb_single(r, g, b).await.unwrap();
            }
            ["accel", "listen", ms, range, dur] => {
                let Ok(ms) = ms.parse::<u32>() else {
                    println!("Bad ms: {ms}");
                    continue;
                };
                let Ok(dur) = dur.parse::<u32>() else {
                    println!("Bad dur: {dur}");
                    continue;
                };
                let range = match *range {
                    "2" => icd::AccelRange::G2,
                    "4" => icd::AccelRange::G4,
                    "8" => icd::AccelRange::G8,
                    "16" => icd::AccelRange::G16,
                    _ => {
                        println!("Bad range: {range}");
                        continue;
                    }
                };

                let mut sub = client
                    .client
                    .subscribe_multi::<icd::AccelTopic>(8)
                    .await
                    .unwrap();
                client.start_accelerometer(ms, range).await.unwrap();
                println!("Started!");
                let dur = Duration::from_millis(dur.into());
                let start = Instant::now();
                while start.elapsed() < dur {
                    let val = sub.recv().await.unwrap();
                    println!("acc: {val:?}");
                }
                client.stop_accelerometer().await.unwrap();
                println!("Stopped!");
            }
            ["accel", "start", ms, range] => {
                let Ok(ms) = ms.parse::<u32>() else {
                    println!("Bad ms: {ms}");
                    continue;
                };
                let range = match *range {
                    "2" => icd::AccelRange::G2,
                    "4" => icd::AccelRange::G4,
                    "8" => icd::AccelRange::G8,
                    "16" => icd::AccelRange::G16,
                    _ => {
                        println!("Bad range: {range}");
                        continue;
                    }
                };

                client.start_accelerometer(ms, range).await.unwrap();
                println!("Started!");
            }
            ["accel", "stop"] => {
                let res = client.stop_accelerometer().await.unwrap();
                println!("Stopped: {res}");
            }
            ["schema"] => {
                let schema = client.client.get_schema_report().await.unwrap();

                println!();
                println!("# Endpoints");
                println!();
                for ep in &schema.endpoints {
                    println!("* '{}'", ep.path);
                    println!("  * Request:  {}", ep.req_ty);
                    println!("  * Response: {}", ep.resp_ty);
                }

                println!();
                println!("# Topics Client -> Server");
                println!();
                for tp in &schema.topics_in {
                    println!("* '{}'", tp.path);
                    println!("  * Message: {}", tp.ty);
                }

                println!();
                println!("# Topics Client <- Server");
                println!();
                for tp in &schema.topics_out {
                    println!("* '{}'", tp.path);
                    println!("  * Message: {}", tp.ty);
                }
                println!();
            }
            other => {
                println!("Error, didn't understand '{other:?};");
            }
        }
    }
}
