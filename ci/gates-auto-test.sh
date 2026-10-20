#!/bin/bash
# ci/gates-auto-test.sh: tries the rule of ci/gates-auto.sh (when --gates=auto may skip the timing
# gates) on a table of cases. Each case gets its own small git repo, made in a temporary folder
# outside this one. ci/check.sh runs it as its "gates rule" row. Prints a PASS or FAIL line for each
# case, then a total. Exit status: 0 when every case passed, 1 when not, 2 when it couldn't start.
# Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
. "$HERE/gates-auto.sh" || exit 2
TMP=$(mktemp -d "${TMPDIR:-/tmp}/m2-gates-rule.XXXXXX") || exit 2
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
for f in core/a.rs crates/gates/src/lib.rs crates/gates/README.md web/src/app.tsx web/package.json \
  e2e/run.mjs e2e/renders.sha256 docs/architecture.md std/src/dsp.ts examples/song/song.ts \
  templates/starter/song.ts ci/check.sh ci/gates.txt Cargo.toml Cargo.lock rust-toolchain.toml \
  README.md HANDOFF.md; do
  edit "$f"
done
printf 'target/\nnode_modules/\ndist/\n' > .gitignore
c fixture || exit 2
cd "$TMP" || exit 2

passed=0
failed=0
n=0
# case <name> <skip|run> <words its verdict line holds> <base> <commands that make the change>:
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
  out=$(cd "$dir" && gates_auto "$4")
  st=$?
  line=$(printf '%s\n' "$out" | head -1)
  if [ "$2" = skip ]; then want=0; else want=1; fi
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

case_ "a UI-only change (web/, docs/)" skip "only web/, docs/ changed since" main \
  'edit web/src/app.tsx && rm web/package.json && edit docs/new.md && c ui'
case_ "the counts and the base" skip "changed since $(cd "$F" && git rev-parse --short=12 HEAD) (3 files)" main \
  'edit web/src/app.tsx && rm web/package.json && edit docs/new.md && c ui'
case_ "README.md and HANDOFF.md" skip "only README.md, HANDOFF.md changed" main \
  'edit HANDOFF.md && edit README.md && c notes'
case_ "the branch merged main in after main changed code" skip "only docs/ changed" main \
  'edit docs/a.md && c docs && git checkout -q main && edit core/a.rs && c code && git checkout -q topic && git merge -q --no-edit main'
case_ "main moved on with code; the branch changed only docs" skip "only docs/ changed" main \
  'git checkout -q main && edit core/a.rs && c code && git checkout -q topic && edit docs/a.md && c docs'
case_ "--base: a commit on the branch" skip "only docs/ changed" HEAD~1 \
  'edit core/a.rs && c code && edit docs/a.md && c docs'
case_ "a build output git ignores (web/dist/)" skip "only web/ changed" main \
  'edit web/src/app.tsx && c ui && edit web/dist/index.html'
case_ "a Rust file" run "core/a.rs changed since" main \
  'edit core/a.rs && c code'
case_ "a UI change and a Rust file" run "core/a.rs changed since" main \
  'edit web/src/app.tsx && edit core/a.rs && c both'
case_ "the gates crate's README" run "crates/gates/README.md changed" main \
  'edit crates/gates/README.md && c readme'
case_ "std/" run "std/src/dsp.ts changed" main \
  'edit std/src/dsp.ts && c std'
case_ "examples/" run "examples/song/song.ts changed" main \
  'edit examples/song/song.ts && c example'
case_ "templates/" run "templates/starter/song.ts changed" main \
  'edit templates/starter/song.ts && c template'
case_ "Cargo.lock" run "Cargo.lock changed" main \
  'edit Cargo.lock && c lock'
case_ "rust-toolchain.toml" run "rust-toolchain.toml changed" main \
  'edit rust-toolchain.toml && c toolchain'
case_ "ci/" run "ci/gates.txt changed" main \
  'edit ci/gates.txt && c gates'
case_ "e2e/ (G12 reads e2e/renders.sha256)" run "e2e/run.mjs changed" main \
  'edit e2e/run.mjs && c suites'
case_ "a new top-level folder" run "webapp/x.ts changed" main \
  'edit webapp/x.ts && c new'
case_ "a name that only starts like a free one" run "README.md.orig changed" main \
  'edit README.md.orig && c orig'
case_ "a rename from core/ to docs/" run "core/a.rs changed" main \
  'git mv core/a.rs docs/a.rs && c move'
case_ "a dirty tree" run "the tree isn't clean: 1 uncommitted" main \
  'edit web/src/app.tsx && c ui && edit web/src/app.tsx'
case_ "a staged change" run "the tree isn't clean: 1 uncommitted" main \
  'edit web/src/app.tsx && c ui && edit docs/architecture.md && git add docs/architecture.md'
case_ "an untracked file" run "the tree isn't clean: 1 uncommitted or untracked" main \
  'edit web/src/app.tsx && c ui && edit web/src/new.tsx'
case_ "an empty diff: main itself" run "nothing changed since" main \
  'git checkout -q main'
case_ "a branch already merged into main" run "nothing changed since" main \
  'edit docs/a.md && c docs && git checkout -q main && git merge -q --ff-only topic && git checkout -q topic'
case_ "a bad base" run "the base no-such-rev isn't a commit" no-such-rev \
  'edit docs/a.md && c docs'
case_ "a base with no history in common" run "no merge base" other \
  'edit docs/a.md && c docs && git checkout -q --orphan other && c other && git checkout -q topic'
case_ "not a git repo" run "isn't a commit" main \
  'edit docs/a.md && c docs && rm -rf .git'
case_ "a symlink" run "HEAD holds a symlink (web/link)" main \
  'ln -s ../core/a.rs web/link && c link'
case_ "a name git quotes" run "changed since" main \
  'edit "web/a\"b.tsx" && c quoted'

echo
echo "$passed of $n cases passed"
[ $failed -eq 0 ] && [ $passed -eq $n ]
