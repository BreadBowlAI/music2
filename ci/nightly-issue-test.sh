#!/bin/bash
# ci/nightly-issue-test.sh: tries ci/nightly-issue.sh (the nightly's two issues, "Nightly failed"
# and "Flaky tests") on a table of cases, with a stand-in for gh: nothing reaches GitHub.
# ci/pr.sh runs it in its "lint" step.
#
# In short: --list reads the flaky files one or two folders down, not at the top or deeper, drops
# blank lines and says each line once, with the OS before it; with none, an empty list. --flaky
# counts the nights a name was on from the issue's body and comments: "suite layout-proj" is not
# "suite x-layout-proj", nor "test once" "test once_more"; a name with * . or \ in it is compared
# as it is; a body or comment edited on GitHub's web page (its lines end in \r\n) still counts.
# A list with no names sends nothing; the night that failed opens "Nightly failed", or comments
# on it.
#
# gh's stand-in comes first on PATH (checked before any case), and no token or login is set. It
# says the open issue's number ($FAKE_N, none when empty) and what the issue said ($TMP/said), and
# writes what it was asked to create or comment on in $TMP/sent. Prints a PASS or FAIL line for
# each case, then a total. Exit status: 0 when every case passed, 1 when not, 2 when it couldn't
# start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-nightly-issue-test.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT
# No token and no login: even a real gh, were it reached, could write nothing.
unset GH_TOKEN GITHUB_TOKEN GH_ENTERPRISE_TOKEN GITHUB_ENTERPRISE_TOKEN GH_REPO GH_HOST GITHUB_OUTPUT
export GH_CONFIG_DIR=$TMP/gh-config XDG_CONFIG_HOME=$TMP/config
mkdir -p "$TMP/bin" "$GH_CONFIG_DIR" || exit 2

# gh's stand-in: "issue list" prints $FAKE_N; "issue view" prints $TMP/said; "issue create" and
# "issue comment" write "create <title>" or "comment <number>", then the body, in $TMP/sent.
# $FAKE_FAIL set: every call fails.
cat > "$TMP/bin/gh" << EOF
#!/bin/sh
if [ -n "\${FAKE_FAIL:-}" ]; then echo "gh: stand-in failure" >&2; exit 1; fi
what=\$2
shift 2
case \$what in
  list) [ -z "\${FAKE_N:-}" ] || echo "\$FAKE_N"; exit 0 ;;
  view) cat "$TMP/said"; exit 0 ;;
esac
num= title= body=
while [ \$# -gt 0 ]; do
  case \$1 in
    --title) title=\$2; shift ;;
    --body) body=\$2; shift ;;
    -*) ;;
    *) num=\$1 ;;
  esac
  shift
done
case \$what in
  create) printf 'create %s\n%s\n' "\$title" "\$body" >> "$TMP/sent" ;;
  comment) printf 'comment %s\n%s\n' "\$num" "\$body" >> "$TMP/sent" ;;
  *) echo "gh stand-in: no \$what" >&2; exit 1 ;;
esac
EOF
chmod +x "$TMP/bin/gh" || exit 2
export PATH="$TMP/bin:$PATH"
if [ "$(command -v gh)" != "$TMP/bin/gh" ]; then echo "ci/nightly-issue-test.sh: gh isn't the stand-in"; exit 2; fi

passed=0
failed=0
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
# run <args...>: ci/nightly-issue.sh with the stand-in; its output in $TMP/out, its exit in $st,
# what it sent in $TMP/sent (empty when nothing).
run() {
  : > "$TMP/sent"
  "$HERE/nightly-issue.sh" "$@" > "$TMP/out" 2>&1 < /dev/null
  st=$?
}
# has <file> <line>: the file holds that line, exactly.
has() { grep -qxF -- "$2" "$1"; }
# count <file> <line>: how many times the file holds that line.
count() { grep -cxF -- "$2" "$1"; }

