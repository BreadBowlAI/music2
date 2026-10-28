#!/bin/bash
# ci/pr-test.sh: tries the verdicts of the PR tier on tables of made-up results, so a change to how
# a result is judged is tested before any real run. ci/pr.sh runs it in its "quick" step.
#
# In short: three tables. First, ci/pr.sh suites: what it says when e2e/run.mjs reports every run
# ok, a run that failed (and then passed or failed again alone), a run that was PART (it passed,
# but not in full), both at once, or a crash, and the list of what passed only on a retry it
# writes (its flaky file; ci/pr.sh rust's too, from nextest's FLAKY lines). Second, ci/check.sh --pr: what its summary says for
# a change that is docs only (no plugins needed), the whole tier with the plugins, with
# M2_NO_PLUGINS, with M2_LATE_SLEEPS, with a step that failed, and that its "passed only on a
# retry" is ci/pr.sh's lists, not words in a step's output. Third, ci/pr.sh changes: whether it says a change
# touches a Cargo.lock or deny.toml (deps=true), so the pull request checks RustSec's advisories
# too. Each case runs the real script in a small git repo
# made in a temporary folder, with stand-ins for everything else: cargo, npm, node and the browser
# for ci/pr.sh, ci/pr.sh itself for ci/check.sh. Nothing is built and no browser opens.
#
# Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case passed, 1
# when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-pr-test.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT
# git without your settings (no global or system config, so no hooks or signing) and a fixed author;
# nothing of CI's own (a case must not write into the real job's summary or outputs).
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES \
  GIT_CEILING_DIRECTORIES GIT_CONFIG_GLOBAL GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT \
  GITHUB_ACTIONS GITHUB_OUTPUT GITHUB_STEP_SUMMARY CLAP_PATH M2_NO_PLUGINS M2_LATE_SLEEPS M2_PR_JOBS M2_CHECK_JOBS \
  CARGO_TARGET_DIR E2E_CHROME M2_PR_OUT M2_CHECK_OUT
export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/config XDG_CACHE_HOME=$TMP/cache GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=check GIT_AUTHOR_EMAIL=check@example.invalid GIT_COMMITTER_NAME=check \
  GIT_COMMITTER_EMAIL=check@example.invalid
mkdir -p "$HOME" "$TMP/clap/Surge XT.clap" "$TMP/clap/Airwindows Consolidated.clap" "$TMP/bin" "$TMP/bin-check" || exit 2
PATH0=$PATH

# Stand-ins for ci/pr.sh suites and rust: npm does nothing, and cargo nothing but print $FAKE_TEST
# (with \n for each new line) when asked to run nextest; ci/tools.sh names the stand-ins' folder;
# the browser is a file that can run; node, asked to run e2e/run.mjs, prints $FAKE_ALL and exits
# $FAKE_ALL_EXIT for `all`, and exits $FAKE_ONE_EXIT for one suite run again alone.
printf '#!/bin/sh\nif [ "$1" = nextest ]; then printf "%%b" "${FAKE_TEST:-}"; fi\nexit 0\n' > "$TMP/bin/cargo"
printf '#!/bin/sh\nexit 0\n' > "$TMP/bin/npm"
printf '#!/bin/sh\nexit 0\n' > "$TMP/chrome"
cat > "$TMP/bin/node" << 'EOF'
#!/bin/sh
shift
if [ "$1" = all ]; then printf '%b' "$FAKE_ALL"; exit "$FAKE_ALL_EXIT"; fi
exit "$FAKE_ONE_EXIT"
EOF
# For ci/check.sh --pr: rustc only names itself.
printf '#!/bin/sh\necho "rustc 0.0.0 (stand-in)"\n' > "$TMP/bin-check/rustc"
chmod +x "$TMP/bin/cargo" "$TMP/bin/npm" "$TMP/bin/node" "$TMP/chrome" "$TMP/bin-check/rustc" || exit 2

