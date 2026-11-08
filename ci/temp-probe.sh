#!/bin/bash
# TEMP (track ci-flakes round 4), taken out before the pull request: the three speed tests of
# ci/shared-cpu-tests.txt, 5 times each, with their output, on the nightly's own build.
set -u -o pipefail
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export M2_AUDIO=none M2_MIDI=none CARGO_TERM_COLOR=never CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
case "$(uname)" in Darwin) export CARGO_TARGET_DIR=$PWD/target/pr ;; *) export CARGO_TARGET_DIR=$PWD/target/nightly ;; esac
PATH="$(ci/tools.sh nextest | tail -1):$PATH"
H=${RUNNER_TEMP:-/tmp}/m2home; mkdir -p "$H"
f='not (binary(=main_thread) | binary(=app_main_thread)) & (test(=every_pack_picture_draws_inside_a_fifth_of_its_limit) | test(=measure::tests::a_drag_frame_of_the_eq_stays_under_8_ms) | test(=measure::tests::a_measure_follows_a_drag_inside_its_budget))'
for i in 1 2 3 4 5; do
  echo "::group::run $i"
  env HOME="$H" cargo nextest run --locked --workspace --no-fail-fast --retries 0 -E "$f" --success-output immediate --failure-output immediate 2>&1 | tee "$H/probe-$i.log" | grep -vE '^\s*$' | tail -60
  echo "::endgroup::"
  echo "PROBE run $i: $(grep -E '^ *Summary' "$H/probe-$i.log" | sed 's/^ *//') $(grep -hE '^ +(PASS|FAIL) ' "$H/probe-$i.log" | awk '{print $1, $NF}' | tr '\n' ' ')"
done
exit 0
