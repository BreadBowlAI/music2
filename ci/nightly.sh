#!/bin/bash
# ci/nightly.sh: the timing gates, every night, on a quiet maintainer machine. GitHub's machines are
# shared, so they can't judge real-time audio (PLAT-1); this runs the whole check WITH the gates on
# a machine you trust and keep quiet at night. There is no self-hosted GitHub runner on purpose: a
# pull request from a fork can change a workflow to run its own code on one.
#
#   ci/nightly.sh            check origin/main: fetch it, clone it fresh, run ci/check.sh --gates
#   ci/nightly.sh --report   the same, then post its verdict as the commit status "nightly/gates"
#                            on that commit (curl, GitHub's API), with the token in the file
#                            M2_NIGHTLY_TOKEN_FILE (default <M2_NIGHTLY>/token): a fine-grained
#                            token for this one repository that can only write commit statuses
#   ci/nightly.sh --keep     keep the clone afterwards (it is deleted otherwise)
#
# In short: it never touches the checkout it lives in (it only fetches origin there), never your
# ~/Music or ~/.music2 (ci/check.sh gives everything a scratch HOME), and never runs twice at once.
# Everything goes under M2_NIGHTLY (default ~/.cache/m2-nightly): the clone (clone/), a build folder
# kept between nights so only music2's own crates rebuild (target/), and for each night a folder
# runs/<date>-<commit>/ with the check's logs and summary.txt. last.txt is the newest summary.
# The first line of a summary is the verdict; the "load" lines say how busy the machine was before
# and after (the gates want it idle). Exit status: ci/check.sh's (0 pass, 1 fail, 2 couldn't
# start), or 2 when the fetch, the clone or posting the status failed.
#
# Security: this builds and runs whatever is on main, and every dependency's build script, with
# this user's rights, every night, with no one watching. So run it as an OS user of its own that
# holds no other credentials (no gh login, no ssh keys, no cloud keys: docs/maintainers.md, part
# 5), and give --report only a token that can do nothing but post a commit status here. If merged
# code or a dependency ever turned bad, that is all it could take. --report refuses any other kind
# of token (one that doesn't start github_pat_, such as gh's own or a classic one), before the
# check starts. The check runs with no GitHub token in its environment, and the token file is read
# in full only after it. The summary warns when this user holds other credentials. The repository is
# M2_NIGHTLY_REPO (owner/name), or else origin's; the API is GITHUB_API_URL, or api.github.com.
#
# Every night at 02:17, with cron, in the crontab of the user it runs as (crontab -e; one line;
# Linux):
#   17 2 * * * cd /path/to/music2 && ci/nightly.sh --report >> "$HOME/.cache/m2-nightly/cron.log" 2>&1
# On the reference Mac, run it from a LaunchAgent in that user's login session, not cron: the app's
# test and G12 need the session (ci/check.sh's top says why). Written for bash 3.2, which macOS
# ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) || exit 2
REPORT=
KEEP=
for a in "$@"; do
  case $a in
    --report) REPORT=1 ;;
    --keep) KEEP=1 ;;
    *) sed -n '2,33p' "$0" >&2; exit 2 ;;
  esac
done
BASE=${M2_NIGHTLY:-${XDG_CACHE_HOME:-$HOME/.cache}/m2-nightly}
mkdir -p "$BASE/runs" || exit 2
# --report: the token and the repository, checked now, so a night isn't spent for nothing. Only
# the token's first characters are read here.
TOKEN_FILE=${M2_NIGHTLY_TOKEN_FILE:-$BASE/token}
API=${GITHUB_API_URL:-https://api.github.com}
REPO=
if [ -n "$REPORT" ]; then
  if [ ! -r "$TOKEN_FILE" ]; then
    echo "ci/nightly.sh: --report needs a token in $TOKEN_FILE (or name the file in M2_NIGHTLY_TOKEN_FILE):"
    echo "a fine-grained token for this one repository, with only Commit statuses: Read and write"
    exit 2
  fi
  case $(head -c 11 "$TOKEN_FILE") in
    github_pat_) ;;
    *) echo "ci/nightly.sh: $TOKEN_FILE isn't a fine-grained token (they start github_pat_). A gh login's or a"
       echo "classic token can reach every repository you can; this one must only write commit statuses here."
       exit 2 ;;
  esac
  # owner/name from origin's address as written: git@host:o/n.git, ssh://git@host/o/n.git or
  # https://host/o/n(.git).
  REPO=${M2_NIGHTLY_REPO:-}
  if [ -z "$REPO" ]; then
    REPO=$(git -C "$ROOT" config --get remote.origin.url 2> /dev/null | sed -E 's#^(git@[^:]+:|ssh://[^/]+/|https?://[^/]+/)##; s#\.git$##')
  fi
  case $REPO in
    [A-Za-z0-9_.-]*/[A-Za-z0-9_.-]*) ;;
    *) echo "ci/nightly.sh: couldn't tell the repository from origin's address: set M2_NIGHTLY_REPO=owner/name"; exit 2 ;;
  esac
