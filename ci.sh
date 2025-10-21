#!/usr/bin/env bash
set -euxo pipefail

rustup target add \
    thumbv6m-none-eabi \
    thumbv7em-none-eabihf \
    riscv32imac-unknown-none-elf \
    wasm32-unknown-unknown

# formatting
cargo fmt --all --manifest-path source/postcard-rpc/Cargo.toml -- --check
cargo fmt --all --manifest-path example/workbook-host/Cargo.toml -- --check
cargo fmt --all --manifest-path example/serial-host/Cargo.toml -- --check
cargo fmt --all --manifest-path example/firmware/Cargo.toml -- --check
cargo fmt --all --manifest-path example/nrf52840-serial/Cargo.toml -- --check
cargo fmt --all --manifest-path example/esp32c6-serial/Cargo.toml -- --check
cargo fmt --all --manifest-path source/postcard-rpc-test/Cargo.toml -- --check

# Host + STD checks
cargo check \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features
cargo test \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features

# Host + all non-wasm host-client impls
cargo check \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features \
    --features=use-std,cobs-serial,raw-nusb
cargo test \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features \
    --features=use-std,cobs-serial,raw-nusb

# Host + wasm host-client impls
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
    cargo check \
        --manifest-path source/postcard-rpc/Cargo.toml \
        --no-default-features \
        --features=use-std,webusb \
        --target wasm32-unknown-unknown
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
    cargo build \
        --manifest-path source/postcard-rpc/Cargo.toml \
        --no-default-features \
        --features=use-std,webusb \
        --target wasm32-unknown-unknown

# Embedded + embassy server impl
cargo check \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features \
    --features=embassy-usb-0_5-server \
    --target thumbv7em-none-eabihf
cargo check \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features \
    --features=embedded-io-async-0_6-server \
    --target thumbv7em-none-eabihf

# USB gadget (OTG) server impl
cargo check \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --no-default-features \
    --features=use-std,tokio,usb-gadget

# Example projects
cargo build \
    --manifest-path example/workbook-host/Cargo.toml
cargo build \
    --manifest-path example/serial-host/Cargo.toml
# Current (embassy-usb v0.5)
cargo build \
    --manifest-path example/firmware/Cargo.toml \
    --target thumbv6m-none-eabi
# embedded-io support
cargo build \
    --manifest-path example/nrf52840-serial/Cargo.toml \
    --target thumbv7em-none-eabihf
cargo build \
    --manifest-path example/esp32c6-serial/Cargo.toml \
    --target riscv32imac-unknown-none-elf
# USB gadget (OTG)
cargo build \
    --manifest-path example/server-usb-gadget/Cargo.toml

# Test Project
cargo test \
    --manifest-path source/postcard-rpc-test/Cargo.toml
