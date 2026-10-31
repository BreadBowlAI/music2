#!/bin/bash
# ci/wasm-test.sh: tries std/build.sh --check (ABI-10: every kernel module music2 ships is its
# source's build, byte for byte) on a table of cases. Each case gets its own copy of std/, packs/
# and the pinned toolchain's file, in a temporary folder outside this one, with one thing changed
# that the check must find: a pack's committed module, a pack's source, std's source, a pack that
# names no module, a bundled module no source builds. The case with nothing changed is the check
# itself, which ci/pr.sh runs in its own row. Then the one property the bytes rest on: every crate of
# the kernels' workspace gets the same `-C metadata` built in two folders of other names (cargo
# hashes a crate's place into it, and its symbols' order with it). ci/pr.sh runs this in its "quick"
# step. Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case
# passed, 1 when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-wasm-test.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT

passed=0
failed=0
n=0
# copy <dir>: the repo's std/, packs/ and the pinned toolchain's file, into <dir>.
copy() {
  mkdir -p "$1" &&
    (cd "$ROOT" && tar --exclude 'std/kernels-src/target' -cf - std packs rust-toolchain.toml) | (cd "$1" && tar -xf -)
}
# case_ <name> <words the check's last line holds> <commands that make the change>: the commands run
# in a copy of the tree; then its std/build.sh --check must fail, its last line holding the words.
case_() {
  n=$((n + 1))
  dir=$TMP/case-$n
  copy "$dir" || exit 2
  if ! (cd "$dir" && eval "$3") > "$TMP/setup.log" 2>&1; then
    failed=$((failed + 1))
    echo "FAIL  $1: setting it up failed: $(head -1 "$TMP/setup.log")"
    return
  fi
  out=$(cd "$dir" && sh std/build.sh --check 2>&1)
  st=$?
  line=$(printf '%s\n' "$out" | tail -1)
  case $line in
    *"$2"*)
      if [ $st -ne 0 ]; then
        passed=$((passed + 1))
        echo "PASS  $1: $line"
        return
      fi ;;
  esac
  failed=$((failed + 1))
  echo "FAIL  $1: wanted a failure saying \"$2\", got status $st: $line"
}

case_ "a pack's committed module, changed" "packs/@m2/effects/kernels/effects.wasm: a rebuild differs from the committed file" \
  'printf "\0" >> packs/@m2/effects/kernels/effects.wasm'
# One more export in a crate: a change its module can't leave out.
export_more='printf "\n#[unsafe(no_mangle)]\npub extern \"C\" fn m2_one_more() -> u32 {\n    7\n}\n" >>'
case_ "a pack's kernel source, changed" "packs/@m2/effects/kernels/effects.wasm: a rebuild differs from the committed file" \
  "$export_more packs/@m2/effects/kernels-src/src/lib.rs"
case_ "std's kernel source, changed" "std/kernels/std.wasm: a rebuild differs from the committed file" \
  "$export_more std/kernels-src/src/lib.rs"
case_ "a pack with kernel source that names no module" "packs/@m2/effects: its crate builds a module, but its m2pkg.toml names no wasm" \
  'sed -i.old "/^wasm = /d" packs/@m2/effects/m2pkg.toml && rm packs/@m2/effects/m2pkg.toml.old'
# A bundled pack that says it has a module, and holds one, but no source builds it.
no_source='m=packs/@m2/drums/m2pkg.toml && { sed -n "1,/^version = /p" $m; echo "wasm = \"kernels/drums.wasm\""; sed "1,/^version = /d" $m; } > $m.new &&
  mv $m.new $m && mkdir -p packs/@m2/drums/kernels && cp packs/@m2/effects/kernels/effects.wasm packs/@m2/drums/kernels/drums.wasm'
case_ "a bundled pack's module with no source" "packs/@m2/drums: its m2pkg.toml names kernels/drums.wasm, but no crate of the kernels' workspace builds it" \
  "$no_source"

# metadata <dir>: each crate of the kernels' workspace in a copy of the tree at <dir>, with the
# `-C metadata` cargo gives it there, one a line.
metadata() {
  copy "$1" || return 1
  (cd "$1/std/kernels-src" && CARGO_TARGET_DIR="$1/target" cargo build -v --locked --release --target wasm32-unknown-unknown --workspace 2>&1) |
    sed -n 's/.*--crate-name \([a-z0-9_]*\) .*-C metadata=\([0-9a-f]*\).*/\1 \2/p' | sort -u
}
n=$((n + 1))
here=$(metadata "$TMP/one")
there=$(metadata "$TMP/a-much-longer-folder-name/two")
if [ -n "$here" ] && [ "$here" = "$there" ]; then
  passed=$((passed + 1))
  echo "PASS  every crate's -C metadata, in two folders: the same ($(printf '%s\n' "$here" | wc -l | tr -d ' ') crates)"
else
  failed=$((failed + 1))
  echo "FAIL  every crate's -C metadata, in two folders: $(diff <(printf '%s\n' "$here") <(printf '%s\n' "$there") | grep '^[<>]' | tr '\n' ' ')"
fi

echo "ci/wasm-test.sh: $passed passed, $failed failed"
[ $failed -eq 0 ]
