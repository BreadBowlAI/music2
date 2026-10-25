#!/bin/bash
# ci/docs-only-test.sh: tries the rule of ci/docs-only.sh (when a pull request is documentation
# only, so CI runs just the docs job) on a table of cases. Each case gets its own small git repo,
# made in a temporary folder outside this one. ci/pr.sh runs it in its "quick" and "docs" steps.
# Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case passed, 1
# when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
. "$HERE/docs-only.sh" || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-docs-rule.XXXXXX") || exit 2
trap 'rm -rf "$TMP"' EXIT
# git without your settings (no global or system config, so no hooks or signing) and a fixed author.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES \
  GIT_CEILING_DIRECTORIES GIT_CONFIG_GLOBAL GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT
export HOME=$TMP XDG_CONFIG_HOME=$TMP/config GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=check \
  GIT_AUTHOR_EMAIL=check@example.invalid GIT_COMMITTER_NAME=check GIT_COMMITTER_EMAIL=check@example.invalid

# c <message>: commit everything.
c() { git add -A && git commit -q -m "$1"; }
# edit <file>: change it (or make it), folders and all.
edit() { mkdir -p "$(dirname "$1")" && echo "changed $RANDOM" >> "$1"; }

# The fixture: a tree shaped like music2's, on main.
F=$TMP/fixture
mkdir -p "$F" && cd "$F" && git init -q && git symbolic-ref HEAD refs/heads/main || exit 2
for f in core/a.rs core/a.md crates/gates/README.md web/src/app.tsx e2e/run.mjs e2e/README.md \
  docs/architecture.md docs/gaps.md docs/designs/step9/plan.md docs/guide/window.md docs/guide/SUMMARY.md \
  docs/book.toml std/src/dsp.ts ci/check.sh Cargo.toml Cargo.lock \
  README.md HANDOFF.md CONTRIBUTING.md LICENSE-MIT .github/workflows/ci.yml .github/ISSUE_TEMPLATE/bug.yml; do
  edit "$f"
done
c fixture || exit 2
cd "$TMP" || exit 2

passed=0
failed=0
n=0
# case <name> <docs|full> <words its verdict line holds> <base> <commands that make the change>:
# the commands run in a copy of the fixture, on a branch "topic" taken from main.
case_() {
  n=$((n + 1))
  dir=$TMP/case-$n
  cp -R "$F" "$dir" || exit 2
  if ! (cd "$dir" && git checkout -q -b topic && eval "$5") > "$TMP/setup.log" 2>&1; then
    failed=$((failed + 1))
    echo "FAIL  $1: setting it up failed: $(head -1 "$TMP/setup.log")"
    return
  fi
  out=$(cd "$dir" && docs_only "$4")
  st=$?
  line=$(printf '%s\n' "$out" | head -1)
  if [ "$2" = docs ]; then want=0; else want=1; fi
  case $line in
    "$2 "*"$3"*)
      if [ $st -eq $want ]; then
        passed=$((passed + 1))
        echo "PASS  $1: $line"
        return
      fi ;;
  esac
  failed=$((failed + 1))
  echo "FAIL  $1: wanted \"$2 ... $3\" (status $want), got status $st: $line"
}

case_ "HANDOFF.md" docs "only HANDOFF.md changed since" main \
  'edit HANDOFF.md && c handoff'
case_ "README.md, which apps/m2/tests/docs.rs reads" full "README.md changed since" main \
  'edit README.md && c readme'
case_ "README.md beside a design note" full "README.md changed since" main \
  'edit docs/designs/step9/plan.md && edit README.md && c both'
case_ "the counts and the base" docs "changed since $(cd "$F" && git rev-parse --short=12 HEAD) (3 files)" main \
  'edit docs/gaps.md && edit docs/designs/step9/plan.md && edit docs/new.png && c docs'
case_ "every top-level document a build doesn't read" docs "only HANDOFF.md, CONTRIBUTING.md, AGENTS.md, CLAUDE.md, CODE_OF_CONDUCT.md, SECURITY.md, GOVERNANCE.md, LICENSE-MIT, LICENSE-APACHE changed" main \
  'for f in HANDOFF.md CONTRIBUTING.md AGENTS.md CLAUDE.md CODE_OF_CONDUCT.md SECURITY.md GOVERNANCE.md LICENSE-MIT LICENSE-APACHE; do edit $f; done && c top'
case_ "GitHub's issue and pull request forms" docs "only .github/ISSUE_TEMPLATE/, .github/PULL_REQUEST_TEMPLATE.md changed" main \
  'edit .github/ISSUE_TEMPLATE/bug.yml && edit .github/ISSUE_TEMPLATE/config.yml && edit .github/PULL_REQUEST_TEMPLATE.md && c forms'
case_ "a deleted doc" docs "only docs/ changed" main \
  'git rm -q docs/gaps.md && c gone'
case_ "a design note" docs "only docs/ changed" main \
  'edit docs/designs/step9/plan.md && c note'
case_ "the site's settings, which only mdBook reads" docs "only docs/ changed" main \
  'edit docs/book.toml && c book'
case_ "a page of the guide, which m2 embeds (D1)" full "docs/guide/window.md changed since" main \
  'edit docs/guide/window.md && c guide'
case_ "a new page of the guide, and the menu" full "docs/guide/SUMMARY.md changed since" main \
  'edit docs/guide/new.md && edit docs/guide/SUMMARY.md && c guide'
