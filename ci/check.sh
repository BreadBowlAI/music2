#!/bin/bash
# ci/check.sh: the full check of music2, the same script on macOS and Linux (PLAT-6).
#
# In short: run it in the repo with no arguments. It builds everything, runs every Rust test,
# clippy, the web build, every browser suite, std's checks and the offline renders; on a Mac it
# also times the gates ci/gates.txt names. Then it prints a short summary to paste back. It
# changes nothing in the repo, never opens the app on a screen you can see, never plays sound,
# and never touches your ~/Music or ~/.music2. If one line says "running" for over 30 minutes,
# press Ctrl-C once: it stops and prints the summary anyway.
#
#   ci/check.sh             everything (on a Mac, the timing gates too)
#   ci/check.sh --gates     on Linux, the timing gates too (recorded for information, PLAT-1)
#   ci/check.sh --no-gates  on a Mac, without them
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
# Exit status: 0 when every check passed with the test plugins, 1 when not, 2 when it couldn't start.
#
# Needs rustup and Node 24. The first run installs Rust 1.99.0 (rust-toolchain.toml) and
# chrome-headless-shell 133.0.6943.141. On a Mac, run it in Terminal on the Mac itself, not over
# ssh: the app's test needs your login session. On Linux with Xvfb it also opens the desktop app
# on an invisible screen. Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
GATES=
case $(uname -s) in
  Darwin)
    OS=macos
    GATES=1
    MACHINE="$(sysctl -n hw.model), macOS $(sw_vers -productVersion), $(uname -m)"
    CHROME_DIR=mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64
    PLUGINS=$HOME/Library/Audio/Plug-Ins/CLAP ;;
  Linux)
    OS=linux
    MACHINE="$(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m), $(nproc) threads"
    CHROME_DIR=linux-133.0.6943.141/chrome-headless-shell-linux64
    PLUGINS=$HOME/.local/share/m2-test-plugins/clap ;;
  *)
    echo "ci/check.sh runs on macOS and Linux"
    exit 2 ;;
esac
for a in "$@"; do
  case $a in
    --gates) GATES=1 ;;
    --no-gates) GATES= ;;
    *) echo "ci/check.sh: no option $a (see the top of the script)"; exit 2 ;;
  esac
