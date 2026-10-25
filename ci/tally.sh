#!/bin/bash
# ci/tally.sh: how ci/check.sh reads cargo test's log: which tests failed, and which test binaries
# failed with no test named. ci/check.sh sources it, and ci/tally-test.sh tries it on made-up logs.
# Written for bash 3.2 and the awk macOS ships.
#
# tally <log> <prefix>: the failed tests in <log>, one name a line, in $SCR/<prefix>tests, and in
# $SCR/<prefix>whole each failed test binary that names no failed test: cargo's rerun arguments, a
# tab, and why. One with no harness, or that crashed, says why by its first panic (or its own "hung"
# or "FAILED" line), and counts as one failure; a failed doctest by its first FAILED line. Prints
# the counts.
#
# A binary's failed tests are the list after its last "failures:" line: names alone, each indented
# four spaces, then a blank line and "test result:". A test's own output, printed above that list,
# can hold lines that look like names (a panic message's JSON: "    1"), so a list a line of
# anything else breaks off names nothing.
tally() {
  awk -v T="$SCR/$2named" -v W="$SCR/$2whole" '
    BEGIN { printf "" > T; printf "" > W }
    /^     Running |^   Doc-tests / { result = 0; failed = 0; named = 0; why = ""; grab = 0; test = ""; list = 0; n = 0 }
    grab { why = why " " $0; grab = 0 }
    why == "" && /panicked at / { why = $0; sub(/^thread .* panicked at /, "", why); grab = 1 }
    why == "" && /^[a-z_]+: (hung|FAILED)/ { why = $0 }
    test == "" && /^test .* \.\.\. FAILED$/ { test = $0 }
    /^failures:$/ { list = 1; n = 0; next }
    list && /^    [A-Za-z0-9_:]+$/ { names[++n] = substr($0, 5); next }
    list && /^$/ { next }
    /^test result:/ {
      if (list) { for (i = 1; i <= n; i++) print names[i] > T; named += n }
      p += $4; f += $6; result = 1; failed = $6
    }
    { list = 0 }
    /^error: (test|doctest) failed, to rerun pass / {
      lone = !(result && failed)
      if (lone) { f++; whole++ }
      if (lone || !named) {
        args = $0; sub(/^[^`]*`/, "", args); sub(/`.*$/, "", args)
        print args "\t" substr(lone ? why : test, 1, 160) > W
      }
    }
    END { printf "%d passed, %d failed", p, f; if (whole) printf " (%d of them a whole test binary)", whole }' "$1"
  sort -u "$SCR/$2named" > "$SCR/$2tests"
}
