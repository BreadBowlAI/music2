#!/bin/bash
# ci/nightly-issue.sh: what a night of the nightly tier left for people to look at, each in ONE
# open issue: a new one if none is open, else a comment on it. .github/workflows/nightly.yml's last
# job runs it, with GH_TOKEN allowed to write issues and nothing else.
#
#   GH_TOKEN=<token> ci/nightly-issue.sh <run url> <what failed>
#       the night failed: issue "Nightly failed"
#   GH_TOKEN=<token> ci/nightly-issue.sh --flaky <run url> <list>
#       tests or suites passed only on a retry: issue "Flaky tests". <list> has one a line, as
#       "macos: test m2-play::no_alloc the_audio_thread_never_allocates" (nextest's name: the
#       binary, then the test), "linux: test the_audio_thread_never_allocates" (cargo test's,
#       from ci/check.sh: no binary) or "macos: suite layout-proj"; each is said with the number
#       of nights it was named on, this one too.
#   ci/nightly-issue.sh --list <os> <folder>
#       for a job's $GITHUB_OUTPUT: "list", every line of each file named flaky one or two
#       folders down in <folder> (ci/check.sh's <commit>/flaky, ci/pr.sh's <commit>/pr-<step>/flaky
#       or pr-<commit>/<step>/flaky), once each, with "<os>: " before it. It reads every commit's
#       folder there: GitHub's machines start empty, so on one that keeps them, empty it first.
#
# In short: people watch one issue, not one a night, and a test that keeps passing only on its
# retry shows as its count goes up. It needs gh (the GitHub CLI) and is never needed on your own
# machine; with GH_REPO set it can be tried against another repo. Exit 1 when gh fails.
set -u -o pipefail
if [ "${1:-}" = --list ]; then
  if [ $# -ne 3 ]; then sed -n '2,22p' "$0" >&2; exit 2; fi
  echo 'list<<M2_FLAKY_END'
  find "$3" -mindepth 2 -maxdepth 3 -type f -name flaky -exec cat {} + 2> /dev/null | sort -u | sed "/^$/d; s/^/$2: /"
  echo M2_FLAKY_END
  exit 0
fi
FLAKY=
if [ "${1:-}" = --flaky ]; then FLAKY=1; shift; fi
if [ $# -ne 2 ]; then sed -n '2,22p' "$0" >&2; exit 2; fi
RUN=$1
DAY=$(date -u +%Y-%m-%d)
if [ -n "$FLAKY" ]; then TITLE="Flaky tests"; else TITLE="Nightly failed"; fi

# The open issue with exactly that title, if there is one (search is fuzzy: check the title).
n=$(gh issue list --state open --search "in:title \"$TITLE\"" --json number,title \
  --jq "map(select(.title == \"$TITLE\")) | .[0].number // empty") || exit 1

if [ -n "$FLAKY" ]; then
  # What the issue said before, to count the nights each name was on: its body and every
  # comment, one line of ours a name ("- night 3: macos: suite layout-proj"). A body or comment
  # edited on GitHub's web page comes back with \r\n line ends: the \r goes.
  said=
  if [ -n "$n" ]; then
    said=$(gh issue view "$n" --json body,comments --jq '.body, .comments[].body' | tr -d '\r') || exit 1
  fi
  lines=
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    # The name reaches awk through the environment, as it is (awk -v would read its backslashes).
    k=$(printf '%s\n' "$said" | k=": $name" awk 'BEGIN { k = ENVIRON["k"] } /^- night [0-9]+: / && substr($0, length($0) - length(k) + 1) == k' | wc -l)
    lines="$lines- night $((k + 1)): $name
"
  done < <(printf '%s\n' "$2" | sort -u)
  if [ -z "$lines" ]; then echo "nightly: no flaky tests named"; exit 0; fi
  BODY="Passed only on a retry on $DAY:

$lines
Run: $RUN

\"night N\": the Nth night this issue names it. Each is a flake to find the cause of (AGENTS.md,
rule 4), and HANDOFF.md's known flakes say what is known. One that keeps coming back can move to
the nightly tier (ci/nightly-tests.txt, or e2e/run.mjs's tier tags): that keeps it out of pull
requests, but the nightly runs it still, so it is named here each night it flakes until its cause
is fixed. Each job's logs are the run's artifacts, kept 14 days."
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
