#!/usr/bin/env bash
set -euxo pipefail

# These tests don't run on Windows, idk.

# Run stub test checks
cargo test \
    --manifest-path source/postcard-rpc/Cargo.toml \
    --features=embassy-usb-0_5-server
