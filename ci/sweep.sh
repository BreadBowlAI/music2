#!/bin/bash
# ci/sweep.sh: 8a's measurements on the owner's Mac, in one go (D23 of the live half's plan).
#
# In short: run it in the repo, in Terminal on the Mac itself. Plug the Mac in, close other apps,
# and set the output to the built-in speakers at 48 kHz (Audio MIDI Setup). It plays nothing
# aloud. It takes about 40 minutes. It writes ~/m2-8a.txt: paste that back.
#
#   ci/sweep.sh
#
# What each step decides (docs/designs/step8/build-notes-8a.md, "The Mac command"):
#   1. models     the pool's orderings and the caller's wake on arm64. A FAIL: stop, fix first.
#   2. m2-os rt   the Mac's own real-time code, run for the first time: the Mach wake, the
#                 time-constraint policy, the output's workgroup joined and left.
#   3. sysctl     the core counts the rules read (4 fast cores per L2 on an M1 Pro).
#   4. offline    G12 --offline after R1: whether 8 threads still beat 4 (the export count),
#                 the song's tail, paced blocks on 4 threads.
#   5. live sweep G12 live, 1 to 4 threads, park and spin, one minute each: the live count, and
#                 whether BETWEEN becomes Spin.
#   6. gate 8     G12 live for 10 minutes at the live rule's count: gate 8 on this Mac.
# Music2 runs get a scratch HOME; cargo and rustup keep yours. Written for bash 3.2.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
[ "$(uname -s)" = Darwin ] || { echo "ci/sweep.sh is for the owner's Mac"; exit 2; }
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup} CARGO_TERM_COLOR=never
OUT=$HOME/m2-8a.txt
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/m2-sweep.XXXXXX") || exit 2
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
caffeinate -i -w $$ &
: > "$OUT"

# say <words>: a heading, on screen and in the file.
say() { printf '\n=== %s (%s)\n' "$*" "$(date +%H:%M)" | tee -a "$OUT"; }
# settle: wait up to 2 minutes for the 1-minute load to fall under 3 (a build just ran), then say it.
settle() {
  for _ in $(seq 24); do
    awk -v l="$(sysctl -n vm.loadavg | awk '{print $2}')" 'BEGIN { exit !(l < 3) }' && break
    sleep 5
  done
  echo "load: $(sysctl -n vm.loadavg)" | tee -a "$OUT"
}

say "music2 $(git rev-parse --short=12 HEAD) on $(sysctl -n hw.model), macOS $(sw_vers -productVersion)"
say "building (about 5 minutes the first time)"
cargo build --locked --release -p m2-gates 2>&1 | tail -3 | tee -a "$OUT"

say "1. models: the pool on arm64"
cargo test --locked --release -p m2-pool --features models 2>&1 | grep -E '^test result|FAILED|panicked|^error' | tee -a "$OUT"

say "2. m2-os rt: the Mac's real-time code"
cargo test --locked -p m2-os rt 2>&1 | grep -E '^test |^test result|panicked|^error' | tee -a "$OUT"

say "3. sysctl: the cores"
sysctl hw.model hw.perflevel0 hw.perflevel1 | tee -a "$OUT"

say "4. offline: G12 --offline on p50"
settle
HOME=$SCRATCH "$TARGET/release/m2-gates" realtime --offline --reference 50 2>&1 | tee -a "$OUT"

say "5. live sweep: 1 to 4 threads, park and spin, a minute each"
settle
HOME=$SCRATCH "$TARGET/release/m2-gates" realtime --reference 50 --minutes 1 --sweep 1,2,3,4 --between park,spin 2>&1 | tee -a "$OUT"

say "6. gate 8: 10 minutes live at the live rule's count"
settle
HOME=$SCRATCH "$TARGET/release/m2-gates" realtime --reference 50 --minutes 10 2>&1 | tee -a "$OUT"

rm -rf "$SCRATCH"
say "done: paste back what is in $OUT"
