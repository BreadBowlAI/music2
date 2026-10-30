#!/bin/bash
# In short: make std's kit again (LIB-8 in docs/architecture.md): std/samples/kit/*.wav, each one
# note of std's own drum machine rendered by music2 itself, cut where it falls 80 dB under its peak
# and brought to a peak of -1 dBFS. The test that writes them here checks them on every other run,
# so a change to std's drums, or to how music2 renders, that changes them fails until this is run.
set -euo pipefail
cd "$(dirname "$0")/../.."
M2_WRITE_KIT=1 cargo test --locked -p m2-editor --lib std_kit_is_made_again_to_the_same_bytes
# Made again from what was just written: the same bytes.
cargo test --locked -p m2-editor --lib std_kit_is_made_again_to_the_same_bytes
