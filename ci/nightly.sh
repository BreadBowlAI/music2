#!/bin/bash
# ci/nightly.sh: the timing gates, every night, on a quiet maintainer machine. GitHub's machines are
# shared, so they can't judge real-time audio (PLAT-1); this runs the whole check WITH the gates on
# a machine you trust and keep quiet at night. There is no self-hosted GitHub runner on purpose: a
# pull request from a fork can change a workflow to run its own code on one.
#
#   ci/nightly.sh            check origin/main: fetch it, clone it fresh, run ci/check.sh --gates
#   ci/nightly.sh --report   the same, then post its verdict as the commit status "nightly/gates"
#                            on that commit, with gh (your own login: it needs the repo scope)
#   ci/nightly.sh --keep     keep the clone afterwards (it is deleted otherwise)
#
# In short: it never touches the checkout it lives in (it only fetches origin there), never your
# ~/Music or ~/.music2 (ci/check.sh gives everything a scratch HOME), and never runs twice at once.
# Everything goes under M2_NIGHTLY (default ~/.cache/m2-nightly): the clone (clone/), a build folder
# kept between nights so only music2's own crates rebuild (target/), and for each night a folder
# runs/<date>-<commit>/ with the check's logs and summary.txt. last.txt is the newest summary.
# The first line of a summary is the verdict; the "load" lines say how busy the machine was before
# and after (the gates want it idle). Exit status: ci/check.sh's (0 pass, 1 fail, 2 couldn't
# start), or 2 when the fetch, the clone or gh failed.
#
# Every night at 02:17, with cron (crontab -e; one line; Linux):
#   17 2 * * * cd /path/to/music2 && ci/nightly.sh --report >> "$HOME/.cache/m2-nightly/cron.log" 2>&1
# On the reference Mac, run it from a LaunchAgent in your login session, not cron: the app's test
# and G12 need the session (ci/check.sh's top says why). Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) || exit 2
REPORT=
KEEP=
for a in "$@"; do
  case $a in
    --report) REPORT=1 ;;
    --keep) KEEP=1 ;;
    *) sed -n '2,12p' "$0" >&2; exit 2 ;;
  esac
done
BASE=${M2_NIGHTLY:-${XDG_CACHE_HOME:-$HOME/.cache}/m2-nightly}
mkdir -p "$BASE/runs" || exit 2
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
{ echo "nightly of $SHORT, started $(date)"; echo "load before: $(load)"; } > "$RUN/nightly.txt"
echo "nightly: ci/check.sh --gates on $SHORT (logs in $RUN/check)"
(cd "$CLONE" && CARGO_TARGET_DIR=$BASE/target M2_CHECK_OUT=$RUN/check ci/check.sh --gates) > "$RUN/check.log" 2>&1 < /dev/null
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
  # GitHub keeps 140 characters of a status's description.
  desc=$(printf '%s' "$verdict" | sed 's/^music2 check: //' | cut -c1-139)
  if (cd "$ROOT" && gh api --silent "repos/{owner}/{repo}/statuses/$SHA" -f state="$state" -f context=nightly/gates -f description="$desc"); then
    echo "nightly: posted nightly/gates = $state on $SHORT"
  else
    echo "ci/nightly.sh: gh couldn't post the status (is gh logged in, with the repo scope?)"
    exit 2
  fi
fi
exit $st
