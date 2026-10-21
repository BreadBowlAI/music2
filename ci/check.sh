#!/bin/bash
# ci/check.sh: the full check of music2, the same script on macOS and Linux (PLAT-6).
#
# In short: run it in the repo with no arguments. It builds everything, runs every Rust test,
# clippy, the pool's models, the web build, every browser suite, std's checks and the offline
# renders; on a Mac it also times the gates ci/gates.txt names. First it builds the old engine
# G7f compares with (S), in a folder of its own: ~/m2-check/s-8aa301e on a Mac,
# ~/.cache/m2-check/s-8aa301e on Linux. Then it prints a short summary to paste back. It changes
# nothing in the repo, never opens the app on a screen you can see, never plays sound (G12 opens the
# Mac's output muted: it plays zeros), and never touches your ~/Music or ~/.music2. If one line says
# "running" for over 30 minutes, press Ctrl-C once: it stops and prints the summary anyway.
#
#   ci/check.sh                  everything (on a Mac, the timing gates too)
#   ci/check.sh --gates          on Linux, the timing gates too (recorded for information, PLAT-1)
#   ci/check.sh --gates=auto     on Linux, the timing gates too, unless git proves the change can't
#                                reach them (below). On a Mac they always run.
#   ci/check.sh --gates=auto --base <rev>
#                                the same, with the change counted from git merge-base HEAD <rev>
#                                instead of from main
#   ci/check.sh --no-gates       on a Mac, without them
#   ci/check.sh --pr             only the PR tier, exactly as CI runs it on a pull request
#                                (ci/pr.sh's steps, one after another, each a row with its time;
#                                below)
#   ci/check.sh --nightly        the nightly tier as CI runs it (.github/workflows/nightly.yml):
#                                everything but the timing gates, on any OS, the browser suites one
#                                at a time, built without debug info into <target>/nightly
#
# --pr: the checks every pull request must pass, and nothing else: rustfmt, actionlint, the gates
# and docs rules, licences, the web build, std's kernels and std.wasm, clippy, the Rust tests and
# browser suites not tagged nightly (ci/nightly-tests.txt, e2e/run.mjs), and the 12 renders. When
# git proves your committed change is documentation only (ci/docs-only.sh, on a clean tree), it
# runs only the docs rule's cases, as CI does. It builds into <target>/pr without debug info, as
# CI does, so it never rebuilds what your own cargo build made. It needs what ci/pr.sh needs (its
# top says): cargo-nextest, cargo-deny and actionlint come from ci/tools.sh.
#
# When --gates=auto skips the gates (ci/gates-auto.sh holds the rule, ci/gates-auto-test.sh its
# cases, which the "gates rule" row runs): only when git proves that HEAD holds, in every file they
# read or build from, what a check on this machine already passed them on. The change is every file
# git diff --no-renames names between git merge-base HEAD main and HEAD. The gates are skipped only
# when the tree is clean (nothing uncommitted or untracked, no change git hides, no ignored file
# where they read), that list isn't empty, HEAD holds no symlink, every file on it is under web/,
# docs/, README.md or HANDOFF.md (the paths m2-gates never reads or builds from; e2e/ isn't one:
# m2-gates reads e2e/renders.sha256), and the files the gates do read are exactly those of a tree
# whose gates passed on this machine. Each Linux check whose gates row passes, on a clean tree, adds
# that tree to ~/.cache/m2-check/gates-passed. Anything else runs them: any other file, a dirty tree,
# main itself (nothing changed), no recorded pass on the same files, a git error, or a failed "gates
# rule" row. The decision is printed at the start. A skip is a SKIP row, "only web/, docs/ changed
# since <base> (12 files); the gates passed at <commit> (<date>) on the same files they read",
# never a PASS, and the first line says the gates weren't run.
#
# The browser suites run several at a time on Linux (e2e/run.mjs --jobs: a quarter of the threads,
# 1 to 4; each suite still on its own server, HOME and browser) and one at a time on a Mac, as
# before. A suite that failed runs once more, alone.
#
# Optional settings:
#   CLAP_PATH         a folder holding "Surge XT.clap" and "Airwindows Consolidated.clap". The
#                     default, when both are there: ~/Library/Audio/Plug-Ins/CLAP on a Mac,
#                     ~/.local/share/m2-test-plugins/clap on Linux (ci/plugins.sh fills it).
#                     Without them the check can't pass (PLAT-6): it says FAIL, even when no row
#                     failed.
#   CARGO_TARGET_DIR  where cargo builds (default: target/ in the repo)
#   M2_CHECK_OUT      where the logs go (default: ~/m2-check/<commit>). Only a folder an earlier
#                     check made is ever replaced.
#   M2_CHECK_JOBS     how many browser suites run at once (default: on Linux a quarter of the
#                     threads, 1 to 4; on a Mac 1)
# Exit status: 0 when every check passed, all of it judged, with the test plugins; 1 when not; 2 when
# it couldn't start.
#
# On a Mac, set the output to the built-in speakers at 48 kHz first (Audio MIDI Setup): G12 plays
# there, muted, and judges that set-up.
#
# Needs rustup and Node 24. The first run installs Rust 1.99.0 (rust-toolchain.toml) and
# chrome-headless-shell 133.0.6943.141. On a Mac, run it in Terminal on the Mac itself, not over
# ssh: the app's test needs your login session. On Linux with Xvfb it also opens the desktop app
# on an invisible screen. Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
# GATES: 1 when the gates run. AUTO: --gates=auto, which decides that after the "gates rule" row.
GATES=
AUTO=
BASE=
PR=
NIGHTLY=
ASKED=
case $(uname -s) in
  Darwin)
    OS=macos
    GATES=1
    JOBS=1
    MACHINE="$(sysctl -n hw.model), macOS $(sw_vers -productVersion), $(uname -m)"
    CHROME_DIR=mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64
    PLUGINS=$HOME/Library/Audio/Plug-Ins/CLAP ;;
  Linux)
    OS=linux
    # A quarter of the threads, 1 to 4: each suite runs a server and a browser.
    JOBS=$(( $(nproc) / 4 )); if [ $JOBS -gt 4 ]; then JOBS=4; elif [ $JOBS -lt 1 ]; then JOBS=1; fi
    MACHINE="$(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m), $(nproc) threads"
    CHROME_DIR=linux-133.0.6943.141/chrome-headless-shell-linux64
    PLUGINS=$HOME/.local/share/m2-test-plugins/clap ;;
  *)
    echo "ci/check.sh runs on macOS and Linux"
    exit 2 ;;