case_ "a picture in the guide" full "docs/guide/images/a.png changed since" main \
  'edit docs/guide/images/a.png && c picture'
case_ "a guide page beside a design note" full "docs/guide/window.md changed since" main \
  'edit docs/designs/step9/plan.md && edit docs/guide/window.md && c both'
case_ "the contract, which a test reads (D2)" full "docs/architecture.md changed since" main \
  'edit docs/architecture.md && c contract'
case_ "the contract deleted" full "docs/architecture.md changed since" main \
  'git rm -q docs/architecture.md && c gone'
case_ "a file named like the contract elsewhere under docs/" docs "only docs/ changed" main \
  'edit docs/designs/architecture.md && c note'
case_ "a name with a space" docs "only docs/ changed" main \
  'edit "docs/my notes.md" && c space'
case_ "main moved on with code; the branch changed only docs" docs "only docs/ changed" main \
  'git checkout -q main && edit core/a.rs && c code && git checkout -q topic && edit docs/a.md && c docs'
case_ "the branch merged main in after main changed code" docs "only docs/ changed" main \
  'edit docs/a.md && c docs && git checkout -q main && edit core/a.rs && c code && git checkout -q topic && git merge -q --no-edit main'
case_ "a pull request's merge commit, judged against its first parent" docs "only HANDOFF.md changed" HEAD^1 \
  'edit HANDOFF.md && c handoff && git checkout -q main && edit core/a.rs && c code && git merge -q --no-ff --no-edit topic'
case_ "a merge commit whose pull request changed code" full "core/a.rs changed since" HEAD^1 \
  'edit core/a.rs && c code && git checkout -q main && edit docs/a.md && c docs && git merge -q --no-ff --no-edit topic'
case_ "a doc and a Rust file" full "core/a.rs changed since" main \
  'edit HANDOFF.md && edit core/a.rs && c both'
case_ "a Markdown file in a code folder" full "core/a.md changed" main \
  'edit core/a.md && c md'
case_ "e2e/README.md" full "e2e/README.md changed" main \
  'edit e2e/README.md && c e2e'
case_ "a crate's README" full "crates/gates/README.md changed" main \
  'edit crates/gates/README.md && c readme'
case_ "a workflow" full ".github/workflows/ci.yml changed" main \
  'edit .github/workflows/ci.yml && c ci'
case_ "dependabot's settings" full ".github/dependabot.yml changed" main \
  'edit .github/dependabot.yml && c deps'
case_ "CODEOWNERS" full ".github/CODEOWNERS changed" main \
  'edit .github/CODEOWNERS && c owners'
case_ "CODEOWNERS under docs/, which GitHub reads too" full "docs/CODEOWNERS changed" main \
  'edit docs/CODEOWNERS && c owners'
case_ "CODEOWNERS at the top" full "CODEOWNERS changed" main \
  'edit CODEOWNERS && c owners'
case_ "CODEOWNERS deeper under docs/" full "docs/team/CODEOWNERS changed" main \
  'edit docs/team/CODEOWNERS && c owners'
case_ "a doc whose name only ends like CODEOWNERS" docs "only docs/ changed" main \
  'edit docs/MYCODEOWNERS && c owners'
case_ "ci/" full "ci/check.sh changed" main \
  'edit ci/check.sh && c ci'
case_ "Cargo.lock" full "Cargo.lock changed" main \
  'edit Cargo.lock && c lock'
case_ "a new top-level file" full "NEWS.md changed" main \
  'edit NEWS.md && c news'
case_ "a name that only starts like a free one" full "README.md.orig changed" main \
  'edit README.md.orig && c orig'
case_ "a folder named like a free file" full "LICENSE-MIT.d/x changed" main \
  'edit LICENSE-MIT.d/x && c dir'
case_ "a path that only starts like docs/" full "docsite/a.md changed" main \
  'edit docsite/a.md && c site'
case_ "a rename from core/ to docs/" full "core/a.rs changed" main \
  'git mv core/a.rs docs/a.rs && c move'
case_ "a deleted code file" full "core/a.rs changed" main \
  'git rm -q core/a.rs && c gone'
case_ "a symlink under docs/" full "docs/link is a symlink" main \
  'ln -s ../core/a.rs docs/link && c link'
case_ "a submodule under docs/" full "docs/sub isn't a plain file (git mode 160000)" main \
  'git update-index --add --cacheinfo 160000,$(git rev-parse HEAD),docs/sub && git commit -q -m sub'
case_ "a name git quotes" full "changed since" main \
  'edit "docs/a\"b.md" && c quoted'
case_ "an empty diff: main itself" full "nothing changed since" main \
  'git checkout -q main'
case_ "a branch already merged into main" full "nothing changed since" main \
  'edit docs/a.md && c docs && git checkout -q main && git merge -q --ff-only topic && git checkout -q topic'
case_ "a bad base" full "the base no-such-rev isn't a commit" no-such-rev \
  'edit docs/a.md && c docs'
case_ "a base with no history in common" full "no merge base" other \
  'edit docs/a.md && c docs && git checkout -q --orphan other && c other && git checkout -q topic'
case_ "not a git repo" full "isn't a commit" main \
  'edit docs/a.md && c docs && rm -rf .git'

echo
echo "$passed of $n cases passed"
[ $failed -eq 0 ] && [ $passed -eq $n ]
