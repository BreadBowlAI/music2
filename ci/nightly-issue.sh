#!/bin/bash
# ci/nightly-issue.sh: when the nightly tier failed, say so in ONE open issue, "Nightly failed": a
# new one if none is open, else a comment on it. .github/workflows/nightly.yml's last job runs it,
# with GH_TOKEN allowed to write issues and nothing else.
#
#   GH_TOKEN=<token> ci/nightly-issue.sh <run url> <what failed>
#
# In short: people watch one issue, not one a night. It needs gh (the GitHub CLI) and is never
# needed on your own machine; with GH_REPO set it can be tried against another repo. Exit 1 when
# gh fails.
set -u -o pipefail
if [ $# -ne 2 ]; then sed -n '2,11p' "$0" >&2; exit 2; fi
RUN=$1
WHAT=$2
TITLE="Nightly failed"
DAY=$(date -u +%Y-%m-%d)
BODY="The nightly tier failed on $DAY: $WHAT.

Run: $RUN

Each job's summary is on the run's page; the logs are its artifacts. A test that fails only here
is probably a flake or a speed bound: see ci/nightly-tests.txt and e2e/run.mjs's tier tags."
# The open issue with exactly that title, if there is one (search is fuzzy: check the title).
n=$(gh issue list --state open --search "in:title \"$TITLE\"" --json number,title \
  --jq "map(select(.title == \"$TITLE\")) | .[0].number // empty") || exit 1
if [ -n "$n" ]; then
  gh issue comment "$n" --body "$BODY" || exit 1
  echo "nightly: commented on issue #$n"
else
  gh issue create --title "$TITLE" --body "$BODY" || exit 1
  echo "nightly: opened an issue"
fi
