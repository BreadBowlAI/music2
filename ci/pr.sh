#!/bin/bash
# ci/pr.sh: the PR tier of music2's checks, one step at a time. Each job of
# .github/workflows/ci.yml runs one step, with the same command you type here; ci/check.sh --pr
# runs every step, one after another, on your machine.
#
# In short: the PR tier is what every pull request must pass, quickly. It holds every check that
# gives the same answer on any machine and any day: no timing gates, none of the tests
# ci/nightly-tests.txt and e2e/run.mjs tag for the nightly tier (a speed bound, a known flake, over
# a minute), and RustSec's advisories only when the change touches a Cargo.lock or deny.toml (they
# change with the date, not the code). The rest runs every night: ci/check.sh does everything this
# script does (it runs the lint and advisories steps as rows of its own) and more. Nothing here
# changes the repo, opens a screen, plays sound, or touches your ~/Music or ~/.music2: whatever
# runs music2 gets a scratch HOME.
#
#   ci/pr.sh changes [<base>]   whether the change since git merge-base HEAD <base> (default: main,
#                               or origin/main) is documentation only (ci/docs-only.sh). Prints the
#                               verdict, then docs_only=true or docs_only=false, then deps=true when
#                               it touches a Cargo.lock or deny.toml (or git can't say), else
#                               deps=false (both also into $GITHUB_OUTPUT when set). Exit 0 either
#                               way, 2 on a usage error.
#   ci/pr.sh docs               the docs job: the docs rule's own cases (ci/docs-only-test.sh)
#   ci/pr.sh toolchain          the Rust toolchain rust-toolchain.toml pins, installed if missing
#                               (CI's jobs run it before their cache step, which asks rustc)
#   ci/pr.sh lint               the checks that build nothing: rustfmt, actionlint, licences
#                               (cargo-deny's bans, licences and sources on both workspaces; npm),
#                               and the cases of the docs rule, of the PR tier's own verdicts
#                               (ci/pr-test.sh) and of ci/nightly.sh (ci/nightly-test.sh)
#   ci/pr.sh quick              lint's checks, then the cases of the gates rule, the pool's models,
#                               the web typecheck and build, std's kernels (tests, clippy) and
#                               std.wasm rebuilt to the committed bytes
#   ci/pr.sh models             the pool's models alone (core/pool/tests/models.rs, in release, and
#                               their lint), as ci/check.sh's "pool models" row: the Mac's nightly
#                               job runs them, as arm64 can catch an ordering too weak that x86 hides
#   ci/pr.sh advisories         RustSec's advisories for both workspaces (cargo-deny; it fetches
#                               their database): on a pull request when changes says deps=true, and
#                               every night
#   ci/pr.sh clippy             clippy on every target, every warning an error
#   ci/pr.sh rust [<i>/<n>]     the Rust tests of the PR tier, part i of n (cargo-nextest's hash
#                               partitions, so a new test lands in one part and the parts stay even);
#                               part 1 also runs the two tests that own their main thread
#                               (main_thread, app_main_thread), which nextest can't list
#   ci/pr.sh suites [<i>/<n>]   the browser suites of the PR tier (e2e/run.mjs all --tier pr --shard),
#                               M2_PR_JOBS at a time (default 2); a run that failed runs once more,
#                               alone, as in ci/check.sh, and is named if it passes then; a run
#                               that was PART (passed, but not in full) fails the step
#   ci/pr.sh renders            the 12 offline renders, against e2e/renders.sha256
#   rust and suites take --all: the nightly-tagged tests too (the Mac's nightly job).
#
# Each step prints a line per check, "PASS", "FAIL" or "SKIP" and why, then
# "ci/pr.sh <step>: PASS|FAIL in <time>", and exits 0 only when every check passed. A failed
# check's log ends the output (its last 60 lines); every log is in M2_PR_OUT (default
# ~/m2-check/pr-<commit>/<step>; only a folder a check made is ever replaced).
#
# Builds: cargo --locked, with CARGO_PROFILE_DEV_DEBUG=0 and CARGO_INCREMENTAL=0 (a debug build
# with debug info is 31 GB, more than a CI runner's disk), into <CARGO_TARGET_DIR or target>/pr,
# so it never rebuilds what your own `cargo build` made. nextest runs no doctests: there are none
# today (2026-10-21), and the nightly tier's cargo test would run any.
#
# Needs: rustup and Node 24 (as ci/check.sh); cargo-nextest, cargo-deny and actionlint at the
# versions ci/tools.sh pins (it fetches them into ~/.cache/m2-tools); for rust and suites, the test
# plugins (CLAP_PATH, or ci/check.sh's default folder; on Linux ci/plugins.sh installs them). On
# Linux the Rust steps need the app's libraries: ci/apt.sh lists them. M2_NO_PLUGINS=1 runs without
# the plugins, their tests skipping, and says so (the Mac's nightly job: no Mac copy is pinned).
# Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
STEP=${1:-}
[ $# -gt 0 ] && shift
usage() { sed -n '/^#   ci\/pr.sh changes/,/^#   rust and suites take/p' "$0" | sed 's/^# //' >&2; exit 2; }
PART=1/1
ALL=
BASE=
case $STEP in
  changes) if [ $# -gt 1 ]; then usage; fi; BASE=${1:-} ;;
  docs|toolchain|lint|quick|models|advisories|clippy|renders) if [ $# -gt 0 ]; then usage; fi ;;
  rust|suites)
    while [ $# -gt 0 ]; do
      case $1 in
        --all) ALL=1 ;;
        [1-9]*/[1-9]*) PART=$1 ;;
        *) usage ;;
      esac
      shift
    done
    case $PART in *[!0-9/]*|*/*/*) usage ;; esac
    if [ "${PART%/*}" -gt "${PART#*/}" ]; then usage; fi ;;
  *) usage ;;
