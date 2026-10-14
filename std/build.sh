#!/bin/sh
# std/build.sh: rebuild std/kernels/std.wasm from std/kernels-src (PLAT-5).
#   std/build.sh           rebuild it in place (into CARGO_TARGET_DIR when it is set)
#   std/build.sh --check   rebuild a copy in a temporary folder and compare it with the committed file
# The bytes depend only on the sources and the toolchain rust-toolchain.toml pins: m2-kernel is a
# member of the kernels' workspace, so no absolute path reaches the build.
set -e
STD=$(cd "$(dirname "$0")" && pwd)
build() { # <std folder> <target folder>: build, then print the wasm's path
  # cargo runs in kernels-src, so a relative target folder (CARGO_TARGET_DIR, TMPDIR) is taken from here first
  case $2 in /*) t=$2 ;; *) t=$PWD/$2 ;; esac
  (cd "$1/kernels-src" && CARGO_TARGET_DIR=$t cargo build --locked --release --target wasm32-unknown-unknown) >&2 || return 1
  echo "$t/wasm32-unknown-unknown/release/m2_std_kernels.wasm"
}
if [ "${1:-}" = --check ]; then
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/m2-std.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT
  # std/ and the pinned toolchain's file, without a local target folder (BSD and GNU tar both take --exclude)
  (cd "$STD/.." && tar --exclude std/kernels-src/target -cf - std rust-toolchain.toml) | (cd "$tmp" && tar -xf -)
  wasm=$(build "$tmp/std" "$tmp/target") || { echo "std.wasm: the rebuild in another folder failed (above)"; exit 1; }
  if grep -q -e /Users/ -e /home/ "$wasm"; then echo "std.wasm: the rebuild holds a machine path"; exit 1; fi
  if ! cmp "$wasm" "$STD/kernels/std.wasm"; then echo "std.wasm: a rebuild differs from the committed file"; exit 1; fi
  echo "std.wasm: the same bytes, rebuilt in $tmp"
  exit 0
fi
wasm=$(build "$STD" "${CARGO_TARGET_DIR:-$STD/kernels-src/target}")
cp "$wasm" "$STD/kernels/std.wasm"
ls -l "$STD/kernels/std.wasm"
