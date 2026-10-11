#!/bin/sh
# Rebuild std/kernels/std.wasm from std/kernels-src.
set -e
cd "$(dirname "$0")/kernels-src"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/m2_std_kernels.wasm ../kernels/std.wasm
ls -l ../kernels/std.wasm