# --list on a made-up folder: ci/check.sh's list (<sha>/flaky), ci/pr.sh's on Linux
# (<sha>/pr-<step>/flaky) and on the Mac (pr-<sha>/<step>/flaky); not one at the top, and not a
# scratch HOME's (deeper).
L=$TMP/m2-check
mkdir -p "$L/abc/pr-rust/home/x" "$L/pr-abc/suites/home" || exit 2
printf 'test top\n' > "$L/flaky"
printf 'test a\n\nsuite b\n' > "$L/abc/flaky"
printf 'test a\n' > "$L/abc/pr-rust/flaky"
printf 'suite c\n' > "$L/pr-abc/suites/flaky"
printf 'test deep\n' > "$L/abc/pr-rust/home/flaky"
printf 'test deeper\n' > "$L/abc/pr-rust/home/x/flaky"
printf 'test four\n' > "$L/pr-abc/suites/home/flaky"
want="list<<M2_FLAKY_END
linux: suite b
linux: suite c
linux: test a
M2_FLAKY_END"
run --list linux "$L"
got=$(LC_ALL=C sort "$TMP/out")
wants=$(printf '%s\n' "$want" | LC_ALL=C sort)
if [ $st -eq 0 ] && [ "$got" = "$wants" ] && [ "$(head -1 "$TMP/out")" = "list<<M2_FLAKY_END" ] && [ "$(tail -1 "$TMP/out")" = M2_FLAKY_END ]; then
  result "--list: one or two folders down, once each, no blank line" 1 "$(grep -c '^linux: ' "$TMP/out") names"
else
  result "--list: one or two folders down, once each, no blank line" 0 "exit $st, got: $(tr '\n' '|' < "$TMP/out")"
fi
run --list macos "$L"
if [ $st -eq 0 ] && [ "$(LC_ALL=C sort "$TMP/out")" = "$(printf '%s\n' "$want" | sed 's/^linux: /macos: /' | LC_ALL=C sort)" ]; then
  result "--list: the OS before each name" 1 "$(grep -c '^macos: ' "$TMP/out") names"
else
  result "--list: the OS before each name" 0 "exit $st, got: $(tr '\n' '|' < "$TMP/out")"
