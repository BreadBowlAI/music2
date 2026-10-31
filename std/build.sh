#!/bin/sh
# std/build.sh: build every kernel module music2 ships from its source (ABI-10, PLAT-5): std's,
# std/kernels/std.wasm from std/kernels-src, and that of each of music2's own packs with kernels,
# from its kernels-src folder (packs/@m2/effects/kernels-src) into the file its m2pkg.toml names
# (`wasm = "kernels/effects.wasm"`).
#   std/build.sh           build them all in place (into CARGO_TARGET_DIR when it is set)
#   std/build.sh --check   build a copy in a temporary folder and compare each with the committed file
# They are one Cargo workspace, std/kernels-src: the kit (kernel/), std's crate, and every pack's
# crate (packs/*/*/kernels-src, which joins it by `package.workspace`). A crate's module is named
# after it: m2-effects-kernels makes m2_effects_kernels.wasm. The bytes depend only on the sources
# and the toolchain rust-toolchain.toml pins: every path reaches the build relative to the repo
# (--remap-path-prefix; a pack's crate is outside the workspace's folder, so cargo names it whole),
# so a build in any folder is the same.
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)

# packages: the folder of each package with kernels to build, from the repo: std, then the packs.
packages() {
  (cd "$ROOT" && for d in std packs/*/*; do if [ -f "$d/kernels-src/Cargo.toml" ]; then echo "$d"; fi; done)
}
# wasm <package>: its module's path in it, as its m2pkg.toml names it (PACK-2).
wasm() { sed -n 's/^wasm = "\(.*\)"$/\1/p' "$ROOT/$1/m2pkg.toml" | head -1; }
# module <package>: the file cargo makes of its crate.
module() { echo "$(sed -n 's/^name = "\(.*\)"$/\1/p' "$ROOT/$1/kernels-src/Cargo.toml" | head -1 | tr - _).wasm"; }
# build <repo> <target folder>: build the workspace of the repo there.
build() {
  # cargo runs in kernels-src, so a relative target folder (CARGO_TARGET_DIR, TMPDIR) is taken from here first
  case $2 in /*) t=$2 ;; *) t=$PWD/$2 ;; esac
  (cd "$1/std/kernels-src" && CARGO_TARGET_DIR=$t CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$1/=" \
    cargo build --locked --release --target wasm32-unknown-unknown --workspace) >&2
}
for p in $(packages); do
  if [ -z "$(wasm "$p")" ]; then echo "$p: it has kernels-src but its m2pkg.toml names no wasm"; exit 1; fi
done
if [ "${1:-}" = --check ]; then
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/m2-wasm.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT
  # The kernels' sources and the pinned toolchain's file, without a target folder (BSD and GNU tar
  # both take --exclude)
  (cd "$ROOT" && tar --exclude 'std/kernels-src/target' --exclude 'packs/*/*/kernels-src/target' -cf - \
    rust-toolchain.toml $(for p in $(packages); do echo "$p/kernels-src"; done)) | (cd "$tmp" && tar -xf -)
  build "$tmp" "$tmp/target" || { echo "wasm: the rebuild in another folder failed (above)"; exit 1; }
  for p in $(packages); do
    made=$tmp/target/wasm32-unknown-unknown/release/$(module "$p")
    if grep -q -e /Users/ -e /home/ -e "$tmp" "$made"; then echo "$p/$(wasm "$p"): the rebuild holds a machine path"; exit 1; fi
    if ! cmp "$made" "$ROOT/$p/$(wasm "$p")"; then echo "$p/$(wasm "$p"): a rebuild differs from the committed file"; exit 1; fi
  done
  echo "wasm: $(packages | tr '\n' ' ')the same bytes, rebuilt in $tmp"
  exit 0
fi
target=${CARGO_TARGET_DIR:-$ROOT/std/kernels-src/target}
build "$ROOT" "$target"
for p in $(packages); do
  mkdir -p "$(dirname "$ROOT/$p/$(wasm "$p")")"
  cp "$target/wasm32-unknown-unknown/release/$(module "$p")" "$ROOT/$p/$(wasm "$p")"
  ls -l "$ROOT/$p/$(wasm "$p")"
done
