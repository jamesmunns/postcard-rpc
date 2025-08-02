# ESP32C6 example

This example uses the USB Serial/JTAG peripheral built into ESP32 microcontrollers, to demonstrate
compatibility with `embedded-io-async` trait implementations.

The example was built for an ESP32-C6-DevKitC-1, which has a single built-in RGB LED on GPIO8. The
dev kit does not include an accelerometer so that part is not implemented.

The example is meant to work with the `comms-serial` workbook client binary.

Make sure `probe-rs` is installed. We use JTAG to program the MCU, so that `cargo run` can be used
to monitor execution, while at the same time the workbook can talk to the USB CDC endpoint over the
same USB cable.
