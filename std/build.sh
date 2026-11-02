#!/bin/sh
# std/build.sh: build every kernel module music2 ships from its source (ABI-10, PLAT-5): std's,
# std/kernels/std.wasm, and that of each of music2's own packs with kernels, into the file its
# m2pkg.toml names (`wasm = "kernels/effects.wasm"`).
#   std/build.sh           build them all in place (into CARGO_TARGET_DIR when it is set)
#   std/build.sh --check   build a copy in a temporary folder and compare each with the committed file
# They are one Cargo workspace, std/kernels-src: the kit (kernel/), the blocks they share (dsp/),
# std's crate, and a crate for each pack, packs/<owner>/<name>/, whose source is beside the pack
# (packs/@m2/effects/kernels-src). Every manifest is in that folder, so cargo names every crate, and
# every file, by a path relative to it: the bytes depend only on the sources and the toolchain
# rust-toolchain.toml pins, and a build in any folder is the same. Nothing overrides the flags in
# std/kernels-src/.cargo/config.toml (the kernels' 64 KiB stack). A crate's module is named after it:
# m2-effects-kernels makes m2_effects_kernels.wasm.
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
KS=std/kernels-src

# packages: the folder of each package with kernels to build, from the repo: std, then each pack
# whose crate is in the workspace.
packages() {
  echo std
  (cd "$ROOT/$KS" && for c in packs/*/*/Cargo.toml; do if [ -f "$c" ]; then dirname "$c"; fi; done)
}
# crate <package>: its crate's folder, from the repo.
crate() { if [ "$1" = std ]; then echo $KS; else echo "$KS/$1"; fi; }
# wasm <package>: its module's path in it, as its m2pkg.toml names it (PACK-2).
wasm() { sed -n 's/^wasm = "\(.*\)"$/\1/p' "$ROOT/$1/m2pkg.toml" 2> /dev/null | head -1; }
# module <package>: the file cargo makes of its crate.
module() { echo "$(sed -n 's/^name = "\(.*\)"$/\1/p' "$ROOT/$(crate "$1")/Cargo.toml" | head -1 | tr - _).wasm"; }
# build <repo> <target folder>: build the workspace of the repo there.
build() {
  # cargo runs in kernels-src, so a relative target folder (CARGO_TARGET_DIR, TMPDIR) is taken from here first
  case $2 in /*) t=$2 ;; *) t=$PWD/$2 ;; esac
  (cd "$1/$KS" && CARGO_TARGET_DIR=$t cargo build --locked --release --target wasm32-unknown-unknown --workspace) >&2
}

# Every module music2 ships is built here, from source beside its package (ABI-10).
for p in $(packages); do
  [ -n "$(wasm "$p")" ] || { echo "$p: its crate builds a module, but its m2pkg.toml names no wasm"; exit 1; }
  if [ "$p" != std ]; then
    [ -d "$ROOT/$p/kernels-src" ] || { echo "$p: its crate's source, $p/kernels-src, isn't there"; exit 1; }
    grep -q "^path = \"../../../../../$p/kernels-src/" "$ROOT/$(crate "$p")/Cargo.toml" ||
      { echo "$p: its crate, $(crate "$p")/Cargo.toml, doesn't build from $p/kernels-src"; exit 1; }
  fi
done
for m in "$ROOT"/packs/*/*/m2pkg.toml; do
  p=${m#"$ROOT/"}
  p=${p%/m2pkg.toml}
  w=$(wasm "$p")
  if [ -n "$w" ] && [ ! -f "$ROOT/$(crate "$p")/Cargo.toml" ]; then
    echo "$p: its m2pkg.toml names $w, but no crate of the kernels' workspace builds it ($(crate "$p")/Cargo.toml)"
    exit 1
  fi
done

if [ "${1:-}" = --check ]; then
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/m2-wasm.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT
  # The kernels' workspace, each pack's source and the pinned toolchain's file, without a target
  # folder (BSD and GNU tar both take --exclude)
  (cd "$ROOT" && tar --exclude "$KS/target" -cf - rust-toolchain.toml $KS \
    $(for p in $(packages); do [ "$p" = std ] || echo "$p/kernels-src"; done)) | (cd "$tmp" && tar -xf -)
  build "$tmp" "$tmp/target" || { echo "wasm: the rebuild in another folder failed (above)"; exit 1; }
  for p in $(packages); do
    made=$tmp/target/wasm32-unknown-unknown/release/$(module "$p")
    if grep -q -e /Users/ -e /home/ -e "$tmp" "$made"; then echo "$p/$(wasm "$p"): the rebuild holds a machine path"; exit 1; fi
    if ! cmp "$made" "$ROOT/$p/$(wasm "$p")"; then echo "$p/$(wasm "$p"): a rebuild differs from the committed file"; exit 1; fi
  done
  echo "wasm: $(packages | tr '\n' ' ')the same bytes, rebuilt in $tmp"
  exit 0
fi
target=${CARGO_TARGET_DIR:-$ROOT/$KS/target}
build "$ROOT" "$target"
for p in $(packages); do
  mkdir -p "$(dirname "$ROOT/$p/$(wasm "$p")")"
  cp "$target/wasm32-unknown-unknown/release/$(module "$p")" "$ROOT/$p/$(wasm "$p")"
  ls -l "$ROOT/$p/$(wasm "$p")"
done