esac
while [ $# -gt 0 ]; do
  case $1 in
    --gates) GATES=1; AUTO=; ASKED=1 ;;
    --gates=auto) AUTO=1 ;;
    --no-gates) GATES=; AUTO= ;;
    --base)
      if [ $# -lt 2 ] || [ -z "$2" ]; then echo "ci/check.sh: --base needs a commit"; exit 2; fi
      BASE=$2
      shift ;;
    --base=?*) BASE=${1#--base=} ;;
    --pr) PR=1 ;;
    --nightly) NIGHTLY=1 ;;
    *) echo "ci/check.sh: no option $1 (see the top of the script)"; exit 2 ;;
  esac
  shift
done
if [ -n "$BASE" ] && [ -z "$AUTO" ]; then echo "ci/check.sh: --base goes with --gates=auto"; exit 2; fi
if [ -n "$PR" ] && [ -n "$NIGHTLY" ]; then echo "ci/check.sh: --pr or --nightly, not both"; exit 2; fi
# --nightly: never the gates (a shared CI machine can't judge them, PLAT-1), the suites one at a
# time (that finds races running several at once hides), and the build CI's disk holds.
if [ -n "$NIGHTLY" ]; then
  if [ -n "$AUTO$ASKED" ]; then echo "ci/check.sh: --nightly runs no gates"; exit 2; fi
  GATES=
  export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
  export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}/nightly
  case $CARGO_TARGET_DIR in /*) ;; *) CARGO_TARGET_DIR=$ROOT/$CARGO_TARGET_DIR ;; esac
fi
if [ -n "$PR" ] && [ -n "$AUTO$ASKED" ]; then echo "ci/check.sh: --pr runs no gates"; exit 2; fi
# The summary's first line says which tier ran, when it isn't the full check.
MODE=
if [ -n "$PR" ]; then MODE=--pr; GATES=; elif [ -n "$NIGHTLY" ]; then MODE=--nightly; fi
JOBS=${M2_CHECK_JOBS:-$JOBS}
case $JOBS in ''|0*|*[!0-9]*) echo "ci/check.sh: M2_CHECK_JOBS must be a whole number, 1 or more"; exit 2 ;; esac
if [ -n "$NIGHTLY" ]; then JOBS=1; fi
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
SHA=$(git rev-parse --short=12 HEAD) || exit 2
OUT=${M2_CHECK_OUT:-$HOME/m2-check/$SHA}
if [ -e "$OUT" ] && [ ! -e "$OUT/.m2-check" ]; then
  echo "ci/check.sh: $OUT wasn't made by a check, so it is left alone: name another folder in M2_CHECK_OUT"
  exit 2
fi
rm -rf "$OUT" && mkdir -p "$OUT/home" && : > "$OUT/.m2-check" || exit 2
SCR=$(mktemp -d "${TMPDIR:-/tmp}/m2-check.XXXXXX") || exit 2
START=$(date +%s)
T0=$START
RUNNING=
TAB=$(printf '\t')
: > "$OUT/results"
: > "$OUT/failed"
: > "$OUT/flaky"

# Whatever music2 starts gets a scratch HOME (tests, suites, renders, gates, the app); cargo and
# rustup keep yours.
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
# No audio device, and no MIDI keyboard: nothing the check starts opens one (the suites' servers
# get M2_MIDI=inject from run.mjs).
export M2_AUDIO=none M2_MIDI=none CARGO_TERM_COLOR=never
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
# Linux: this machine's record of the trees the gates passed on, which --gates=auto reads
# (ci/gates-auto.sh). Steps 0b and 11 keep it; a Mac keeps none.
if [ $OS = linux ]; then GATES_PASSED=${XDG_CACHE_HOME:-$HOME/.cache}/m2-check/gates-passed; else GATES_PASSED=; fi
CHROME=${E2E_CHROME:-$HOME/.cache/puppeteer/chrome-headless-shell/$CHROME_DIR/chrome-headless-shell}
if [ -z "${CLAP_PATH:-}" ] && [ -e "$PLUGINS/Surge XT.clap" ] && [ -e "$PLUGINS/Airwindows Consolidated.clap" ]; then
  CLAP_PATH=$PLUGINS
fi
if [ -n "${CLAP_PATH:-}" ]; then export CLAP_PATH; else unset CLAP_PATH; fi
# Whether both test plugins are there: a check passes only with them (PLAT-6).
TESTPLUGINS=
if [ -e "${CLAP_PATH:-}/Surge XT.clap" ] && [ -e "${CLAP_PATH:-}/Airwindows Consolidated.clap" ]; then TESTPLUGINS=1; fi
# A Mac stays awake while this runs: idle sleep breaks timed tests.
if [ $OS = macos ]; then caffeinate -i -w $$ & fi

# begin <check> <usual time>: say a check started, so "slow" and "stuck" can be told apart.
begin() {
  RUNNING=$1
  T0=$(date +%s)
  printf '%-16s ...   running, usually %s (started %s)\n' "$1" "$2" "$(date +%H:%M)"
}
# record <check> <PASS|PART|FAIL|SKIP> <note>: one row of the summary. PART: it ran and nothing failed,
# but not all of it could be judged, so the check isn't a pass.
record() {
  RUNNING=
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$OUT/results"
  printf '%-16s %-4s  %s\n' "$1" "$2" "$3"
}
# quiet <log> <command...>: run it, with its output in $OUT/<log>.
quiet() {
  log=$OUT/$1
  shift
  "$@" > "$log" 2>&1 < /dev/null
}
# first <file> <pattern>: the first line that matches, cut to 160 characters.
first() { grep -m1 -E "$2" "$1" 2> /dev/null | cut -c1-160; }
# panicked <test> <log>: where the test panicked, and the first line of why. Rust prints the
# thread's id after its name: "thread 'a::b' (1234) panicked at src/x.rs:1:2:".
panicked() {
  grep -A1 -E "^thread '$1' (\([0-9]+\) )?panicked at " "$2" | head -2 | tr '\n' ' ' |
    sed -E "s/^thread '[^']*' (\([0-9]+\) )?panicked at //; s/ *$//" | cut -c1-160
}
# tally <log> <prefix>: the failed tests in <log>, one name a line, in $SCR/<prefix>tests, and in
# $SCR/<prefix>whole each failed test binary that names no failed test: cargo's rerun arguments, a
# tab, and why. One with no harness, or that crashed, says why by its first panic (or its own "hung"
# or "FAILED" line), and counts as one failure; a failed doctest by its first FAILED line. Prints
# the counts.
tally() {
  grep -E '^    [A-Za-z0-9_:]+$' "$1" | sed 's/^ *//' | sort -u > "$SCR/$2tests"
  awk -v W="$SCR/$2whole" '
    BEGIN { printf "" > W }
    /^     Running |^   Doc-tests / { result = 0; failed = 0; named = 0; why = ""; grab = 0; test = "" }
    grab { why = why " " $0; grab = 0 }
    why == "" && /panicked at / { why = $0; sub(/^thread .* panicked at /, "", why); grab = 1 }
    why == "" && /^[a-z_]+: (hung|FAILED)/ { why = $0 }
    test == "" && /^test .* \.\.\. FAILED$/ { test = $0 }
    /^    [A-Za-z0-9_:]+$/ { named++ }
    /^test result:/ { p += $4; f += $6; result = 1; failed = $6 }
    /^error: (test|doctest) failed, to rerun pass / {
      lone = !(result && failed)
      if (lone) { f++; whole++ }
      if (lone || !named) {
        args = $0; sub(/^[^`]*`/, "", args); sub(/`.*$/, "", args)
        print args "\t" substr(lone ? why : test, 1, 160) > W
      }
    }
    END { printf "%d passed, %d failed", p, f; if (whole) printf " (%d of them a whole test binary)", whole }' "$1"
}
# failures <prefix> <log>: what failed, for "what failed": each test with where it panicked, each
# whole test binary with why.
failures() {
  while read -r t; do
    echo "test $t"
    r=$(panicked "$t" "$2")
    if [ -n "$r" ]; then echo "  $r"; fi
  done < "$SCR/$1tests"
  while IFS="$TAB" read -r args why; do
    echo "test binary $args"
    if [ -n "$why" ]; then echo "  $why"; fi
  done < "$SCR/$1whole"
}
# cause <log>: the line after "error:" that says why a build failed: a link error's missing symbol
# (named only below "linking with cc failed"), else a compile error's place.
cause() {
  w=$(grep -m1 -A1 -E 'Undefined symbols|undefined reference' "$1" 2> /dev/null | tr -s ' \n' '  ' | sed 's/^ *//' | cut -c1-160)
  if [ -z "$w" ]; then w=$(first "$1" '^ *--> ' | sed 's/^ *//'); fi
  if [ -n "$w" ]; then echo "  $w"; fi
}
# stuck: the tests libtest called slow that never finished, when a run was stopped.
stuck() {
  grep -E '^test .* has been running for over 60 seconds' "$OUT/test.log" 2> /dev/null | awk '{print $2}' | sort -u |
    while read -r t; do
      grep -q "^test $t \.\.\. " "$OUT/test.log" || echo "test $t (still running when stopped)"
    done
}
# suites <run.mjs arguments>: the browser suites, each on a fresh headless server.
suites() { env E2E_CHROME="$CHROME" M2_HEADLESS="$TARGET/debug/examples/headless" node e2e/run.mjs "$@"; }
# version <plugin>: what a Mac plugin bundle says its version is.
version() { /usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$CLAP_PATH/$1.clap/Contents/Info.plist" 2> /dev/null || echo '?'; }
# plugins: the folder the plugin checks used, and which builds are in it.
plugins() {
  if [ -z "$TESTPLUGINS" ]; then
    echo "none: ${CLAP_PATH:-$PLUGINS} doesn't hold both"
  elif [ $OS = macos ]; then
    echo "$CLAP_PATH (Surge XT $(version 'Surge XT'), Airwindows Consolidated $(version 'Airwindows Consolidated'))"
  elif (cd "$CLAP_PATH" && grep '\.clap$' "$ROOT/ci/plugins.sha256" | sha256sum -c --quiet) > /dev/null 2>&1; then
    echo "$CLAP_PATH (the builds ci/plugins.sha256 records)"
  else
    echo "$CLAP_PATH (NOT the builds ci/plugins.sha256 records)"
  fi
}

# The summary to paste back, and every log packed into one small file beside the logs.
summary() {
  fails=$(awk -F"$TAB" '$2 == "FAIL"' "$OUT/results" | wc -l | tr -d ' ')
  # The rows that ran but weren't judged in full (the gates on a Mac S's records don't name).
  partial=$(awk -F"$TAB" '$2 == "PART" { printf "%s%s", (n++ ? ", " : ""), $1 }' "$OUT/results")
  total=$(wc -l < "$OUT/results" | tr -d ' ')
  mins=$(( ($(date +%s) - START + 59) / 60 ))
  if [ -s "$SCR/before" ]; then state="with local changes"; else state=clean; fi
  if [ "$fails" -gt 0 ]; then
    verdict="FAIL ($fails of $total checks)"
  elif [ -z "$TESTPLUGINS" ]; then
    verdict="FAIL: no row failed, but the test plugins weren't there, so it isn't a pass (PLAT-6)"
  elif [ -n "$partial" ]; then
    verdict="PART: no row failed, but the $partial row wasn't judged in full, so it isn't a pass: see that row"
  else
    verdict=PASS
  fi
  if awk -F"$TAB" '$1 == "gates" && $2 == "SKIP" { found = 1 } END { exit !found }' "$OUT/results"; then
    verdict="$verdict, without running the gates: --gates=auto skipped them, as they passed before on the same files (see the gates row)"
  fi
  if [ -s "$OUT/flaky" ]; then verdict="$verdict; $(grep -c '' "$OUT/flaky") passed only on a retry"; fi
  # Logs only: no renders, no screenshots, no kept projects (a failed fx-fixes keeps a file of
  # over 300 MB).
  (cd "$OUT" && tar -czf "$OUT.tgz" $(find . -maxdepth 2 -type f \( -name '*.log' -o -name '*.sum' -o -name results -o -name failed -o -name flaky \))) 2> /dev/null
  {
    echo
    echo "music2 check${MODE:+ $MODE}: $verdict"
    echo "commit   $SHA on $(git rev-parse --abbrev-ref HEAD) ($state)"
    echo "machine  $MACHINE"
    echo "tools    $(rustc -V 2> /dev/null | cut -d' ' -f1-2), node $(node -v 2> /dev/null), $mins min"
    echo "plugins  $(plugins)"
    echo
    awk -F"$TAB" '{printf "%-16s %-4s  %s\n", $1, $2, $3}' "$OUT/results"
    if [ -s "$OUT/gates.sum" ]; then echo; echo "gates:"; sed 's/^/  /' "$OUT/gates.sum"; fi
    if [ -s "$OUT/flaky" ]; then echo; echo "passed only on a retry:"; sed 's/^/  /' "$OUT/flaky"; fi
    if [ -s "$OUT/failed" ]; then
      echo
      echo "what failed:"
      head -40 "$OUT/failed" | sed 's/^/  /'
      cut=$(( $(grep -c '' "$OUT/failed") - 40 ))
      if [ $cut -eq 1 ]; then more=line; else more=lines; fi
      if [ $cut -gt 0 ]; then echo "  ($cut $more more: the whole list is $OUT/failed, and \"failed\" in the logs file)"; fi
    fi
    echo
    echo "logs     $OUT.tgz"
  } | tee "$OUT/summary.txt"
}

# The app row's invisible screen and app, if one is open when the check stops.
XVFB=
APP=
closeapp() {
  if [ -n "$APP" ]; then kill "$APP" 2> /dev/null; sleep 1; kill -9 "$APP" 2> /dev/null; fi
  if [ -n "$XVFB" ]; then kill "$XVFB" 2> /dev/null; fi
  APP=
  XVFB=
}

# Ctrl-C: the check that was running is stuck; say so, and still print the summary.
stopped() {
  trap '' INT TERM
  closeapp
  if [ -n "$RUNNING" ]; then
    record "$RUNNING" FAIL "stopped after $(( ($(date +%s) - T0) / 60 )) min (stuck?)"
    stuck >> "$OUT/failed"
  fi
  summary
  exit 1
}
trap 'closeapp; rm -rf "$SCR"' EXIT
trap stopped INT TERM

echo "music2 check of $SHA on $MACHINE (logs in $OUT)"
git status --porcelain > "$SCR/before"
if [ -z "$TESTPLUGINS" ]; then
  echo "plugins: Surge XT.clap and Airwindows Consolidated.clap aren't both in ${CLAP_PATH:-$PLUGINS} (on Linux, ci/plugins.sh installs them)" >> "$OUT/failed"
fi

# 0. Plugins in a folder music2 always searches make fx-fixes fail, so the run could never pass:
#    stop at once, before 45 minutes are spent, and say what to do.
for d in /Library/Audio/Plug-Ins/CLAP /usr/lib/clap; do
  if [ -e "$d/Surge XT.clap" ] || [ -e "$d/Airwindows Consolidated.clap" ]; then
    echo "ci/check.sh: $d holds Surge XT or Airwindows, and one browser suite needs them out of it."
    echo "Move them to your own folder (~/Library/Audio/Plug-Ins/CLAP on a Mac), then run ci/check.sh again."
    exit 2
  fi
done

# --pr: the PR tier, as CI runs it: ci/pr.sh's steps, each a row with its time, then the summary.
#    A step that failed has its FAIL lines (and the tail of its logs) in "what failed"; each step's
#    output is pr-<step>.log, and its own logs are in pr-<step>/.
if [ -n "$PR" ]; then
  # pr <row> <usual time> <ci/pr.sh arguments...>: one step of the PR tier as one row.
  pr() {
    row=$1 usual=$2
    shift 2
    begin "$row" "$usual"
    log=$OUT/pr-$1.log
    M2_PR_OUT=$OUT/pr-$1 M2_PR_JOBS=$JOBS ci/pr.sh "$@" > "$log" 2>&1 < /dev/null
    st=$?
    said=$(grep -E '^ci/pr.sh .*: (PASS|FAIL) in ' "$log" | tail -1 | sed -E 's/^ci\/pr.sh [^:]*: (PASS|FAIL) in //; s/ \(logs in .*$//')
    if [ $st -eq 0 ]; then
      record "$row" PASS "${said:-done}"
    else
      record "$row" FAIL "${said:+after $said: }see pr-$1.log"
      # The step's FAIL rows, then each Rust test (nextest's FAIL lines) and suite that failed.
      {
        grep -E '^[a-z][a-zA-Z0-9. -]* +FAIL  ' "$log" | sed "s/^/$1: /"
        grep -E '^ +(FAIL|SIGSEGV|SIGABRT|TIMEOUT) \[' "$log" | sed -E 's/^ +[A-Z]+ \[[^]]*\] (\([^)]*\) )?/test /' | sort -u
        grep -E '^  failed: ' "$OUT/pr-$1/e2e.log" 2> /dev/null | sed -E 's/^  failed: ([^ ]+).*/suite \1/'
      } | cut -c1-160 >> "$OUT/failed"
      if ! grep -qE ' +FAIL  ' "$log"; then echo "$1: $(tail -1 "$log" | cut -c1-140)" >> "$OUT/failed"; fi
    fi
  }
  begin changes "seconds"
  if ci/pr.sh changes > "$OUT/pr-changes.log" 2>&1 && grep -q '^docs_only=true$' "$OUT/pr-changes.log"; then
    record changes PASS "$(head -1 "$OUT/pr-changes.log" | sed 's/^docs  //'): CI runs only the docs job"
    pr "docs" "seconds" docs
  else
    record changes PASS "$(head -1 "$OUT/pr-changes.log" | sed -E 's/^(full|docs)  //' | cut -c1-120): the whole PR tier runs"
    pr "pr quick" "2 min" quick
    pr "pr clippy" "1 min; 4 the first time" clippy
    pr "pr rust" "6 min; 12 the first time" rust 1/1
    pr "pr suites" "4 min" suites 1/1
    pr "pr renders" "seconds" renders
  fi
  begin "repo unchanged" "seconds"
  git status --porcelain > "$SCR/after"
  if cmp -s "$SCR/before" "$SCR/after"; then
    record "repo unchanged" PASS ""
  else
    record "repo unchanged" FAIL "$(diff "$SCR/before" "$SCR/after" | grep -c '^[<>]') entries changed: run git status"
  fi
  trap '' INT TERM
  summary
  [ "$fails" -eq 0 ] && [ -n "$TESTPLUGINS" ] && [ -z "$partial" ]
  exit
fi

# 0b. The rule --gates=auto follows (ci/gates-auto.sh), tried on its table of cases in scratch git
#     repos (ci/gates-auto-test.sh). On Linux, when they pass, the rule is loaded (RULE), and
#     TREE0 is HEAD's gates_tree if the files on disk are HEAD's alone (gates_unseen): step 11
#     records it as passed when the gates row passes. Then, with --gates=auto, whether the gates
#     run: on a Mac always; on Linux by the rule, judged on the tree as the check found it, and
#     always when the rule's own cases failed. GATESWHY says why they run, SKIPPED why they don't.
GATESWHY=
SKIPPED=
RULE=
TREE0=
begin "gates rule" "seconds"
if quiet gates-rule.log ci/gates-auto-test.sh; then
  rule=PASS
  record "gates rule" PASS "$(tail -1 "$OUT/gates-rule.log")"
else
  rule=FAIL
  record "gates rule" FAIL "see gates-rule.log"
  { grep '^FAIL' "$OUT/gates-rule.log" || tail -1 "$OUT/gates-rule.log"; } | sed 's/^FAIL  //; s/^/gates rule: /' | cut -c1-160 >> "$OUT/failed"
fi
if [ $OS = linux ] && [ $rule = PASS ] && . ci/gates-auto.sh > "$OUT/gates-auto.log" 2>&1; then
  RULE=1
  if gates_unseen > "$OUT/gates-unseen.log" 2>&1 && ! TREE0=$(gates_tree); then
    TREE0=
    echo "git couldn't hash HEAD's tree" > "$OUT/gates-unseen.log"
  fi
fi
if [ -n "$AUTO" ]; then
  if [ $OS = macos ]; then
    GATES=1
    GATESWHY="on a Mac the gates always run"
  elif [ $rule != PASS ]; then
    GATES=1
    GATESWHY="the gates rule row failed"
  else
    # Any failure here (the file, the function, git) leaves the gates on.
    GATES=1
    GATESWHY="the rule's answer couldn't be read: see gates-auto.log"
    if [ -n "$RULE" ]; then
      gates_auto "${BASE:-main}" >> "$OUT/gates-auto.log" 2>&1
      st=$?
      said=$(head -1 "$OUT/gates-auto.log")
      case $st:$said in
        "0:skip  "?*) GATES= SKIPPED=${said#skip  } ;;
        *:"run   "?*) GATESWHY=${said#run   } ;;
      esac
    fi
  fi
  if [ -n "$GATES" ]; then
    echo "gates            --gates=auto runs them: $GATESWHY"
  else
    echo "gates            --gates=auto skips them: $SKIPPED"
  fi
fi

# 1. The pinned toolchain, installed if missing (rustup 1.28 or later reads rust-toolchain.toml).
begin toolchain "1 min"
quiet toolchain.log sh -c 'rustup toolchain install || rustup show active-toolchain; rustc -V'
if grep -q '^rustc 1\.99\.0 ' "$OUT/toolchain.log"; then
  record toolchain PASS "rustc 1.99.0"
else
  record toolchain FAIL "not Rust 1.99.0: see toolchain.log"
fi

# 2. The web UI: the desktop app embeds it, and the browser suites load it.
begin "web build" "1 min"
if quiet web.log sh -c 'cd web && npm ci --no-audit --no-fund && npm run build'; then
  record "web build" PASS ""
else
  record "web build" FAIL "see web.log"
fi

# 3. Everything: the desktop app, m2, the headless server and every test binary.
begin build "5 min; 15 the first time"
if quiet build.log cargo build --locked --workspace --all-targets; then
  record build PASS ""
else
  record build FAIL "see build.log"
  { first "$OUT/build.log" '^error' | sed 's/^/build: /'; cause "$OUT/build.log"; } >> "$OUT/failed"
fi

# 4. Linux only: the desktop app, opened on an invisible screen (Xvfb), never on one you can see,
#    with a scratch HOME and no sound: once with examples/first-song, once with
#    examples/with-plugins and the test plugins. Each run passes when the app's log says its UI
#    connected within 30 s and nothing panicked. A screenshot of each lands beside the logs,
#    taken by ci/xshot.c, built here (the row passes without one, and says so). macOS: skipped,
#    since the app would open on your screen.
# app <name> <project> [VAR=value...]: one run, on a copy of <project>; its log is app-<name>.log.
# Fails, with a line in $OUT/failed, unless the UI connected and nothing panicked; SAID tells how
# long the UI took.
app() {
  name=$1
  dir=$SCR/app-$1
  if ! err=$({ mkdir -p "$dir/home" "$dir/run" && chmod 700 "$dir/run" && cp -R "$2" "$dir/song"; } 2>&1); then
    echo "app $name: $2 couldn't be copied: $(echo "$err" | head -1 | cut -c1-120)" >> "$OUT/failed"
    return 1
  fi
  shift 2
  log=$OUT/app-$name.log
  env -i PATH=/usr/bin:/bin HOME="$dir/home" DISPLAY=":$DISPLAYN" XDG_RUNTIME_DIR="$dir/run" M2_AUDIO=none M2_MIDI=none \
    WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1 "$@" "$TARGET/debug/music2-desktop" "$dir/song" \
    > "$log" 2>&1 < /dev/null &
  APP=$!
  t=0
  while [ $t -lt 300 ] && ! grep -q 'UI connected to the native shell' "$log" && kill -0 "$APP" 2> /dev/null; do
    sleep 0.1
    t=$((t + 1))
  done
  if ! grep -q 'UI connected to the native shell' "$log"; then
    quitapp
    echo "app $name: the UI didn't connect within 30 s: see app-$name.log" >> "$OUT/failed"
    return 1
  fi
  # Let the window draw its song, then keep a picture of it.
  sleep 5
  if [ -x "$SCR/xshot" ]; then DISPLAY=":$DISPLAYN" "$SCR/xshot" "$OUT/app-$name.png" >> "$OUT/xshot.log" 2>&1; fi
  quitapp
  if grep -q 'panicked' "$log"; then
    echo "app $name: $(first "$log" 'panicked')" >> "$OUT/failed"
    return 1
  fi
  SAID="$SAID${SAID:+, }$name in $(( (t + 9) / 10 )) s"
}
# quitapp: close the app the app row opened.
quitapp() {
  kill "$APP" 2> /dev/null
  sleep 1
  kill -9 "$APP" 2> /dev/null
  wait "$APP" 2> /dev/null
  APP=
}
if [ $OS = linux ] && ! command -v Xvfb > /dev/null; then
  record app SKIP "no Xvfb: install it (apt install xvfb) to open the app on an invisible screen"
elif [ $OS = linux ]; then
  begin app "1 min"
  SAID=
  notes=
  if ! cc -O2 -o "$SCR/xshot" ci/xshot.c -lX11 -lpng > "$OUT/xshot.log" 2>&1; then
    notes="; no screenshots: ci/xshot.c didn't build (X11 and libpng headers?)"
  fi
  # Xvfb picks a free display itself and writes its number to fd 9.
  Xvfb -displayfd 9 -screen 0 1600x1000x24 -nolisten tcp 9> "$SCR/display" > "$OUT/xvfb.log" 2>&1 &
  XVFB=$!
  t=0
  while [ $t -lt 100 ] && [ ! -s "$SCR/display" ]; do sleep 0.1; t=$((t + 1)); done
  DISPLAYN=$(tr -dc 0-9 < "$SCR/display")
  ok=0
  if [ -z "$DISPLAYN" ]; then
    ok=1
    echo "app: Xvfb didn't start: see xvfb.log" >> "$OUT/failed"
  elif [ ! -x "$TARGET/debug/music2-desktop" ]; then
    ok=1
    echo "app: no $TARGET/debug/music2-desktop (the build failed)" >> "$OUT/failed"
  else
    app first-song examples/first-song || ok=1
    if [ -n "${CLAP_PATH:-}" ]; then
      app with-plugins examples/with-plugins CLAP_PATH="$CLAP_PATH" || ok=1
    else
      app with-plugins examples/with-plugins || ok=1
    fi
  fi
  closeapp
  # The app hosts plugins on every OS (HOST-2, OS-26).
  for log in "$OUT"/app-*.log; do
    if grep -q "plugins can't be hosted" "$log" 2> /dev/null; then
      ok=1
      echo "app $(basename "$log" .log | sed 's/^app-//'): $(first "$log" "plugins can't be hosted")" >> "$OUT/failed"
    fi
  done
  if [ $ok -eq 0 ]; then
    record app PASS "the UI connected: $SAID$notes"
  else
    record app FAIL "see \"what failed\"$notes"
  fi
fi

# 5. Every Rust test, the desktop app's included, with a scratch HOME. A test binary that failed
#    runs once more alone; if every one passes then, the run passes and names them as retried.
#    Otherwise "what failed" names what failed again, and what passed then is named as retried.
begin "rust tests" "10 min"
quiet test.log env HOME="$OUT/home" cargo test --locked --workspace --no-fail-fast
st=$?
counts=$(tally "$OUT/test.log" "")
grep -o 'to rerun pass `[^`]*`' "$OUT/test.log" | sed 's/^to rerun pass `//; s/`$//' > "$SCR/binaries"
again=1
if [ $st -ne 0 ] && [ -s "$SCR/binaries" ]; then
  again=0
  : > "$OUT/test-retry.log"
  while read -r args; do
    # cargo's own "-p <crate> --lib" or "--test <name>": split into words on purpose.
    env HOME="$OUT/home" cargo test --locked --no-fail-fast $args >> "$OUT/test-retry.log" 2>&1 < /dev/null || again=1
  done < "$SCR/binaries"
  tally "$OUT/test-retry.log" again- > /dev/null
fi
# retried: what failed at first but not on its retry, for "passed only on a retry".
retried() {
  comm -23 "$SCR/tests" "$SCR/again-tests" | sed 's/^/test /'
  cut -f1 "$SCR/whole" | sort -u | comm -23 - "$SCR/again-binaries" | sed 's/^/test binary /'
}
if [ $st -eq 0 ]; then
  record "rust tests" PASS "$counts"
elif [ $again -eq 0 ]; then
  record "rust tests" PASS "$counts; passed on a retry"
  : > "$SCR/again-binaries"
  retried > "$SCR/retried"
  # A failure "what failed" can't name (a failed doctest): the binaries, by cargo's arguments.
  if [ -s "$SCR/retried" ]; then cat "$SCR/retried"; else sed 's/^/test binary /' "$SCR/binaries"; fi >> "$OUT/flaky"
elif [ -s "$SCR/binaries" ]; then
  record "rust tests" FAIL "$counts: see test.log and test-retry.log"
  # Only what failed again; a binary that failed again with nothing named is listed by its arguments.
  cut -f1 "$SCR/again-whole" | sort -u > "$SCR/again-binaries"
  failures again- "$OUT/test-retry.log" >> "$OUT/failed"
  if [ ! -s "$SCR/again-tests" ] && [ ! -s "$SCR/again-whole" ]; then
    grep -o 'to rerun pass `[^`]*`' "$OUT/test-retry.log" | sed 's/^to rerun pass `/test binary /; s/`$//' >> "$OUT/failed"
  fi
  retried >> "$OUT/flaky"
else
  record "rust tests" FAIL "$counts: see test.log"
  failures "" "$OUT/test.log" >> "$OUT/failed"
  { first "$OUT/test.log" '^error' | sed 's/^/tests: /'; cause "$OUT/test.log"; } >> "$OUT/failed"
fi

# 6. Clippy, every warning an error.
begin clippy "2 min"
if quiet clippy.log cargo clippy --locked --workspace --all-targets -- -D warnings; then
  record clippy PASS ""
else
  # Count the lints, not cargo's "could not compile" and "build failed" lines.
  n=$(grep -E '^(warning|error)(\[[^]]*\])?: ' "$OUT/clippy.log" | grep -cvE '^(error: could not compile|warning: build failed|error: aborting)')
  record clippy FAIL "$n findings: see clippy.log"
  first "$OUT/clippy.log" '^ *--> ' | sed 's/^ *--> /clippy: /' >> "$OUT/failed"
fi

# 6b. The pool's models: m2-pool's protocol under a sync shim that yields and spins at random before
#     each atomic step and each ring and wait of the caller's wake, in release, thousands of times
#     (core/pool/tests/models.rs; `the_caller_is_woken` is the wake's), with its unit tests built
#     the same way. The crate needs only m2-os and libc, so this builds in seconds. loom isn't
#     used: it isn't in the offline registry. x86 keeps stores in order, so an ordering too weak
#     for arm64 may pass on Linux and fail only on a Mac. No retry: a model that fails is a bug.
begin "pool models" "10 s"
if quiet pool-models.log cargo test --locked --release -p m2-pool --features models &&
   quiet pool-lint.log cargo clippy --locked -p m2-pool --features models --all-targets -- -D warnings; then
  record "pool models" PASS "$(tally "$OUT/pool-models.log" pool-)"
else
  record "pool models" FAIL "see pool-models.log and pool-lint.log"
  tally "$OUT/pool-models.log" pool- > /dev/null
  failures pool- "$OUT/pool-models.log" >> "$OUT/failed"
  first "$OUT/pool-lint.log" '^ *--> ' | sed 's/^ *--> /pool lint: /' >> "$OUT/failed"
fi

# 7. std: the kernels' own workspace (tests and lints), and std.wasm rebuilt to the committed
#    bytes in another folder (PLAT-5).
begin "std kernels" "2 min"
if quiet kernels.log sh -c 'cd std/kernels-src && cargo test --locked --release --workspace && cargo clippy --locked --workspace --all-targets -- -D warnings'; then
  record "std kernels" PASS "tests and clippy"
else
  record "std kernels" FAIL "see kernels.log"
fi
begin std.wasm "2 min"
if quiet std-wasm.log std/build.sh --check; then
  record std.wasm PASS "rebuilt to the same bytes in another folder"
else
  record std.wasm FAIL "$(tail -1 "$OUT/std-wasm.log" | cut -c1-100)"
fi

# 8. Linux only: the macOS-only code, type-checked and linted for aarch64-apple-darwin. There is
#    no macOS SDK here, so ci/stub-cc and ci/stub-ar stand in for C; clippy never links.
if [ $OS = linux ]; then
  begin "mac lint" "1 min; 3 the first time"
  quiet mac-target.log rustup target add aarch64-apple-darwin
  if quiet mac-lint.log env CC_aarch64_apple_darwin="$ROOT/ci/stub-cc" CXX_aarch64_apple_darwin="$ROOT/ci/stub-cc" \
      AR_aarch64_apple_darwin="$ROOT/ci/stub-ar" \
      cargo clippy --locked --target aarch64-apple-darwin --workspace --all-targets -- -D warnings; then
    record "mac lint" PASS "the macOS code type-checks and lints"
  else
    record "mac lint" FAIL "see mac-lint.log"
    first "$OUT/mac-lint.log" '^ *--> ' | sed 's/^ *--> /mac lint: /' >> "$OUT/failed"
  fi
fi

# 9. Every browser suite, each on a fresh headless server with a scratch HOME (e2e/run.mjs), $JOBS at
#    a time (the suites run.mjs marks serial after the others, alone). A run that failed runs once
#    more, alone, with nothing else of run.mjs's running; if it passes then, it is named as retried.
if [ "$JOBS" -gt 1 ]; then begin "browser suites" "4 min"; else begin "browser suites" "10 min"; fi
quiet e2e-npm.log sh -c 'cd e2e && npm ci --no-audit --no-fund'
if [ ! -x "$CHROME" ]; then
  (cd e2e && node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path "$HOME/.cache/puppeteer") \
    >> "$OUT/e2e-npm.log" 2>&1
fi
quiet e2e.log suites all --jobs "$JOBS" --out "$OUT/e2e"
st=$?
line=$(grep -E '^[0-9]+/[0-9]+ suites ok' "$OUT/e2e.log" | tail -1 | cut -d';' -f1)
took=$(grep -m1 '^time: ' "$OUT/e2e.log" | sed 's/^time: //; s/,.*//')
parts=$(grep -c '^  PART:' "$OUT/e2e.log")
others=$(grep '^  PART:' "$OUT/e2e.log" | grep -vc 'live3b-proj')
grep '^  failed: ' "$OUT/e2e.log" | awk '{print $2, $NF}' > "$SCR/runs"
again=0
while read -r run log; do
  if suites "${run%-*}" "${run##*-}" --out "$OUT/e2e-retry" >> "$OUT/e2e-retry.log" 2>&1 < /dev/null; then
    echo "suite $run" >> "$OUT/flaky"
  else
    again=1
    # Its first FAIL line, or the error that stopped it (a suite that throws prints no FAIL).
    { echo "suite $run"; echo "  $(first "$log" '^FAIL|^[A-Za-z]*Error: ')"; } >> "$OUT/failed"
  fi
done < "$SCR/runs"
# No total line, or an exit 1 that names no failed suite: run.mjs itself stopped (a crash exits 1
# too), so nothing can be trusted.
if [ -z "$line" ] || { [ $st -ne 0 ] && [ $st -ne 1 ] && [ $st -ne 3 ]; } || { [ $st -eq 1 ] && [ ! -s "$SCR/runs" ]; }; then
  record "browser suites" FAIL "run.mjs stopped (exit $st): see e2e.log"
  echo "browser suites: run.mjs stopped (exit $st): $(tail -1 "$OUT/e2e.log" | cut -c1-120)" >> "$OUT/failed"
elif [ $again -ne 0 ] || [ "$others" -gt 0 ] || { [ "$parts" -gt 0 ] && [ -n "${CLAP_PATH:-}" ]; }; then
  record "browser suites" FAIL "$line: see e2e.log"
  grep '^  PART:' "$OUT/e2e.log" | sed 's/^  PART: */suite /' | cut -c1-160 >> "$OUT/failed"
elif [ "$parts" -gt 0 ]; then
  record "browser suites" PART "$line (live3b without plugins)"
elif [ -s "$SCR/runs" ]; then
  record "browser suites" PASS "$line, $JOBS at a time, $took; passed on a retry"
else
  record "browser suites" PASS "$line, $JOBS at a time, $took"
fi

# 10. The offline renders with the debug m2 from step 3 (it renders the release m2's bits),
#     against the one record every OS shares (PLAT-4).
begin renders "1 min"
want=$(grep -c . "$ROOT/e2e/renders.sha256")
if quiet renders.log env M2="$TARGET/debug/m2" e2e/baseline.sh --tree . "$OUT/renders" &&
   (cd "$OUT/renders" && shasum -a 256 -c "$ROOT/e2e/renders.sha256") > "$OUT/renders-check.log" 2>&1; then
  record renders PASS "$want of $want match e2e/renders.sha256"
else
  n=$(grep -c ': OK$' "$OUT/renders-check.log" 2> /dev/null)
  record renders FAIL "${n:-0} of $want match: see renders-check.log"
  grep -v ': OK$' "$OUT/renders-check.log" 2> /dev/null | sed 's/^/render /' >> "$OUT/failed"
fi

# 11. The timing gates ci/gates.txt names, in release: on a Mac by default, on Linux with --gates,
#     or with --gates=auto unless its rule skipped them (step 0b; then the row says SKIP and why).
#     Only the reference Mac's timings are judged; elsewhere they are recorded (PLAT-1). First, G7f's
#     old engine: S, the commit before step 7, which G7f measures beside the app in every run. Its
#     runner is built from a clean copy of S, in a cache folder outside the repo, and kept there
#     (crates/gates/s/README.md).
if [ -n "$GATES" ]; then
  if [ $OS = macos ]; then SCACHE=$HOME/m2-check/s-8aa301e; else SCACHE=${XDG_CACHE_HOME:-$HOME/.cache}/m2-check/s-8aa301e; fi
  begin "old engine (S)" "seconds; 3 min the first time"
  echo "                 building the old engine (S) for G7f in $SCACHE"
  if quiet gates-s.log crates/gates/s/build.sh "$SCACHE" && [ -x "$(tail -1 "$OUT/gates-s.log")" ]; then
    M2_GATES_S=$(tail -1 "$OUT/gates-s.log")
    export M2_GATES_S
    record "old engine (S)" PASS "built for G7f from a clean copy of 8aa301e, in $SCACHE"
  else
    unset M2_GATES_S
    record "old engine (S)" FAIL "it didn't build, so G7f can't run: see gates-s.log"
    echo "old engine (S): $(grep -m1 -E '^build\.sh: |^error' "$OUT/gates-s.log" | cut -c1-140)" >> "$OUT/failed"
  fi
  begin gates "23 min"
  # ci/gates.txt: one m2-gates run a line; # starts a comment.
  grep -v -e '^#' -e '^[[:space:]]*$' ci/gates.txt > "$SCR/gates"
  bad=
  : > "$OUT/gates.log"
  if quiet gates-build.log cargo build --locked --release -p m2-gates; then
    while read -r args; do
      echo "=== m2-gates $args" >> "$OUT/gates.log"
      # The line's m2-gates options: split into words on purpose.
      env HOME="$OUT/home" "$TARGET/release/m2-gates" $args >> "$OUT/gates.log" 2>&1 < /dev/null || bad="$bad${bad:+; }$args"
    done < "$SCR/gates"
  else
    bad=build
    first "$OUT/gates-build.log" '^error' | sed 's/^/gates: /' >> "$OUT/failed"
  fi
  # A Mac S's records don't name runs G7f but can't judge its timings (PLAT-1): the row says PART, and
  # so does the check's first line, never PASS.
  # With --gates=auto, why they ran.
  if [ -n "$AUTO" ]; then auto=" (--gates=auto: $GATESWHY)"; else auto=; fi
  if [ -z "$bad" ] && grep -q '^      skip  .*not judged on this Mac (PLAT-1)' "$OUT/gates.log"; then
    record gates PART "the $(grep -c '' "$SCR/gates") runs ci/gates.txt names, but G7f wasn't judged: this Mac isn't the one S's records name (see gates)$auto"
  elif [ -z "$bad" ]; then
    # Linux: recorded as passed, so --gates=auto may skip them on the same files later.
    kept=
    if [ $OS = linux ] && [ -z "$RULE" ]; then
      kept="; not recorded for --gates=auto: the gates rule row failed"
    elif [ $OS = linux ] && [ -z "$TREE0" ]; then
      kept="; not recorded for --gates=auto: when the check began, $(head -1 "$OUT/gates-unseen.log")"
    elif [ $OS = linux ]; then
      kept="; $(gates_record "$TREE0" "$SHA" 2>&1 | head -1)"
    fi
    record gates PASS "the $(grep -c '' "$SCR/gates") runs ci/gates.txt names$auto$kept"
  else
    record gates FAIL "$bad: see gates.log$auto"
  fi
  # First the machine m2-gates names, once: only on the one S's records name are G7f's timings
  # judged, and on a Mac they don't name it is the line to copy into them (PLAT-1). Then each run,
  # its verdicts, and each judged check with its numbers (p95 or p99, or the CPU as a share of a
  # core), so a green run's timings can be recorded from the summary alone; and each
  # comparison with a record that isn't from this machine, so not judged (PLAT-1), with the two
  # machines, so a record's machine can be corrected from the summary alone too. G7f's proof that
  # the app and S played the same music, and what it printed beside its verdict: P1 against S
  # (step 7 with wasmtime's default layout), the app's layout against P1, and what starting a turn
  # cost each side (the steps it leaves out, and whether any is left in the judged ones); S measured
  # now beside S's record (on the reference Mac only: if the two differ by much, doubt the runner
  # first); the build and footprint checks, whose ok lines have no p99; and G12's live set-up and
  # dropouts, and its note when this Mac only stands in for gate 8's.
  { grep -m1 '^machine: ' "$OUT/gates.log"; grep -E '^=== |^(PASS|FAIL|SKIP)  |^      FAIL  |^      ok    .*(p95|p99|of a core)|^      skip  .*\(PLAT-1\)|^      info  machines: |^      ok    the same music|^      info  (block|callback), (P1 against S|the app.s layout against P1|starting a turn)|^      info  S as recorded: |^      ok    (full build within|footprint within) |^      ok    .*: (set-up|dropouts): |^      info  gate 8: ' "$OUT/gates.log"; } 2> /dev/null |
    cut -c1-220 > "$OUT/gates.sum"
elif [ -n "$SKIPPED" ]; then
  # Never a PASS: nothing was measured.
  record gates SKIP "$SKIPPED"
fi

# 12. The check changed nothing in the repo.
begin "repo unchanged" "seconds"
git status --porcelain > "$SCR/after"
if cmp -s "$SCR/before" "$SCR/after"; then
  record "repo unchanged" PASS ""
else
  record "repo unchanged" FAIL "$(diff "$SCR/before" "$SCR/after" | grep -c '^[<>]') entries changed: run git status"
fi

trap '' INT TERM
summary
[ "$fails" -eq 0 ] && [ -n "$TESTPLUGINS" ] && [ -z "$partial" ]
