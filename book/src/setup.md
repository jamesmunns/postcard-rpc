# Setup

Prior to the workshop, you'll need to install a few things.

**Please do and check these BEFORE you come to the workshop, in case the internet is slow!**

If you have any questions prior to the workshop, please contact [contact@onevariable.com](mailto:contact@onevariable.com)
for assistance.

## Rust

You'll want to install Rust, ideally using `rustup`, not using your operating system's package manager.

You can follow the instructions here:

<https://www.rust-lang.org/tools/install>

## Rust Toolchain Components

You'll want to make sure you are on the newest stable Rust version. We'll be using `1.77.2`.

You can do this with:

```sh
rustup update stable
rustup default stable
```

You'll also want to add a couple of additional pieces:

```sh
rustup component add llvm-tools
rustup target add thumbv6m-none-eabi
```

## probe-rs

We'll use `probe-rs` for debugging the board during the workshop.

You can follow the instructions here:

<https://probe.rs/docs/getting-started/installation/>


## USB permissions

You may need to set up USB drivers or permissions for both the probe, as well as the USB device.

We recommend following the steps listed here: <https://probe.rs/docs/getting-started/probe-setup/>. If you've used `probe-rs` before, you are probably already fine.

There are also instructions listed on the `nusb` docs page: <https://docs.rs/nusb/latest/nusb/#platform-support>. You may need to add permissions rules for:

* Vendor ID: 0x16c0
* Product ID: 0x27DD

## USB Cabling

The training device will require a single USB port on your computer. You will need a cable that allows you to connect to a USB-C device.

Depending on your computer, you will need either a USB A-to-C or USB C-to-C cable. We will have some spares, but please bring one if you can.
