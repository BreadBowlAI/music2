#!/bin/bash
# ci/guide.sh: the guide (docs/guide/) as a website, and the two pages made from code.
#
#   ci/guide.sh check   the link check (ci/links.mjs), then the site, built by mdBook into
#                       target/guide. Any warning or error mdBook prints fails it. ci/pr.sh's docs
#                       and lint steps run it, and .github/workflows/pages.yml publishes what it builds.
#   ci/guide.sh gen     writes docs/guide/commands.md and docs/guide/routes.md again, from
#                       m2 docs commands and m2 docs routes (a debug m2, built here, run with a
#                       scratch HOME, M2_AUDIO=none and M2_MIDI=none). apps/m2/tests/docs.rs fails
#                       when the committed pages differ from what m2 makes.
#   ci/guide.sh serve   a preview at http://localhost:3000 while you write (mdbook serve), rebuilt
#                       at each save.
#
# In short: mdBook comes from ci/tools.sh (pinned, its sha256 checked), so the site builds the same
# here and in CI. The settings are docs/book.toml. Exit 0 when it worked, 1 when not, 2 on a usage
# error. Written for bash 3.2, which macOS ships.

set -u -o pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd) && cd "$ROOT" || exit 2
usage() { sed -n '/^#   ci\/guide.sh check/,/^#                       at each save/p' "$0" | sed 's/^# //' >&2; exit 2; }
[ $# -eq 1 ] || usage

# mdbook: put ci/tools.sh's pinned mdBook on PATH.
mdbook_path() {
  local p
  if ! p=$(ci/tools.sh mdbook | tail -1) || [ -z "$p" ]; then
    echo "ci/guide.sh: ci/tools.sh couldn't get mdBook" >&2
    return 1
  fi
  PATH=$p:$PATH
}

case $1 in
  check)
    node ci/links.mjs || exit 1
    mdbook_path || exit 1
    log=$(mktemp "${TMPDIR:-/tmp}/m2-guide.XXXXXX") || exit 2
    mdbook build docs > "$log" 2>&1
    st=$?
    cat "$log"
    # mdBook says WARN or ERROR for what it couldn't do, and may still exit 0.
    if [ $st -ne 0 ] || grep -qE '(WARN|ERROR)' "$log"; then
      rm -f "$log"
      echo "ci/guide.sh: mdBook didn't build the site cleanly (above)"
      exit 1
    fi
    rm -f "$log"
    echo "the site is in target/guide ($(find target/guide -name '*.html' | wc -l | tr -d ' ') pages, with mdBook's own)" ;;
  gen)
    cargo build --locked -p m2 || exit 1
    bin=${CARGO_TARGET_DIR:-target}/debug/m2
    home=$(mktemp -d "${TMPDIR:-/tmp}/m2-guide-home.XXXXXX") || exit 2
    for page in commands routes; do
      if ! env HOME="$home" M2_AUDIO=none M2_MIDI=none "$bin" docs $page > "docs/guide/$page.md.part"; then
        rm -f "docs/guide/$page.md.part"
        rm -rf "$home"
        echo "ci/guide.sh: m2 docs $page failed"
        exit 1
      fi
      mv "docs/guide/$page.md.part" "docs/guide/$page.md" || exit 1
      echo "wrote docs/guide/$page.md"
    done
    rm -rf "$home" ;;
  serve)
    mdbook_path || exit 1
    exec mdbook serve docs ;;
  *) usage ;;
esac
