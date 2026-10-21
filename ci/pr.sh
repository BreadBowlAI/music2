#!/bin/bash
# ci/pr.sh: the PR tier of music2's checks, one step at a time. Each job of
# .github/workflows/ci.yml runs one step, with the same command you type here; ci/check.sh --pr
# runs every step, one after another, on your machine.
#
# In short: the PR tier is what every pull request must pass, quickly. It holds every check that
# gives the same answer on any machine: no timing gates, and none of the tests ci/nightly-tests.txt
# and e2e/run.mjs tag for the nightly tier (a speed bound, a known flake, over a minute). Those run
# every night (ci/check.sh does everything). Nothing here changes the repo, opens a screen, plays
# sound, or touches your ~/Music or ~/.music2: whatever runs music2 gets a scratch HOME.
#
#   ci/pr.sh changes [<base>]   whether the change since git merge-base HEAD <base> (default: main,
#                               or origin/main) is documentation only (ci/docs-only.sh). Prints the
#                               verdict, then docs_only=true or docs_only=false (also into
#                               $GITHUB_OUTPUT when set). Exit 0 either way, 2 on a usage error.
#   ci/pr.sh docs               the docs job: the docs rule's own cases (ci/docs-only-test.sh)
#   ci/pr.sh toolchain          the Rust toolchain rust-toolchain.toml pins, installed if missing
#                               (CI's jobs run it before their cache step, which asks rustc)
#   ci/pr.sh quick              rustfmt, actionlint, the gates rule's and the docs rule's cases,
#                               licences (cargo-deny on both workspaces, npm), the web typecheck and
#                               build, std's kernels (tests, clippy) and std.wasm rebuilt to the
#                               committed bytes
#   ci/pr.sh clippy             clippy on every target, every warning an error
#   ci/pr.sh rust [<i>/<n>]     the Rust tests of the PR tier, part i of n (cargo-nextest's hash
#                               partitions, so a new test lands in one part and the parts stay even);
#                               part 1 also runs the two tests that own their main thread
#                               (main_thread, app_main_thread), which nextest can't list
#   ci/pr.sh suites [<i>/<n>]   the browser suites of the PR tier (e2e/run.mjs all --tier pr --shard),
#                               M2_PR_JOBS at a time (default 2)
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
  docs|toolchain|quick|clippy|renders) if [ $# -gt 0 ]; then usage; fi ;;
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
  echo "docs_only=$d"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then echo "docs_only=$d" >> "$GITHUB_OUTPUT"; fi
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    { echo "### Docs only: $d"; echo; echo '```'; printf '%s\n' "$verdict" | head -30; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
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
# build: the binaries the browser suites and the renders use, in one cargo run (two would resolve
# features apart and build the shared crates twice).
build() { cargo build --locked -p m2-server --example headless -p m2 -p m2-plugins --bin m2 --bin m2-plugin-host; }
# nightly_list: whether nextest finds exactly the tests ci/nightly-tests.txt names.
nightly_list() {
  awk '!/^#/ && NF >= 2 { print $1, $2 }' ci/nightly-tests.txt | sort > "$OUT/nightly-want.txt"
  cargo nextest list --locked --workspace --message-format oneline -E "$(nightly_filter)" 2> /dev/null | sort > "$OUT/nightly-found.txt" || return 1
  if cmp -s "$OUT/nightly-want.txt" "$OUT/nightly-found.txt"; then
    echo "all $(grep -c '' "$OUT/nightly-want.txt") tests ci/nightly-tests.txt names are there"
  else
    echo "ci/nightly-tests.txt names tests nextest doesn't find (<), or finds them twice (>):"
    diff "$OUT/nightly-want.txt" "$OUT/nightly-found.txt"
    return 1
  fi
}
# nightly_filter: ci/nightly-tests.txt as a nextest filter that matches those tests.
nightly_filter() {
  awk '!/^#/ && NF >= 2 { printf "%s(binary_id(=%s) & test(=%s))", (n++ ? " | " : ""), $1, $2 } END { if (!n) printf "none()" }' ci/nightly-tests.txt
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

  quick)
    check fmt fmt.log sh -c 'cargo fmt --all --check && cd std/kernels-src && cargo fmt --all --check'
    if tools actionlint deny; then
      # No shellcheck or pyflakes inside actionlint: their versions differ from machine to machine,
      # and every run: block only calls a script here.
      check workflows actionlint.log sh -c 'actionlint -color -shellcheck= -pyflakes= && node ci/ci-ok.mjs --check-workflow'
      check licences licences.log sh -c 'cargo deny --locked --color never --config deny.toml check &&
        cargo deny --locked --color never --manifest-path std/kernels-src/Cargo.toml --config deny.toml check -A advisory-not-detected -A license-not-encountered &&
        node ci/npm-licenses.mjs'
    fi
    check "gates rule" gates-rule.log ci/gates-auto-test.sh
    check "docs rule" docs-rule.log ci/docs-only-test.sh
    check "web build" web.log web
    check "std kernels" kernels.log sh -c 'cd std/kernels-src && cargo test --locked --release --workspace && cargo clippy --locked --workspace --all-targets -- -D warnings'
    check std.wasm std-wasm.log std/build.sh --check ;;

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
        if [ $OS = linux ]; then check "nightly list" nightly-list.log nightly_list; fi
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
      check "browser suites" e2e.log env E2E_CHROME="$CHROME" M2_HEADLESS="$TARGET/debug/examples/headless" \
        node e2e/run.mjs all $tier --shard "$PART" --jobs "$JOBS" --out "$OUT/e2e"
      grep -E '^(tier|shard|--no-clap|[0-9]+/[0-9]+ suites ok|time:)|^  (PART|failed):' "$OUT/e2e.log" | sed 's/^/                       /'
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