esac

case $(uname -s) in
  Darwin) OS=macos; PLUGINS=$HOME/Library/Audio/Plug-Ins/CLAP ;;
  Linux) OS=linux; PLUGINS=$HOME/.local/share/m2-test-plugins/clap ;;
  *) echo "ci/pr.sh runs on macOS and Linux"; exit 2 ;;
esac
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export M2_AUDIO=none M2_MIDI=none CARGO_TERM_COLOR=never CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}/pr
case $CARGO_TARGET_DIR in /*) ;; *) CARGO_TARGET_DIR=$ROOT/$CARGO_TARGET_DIR ;; esac
TARGET=$CARGO_TARGET_DIR
CHROME_VERSION=133.0.6943.141
if [ $OS = macos ]; then CHROME_DIR=mac_arm-$CHROME_VERSION/chrome-headless-shell-mac-arm64; else CHROME_DIR=linux-$CHROME_VERSION/chrome-headless-shell-linux64; fi
CHROME=${E2E_CHROME:-$HOME/.cache/puppeteer/chrome-headless-shell/$CHROME_DIR/chrome-headless-shell}
SHA=$(git rev-parse --short=12 HEAD) || exit 2
START=$(date +%s)
TAB=$(printf '\t')

# changes: no logs, no builds; the first job of ci.yml, so it must be quick and say one thing.
if [ "$STEP" = changes ]; then
  . ci/docs-only.sh || exit 2
  if [ -z "$BASE" ]; then
    if git rev-parse --verify --quiet main^{commit} > /dev/null; then BASE=main; else BASE=origin/main; fi
  fi
  if [ -n "$(git status --porcelain 2> /dev/null)" ]; then
    verdict="full  the tree isn't clean: uncommitted or untracked files can't be judged"
  else
    verdict=$(docs_only "$BASE")
  fi
  printf '%s\n' "$verdict"
  case $verdict in docs\ *) d=true ;; *) d=false ;; esac
  # deps: whether the change touches what RustSec's advisories are judged on (a Cargo.lock) or how
  # (deny.toml). Anything git can't say, or a tree that isn't clean: yes.
  deps=true
  if [ -z "$(git status --porcelain 2> /dev/null)" ] && mb=$(git merge-base HEAD "$BASE" 2> /dev/null) &&
     files=$(git diff --no-renames --name-only "$mb" HEAD 2> /dev/null); then
    if ! printf '%s\n' "$files" | grep -qE '(^|/)Cargo\.lock$|^deny\.toml$'; then deps=false; fi
  fi
  echo "docs_only=$d"
  echo "deps=$deps"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then printf 'docs_only=%s\ndeps=%s\n' "$d" "$deps" >> "$GITHUB_OUTPUT"; fi
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    { echo "### Docs only: $d; a Cargo.lock or deny.toml changed: $deps"; echo; echo '```'; printf '%s\n' "$verdict" | head -30; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
  fi
  exit 0
fi

OUT=${M2_PR_OUT:-$HOME/m2-check/pr-$SHA/$STEP}
if [ -e "$OUT" ] && [ ! -e "$OUT/.m2-check" ]; then
  echo "ci/pr.sh: $OUT wasn't made by a check, so it is left alone: name another folder in M2_PR_OUT"
  exit 2
fi
rm -rf "$OUT" && mkdir -p "$OUT/home" && : > "$OUT/.m2-check" || exit 2
: > "$OUT/results"
BEFORE=$(git status --porcelain)

# row <name> <PASS|FAIL|SKIP> <note>: one line of the step's results.
row() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$OUT/results"
  printf '%-16s %-4s  %s\n' "$1" "$2" "$3"
}
# check <name> <log> <command...>: run it with its output in $OUT/<log>; a row says how it went
# and how long it took. On a failure the log's tail goes to $OUT/tails, printed at the end.
check() {
  local name=$1 log=$OUT/$2 t0 st
  shift 2
  t0=$(date +%s)
  "$@" > "$log" 2>&1 < /dev/null
  st=$?
  if [ $st -eq 0 ]; then
    row "$name" PASS "$(( $(date +%s) - t0 )) s"
  else
    row "$name" FAIL "exit $st after $(( $(date +%s) - t0 )) s: see $(basename "$log")"
    { echo; echo "=== $name: the last 60 lines of $log"; tail -60 "$log"; } >> "$OUT/tails"
  fi
  return $st
}
# tools <names...>: put ci/tools.sh's pinned tools on PATH, or say why not.
tools() {
  local p
  if ! p=$(ci/tools.sh "$@" 2> "$OUT/tools.log" | tail -1) || [ -z "$p" ]; then
    row tools FAIL "ci/tools.sh couldn't get $*: see tools.log"
    { echo; echo "=== tools"; cat "$OUT/tools.log"; } >> "$OUT/tails"
    return 1
  fi
  PATH=$p:$PATH
}
# web: the web UI's build. The desktop app's build script needs web/dist, and the servers serve it.
web() { (cd web && npm ci --no-audit --no-fund && npm run build); }
# plugins: CLAP_PATH for the tests that host Surge XT and Airwindows (PLAT-6), or why not.
plugins() {
  if [ -z "${CLAP_PATH:-}" ] && [ -e "$PLUGINS/Surge XT.clap" ] && [ -e "$PLUGINS/Airwindows Consolidated.clap" ]; then
    CLAP_PATH=$PLUGINS
  fi
  if [ -e "${CLAP_PATH:-}/Surge XT.clap" ] && [ -e "${CLAP_PATH:-}/Airwindows Consolidated.clap" ]; then
    export CLAP_PATH
    row plugins PASS "$CLAP_PATH"
  elif [ -n "${M2_NO_PLUGINS:-}" ]; then
    unset CLAP_PATH
    row plugins SKIP "M2_NO_PLUGINS is set: the tests that host plugins skip"
  else
    row plugins FAIL "Surge XT.clap and Airwindows Consolidated.clap aren't both in ${CLAP_PATH:-$PLUGINS} (on Linux, ci/plugins.sh installs them)"
    return 1
  fi
}
# suites <run.mjs arguments>: the browser suites, each on a fresh headless server.
suites() { env E2E_CHROME="$CHROME" M2_HEADLESS="$TARGET/debug/examples/headless" node e2e/run.mjs "$@"; }
# build: the binaries the browser suites and the renders use, in one cargo run (two would resolve
# features apart and build the shared crates twice).
build() { cargo build --locked -p m2-server --example headless -p m2 -p m2-plugins --bin m2 --bin m2-plugin-host; }
# nightly_filter, nightly_list <dir>: ci/nightly-tests.txt as nextest sees it (ci/check.sh judges
# the list with the same functions).
. ci/nightly-list.sh || exit 2
# lint: the PR tier's checks that build nothing (its own step, and the start of quick).
lint() {
  check fmt fmt.log sh -c 'cargo fmt --all --check && cd std/kernels-src && cargo fmt --all --check'
  if tools actionlint deny; then
    # No shellcheck or pyflakes inside actionlint: their versions differ from machine to machine,
    # and every run: block only calls a script here.
    check workflows actionlint.log sh -c 'actionlint -color -shellcheck= -pyflakes= && node ci/ci-ok.mjs --check-workflow'
    # No advisories (the advisories step), so no advisory database is fetched: the same answer every day.
    check licences licences.log sh -c 'cargo deny --locked --color never --config deny.toml check bans licenses sources &&
      cargo deny --locked --color never --manifest-path std/kernels-src/Cargo.toml --config deny.toml check -A license-not-encountered bans licenses sources &&
      node ci/npm-licenses.mjs'
  fi
  check "docs rule" docs-rule.log ci/docs-only-test.sh
  check "pr verdicts" pr-test.log ci/pr-test.sh
  check "nightly.sh" nightly-test.log ci/nightly-test.sh
}
# models: the pool's models, in release, and their lint (the same commands as ci/check.sh's step
# 6b). m2-pool needs only m2-os and libc, so this builds in seconds, with no system library.
models() {
  cargo test --locked --release -p m2-pool --features models &&
    cargo clippy --locked -p m2-pool --features models --all-targets -- -D warnings
}

# toolchain: the one rust-toolchain.toml names, installed if missing (rustup 1.28 and later take
# no arguments for that; an older one installs it when asked what is active), then checked.
toolchain() {
  want=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
  { rustup toolchain install || rustup show active-toolchain; } && rustc -V &&
    rustc -V | grep -q "^rustc $want " || { echo "not Rust $want"; return 1; }
}

case $STEP in
  docs)
    check "docs rule" docs-rule.log ci/docs-only-test.sh ;;

  toolchain)
    check toolchain toolchain.log toolchain && row rustc PASS "$(rustc -V)" ;;

  lint)
    lint ;;

  quick)
    lint
    check "gates rule" gates-rule.log ci/gates-auto-test.sh
    check "pool models" pool-models.log models
    check "web build" web.log web
    check "std kernels" kernels.log sh -c 'cd std/kernels-src && cargo test --locked --release --workspace && cargo clippy --locked --workspace --all-targets -- -D warnings'
    check std.wasm std-wasm.log std/build.sh --check ;;

  models)
    check "pool models" pool-models.log models ;;

  advisories)
    # std's kernels use no wasmtime, so deny.toml's wasmtime advisories aren't there to ignore.
    if tools deny; then
      check advisories advisories.log sh -c 'cargo deny --locked --color never --config deny.toml check advisories &&
        cargo deny --locked --color never --manifest-path std/kernels-src/Cargo.toml --config deny.toml check -A advisory-not-detected advisories'
    fi ;;

  clippy)
    check "web build" web.log web &&
      check clippy clippy.log cargo clippy --locked --workspace --all-targets -- -D warnings ;;

  rust)
    filter="not (binary(=main_thread) | binary(=app_main_thread))"
    if [ -z "$ALL" ]; then filter="$filter and not ($(nightly_filter))"; fi
    if tools nextest && plugins && check "web build" web.log web &&
       check "build tests" build.log cargo nextest run --locked --workspace --no-run -E "$filter"; then
      check "rust tests" test.log env HOME="$OUT/home" cargo nextest run --locked --workspace --no-tests=warn \
        --partition "hash:$PART" -E "$filter"
      # What nextest said: the counts, and each test that passed only when run again.
      sum=$(grep -E '^ *Summary \[' "$OUT/test.log" | tail -1 | sed 's/^ *//')
      if [ -n "$sum" ]; then echo "                       $sum"; fi
      grep -E '^ *FLAKY ' "$OUT/test.log" | sed 's/^ */                       /'
      if [ -n "${GITHUB_ACTIONS:-}" ]; then
        grep -E '^ *FLAKY ' "$OUT/test.log" | sed 's/^ */::warning title=Passed only on a retry::/'
      fi
      if [ "${PART%/*}" = 1 ]; then
        check "own-main tests" main-thread.log env HOME="$OUT/home" cargo test --locked --workspace --test main_thread --test app_main_thread
        # Every line of ci/nightly-tests.txt names a test that is there, so a renamed test can't
        # slip back into the PR tier unseen (Linux: some names are Linux's own modules).
        if [ $OS = linux ]; then check "nightly list" nightly-list.log nightly_list "$OUT"; fi
      fi
    fi ;;

  suites)
    if [ -n "$ALL" ]; then tier=; else tier="--tier pr"; fi
    JOBS=${M2_PR_JOBS:-2}
    # Without the test plugins (M2_NO_PLUGINS), the runs that need them are left out, and said.
    if [ -n "${M2_NO_PLUGINS:-}" ] && [ -z "${CLAP_PATH:-}" ] && ! { [ -e "$PLUGINS/Surge XT.clap" ] && [ -e "$PLUGINS/Airwindows Consolidated.clap" ]; }; then
      tier="$tier --no-clap"
    fi
    if plugins && check "web build" web.log web &&
       check build build.log build &&
       check "e2e npm" e2e-npm.log sh -c 'cd e2e && npm ci --no-audit --no-fund' &&
       { [ -x "$CHROME" ] || check chrome chrome.log sh -c "cd e2e && node_modules/.bin/browsers install chrome-headless-shell@$CHROME_VERSION --path \"\$HOME/.cache/puppeteer\""; }; then
      # $tier: no option, "--tier pr", "--no-clap" or both: split into words on purpose.
      t0=$(date +%s)
      suites all $tier --shard "$PART" --jobs "$JOBS" --out "$OUT/e2e" > "$OUT/e2e.log" 2>&1 < /dev/null
      st=$?
      took="$(( $(date +%s) - t0 )) s"
      grep -E '^(tier|shard|--no-clap|[0-9]+/[0-9]+ suites ok|time:)|^  (PART|failed):' "$OUT/e2e.log" | sed 's/^/                       /'
      # Judged on what run.mjs said, whatever its exit status: every PART line and every failed
      # run, as ci/check.sh does. A run that failed runs once more, alone: if it passes then, it is
      # named (a flake to tag); if it fails again, the step fails. A PART run always fails the step:
      # it passed, but not in full (here the plugins are there, or --no-clap left its runs out).
      runs=$(grep '^  failed: ' "$OUT/e2e.log" | awk '{print $2}')
      parts=$(grep '^  PART: ' "$OUT/e2e.log" | sed -E 's/^  PART: +([^:]+):.*/\1/' | tr '\n' ' ' | sed 's/ $//; s/ /, /g')
      total=$(grep -cE '^[0-9]+/[0-9]+ suites ok' "$OUT/e2e.log")
      # No total line, an exit status run.mjs never gives, or an exit 1 that names no failed run:
      # run.mjs itself stopped (a crash exits 1 too), so nothing it said can be trusted.
      if [ "$total" -eq 0 ] || { [ $st -ne 0 ] && [ $st -ne 1 ] && [ $st -ne 3 ]; } || { [ $st -eq 1 ] && [ -z "$runs" ]; }; then
        row "browser suites" FAIL "run.mjs stopped (exit $st after $took): see e2e.log"
        { echo; echo "=== browser suites: the last 60 lines of $OUT/e2e.log"; tail -60 "$OUT/e2e.log"; } >> "$OUT/tails"
      else
        bad=
        again=
        for run in $runs; do
          if suites "${run%-*}" "${run##*-}" --quiet --out "$OUT/e2e-retry" >> "$OUT/e2e-retry.log" 2>&1 < /dev/null; then
            again="$again${again:+, }$run"
            if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning title=Passed only on a retry::suite $run"; fi
          else
            bad="$bad${bad:+, }$run"
          fi
        done
        if [ -n "$bad$parts" ]; then
          why=
          if [ -n "$bad" ]; then why="$bad failed, and again alone"; fi
          if [ -n "$parts" ]; then why="$why${why:+; }PART (passed, but not in full): $parts"; fi
          if [ -n "$again" ]; then why="$why; passed only on a retry alone: $again"; fi
          row "browser suites" FAIL "$why: see e2e.log${bad:+ and e2e-retry.log}"
          if [ -n "$bad" ]; then log=e2e-retry.log; else log=e2e.log; fi
          { echo; echo "=== browser suites: the last 60 lines of $OUT/$log"; tail -60 "$OUT/$log"; } >> "$OUT/tails"
        elif [ -n "$again" ]; then
          row "browser suites" PASS "$took; passed only on a retry alone: $again"
        else
          row "browser suites" PASS "$took"
        fi
      fi
    fi ;;

  renders)
    want=$(grep -c . e2e/renders.sha256)
    if check build build.log build &&
       check renders renders.log env M2="$TARGET/debug/m2" e2e/baseline.sh --tree . "$OUT/renders"; then
      if (cd "$OUT/renders" && shasum -a 256 -c "$ROOT/e2e/renders.sha256") > "$OUT/renders-check.log" 2>&1; then
        row "renders match" PASS "$want of $want match e2e/renders.sha256"
      else
        row "renders match" FAIL "$(grep -c ': OK$' "$OUT/renders-check.log") of $want match: see renders-check.log"
        { echo; echo "=== renders that differ"; grep -v ': OK$' "$OUT/renders-check.log"; } >> "$OUT/tails"
      fi
    fi ;;
esac

# The step changed nothing in the repo (git status the same as before it).
if [ "$(git status --porcelain)" = "$BEFORE" ]; then
  row "repo unchanged" PASS ""
else
  row "repo unchanged" FAIL "git status changed: a step wrote into the checkout"
fi
if [ -s "$OUT/tails" ]; then cat "$OUT/tails"; fi
took=$(( $(date +%s) - START ))
if awk -F"$TAB" '$2 == "FAIL" { bad = 1 } END { exit bad }' "$OUT/results"; then verdict=PASS; else verdict=FAIL; fi
name=$STEP
case $STEP in rust|suites) name="$name $PART${ALL:+ --all}" ;; esac
line="ci/pr.sh $name: $verdict in $((took / 60)) min $((took % 60)) s"
echo
echo "$line (logs in $OUT)"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  { echo "### $line"; echo; echo '```'; awk -F"$TAB" '{printf "%-16s %-4s  %s\n", $1, $2, $3}' "$OUT/results"; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
fi
[ $verdict = PASS ]