# fixture <dir> <ci/pr.sh to use>: a git repo holding ci/check.sh, that ci/pr.sh (and the file it
# sources), and the folders the steps go into.
fixture() {
  mkdir -p "$1/ci" "$1/web" "$1/e2e" && cp "$HERE/check.sh" "$1/ci/check.sh" && cp "$2" "$1/ci/pr.sh" &&
    cp "$HERE/nightly-list.sh" "$1/ci/nightly-list.sh" && cp "$HERE/tally.sh" "$1/ci/tally.sh" &&
    echo '# a song-as-program DAW' > "$1/README.md" && echo '{}' > "$1/web/package.json" &&
    echo '{}' > "$1/e2e/package.json" && printf 'target/\n' > "$1/.gitignore" &&
    (cd "$1" && git init -q && git symbolic-ref HEAD refs/heads/main && git add -A && git commit -q -m fixture)
}
mkdir -p "$TMP/suites/ci" && printf '#!/bin/sh\necho "%s"\n' "$TMP/bin" > "$TMP/suites/ci/tools.sh" &&
  chmod +x "$TMP/suites/ci/tools.sh" && fixture "$TMP/suites" "$HERE/pr.sh" || exit 2

passed=0
failed=0
n=0
# result <name> <ok?> <what it said>: one line of the table.
result() {
  if [ "$2" = 1 ]; then
    passed=$((passed + 1))
    echo "PASS  $1: $3"
  else
    failed=$((failed + 1))
    echo "FAIL  $1: $3"
  fi
}

# flaky_is <name> <its folder> <want>: the step's flaky file holds exactly the lines <want>.
flaky_is() {
  got=$(cat "$2/flaky" 2> /dev/null)
  if [ "$got" = "$3" ]; then
    result "$1" 1 "$(printf '%s' "${got:-nothing}" | tr '\n' '|')"
  else
    result "$1" 0 "wanted \"$(printf '%s' "$3" | tr '\n' '|')\", got \"$(printf '%s' "$got" | tr '\n' '|')\""
  fi
}
# suites_case <name> <want: PASS|FAIL> <words its row holds> <run.mjs's lines for all> <its exit>
#   <a run's exit, alone> <flaky>: ci/pr.sh suites with that made-up run.mjs. The step must exit 0
#   exactly when <want> is PASS, and its "browser suites" row must say <want> and hold <words>;
#   its flaky file must hold exactly <flaky> (the runs that passed only on a retry).
suites_case() {
  n=$((n + 1))
  out=$TMP/suites-out-$n
  (cd "$TMP/suites" && env PATH="$TMP/bin:$PATH0" CLAP_PATH="$TMP/clap" E2E_CHROME="$TMP/chrome" M2_PR_OUT="$out" \
    FAKE_ALL="$4" FAKE_ALL_EXIT="$5" FAKE_ONE_EXIT="$6" ci/pr.sh suites) > "$TMP/suites-$n.log" 2>&1 < /dev/null
  st=$?
  row=$(awk -F'\t' '$1 == "browser suites" { print $2 "  " $3 }' "$out/results" 2> /dev/null)
  if [ "$2" = PASS ]; then want=0; else want=1; fi
  case $st:$row in
    "$want:$2  "*"$3"*) result "suites: $1" 1 "$row" ;;
    *) result "suites: $1" 0 "wanted $2 with \"$3\", got exit $st and \"${row:-no row}\" (see below)"
       tail -5 "$TMP/suites-$n.log" | sed 's/^/        /' ;;
  esac
  flaky_is "suites: $1: its flaky list" "$out" "$7"
}

