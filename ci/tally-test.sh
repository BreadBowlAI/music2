#!/bin/bash
# ci/tally-test.sh: tries ci/tally.sh, how ci/check.sh reads cargo test's log, on made-up logs: which
# tests it names as failed, which test binaries it names as failed whole, and the counts it prints.
# ci/pr.sh runs it in its lint step, so the PR tier and ci/check.sh both do.
#
# In short: a failed test's own output may hold lines that look like a test's name. The Mac's first
# full check named a test "1": the second line of `"bars": [1, 1]` in a panic message's JSON. Only
# the list after a binary's last "failures:" line names what failed.
#
# Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case passed, 1
# when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
. "$HERE/tally.sh" || exit 2
SCR=$(mktemp -d "${TMPDIR:-/tmp}/m2-tally-test.XXXXXX") || exit 2
trap 'rm -rf "$SCR"' EXIT
TAB=$(printf '\t')

passed=0
failed=0
n=0
# case_ <what> <log> <counts> <tests, one a line> <binaries named whole, one a line>: tally <log>
# prints <counts>, names exactly <tests> as failed, and <binaries> as failed whole.
case_() {
  n=$((n + 1))
  printf '%s' "$2" > "$SCR/log"
  got=$(tally "$SCR/log" c$n-)
  tests=$(cat "$SCR/c$n-tests")
  whole=$(cut -f1 "$SCR/c$n-whole")
  if [ "$got" = "$3" ] && [ "$tests" = "$4" ] && [ "$whole" = "$5" ]; then
    passed=$((passed + 1))
    echo "PASS  $1"
  else
    failed=$((failed + 1))
    echo "FAIL  $1: counts \"$got\" (want \"$3\"); tests [$tests] (want [$4]); whole [$whole] (want [$5])"
  fi
}

# A binary whose failed test printed pretty JSON: its lines "    1" and "    true" are no tests.
case_ "a panic message's JSON names no test" '     Running tests/listen.rs (target/debug/deps/listen-0a72561ace77708c)

running 3 tests
test a_second_listen_waits_for_the_first ... ok
test m2_listen_runs_at_background_priority ... FAILED
test m2_listen_png_writes_a_band_picture ... ok

failures:

---- m2_listen_runs_at_background_priority stdout ----

thread '"'"'m2_listen_runs_at_background_priority'"'"' (199523) panicked at apps/m2/tests/listen.rs:362:5:
assertion `left == right` failed: {
  "bars": [
    1,
    1
  ],
  "exact": [
    true
  ]
}
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    m2_listen_runs_at_background_priority

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 197.35s

error: test failed, to rerun pass `-p m2 --test listen`
' "2 passed, 1 failed" "m2_listen_runs_at_background_priority" ""

# Two binaries, two failed tests in one, a module path in the other; a passing binary between.
case_ "every failed test, from each binary" '     Running tests/forward.rs (target/debug/deps/forward-8d73bf6184b49189)

running 3 tests
test b ... FAILED
test a ... FAILED
test c ... ok

failures:

---- b stdout ----
    looks_like_a_name
---- a stdout ----

failures:
    a
    b

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s

     Running unittests src/lib.rs (target/debug/deps/m2_os-1)

running 1 test
test x ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/m2_plugins-5441f1680a12e0da)

running 2 tests
test remote::tests::a_host_asked_to_runs_at_background_priority ... FAILED
test tests::state_round_trips ... ok

failures:

---- remote::tests::a_host_asked_to_runs_at_background_priority stdout ----
  left: Some(false)

failures:
    remote::tests::a_host_asked_to_runs_at_background_priority

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.05s

error: test failed, to rerun pass `-p m2 --test forward`
error: test failed, to rerun pass `-p m2-plugins --lib`
' "3 passed, 3 failed" "a
b
remote::tests::a_host_asked_to_runs_at_background_priority" ""

# A binary that crashed before its summary: failed whole, named by its rerun arguments and its panic.
case_ "a binary that crashed is failed whole" '     Running tests/live.rs (target/debug/deps/live-1)

running 2 tests
test one ... ok
thread '"'"'main'"'"' (1) panicked at core/play/tests/live.rs:9:5:
    looks_like_a_name
error: test failed, to rerun pass `-p m2-play --test live`
' "0 passed, 1 failed (1 of them a whole test binary)" "" "-p m2-play --test live"

echo
echo "$passed of $n cases passed"
[ $failed -eq 0 ] && [ $passed -eq $n ]