done
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
export M2_AUDIO=none CARGO_TERM_COLOR=never
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
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
# record <check> <PASS|PART|FAIL|SKIP> <note>: one row of the summary.
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
# panicked <test>: where the test panicked, and the first line of why. Rust prints the thread's
# id after its name: "thread 'a::b' (1234) panicked at src/x.rs:1:2:".
panicked() {
  grep -A1 -E "^thread '$1' (\([0-9]+\) )?panicked at " "$OUT/test.log" | head -2 | tr '\n' ' ' |
    sed -E "s/^thread '[^']*' (\([0-9]+\) )?panicked at //; s/ *$//" | cut -c1-160
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
  total=$(wc -l < "$OUT/results" | tr -d ' ')
  mins=$(( ($(date +%s) - START + 59) / 60 ))
  if [ -s "$SCR/before" ]; then state="with local changes"; else state=clean; fi
  if [ "$fails" -gt 0 ]; then
    verdict="FAIL ($fails of $total checks)"
  elif [ -z "$TESTPLUGINS" ]; then
    verdict="FAIL: no row failed, but the test plugins weren't there, so it isn't a pass (PLAT-6)"
  else
    verdict=PASS
  fi
  if [ -s "$OUT/flaky" ]; then verdict="$verdict; $(grep -c '' "$OUT/flaky") passed only on a retry"; fi
  # Logs only: no renders, no screenshots, no kept projects (a failed fx-fixes keeps a file of
  # over 300 MB).
  (cd "$OUT" && tar -czf "$OUT.tgz" $(find . -maxdepth 2 -type f \( -name '*.log' -o -name '*.sum' -o -name results -o -name failed -o -name flaky \))) 2> /dev/null
  {
    echo
    echo "music2 check: $verdict"
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
  first "$OUT/build.log" '^error' | sed 's/^/build: /' >> "$OUT/failed"
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
  env -i PATH=/usr/bin:/bin HOME="$dir/home" DISPLAY=":$DISPLAYN" XDG_RUNTIME_DIR="$dir/run" M2_AUDIO=none \
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
begin "rust tests" "10 min"
quiet test.log env HOME="$OUT/home" cargo test --locked --workspace --no-fail-fast
st=$?
# A test binary that failed with no failed test to show for it (no harness, or it crashed) counts
# as one failure.
counts=$(awk '
  /^     Running |^   Doc-tests / { result = 0; failed = 0 }
  /^test result:/ { p += $4; f += $6; result = 1; failed = $6 }
  /^error: (test|doctest) failed, to rerun pass / && !(result && failed) { f++; whole++ }
  END { printf "%d passed, %d failed", p, f; if (whole) printf " (%d of them a whole test binary)", whole }' "$OUT/test.log")
grep -E '^    [A-Za-z0-9_:]+$' "$OUT/test.log" | sed 's/^ *//' | sort -u > "$SCR/tests"
grep -o 'to rerun pass `[^`]*`' "$OUT/test.log" | sed 's/^to rerun pass `//; s/`$//' > "$SCR/binaries"
again=1
if [ $st -ne 0 ] && [ -s "$SCR/binaries" ]; then
  again=0
  while read -r args; do
    # cargo's own "-p <crate> --lib" or "--test <name>": split into words on purpose.
    env HOME="$OUT/home" cargo test --locked --no-fail-fast $args >> "$OUT/test-retry.log" 2>&1 < /dev/null || again=1
  done < "$SCR/binaries"
fi
if [ $st -eq 0 ]; then
  record "rust tests" PASS "$counts"
elif [ $again -eq 0 ]; then
  record "rust tests" PASS "$counts; passed on a retry"
  if [ -s "$SCR/tests" ]; then sed 's/^/test /' "$SCR/tests"; else sed 's/^/test binary /' "$SCR/binaries"; fi >> "$OUT/flaky"
else
  record "rust tests" FAIL "$counts: see test.log"
  while read -r t; do
    echo "test $t"
    r=$(panicked "$t")
    if [ -n "$r" ]; then echo "  $r"; fi
  done < "$SCR/tests" >> "$OUT/failed"
  sed 's/^/test binary /' "$SCR/binaries" >> "$OUT/failed"
  if [ ! -s "$SCR/binaries" ]; then first "$OUT/test.log" '^error' | sed 's/^/tests: /' >> "$OUT/failed"; fi
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

# 9. Every browser suite, each on a fresh headless server with a scratch HOME (e2e/run.mjs). A run
#    that failed runs once more alone; if it passes then, it is named as retried.
begin "browser suites" "8 min"
quiet e2e-npm.log sh -c 'cd e2e && npm ci --no-audit --no-fund'
if [ ! -x "$CHROME" ]; then
  (cd e2e && node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path "$HOME/.cache/puppeteer") \
    >> "$OUT/e2e-npm.log" 2>&1
fi
quiet e2e.log suites all --out "$OUT/e2e"
st=$?
line=$(grep -E 'suites ok' "$OUT/e2e.log" | tail -1 | cut -d';' -f1)
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
if [ $st -ne 0 ] && [ $st -ne 1 ] && [ $st -ne 3 ]; then
  record "browser suites" FAIL "run.mjs stopped (exit $st): see e2e.log"
elif [ $again -ne 0 ] || [ "$others" -gt 0 ] || { [ "$parts" -gt 0 ] && [ -n "${CLAP_PATH:-}" ]; }; then
  record "browser suites" FAIL "$line: see e2e.log"
  grep '^  PART:' "$OUT/e2e.log" | sed 's/^  PART: */suite /' | cut -c1-160 >> "$OUT/failed"
elif [ "$parts" -gt 0 ]; then
  record "browser suites" PART "$line (live3b without plugins)"
elif [ -s "$SCR/runs" ]; then
  record "browser suites" PASS "$line; passed on a retry"
else
  record "browser suites" PASS "$line"
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

# 11. The timing gates ci/gates.txt names, in release: on a Mac by default, on Linux with --gates.
#     Only the reference Mac's timings are judged; elsewhere they are recorded (PLAT-1).
if [ -n "$GATES" ]; then
  begin gates "15 min"
  args=$(grep -v '^#' ci/gates.txt | head -1)
  # ci/gates.txt's line of m2-gates options: split into words on purpose.
  if quiet gates-build.log cargo build --locked --release -p m2-gates &&
     quiet gates.log env HOME="$OUT/home" "$TARGET/release/m2-gates" $args; then
    record gates PASS "$args"
  else
    record gates FAIL "$args: see gates.log"
    first "$OUT/gates-build.log" '^error' | sed 's/^/gates: /' >> "$OUT/failed"
  fi
  grep -E '^(PASS|FAIL|SKIP)  |^      FAIL  ' "$OUT/gates.log" 2> /dev/null | cut -c1-160 > "$OUT/gates.sum"
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
[ "$fails" -eq 0 ] && [ -n "$TESTPLUGINS" ]