TOTAL='\n2/2 suites ok; 16 of 16 expected checks passed, 0 failed, 0 skipped. Logs: /x\ntime: 0.1 min, up to 2 suites at once\n'
ONEFAILED='\n1/2 suites ok, 1 failed; 8 of 16 expected checks passed, 1 failed, 0 skipped. Logs: /x\ntime: 0.1 min, up to 2 suites at once\n  failed: bus-mix-fix (exit 1) /x/bus-mix-fix.log\n'
ONEPART='\n1/2 suites ok, 1 PART (passed, but not in full); 14 of 16 expected checks passed, 0 failed, 0 skipped. Logs: /x\ntime: 0.1 min, up to 2 suites at once\n  PART:   s4-meter-fix: passed 6 checks, not the 8 expected. /x/s4-meter-fix.log\n'
BOTH='\n1/3 suites ok, 1 PART (passed, but not in full), 1 failed; 20 of 24 expected checks passed, 1 failed, 0 skipped. Logs: /x\ntime: 0.1 min, up to 2 suites at once\n  PART:   s4-meter-fix: passed 6 checks, not the 8 expected. /x/s4-meter-fix.log\n  failed: bus-mix-fix (exit 1) /x/bus-mix-fix.log\n'

suites_case "every run ok" PASS "s" "$TOTAL" 0 0 ""
suites_case "a run failed, then passed alone" PASS "passed only on a retry alone: bus-mix-fix" "$ONEFAILED" 1 0 "suite bus-mix-fix"
suites_case "a run failed, and failed again alone" FAIL "bus-mix-fix failed" "$ONEFAILED" 1 1 ""
suites_case "a run was PART" FAIL "s4-meter-fix" "$ONEPART" 3 0 ""
suites_case "a run was PART, and another failed, then passed alone" FAIL "s4-meter-fix" "$BOTH" 1 0 "suite bus-mix-fix"
suites_case "run.mjs stopped, naming no run" FAIL "run.mjs stopped" 'Error: something broke\n' 1 0 ""
suites_case "run.mjs crashed" FAIL "run.mjs stopped" 'Aborted\n' 134 0 ""
suites_case "exit 0, but no total line" FAIL "run.mjs stopped" 'time: 0.1 min\n' 0 0 ""

# ci/pr.sh rust (part 2 of 2: no own-main tests) on nextest's made-up log: its flaky file names
# each FLAKY test once, as ci/check.sh's own list does ("test <binary> <name>"), with or without
# the counter, and not a test that failed.
n=$((n + 1))
out=$TMP/rust-out-$n
(cd "$TMP/suites" && env PATH="$TMP/bin:$PATH0" CLAP_PATH="$TMP/clap" M2_PR_OUT="$out" \
  FAKE_TEST='  TRY 1 FAIL [   0.500s] ( 100/1975) m2-song data::tests::once\n   FLAKY 2/2 [   0.512s] ( 100/1975) m2-song data::tests::once\n  TRY 2 FAIL [   1.111s] ( 720/1975) m2-play tests::bad\n     Summary [ 125.195s] 1975 tests run: 1974 passed, 1 failed\n   FLAKY 2/2 [   0.914s] (1387/1975) m2-server graph::tests::a_failed_build_then_a_lane_edit_rebuilds\n   FLAKY 2/2 [   0.512s] ( 100/1975) m2-song data::tests::once\n   FLAKY 2/2 [   0.300s] m2-os tests::no_counter\n' \
  ci/pr.sh rust 2/2) > "$TMP/rust-$n.log" 2>&1 < /dev/null
flaky_is "rust: nextest's FLAKY tests, its flaky list" "$out" "test m2-os tests::no_counter
test m2-server graph::tests::a_failed_build_then_a_lane_edit_rebuilds
test m2-song data::tests::once"

# changes_case <name> <want: true|false> <base> <commands that make the change>: whether
#   ci/pr.sh changes <base> says deps=<want> (a Cargo.lock or deny.toml changed, or git can't
#   say), on a branch "topic" taken from main in a copy of a small repo.
fixture "$TMP/changes" "$HERE/pr.sh" && cp "$HERE/docs-only.sh" "$TMP/changes/ci/docs-only.sh" &&
  (cd "$TMP/changes" && mkdir -p core docs std/kernels-src && for f in Cargo.lock std/kernels-src/Cargo.lock deny.toml core/a.rs docs/a.md; do
    echo one > "$f"; done && git add -A && git commit -q -m files) || exit 2
