#!/bin/bash
# ci/nightly-issue.sh: what a night of the nightly tier left for people to look at, each in ONE
# open issue: a new one if none is open, else a comment on it. .github/workflows/nightly.yml's last
# job runs it, with GH_TOKEN allowed to write issues and nothing else.
#
#   GH_TOKEN=<token> ci/nightly-issue.sh <run url> <what failed>
#       the night failed: issue "Nightly failed"
#   GH_TOKEN=<token> ci/nightly-issue.sh --flaky <run url> <list>
#       tests or suites passed only on a retry: issue "Flaky tests". <list> has one a line, as
#       "linux: test m2-play::no_alloc the_audio_thread_never_allocates" or "macos: suite
#       layout-proj"; each is said with the number of nights it was named on, this one too.
#   ci/nightly-issue.sh --list <os> <folder>
#       for a job's $GITHUB_OUTPUT: "list", every line of each file named flaky in <folder> (two
#       levels down at most: ci/check.sh's and ci/pr.sh's), once each, with "<os>: " before it.
#
# In short: people watch one issue, not one a night, and a test that keeps passing only on its
# retry shows as its count goes up. It needs gh (the GitHub CLI) and is never needed on your own
# machine; with GH_REPO set it can be tried against another repo. Exit 1 when gh fails.
set -u -o pipefail
if [ "${1:-}" = --list ]; then
  if [ $# -ne 3 ]; then sed -n '2,18p' "$0" >&2; exit 2; fi
  echo 'list<<M2_FLAKY_END'
  find "$3" -mindepth 2 -maxdepth 3 -type f -name flaky -exec cat {} + 2> /dev/null | sort -u | sed "/^$/d; s/^/$2: /"
  echo M2_FLAKY_END
  exit 0
fi
FLAKY=
if [ "${1:-}" = --flaky ]; then FLAKY=1; shift; fi
if [ $# -ne 2 ]; then sed -n '2,18p' "$0" >&2; exit 2; fi
RUN=$1
DAY=$(date -u +%Y-%m-%d)
if [ -n "$FLAKY" ]; then TITLE="Flaky tests"; else TITLE="Nightly failed"; fi

# The open issue with exactly that title, if there is one (search is fuzzy: check the title).
n=$(gh issue list --state open --search "in:title \"$TITLE\"" --json number,title \
  --jq "map(select(.title == \"$TITLE\")) | .[0].number // empty") || exit 1

if [ -n "$FLAKY" ]; then
  # What the issue said before, to count the nights each name was on: its body and every
  # comment, one line of ours a name ("- night 3: macos: suite layout-proj").
  said=
  if [ -n "$n" ]; then
    said=$(gh issue view "$n" --json body,comments --jq '.body, .comments[].body') || exit 1
  fi
  lines=
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    k=$(printf '%s\n' "$said" | awk -v k=": $name" '/^- night [0-9]+: / && substr($0, length($0) - length(k) + 1) == k' | wc -l)
    lines="$lines- night $((k + 1)): $name
"
  done < <(printf '%s\n' "$2" | sort -u)
  if [ -z "$lines" ]; then echo "nightly: no flaky tests named"; exit 0; fi
  BODY="Passed only on a retry on $DAY:

$lines
Run: $RUN

\"night N\": the Nth night this issue names it. Each is a flake to find the cause of (AGENTS.md,
rule 4); one that keeps coming back moves to the nightly tier until it is fixed
(ci/nightly-tests.txt, or e2e/run.mjs's tier tags), and HANDOFF.md's known flakes say what is known.
Each job's logs are the run's artifacts, kept 14 days."
else
  BODY="The nightly tier failed on $DAY: $2.

Run: $RUN

Each job's summary is on the run's page; the logs are its artifacts. A test that fails only here
is probably a flake or a speed bound: see ci/nightly-tests.txt and e2e/run.mjs's tier tags."
fi

if [ -n "$n" ]; then
  gh issue comment "$n" --body "$BODY" || exit 1
  echo "nightly: commented on issue #$n ($TITLE)"
else
  gh issue create --title "$TITLE" --body "$BODY" || exit 1
  echo "nightly: opened an issue ($TITLE)"
fi
