#!/bin/bash
# TEMP (track ci-flakes, round 2), taken out before the pull request. On GitHub's machines, after
# the nightly's own steps, with their build:
#   six:      the six speed tests, each saying this machine's ratio and its times;
#   mutation: the pack speed test with one picture's draw about twice the work (the phaser's
#             motion worked out twice, ci/temp-speed-mutation.patch): it must fail;
#   pace:     the silent player's pace test, 5 times, each saying its pace;
#   burst:    (the Mac) the burst test, 20 times, each saying whether the names overflowed by the
#             flush or only after it.
set -u -o pipefail
os=$1
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export M2_AUDIO=none M2_MIDI=none CARGO_TERM_COLOR=never CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
export LIBC_FATAL_STDERR_=1
H=${RUNNER_TEMP:-/tmp}/m2home
mkdir -p "$H"
SIX="every_pack_picture_draws_inside_a_fifth_of_its_limit every_instrument_picture_draws_in_a_fifth_of_its_limit an_eight_band_curve_draws_inside_its_budget a_drag_frame_of_the_eq_stays_under_8_ms thirty_one_spectra_fit_in_a_read_with_room a_measure_follows_a_drag_inside_its_budget"
case $os in
  linux)
    export CARGO_TARGET_DIR=$PWD/target/nightly
    # run <names...>: those tests, by name, their output shown.
    run() { env HOME="$H" cargo test --locked --workspace --tests --no-fail-fast -- "$@" --nocapture 2>&1; } ;;
  local)
    # A try of this script on the Linux server: three crates, into $CARGO_TARGET_DIR.
    run() { env HOME="$H" cargo test --locked -p m2-script -p m2-play -p m2-editor --tests --no-fail-fast -- "$@" --nocapture 2>&1; } ;;
  macos)
    export CARGO_TARGET_DIR=$PWD/target/pr
    PATH="$(ci/tools.sh nextest | tail -1):$PATH"
    run() {
      local f="" n
      for n in "$@"; do f="${f:+$f | }test(/(^|::)$n\$/)"; done
      env HOME="$H" cargo nextest run --locked --workspace --no-fail-fast --no-tests=fail -E "$f" \
        --success-output immediate --failure-output immediate 2>&1
    } ;;
esac
out=${RUNNER_TEMP:-/tmp}/probe
mkdir -p "$out"
say() { echo "PROBE $*" | tee -a "$out/summary.txt"; }

echo "::group::six"
# shellcheck disable=SC2086
run $SIX | tee "$out/six.log"; st=$?
echo "::endgroup::"
say "six: exit $st; $(grep -c 'speed: std' "$out/six.log") ratio lines; $(grep -E '^speed: std' "$out/six.log" | head -1)"
grep -hE 'panicked|past a fifth|past [0-9.]+m?s|over the|p99 .* against|, against ' "$out/six.log" | head -20 | sed 's/^/PROBE   /' | tee -a "$out/summary.txt"

echo "::group::mutation"
git apply ci/temp-speed-mutation.patch
run every_pack_picture_draws_inside_a_fifth_of_its_limit | tee "$out/mutation.log"; st=$?
git checkout -- packs/@m2/effects/src/pictures.ts
echo "::endgroup::"
if [ $st -ne 0 ] && grep -q "phaser's motion" "$out/mutation.log"; then
  say "mutation: FAILED, as it must: $(grep -E "phaser's motion" "$out/mutation.log" | head -3 | tr '\n' ' ')"
else
  say "mutation: exit $st, NOT the failure it must be"
fi
say "mutation: $(grep -E '^speed: std' "$out/mutation.log" | head -1)"

echo "::group::pace"
for i in 1 2 3 4 5; do
  run a_silent_player_whose_sleeps_run_late_keeps_the_wall_clocks_pace > "$out/pace-$i.log"; st=$?
  say "pace $i: exit $st: $(grep -E 's of the song in' "$out/pace-$i.log" | head -1)"
done
echo "::endgroup::"

if [ "$os" = macos ]; then
  echo "::group::burst"
  for i in $(seq 1 20); do
    run a_burst_past_the_hint_limit_is_walked_and_never_missed > "$out/burst-$i.log"; st=$?
    say "burst $i: exit $st: $(grep -E "the burst's [0-9]+ names" "$out/burst-$i.log" | head -1) $(grep -E 'panicked|never overflowed|walk was a rescan' "$out/burst-$i.log" | head -2 | tr '\n' ' ')"
  done
  echo "::endgroup::"
fi
echo
cat "$out/summary.txt"
exit 0