changes_case() {
  n=$((n + 1))
  d=$TMP/changes-$n
  cp -R "$TMP/changes" "$d" || exit 2
  if ! (cd "$d" && git checkout -q -b topic && eval "$4") > "$TMP/setup.log" 2>&1; then
    result "changes: $1" 0 "setting it up failed: $(head -1 "$TMP/setup.log")"
    return
  fi
  said=$(cd "$d" && ci/pr.sh changes "$3" 2>&1 < /dev/null)
  st=$?
  got=$(printf '%s\n' "$said" | sed -n 's/^deps=//p')
  if [ $st -eq 0 ] && [ "$got" = "$2" ]; then
    result "changes: $1" 1 "deps=$got"
  else
    result "changes: $1" 0 "wanted deps=$2, got exit $st and deps=${got:-nothing}: $(printf '%s' "$said" | head -1)"
  fi
}
edit='echo two >> "$f" && git commit -qam change'
changes_case "a docs change" false main "f=docs/a.md && $edit"
changes_case "a Rust change, the same crates" false main "f=core/a.rs && $edit"
changes_case "Cargo.lock" true main "f=Cargo.lock && $edit"
changes_case "std's kernels' Cargo.lock" true main "f=std/kernels-src/Cargo.lock && $edit"
changes_case "deny.toml" true main "f=deny.toml && $edit"
changes_case "a new Cargo.lock in a new folder" true main "mkdir -p tools/x && echo a > tools/x/Cargo.lock && git add -A && git commit -qm new"
changes_case "nothing changed (main itself)" false main ":"
changes_case "a change not yet committed" true main "echo two >> core/a.rs"
changes_case "a base git doesn't know" true no-such-branch "f=core/a.rs && $edit"

# A stand-in for ci/pr.sh, for ci/check.sh --pr: it prints the file $FAKE/<step> (its rows) and
# exits with $FAKE/<step>.exit (default 0), then its last line as ci/pr.sh's; $FAKE/<step>.flaky,
# if there, is its list of what passed only on a retry (M2_PR_OUT/flaky).
cat > "$TMP/fake-pr.sh" << 'EOF'
#!/bin/sh
step=$1
cat "$FAKE/$step" 2> /dev/null
if [ -f "$FAKE/$step.flaky" ]; then mkdir -p "$M2_PR_OUT" && cp "$FAKE/$step.flaky" "$M2_PR_OUT/flaky"; fi
st=$(cat "$FAKE/$step.exit" 2> /dev/null || echo 0)
if [ "$step" != changes ]; then
  if [ "$st" -eq 0 ]; then v=PASS; else v=FAIL; fi
  echo
  echo "ci/pr.sh $step: $v in 0 min 1 s (logs in /x)"
fi
exit "$st"
EOF
chmod +x "$TMP/fake-pr.sh" || exit 2
fixture "$TMP/check" "$TMP/fake-pr.sh" || exit 2

