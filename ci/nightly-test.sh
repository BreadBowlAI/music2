#!/bin/bash
# ci/nightly-test.sh: tries ci/nightly.sh (the timing gates on a maintainer's machine) on a table of
# cases, with no network and no real check. ci/pr.sh runs it in its "quick" step.
#
# In short: each case runs this ci/nightly.sh in a small git repo whose origin is another small
# repo, made in a temporary folder. origin's main holds a stand-in ci/check.sh that writes what it
# saw and a summary; curl is a stand-in that writes what it was given. The cases: --report refuses
# a missing token file and any token that isn't fine-grained, before the check; it posts the
# verdict with the token only on curl's standard input; the check sees no GitHub token; a failed
# check posts "failure"; the summary warns when the user holds other credentials.
#
# Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case passed, 1
# when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-nightly-test.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES \
  GIT_CEILING_DIRECTORIES GIT_CONFIG_GLOBAL GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT \
  M2_NIGHTLY M2_NIGHTLY_TOKEN_FILE M2_NIGHTLY_REPO GITHUB_API_URL
export XDG_CONFIG_HOME=$TMP/config GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=check \
  GIT_AUTHOR_EMAIL=check@example.invalid GIT_COMMITTER_NAME=check GIT_COMMITTER_EMAIL=check@example.invalid
mkdir -p "$TMP/bin" || exit 2

# curl's stand-in: its arguments one a line in $TMP/curl-args, its standard input in $TMP/curl-in.
cat > "$TMP/bin/curl" << EOF
#!/bin/sh
printf '%s\n' "\$@" > "$TMP/curl-args"
cat > "$TMP/curl-in"
exit 0
EOF
# origin: main holds the stand-in check, which exits with the number in \$CHECK_EXIT's file.
O=$TMP/origin
mkdir -p "$O/ci" || exit 2
cat > "$O/ci/check.sh" << EOF
#!/bin/sh
mkdir -p "\$M2_CHECK_OUT"
env > "\$M2_CHECK_OUT/env.txt"
st=\$(cat "$TMP/check-exit")
if [ "\$st" -eq 0 ]; then v=PASS; else v='FAIL (1 of 20 checks)'; fi
printf '\nmusic2 check: %s\ncommit   x\n\ngates            PASS  \n' "\$v" > "\$M2_CHECK_OUT/summary.txt"
exit "\$st"
EOF
chmod +x "$TMP/bin/curl" "$O/ci/check.sh" || exit 2
(cd "$O" && git init -q && git symbolic-ref HEAD refs/heads/main && git add -A && git commit -q -m check) || exit 2
SHA=$(git -C "$O" rev-parse HEAD) || exit 2