fi
# What else this user could hand to code that turned bad (said in the summary, not refused).
creds=
for f in "$HOME/.config/gh/hosts.yml" "$HOME/.git-credentials" "$HOME/.netrc" "$HOME/.aws/credentials" \
  "$HOME/.docker/config.json" "$HOME/.cargo/credentials.toml" "$HOME/.npmrc" "$HOME"/.ssh/id_*; do
  if [ -e "$f" ]; then creds="$creds${creds:+, }${f#"$HOME"/}"; fi
done
# One at a time: mkdir is atomic. A lock left by a run that died says when it was taken.
LOCK=$BASE/lock
if ! mkdir "$LOCK" 2> /dev/null; then
  echo "ci/nightly.sh: another run holds $LOCK (since $(cat "$LOCK/since" 2> /dev/null || echo '?')); remove it if none is running"
  exit 2
fi
date > "$LOCK/since"
trap 'rm -rf "$LOCK"' EXIT
load() { uptime | sed 's/.*load average[s]*: *//'; }

echo "nightly: fetching origin/main in $ROOT"
if ! git -C "$ROOT" fetch --quiet origin main || ! SHA=$(git -C "$ROOT" rev-parse --verify origin/main^{commit}); then
  echo "ci/nightly.sh: couldn't fetch origin/main"
  exit 2
fi
SHORT=$(printf '%s' "$SHA" | cut -c1-12)
CLONE=$BASE/clone
RUN=$BASE/runs/$(date +%Y-%m-%d-%H%M)-$SHORT
rm -rf "$CLONE"
# A clone of this checkout's objects, at origin/main's commit, with every file fresh from git and
# the whole history (the gates' old engine S is built from an old commit).
if ! git clone --quiet --no-checkout "$ROOT" "$CLONE" || ! git -C "$CLONE" fetch --quiet "$ROOT" refs/remotes/origin/main ||
   ! git -C "$CLONE" checkout --quiet --detach "$SHA"; then
  echo "ci/nightly.sh: couldn't clone $ROOT at $SHORT into $CLONE"
  exit 2
fi
mkdir -p "$RUN" || exit 2
{
  echo "nightly of $SHORT, started $(date)"
  echo "load before: $(load)"
  if [ -n "$creds" ]; then echo "warning: this user also holds $creds; run it as a user with none (docs/maintainers.md, part 5)"; fi
} > "$RUN/nightly.txt"
echo "nightly: ci/check.sh --gates on $SHORT (logs in $RUN/check)"
# No GitHub token of any kind in the check's environment.
(cd "$CLONE" && env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN -u M2_NIGHTLY_TOKEN_FILE \
  CARGO_TARGET_DIR="$BASE/target" M2_CHECK_OUT="$RUN/check" ci/check.sh --gates) > "$RUN/check.log" 2>&1 < /dev/null
st=$?
echo "load after: $(load)" >> "$RUN/nightly.txt"
if [ -s "$RUN/check/summary.txt" ]; then
  sed '/^$/d' "$RUN/check/summary.txt" | head -1 > "$RUN/summary.txt"
  cat "$RUN/nightly.txt" >> "$RUN/summary.txt"
  sed '1,/^music2 check/d' "$RUN/check/summary.txt" >> "$RUN/summary.txt"
else
  { echo "music2 check: FAIL: it printed no summary (exit $st)"; cat "$RUN/nightly.txt"; tail -20 "$RUN/check.log"; } > "$RUN/summary.txt"
fi
cp "$RUN/summary.txt" "$BASE/last.txt"
if [ -z "$KEEP" ]; then rm -rf "$CLONE"; fi
verdict=$(head -1 "$RUN/summary.txt")
echo "$verdict"
echo "nightly: the summary is $RUN/summary.txt"

if [ -n "$REPORT" ]; then
  if [ $st -eq 0 ]; then state=success; else state=failure; fi
  # GitHub keeps 140 characters of a status's description; JSON wants \ and " escaped.
  desc=$(printf '%s' "$verdict" | sed 's/^music2 check: //' | cut -c1-139 | sed 's/[\\"]/\\&/g')
  # The token goes to curl on its standard input, never on its command line (ps shows that).
  if { printf 'header = "Authorization: Bearer %s"\n' "$(head -1 "$TOKEN_FILE" | tr -d '\r\n')"; } |
     curl --config - -fsS -o /dev/null -X POST -H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28' \
       --data "{\"state\":\"$state\",\"context\":\"nightly/gates\",\"description\":\"$desc\"}" "$API/repos/$REPO/statuses/$SHA"; then
    echo "nightly: posted nightly/gates = $state on $SHORT of $REPO"
  else
    echo "ci/nightly.sh: couldn't post the status to $REPO (is the token in $TOKEN_FILE current, for this repository,"
    echo "with Commit statuses: Read and write?)"
    exit 2
  fi
fi
exit $st