# fake <case> <step> <exit> <rows...>: what the stand-in prints for that step in that case.
fake() {
  d=$TMP/fake-$1 s=$2 e=$3
  shift 3
  mkdir -p "$d" && printf '%s\n' "$@" > "$d/$s" && echo "$e" > "$d/$s.exit"
}
# check_case <name> <case> <want exit> <the words the summary's first line starts with> [VAR=value...]:
#   ci/check.sh --pr with the stand-in's files of <case>. With $NAMED set, the summary must also hold
#   that line under "what failed"; with $UNNAMED set, it must not. With $RETRIED set, its "passed
#   only on a retry" must be exactly those lines ("-": none at all).
check_case() {
  n=$((n + 1))
  name=$1 c=$2 want=$3 words=$4
  shift 4
  out=$TMP/check-out-$n
  (cd "$TMP/check" && env PATH="$TMP/bin-check:$PATH0" FAKE="$TMP/fake-$c" M2_CHECK_OUT="$out" "$@" ci/check.sh --pr) \
    > "$TMP/check-$n.log" 2>&1 < /dev/null
  st=$?
  line=$(grep -m1 '^music2 check --pr: ' "$out/summary.txt" 2> /dev/null)
  failed_list=$(sed -n '/^what failed:$/,/^$/p' "$out/summary.txt" 2> /dev/null)
  if [ -n "${NAMED:-}" ] && ! printf '%s\n' "$failed_list" | grep -qxF "  $NAMED"; then line="$line (what failed doesn't name \"$NAMED\")"; fi
  if [ -n "${UNNAMED:-}" ] && printf '%s\n' "$failed_list" | grep -qF "$UNNAMED"; then line="$line (what failed names \"$UNNAMED\")"; fi
  if [ -n "${RETRIED:-}" ]; then
    retried=$(sed -n '/^passed only on a retry:$/,/^$/p' "$out/summary.txt" 2> /dev/null | sed '1d; /^$/d; s/^  //')
    if [ "$retried" != "${RETRIED#-}" ]; then line="$line (passed only on a retry: \"$retried\", not \"${RETRIED#-}\")"; fi
  fi
  case $st:$line in
    *"(what failed "*|*"(passed only on a retry: "*) result "check --pr: $name" 0 "$line"; sed 's/^/        /' "$out/summary.txt" | tail -8 ;;
    "$want:music2 check --pr: $words"*) result "check --pr: $name" 1 "$line" ;;
    *) result "check --pr: $name" 0 "wanted exit $want and \"$words\", got exit $st and \"${line:-no summary}\" (see below)"
       tail -5 "$TMP/check-$n.log" | sed 's/^/        /' ;;
  esac
}

PLUGINS_PASS="plugins          PASS  $TMP/clap"
PLUGINS_SKIP='plugins          SKIP  M2_NO_PLUGINS is set: the tests that host plugins skip'
fake docs changes 0 'docs  only docs/ changed since main (1 file)' 'docs_only=true'
fake docs docs 0 'docs rule        PASS  1 s'
fake full changes 0 'full  ci/pr.sh changed' 'docs_only=false'
for s in quick clippy renders; do fake full $s 0 "$s            PASS  1 s"; done
fake full rust 0 "$PLUGINS_PASS" 'rust tests       PASS  1 s'
fake full suites 0 "$PLUGINS_PASS" 'browser suites   PASS  1 s'
cp -R "$TMP/fake-full" "$TMP/fake-noplugins" && cp -R "$TMP/fake-full" "$TMP/fake-failed" || exit 2
fake noplugins rust 0 "$PLUGINS_SKIP" 'rust tests       PASS  1 s'
fake noplugins suites 0 "$PLUGINS_SKIP" 'browser suites   PASS  1 s'
fake failed rust 1 "$PLUGINS_PASS" 'rust tests       FAIL  exit 100 after 1 s: see test.log'
# The Mac's nightly job: its sleeps run long, so the steps leave out what needs them on time.
cp -R "$TMP/fake-full" "$TMP/fake-sleeps" || exit 2
SLEEPS_SKIP="late sleeps      SKIP  M2_LATE_SLEEPS is set: this machine's sleeps run long, so what needs them on time is left out"
fake sleeps rust 0 "$PLUGINS_PASS" "$SLEEPS_SKIP: a_test (ci/sleeps-tests.txt)" 'rust tests       PASS  1 s'
fake sleeps suites 0 "$PLUGINS_PASS" "$SLEEPS_SKIP: a-fix (e2e/run.mjs's sleeps tags)" 'browser suites   PASS  1 s'
cp -R "$TMP/fake-full" "$TMP/fake-advisory" || exit 2
fake advisory changes 0 'full  Cargo.lock changed' 'docs_only=false' 'deps=true'
fake advisory advisories 1 'advisories       FAIL  exit 1 after 1 s: see advisories.log'
# A Rust test whose process crashed on both tries, as nextest prints it, and one that failed once
# and then passed (FLAKY): only the first is a failure.
cp -R "$TMP/fake-full" "$TMP/fake-crash" || exit 2
fake crash rust 1 "$PLUGINS_PASS" 'rust tests       FAIL  exit 100 after 133 s: see test.log' \
  '                       Summary [ 125.195s] 1733 tests run: 1732 passed, 1 failed, 47 skipped' \
  '                       FLAKY 2/2 [   0.512s] ( 100/1733) m2-song data::tests::once' \
  '' '=== rust tests: the last 60 lines of /x/test.log' \
  '  TRY 1 FAIL [   0.500s] ( 100/1733) m2-song data::tests::once' \
  '  TRY 1 SEGV [   1.104s] ( 719/1733) m2-plugins tests::airwindows_processes_audio' \
  '  TRY 2 SEGV [   1.111s] ( 720/1733) m2-plugins tests::airwindows_processes_audio' \
  '     Summary [ 125.195s] 1733 tests run: 1732 passed, 1 failed, 47 skipped' \
  '   FLAKY 2/2 [   0.512s] ( 100/1733) m2-song data::tests::once' \
  '  TRY 2 SEGV [   1.111s] ( 720/1733) m2-plugins tests::airwindows_processes_audio' \
  'error: test run failed'
