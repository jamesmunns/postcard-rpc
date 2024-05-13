# Host side

In `workbook-host/src/client.rs`, there's two important parts we need to look at:

```rust
impl WorkbookClient {
    pub fn new() -> Self {
        let client =
            HostClient::new_raw_nusb(|d| d.product_string() == Some("ov-twin"), ERROR_PATH, 8);
        Self { client }
    }
// ...
}
```

`postcard-rpc` provides a `HostClient` struct that handles the PC side of communication.

Here, we tell it that we want to use the "raw_nusb" transport, which takes a closure it uses
to find the relevant USB device we want to connect to. Here, we just look for the first device
with a product string of "ov-twin", which we configured in the firmware. You might need something
smarter if you expect to have more than one device attached at a time!

`HostClient` allows for custom paths for errors, and allows you to configure the number of
"in flight" requests at once. We don't need to worry about those for now.

We then have a method called `ping`:

```rust
pub async fn ping(&self, id: u32) -> Result<u32, WorkbookError<Infallible>> {
    let val = self.client.send_resp::<PingEndpoint>(&id).await?;
    Ok(val)
}
```

The main method here is `HostClient::send_resp`, which takes the `Endpoint` as a generic argument,
which lets it know that it should take `Request` as an argument, and will return `Result<u32, ...>`.

The `Err` part of the `Result` is a little tricky, but this comes from the fact that errors can come
from three different "layers":

* A USB error, e.g. if the USB device disconnects or crashes
* A `postcard-rpc` "transport" error, e.g. if the device replies "I don't know that Endpoint".
* (optional) if the Response type is `Result<T, E>`, we can "flatten" that error so that if we
  receive a message, but it's an `Err`, we can return that error. See the `FlattenErr` trait for
  how we do this.

Finally, in `workbook-host/src/bin/comms-01.rs`, we have a binary that uses this client:

```rust
#[tokio::main]
pub async fn main() {
    let client = WorkbookClient::new();
    let mut ticker = interval(Duration::from_millis(250));

    for i in 0..10 {
        ticker.tick().await;
        print!("Pinging with {i}... ");
        let res = client.ping(i).await.unwrap();
        println!("got {res}!");
        assert_eq!(res, i);
    }
}
```

Give it a try!

In your firmware terminal, run `cargo run --release --bin comms-01`, and in your host terminal,
run `cargo run --release --bin comms-01` as well.

On the host side, you should see:

```sh
$ cargo run --release --bin comms-01
   Compiling workbook-host-client v0.1.0 (/Users/james/onevariable/ovtwin-fw/source/workbook/workbook-host)
    Finished release [optimized] target(s) in 0.33s
     Running `target/release/comms-01`
Pinging with 0... got 0!
Pinging with 1... got 1!
Pinging with 2... got 2!
Pinging with 3... got 3!
Pinging with 4... got 4!
Pinging with 5... got 5!
Pinging with 6... got 6!
Pinging with 7... got 7!
Pinging with 8... got 8!
Pinging with 9... got 9!
```

On the target side, you should see:

```sh
$ cargo run --release --bin comms-01
    Finished release [optimized + debuginfo] target(s) in 0.09s
     Running `probe-rs run --chip RP2040 --speed 12000 --protocol swd target/thumbv6m-none-eabi/release/comms-01`
      Erasing ✔ [00:00:00] [######################################################] 44.00 KiB/44.00 KiB @ 75.88 KiB/s (eta 0s )
  Programming ✔ [00:00:00] [#####################################################] 44.00 KiB/44.00 KiB @ 126.11 KiB/s (eta 0s )    Finished in 0.94s
 WARN defmt_decoder::log::format: logger format contains timestamp but no timestamp implementation was provided; consider removing the timestamp (`{t}` or `{T}`) from the logger format or provide a `defmt::timestamp!` implementation
0.000985 INFO  Start
└─ comms_01::____embassy_main_task::{async_fn#0} @ src/bin/comms-01.rs:41
0.002458 INFO  id: E4629076D3222C21
└─ comms_01::____embassy_main_task::{async_fn#0} @ src/bin/comms-01.rs:45
0.003023 INFO  USB: config_descriptor used: 40
└─ embassy_usb::builder::{impl#1}::build @ /Users/james/.cargo/git/checkouts/embassy-69e86c528471812c/0d0d8e1/embassy-usb/src/fmt.rs:143
0.003057 INFO  USB: bos_descriptor used: 40
└─ embassy_usb::builder::{impl#1}::build @ /Users/james/.cargo/git/checkouts/embassy-69e86c528471812c/0d0d8e1/embassy-usb/src/fmt.rs:143
0.003081 INFO  USB: msos_descriptor used: 162
└─ embassy_usb::builder::{impl#1}::build @ /Users/james/.cargo/git/checkouts/embassy-69e86c528471812c/0d0d8e1/embassy-usb/src/fmt.rs:143
0.003106 INFO  USB: control_buf size: 64
└─ embassy_usb::builder::{impl#1}::build @ /Users/james/.cargo/git/checkouts/embassy-69e86c528471812c/0d0d8e1/embassy-usb/src/fmt.rs:143
0.444093 DEBUG SET_CONFIGURATION: configured
└─ embassy_usb::{impl#2}::handle_control_out @ /Users/james/.cargo/git/checkouts/embassy-69e86c528471812c/0d0d8e1/embassy-usb/src/fmt.rs:130
260.649991 INFO  ping: seq - 0
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
260.904715 INFO  ping: seq - 1
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
261.154425 INFO  ping: seq - 2
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
261.405078 INFO  ping: seq - 3
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
261.651749 INFO  ping: seq - 4
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
261.900945 INFO  ping: seq - 5
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
262.154443 INFO  ping: seq - 6
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
262.405163 INFO  ping: seq - 7
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
262.653731 INFO  ping: seq - 8
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
262.902596 INFO  ping: seq - 9
└─ comms_01::ping_handler @ src/bin/comms-01.rs:77
```

Hooray! We have [internection]!

[internection]: https://en.wiktionary.org/wiki/internection

