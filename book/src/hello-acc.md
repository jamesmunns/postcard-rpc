# Accelerometer

Our last sensor is a 3-axis Accelerometer. It has many more features than we'll use during the
exercise, but it can read acceleration in three axis: X, Y, and Z.

It reads accleration, e.g. due to gravity, as a positive number. It can measure up to 8g of
acceleration, returning `i16::MAX` for 8.0g, or `4096` as 1.0g, or `-4096` for -1.0g.

If the board is sitting level, it should read approximately:

* x: 0
* y: 0
* z: 4096

If you tilt the board so the potentometer is facing RIGHT, it should read approximately:

* x: 4096
* y: 0
* z: 0

If you tilt the board so the potentiometer is facing AWAY from you, it should read approximately:

* x: 0
* y: 4096
* z: 0

## Running the code

We can start the code by running the `hello-03` project. It will begin immediately printing out
acceleration values at 4Hz.

```sh
cargo run --release --bin hello-03
    Finished release [optimized + debuginfo] target(s) in 0.10s
     Running `probe-rs run --chip RP2040 --speed 12000 --protocol swd target/thumbv6m-none-eabi/release/hello-03`
      Erasing ✔ [00:00:00] [######################################################] 40.00 KiB/40.00 KiB @ 75.76 KiB/s (eta 0s )
  Programming ✔ [00:00:00] [#####################################################] 40.00 KiB/40.00 KiB @ 132.75 KiB/s (eta 0s )    Finished in 0.841s
 WARN defmt_decoder::log::format: logger format contains timestamp but no timestamp implementation was provided; consider removing the timestamp (`{t}` or `{T}`) from the logger format or provide a `defmt::timestamp!` implementation
0.000976 INFO  Start
└─ hello_03::____embassy_main_task::{async_fn#0} @ src/bin/hello-03.rs:38
0.002457 INFO  id: E4629076D3222C21
└─ hello_03::____embassy_main_task::{async_fn#0} @ src/bin/hello-03.rs:42
0.253936 INFO  accelerometer: AccelReading { x: -80, y: 0, z: 4064 }
└─ hello_03::__accel_task_task::{async_fn#0} @ src/bin/hello-03.rs:82
0.503916 INFO  accelerometer: AccelReading { x: -112, y: -16, z: 4048 }
└─ hello_03::__accel_task_task::{async_fn#0} @ src/bin/hello-03.rs:82
0.753916 INFO  accelerometer: AccelReading { x: -112, y: 16, z: 4048 }
└─ hello_03::__accel_task_task::{async_fn#0} @ src/bin/hello-03.rs:82
```

## Reading the code

Similar to the other exercises, we've added some new tasks:

```rust
// in main
let accel = Accelerometer::new(
    p.SPI0, p.PIN_6, p.PIN_7, p.PIN_4, p.PIN_5, p.DMA_CH1, p.DMA_CH2,
)
.await;

// as a task
#[embassy_executor::task]
async fn accel_task(mut accel: Accelerometer) {
    let mut ticker = Ticker::every(Duration::from_millis(250));
    loop {
        ticker.next().await;
        let reading = accel.read().await;
        info!("accelerometer: {:?}", reading);
    }
}
```

One thing to note is that the constructor, `Accelerometer::new()` is an `async` function. This is
because the driver establishes the connection, and ensures we are talking to the accelerometer
using async SPI methods.

You can access the raw driver through the [`lis3dh-async` crate](https://docs.rs/lis3dh-async).

We have also wired up the accelerometer's interrupt pins, which can serve as a "notification" when
some event has happened, however we will not use that as part of the exercise today.