fi
mkdir -p "$TMP/none/abc" || exit 2
for f in "$TMP/none" "$TMP/missing"; do
  run --list linux "$f"
  if [ $st -eq 0 ] && [ "$(cat "$TMP/out")" = "list<<M2_FLAKY_END
M2_FLAKY_END" ]; then
    result "--list: no flaky file in $(basename "$f")" 1 "an empty list"
  else
    result "--list: no flaky file in $(basename "$f")" 0 "exit $st, got: $(tr '\n' '|' < "$TMP/out")"
  fi
done

# flaky_case <name> <issue number, or empty> <what it said before> <the list> <lines the sent
# body must hold, one a line>: ci/nightly-issue.sh --flaky; it must send one create (no issue
# open) or one comment on <number>, holding each of those lines exactly once.
flaky_case() {
  printf '%b' "$3" > "$TMP/said"
  FAKE_N=$2 run --flaky https://run/1 "$4"
  if [ -n "$2" ]; then first="comment $2"; else first="create Flaky tests"; fi
  bad=
  [ $st -eq 0 ] || bad="exit $st"
  [ "$(head -1 "$TMP/sent")" = "$first" ] || bad="${bad:+$bad; }sent \"$(head -1 "$TMP/sent")\", not \"$first\""
  [ "$(grep -cE '^(create|comment) ' "$TMP/sent")" = 1 ] || bad="${bad:+$bad; }not one create or comment"
  while IFS= read -r l; do
    [ -n "$l" ] || continue
    [ "$(count "$TMP/sent" "$l")" = 1 ] || bad="${bad:+$bad; }not once: \"$l\""
  done << EOF
$5
EOF
  if [ -z "$bad" ]; then result "--flaky: $1" 1 "$first"; else result "--flaky: $1" 0 "$bad: $(tr '\n' '|' < "$TMP/sent")"; fi
}

flaky_case "no issue open: night 1 each" "" "" "linux: test a
macos: suite layout-proj" "- night 1: linux: test a
- night 1: macos: suite layout-proj"
flaky_case "a name twice in the list: one line" "" "" "linux: test a
linux: test a" "- night 1: linux: test a"
flaky_case "nights counted in the body and every comment" 7 \
  'Passed only on a retry on 2026-10-01:\n\n- night 1: macos: suite layout-proj\n- night 1: linux: test a\n\nRun: x\n- night 2: macos: suite layout-proj\n' \
  "macos: suite layout-proj
linux: test b" "- night 3: macos: suite layout-proj
- night 1: linux: test b"
flaky_case "suite layout-proj is not suite x-layout-proj, nor test once test once_more" 7 \
  '- night 1: macos: suite x-layout-proj\n- night 2: macos: suite x-layout-proj\n- night 1: macos: suite layout-proj\n- night 1: macos: test m2-song tests::once_more\n' \
  "macos: suite layout-proj
macos: suite x-layout-proj
macos: test m2-song tests::once" "- night 2: macos: suite layout-proj
- night 3: macos: suite x-layout-proj
- night 1: macos: test m2-song tests::once"
flaky_case "only our lines count: a name in other words is not a night" 7 \
  'known: macos: suite layout-proj\n-night 1: macos: suite layout-proj\n' \
  "macos: suite layout-proj" "- night 1: macos: suite layout-proj"
flaky_case 'a name with * . and \ in it, compared as it is' 7 \
  '- night 1: linux: test a\\b.*c\n- night 1: linux: test aXb\n' \
  'linux: test a\b.*c
linux: test a.b' '- night 2: linux: test a\b.*c
- night 1: linux: test a.b'
flaky_case "a comment edited on the web page (\\r\\n line ends) still counts" 7 \
  'Passed only on a retry:\r\n\r\n- night 1: macos: suite crlf\r\n- night 2: macos: suite crlf\r\n' \
  "macos: suite crlf" "- night 3: macos: suite crlf"

: > "$TMP/said"
FAKE_N=7 run --flaky https://run/1 "

"
if [ $st -eq 0 ] && [ ! -s "$TMP/sent" ] && grep -q 'no flaky tests named' "$TMP/out"; then
  result "--flaky: no names: nothing sent" 1 "$(cat "$TMP/out")"
else
  result "--flaky: no names: nothing sent" 0 "exit $st, sent: $(tr '\n' '|' < "$TMP/sent")"
fi

# The night failed: "Nightly failed", opened or commented on, with the run and what failed.
for num in "" 5; do
  FAKE_N=$num run https://run/2 "linux failure, macos success"
  if [ -n "$num" ]; then first="comment $num" what="issue $num open"; else first="create Nightly failed" what="none open"; fi
  if [ $st -eq 0 ] && [ "$(head -1 "$TMP/sent")" = "$first" ] && grep -qF 'linux failure, macos success' "$TMP/sent" &&
     has "$TMP/sent" "Run: https://run/2"; then
    result "Nightly failed, $what" 1 "$first"
  else
    result "Nightly failed, $what" 0 "exit $st, sent: $(tr '\n' '|' < "$TMP/sent")"
  fi
done

# gh failed: exit 1. Wrong arguments: exit 2.
FAKE_FAIL=1 run --flaky https://run/1 "linux: test a"
if [ $st -eq 1 ]; then result "gh failed: exit 1" 1 "exit 1"; else result "gh failed: exit 1" 0 "exit $st"; fi
run --flaky https://run/1
if [ $st -eq 2 ] && [ ! -s "$TMP/sent" ]; then result "a usage error: exit 2" 1 "exit 2"; else result "a usage error: exit 2" 0 "exit $st"; fi

echo "ci/nightly-issue.sh: $passed of $((passed + failed)) cases passed"
[ $failed -eq 0 ]