passed=0
failed=0
n=0
# case_ <name> <check's exit> <want exit> <test, run in the case's folder> [VAR=value...] -- [options]:
#   ci/nightly.sh [options] in a fresh clone of origin (an address GitHub's form names, rewritten
#   to the local repo), with HOME, M2_NIGHTLY and the stand-ins of its own. <test> is a shell
#   condition on its output (out.txt), the files the stand-ins wrote, and $SHA. $PRE, if set, runs
#   first in the case's folder.
case_() {
  n=$((n + 1))
  name=$1 check=$2 want=$3 test=$4
  shift 4
  d=$TMP/case-$n
  mkdir -p "$d/home" && echo "$check" > "$TMP/check-exit" && rm -f "$TMP/curl-args" "$TMP/curl-in" || exit 2
  git clone -q "$O" "$d/repo" && git -C "$d/repo" remote set-url origin git@github.com:owner/name.git &&
    git -C "$d/repo" config url."$O".insteadOf git@github.com:owner/name.git &&
    mkdir -p "$d/repo/ci" && cp "$HERE/nightly.sh" "$d/repo/ci/nightly.sh" && (cd "$d" && eval "${PRE:-:}") || exit 2
  vars=
  while [ $# -gt 0 ] && [ "$1" != -- ]; do vars="$vars $1"; shift; done
  [ $# -gt 0 ] && shift
  # $vars: VAR=value words, split on purpose.
  (cd "$d" && env HOME="$d/home" M2_NIGHTLY="$d/nightly" PATH="$TMP/bin:$PATH" $vars repo/ci/nightly.sh "$@") \
    > "$d/out.txt" 2>&1 < /dev/null
  st=$?
  if [ $st -eq "$want" ] && (cd "$d" && eval "$test"); then
    passed=$((passed + 1))
    echo "PASS  $name"
  else
    failed=$((failed + 1))
    echo "FAIL  $name: exit $st (wanted $want), or not: $test"
    tail -4 "$d/out.txt" | sed 's/^/        /'
  fi
}
# nocheck: refused before anything ran (no clone, no night's folder, no post).
nocheck="[ ! -d nightly/clone ] && [ -z \"\$(ls nightly/runs)\" ] && [ ! -e '$TMP/curl-args' ]"
# posted <state>: one post of <state> to the commit's status, the token only on curl's input.
posted() {
  printf '%s' "grep -qx 'https://api.github.com/repos/owner/name/statuses/$SHA' '$TMP/curl-args' &&
    grep -q '\"state\":\"$1\",\"context\":\"nightly/gates\"' '$TMP/curl-args' &&
    grep -qx 'header = \"Authorization: Bearer github_pat_test\"' '$TMP/curl-in' && ! grep -q github_pat_ '$TMP/curl-args'"
}
# notoken: the check's environment held no token (neither of the two the case set, nor the file's).
notoken="[ -s nightly/runs/*/check/env.txt ] && ! grep -qE 'secret-one|secret-two|github_pat_|M2_NIGHTLY_TOKEN' nightly/runs/*/check/env.txt"

PRE=
case_ "--report with no token file: refused before the check" 0 2 "grep -q 'needs a token' out.txt && $nocheck" -- --report
printf 'ghp_classic\n' > "$TMP/classic"
case_ "--report with a classic token: refused before the check" 0 2 "grep -q \"isn't a fine-grained token\" out.txt && $nocheck" \
  M2_NIGHTLY_TOKEN_FILE="$TMP/classic" -- --report
printf 'gho_fromghlogin\n' > "$TMP/gh"
case_ "--report with gh's own token: refused before the check" 0 2 "grep -q \"isn't a fine-grained token\" out.txt && $nocheck" \
  M2_NIGHTLY_TOKEN_FILE="$TMP/gh" -- --report
printf 'github_pat_test\n' > "$TMP/fine"
case_ "--report, the check passed: success posted, the token on curl's input only, none in the check" 0 0 \
  "$(posted success) && $notoken && grep -q '^music2 check: PASS' nightly/last.txt" \
  M2_NIGHTLY_TOKEN_FILE="$TMP/fine" GH_TOKEN=secret-one GITHUB_TOKEN=secret-two -- --report
case_ "--report, the check failed: failure posted, exit 1" 1 1 "$(posted failure) && grep -q '^music2 check: FAIL' nightly/last.txt" \
  M2_NIGHTLY_TOKEN_FILE="$TMP/fine" -- --report
case_ "no --report: nothing posted" 0 0 "[ ! -e '$TMP/curl-args' ] && grep -q '^music2 check: PASS' nightly/last.txt"
PRE='mkdir -p home/.ssh home/.config/gh && : > home/.ssh/id_ed25519 && : > home/.config/gh/hosts.yml'
case_ "a user with an ssh key and a gh login: the summary warns" 0 0 \
  "grep -q '^warning: this user also holds .config/gh/hosts.yml, .ssh/id_ed25519' nightly/last.txt"
PRE=

echo "ci/nightly.sh: $passed of $((passed + failed)) cases passed"
[ $failed -eq 0 ]