printf 'test m2-song data::tests::once\n' > "$TMP/fake-crash/rust.flaky" || exit 2
# What passed only on a retry is ci/pr.sh's lists, and only them: a test in rust's list that its
# output doesn't print, and a suite in suites'; not the suite pr-test.sh's own table names in its
# rows, printed in quick's output when its "pr verdicts" row failed.
cp -R "$TMP/fake-full" "$TMP/fake-retried" || exit 2
fake retried quick 1 'pr verdicts      FAIL  exit 1 after 9 s: see pr-test.log' \
  '' '=== pr verdicts: the last 60 lines of /x/pr-test.log' \
  'PASS  suites: a run failed, then passed alone: PASS  0 s; passed only on a retry alone: bus-mix-fix' \
  'PASS  suites: a run was PART, and another failed, then passed alone: FAIL  PART (passed, but not in full): s4-meter-fix; passed only on a retry alone: bus-mix-fix: see e2e.log' \
  'FAIL  check --pr: a made-up case: wanted PASS'
printf 'test m2-server graph::tests::a_failed_build_then_a_lane_edit_rebuilds\n' > "$TMP/fake-retried/rust.flaky" &&
  printf 'suite sx-follow\n' > "$TMP/fake-retried/suites.flaky" || exit 2
fake retried suites 0 "$PLUGINS_PASS" 'browser suites   PASS  1 s; passed only on a retry alone: sx-follow'

check_case "docs only, with no plugins: what CI passes" docs 0 "PASS"
check_case "the whole tier, with the plugins" full 0 "PASS" CLAP_PATH="$TMP/clap"
check_case "the whole tier, M2_NO_PLUGINS: not all CI runs" noplugins 1 "PART" M2_NO_PLUGINS=1
check_case "the whole tier, M2_LATE_SLEEPS: not all CI runs" sleeps 1 "PART: no row failed, but M2_LATE_SLEEPS left out the tests that need sleeps on time (rust, suites)" CLAP_PATH="$TMP/clap" M2_LATE_SLEEPS=1
check_case "the whole tier, a step failed" failed 1 "FAIL (1 of" CLAP_PATH="$TMP/clap"
check_case "a Cargo.lock changed, and RustSec has an advisory" advisory 1 "FAIL (1 of" CLAP_PATH="$TMP/clap"
NAMED="test m2-plugins tests::airwindows_processes_audio" UNNAMED="data::tests::once" RETRIED="test m2-song data::tests::once"
check_case "a Rust test crashed twice: what failed names it, and not the flaky one" crash 1 "FAIL (1 of" CLAP_PATH="$TMP/clap"
NAMED= UNNAMED=
RETRIED="test m2-server graph::tests::a_failed_build_then_a_lane_edit_rebuilds
suite sx-follow"
check_case "passed only on a retry: ci/pr.sh's lists, not a failed step's words" retried 1 "FAIL (1 of" CLAP_PATH="$TMP/clap"
RETRIED=-
check_case "every step passed, none on a retry: no such list" full 0 "PASS" CLAP_PATH="$TMP/clap"
RETRIED=

echo "PR tier verdicts: $passed of $((passed + failed)) cases passed"
[ $failed -eq 0 ]
