#!/bin/bash
# crates/gates/s/build.sh [cache folder]: build S's runner for G7f (RENDER-32), and print its path on
# the last line.
#
# S is HEAD 8aa301e, the commit before step 7. This takes a clean copy of it from git (git archive,
# so the repo and .git are left as they are), adds this folder's runner to S's m2-gates as an
# example, and builds that in release with S's own Cargo.lock (--locked; --offline first, when every
# crate is already downloaded) and the toolchain rust-toolchain.toml names. The copy and its build
# live in the cache folder, outside the repo: by default ~/m2-check/s-8aa301e on a Mac and
# ~/.cache/m2-check/s-8aa301e on Linux. The copy is taken again only when the runner's code changes.
# S builds as it is on macOS and Linux: no patch.
#
# Then: M2_GATES_S=<that path> m2-gates store|arena (ci/check.sh does both). Written for bash 3.2.

set -u -o pipefail
S=8aa301e
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
ROOT=$(cd "$HERE/../../.." && pwd) || exit 2
if [ $# -gt 0 ]; then
  CACHE=$1
elif [ "$(uname -s)" = Darwin ]; then
  CACHE=$HOME/m2-check/s-$S
else
  CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/m2-check/s-$S
fi
fail() { echo "build.sh: $*" >&2; exit 1; }
mkdir -p "$CACHE" || fail "can't make $CACHE"
CACHE=$(cd "$CACHE" && pwd)
case $CACHE/ in "$ROOT"/*) fail "the cache folder must be outside the repo ($ROOT)" ;; esac
TOOLCHAIN=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")
[ -n "$TOOLCHAIN" ] || fail "no channel in $ROOT/rust-toolchain.toml"

# The copy: S's tree, with this folder as the example s_runner. What it was made from names it.
stamp=$( { echo "$S $TOOLCHAIN"; cat "$HERE/main.rs" "$HERE/wire.rs"; } | shasum -a 256 | cut -c1-16)
if [ "$(cat "$CACHE/src.stamp" 2> /dev/null)" != "$stamp" ]; then
  git -C "$ROOT" cat-file -e "$S^{commit}" 2> /dev/null || fail "this clone doesn't have commit $S (git fetch, then try again)"
  echo "taking a clean copy of S ($S) into $CACHE/src" >&2
  rm -rf "$CACHE/src.new" "$CACHE/src.stamp" && mkdir -p "$CACHE/src.new" || fail "can't write to $CACHE"
  # git archive keeps each file's commit time, so cargo rebuilds nothing of S's own the next time.
  git -C "$ROOT" archive "$S" | tar -x -C "$CACHE/src.new" || fail "git archive $S failed"
  mkdir -p "$CACHE/src.new/crates/gates/examples/s_runner" &&
    cp "$HERE/main.rs" "$HERE/wire.rs" "$CACHE/src.new/crates/gates/examples/s_runner/" || fail "can't copy the runner"
  rm -rf "$CACHE/src" && mv "$CACHE/src.new" "$CACHE/src" && echo "$stamp" > "$CACHE/src.stamp" || fail "can't write to $CACHE"
fi

# S's own lock file, the pinned toolchain, and a target folder of its own; offline when it can be.
cd "$CACHE/src" || exit 1
build() {
  env RUSTUP_TOOLCHAIN="$TOOLCHAIN" CARGO_TARGET_DIR="$CACHE/target" M2_S_COMMIT=$S \
    cargo build --locked "$@" --release -p m2-gates --example s_runner >&2
}
echo "building S's runner with Rust $TOOLCHAIN (cargo's output follows)" >&2
build --offline || { echo "building offline failed: once more, downloading what is missing" >&2; build; } || fail "S's runner didn't build"
BIN=$CACHE/target/release/examples/s_runner
[ -x "$BIN" ] || fail "no $BIN after the build"
echo "$BIN"
