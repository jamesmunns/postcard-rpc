# Buttons and Potentiometer

## Running the code

We'll move on to the next project, `hello-02`. Let's start by running the project:

```sh
cargo run --release --bin hello-02
    Finished release [optimized + debuginfo] target(s) in 0.10s
     Running `probe-rs run --chip RP2040 --speed 12000 --protocol swd target/thumbv6m-none-eabi/release/hello-02`
      Erasing ✔ [00:00:00] [######################################################] 28.00 KiB/28.00 KiB @ 74.12 KiB/s (eta 0s )
  Programming ✔ [00:00:00] [#####################################################] 28.00 KiB/28.00 KiB @ 115.92 KiB/s (eta 0s )    Finished in 0.631s
 WARN defmt_decoder::log::format: logger format contains timestamp but no timestamp implementation was provided; consider removing the timestamp (`{t}` or `{T}`) from the logger format or provide a `defmt::timestamp!` implementation
0.000962 INFO  Start
└─ hello_02::____embassy_main_task::{async_fn#0} @ src/bin/hello-02.rs:35
0.002448 INFO  id: E4629076D3222C21
└─ hello_02::____embassy_main_task::{async_fn#0} @ src/bin/hello-02.rs:39
```

You can now start pressing buttons, and should see corresponding logs every time you press or
release a one of the eight buttons on the board:

```text
0.732839 INFO  Buttons changed: [false, true, false, false, false, false, false, false]
└─ hello_02::__button_task_task::{async_fn#0} @ src/bin/hello-02.rs:68
1.112836 INFO  Buttons changed: [false, false, false, false, false, false, false, false]
└─ hello_02::__button_task_task::{async_fn#0} @ src/bin/hello-02.rs:68
2.192836 INFO  Buttons changed: [true, false, false, false, false, false, false, false]
└─ hello_02::__button_task_task::{async_fn#0} @ src/bin/hello-02.rs:68
2.562838 INFO  Buttons changed: [false, false, false, false, false, false, false, false]
```

You'll also see the potentiometer value as you turn the dial left and right:

```text
1.602946 INFO  Potentiometer changed: 1922
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
2.702941 INFO  Potentiometer changed: 2666
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
3.802943 INFO  Potentiometer changed: 4088
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
5.602941 INFO  Potentiometer changed: 3149
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
5.702941 INFO  Potentiometer changed: 2904
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
6.402940 INFO  Potentiometer changed: 1621
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
6.502941 INFO  Potentiometer changed: 1444
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
7.002944 INFO  Potentiometer changed: 17
└─ hello_02::__pot_task_task::{async_fn#0} @ src/bin/hello-02.rs:86
```

## Reading the code

We've added a little more code to main:

```rust
let buttons = Buttons::new(
    p.PIN_0, p.PIN_1, p.PIN_2, p.PIN_3, p.PIN_18, p.PIN_19, p.PIN_20, p.PIN_21,
);
let potentiometer = Potentiometer::new(p.ADC, p.PIN_26);

// Start the Button task
spawner.must_spawn(button_task(buttons));

// Start the Potentiometer task
spawner.must_spawn(pot_task(potentiometer));
```

And two new tasks:

```rust
// This is our Button task
#[embassy_executor::task]
async fn button_task(buttons: Buttons) {
    let mut last = [false; Buttons::COUNT];
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        ticker.next().await;
        let now = buttons.read_all();
        if now != last {
            info!("Buttons changed: {:?}", now);
            last = now;
        }
    }
}

// This is our Potentiometer task
#[embassy_executor::task]
async fn pot_task(mut pot: Potentiometer) {
    let mut last = pot.read().await;
    let mut ticker = Ticker::every(Duration::from_millis(100));
    loop {
        ticker.next().await;
        let now = pot.read().await;
        if now.abs_diff(last) > 64 {
            info!("Potentiometer changed: {=u16}", now);
            last = now;
        }
    }
}
```

Both of these store the last state measured, so that we don't flood the logs too much.

Again, you can try to customize these a bit before moving forward.
