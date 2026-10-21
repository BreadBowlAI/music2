# ci/docs-only.sh: the rule that lets a pull request that changes only documentation skip the PR
# tier's builds and tests. Its verdict is one shell function, docs_only; ci/pr.sh sources it (its
# "changes" step, the first job in .github/workflows/ci.yml), and ci/docs-only-test.sh tries it on
# a table of cases. Bash 3.2.
#
# In short: the change is every file git diff --no-renames lists between HEAD and its merge base
# with the base you give. It is docs only when that list isn't empty, every file on it is under
# DOCS_FREE, and each is a plain file (or deleted) in HEAD: no symlink, no submodule. Anything else
# runs everything: another file, an empty list, a git error, a name git can only print quoted.
# --no-renames makes a move from core/ to docs/ count as a change to core/. A docs-only change still
# runs the docs job (ci/pr.sh docs), and ci-ok passes.
#
# DOCS_FREE: paths no build, test, browser suite or render reads. Checked at 381e02d (2026-10-21),
# three ways, as ci/gates-auto.sh proves its own list (the audit's section 5):
#   by hand: include_str!/include_bytes! and CARGO_MANIFEST_DIR in every .rs file, every build.rs and
#     its rerun-if-changed lines, every readFileSync/readdirSync in e2e/*.mjs, the web build's
#     tsconfig and vite config, and strings naming docs in code: none reads a path below. Tailwind
#     scans web/ only (a class written into docs/gaps.md and README.md left the CSS byte-identical).
#   from cargo's own record: the 1,128 dep-info files of a whole build name 419 files of the repo,
#     none under docs/ and none at the top level.
#   by watching: an inotify watch on docs/, the top folder and e2e/ through every PR-tier step (1 h
#     51 min of builds, tests, suites and renders) saw nothing below opened.
# Check again when a build, a test or a suite starts to read a file under one of these paths, or
# when a path is added here.
#   docs/                                    the design docs, the manual, gaps.md, pictures
#   README.md HANDOFF.md CONTRIBUTING.md AGENTS.md CLAUDE.md CODE_OF_CONDUCT.md SECURITY.md
#   GOVERNANCE.md LICENSE-MIT LICENSE-APACHE   the top-level documents (cargo reads none: every
#                                            manifest says license =, never license-file =)
#   .github/ISSUE_TEMPLATE/ .github/PULL_REQUEST_TEMPLATE.md
#                                            GitHub's forms; no workflow reads them
# Not free, on purpose: .github/workflows/ and .github/dependabot.yml (they are CI), a file named
# CODEOWNERS in any folder, docs/ too (it decides who must review, and GitHub reads docs/CODEOWNERS
# when .github/ has none), e2e/README.md and the crates' own README.md files (cargo knows a
# package's readme, and a code folder is simpler left whole), ci/, and any new top-level path.
DOCS_FREE="docs/ README.md HANDOFF.md CONTRIBUTING.md AGENTS.md CLAUDE.md CODE_OF_CONDUCT.md SECURITY.md GOVERNANCE.md LICENSE-MIT LICENSE-APACHE .github/ISSUE_TEMPLATE/ .github/PULL_REQUEST_TEMPLATE.md"

# docs_under <file>: prints the path of DOCS_FREE that <file> is, or is under (one ending in /),
# and returns 0. Returns 1 when there is none, and for any file named CODEOWNERS.
docs_under() {
  local p
  case /$1 in */CODEOWNERS) return 1 ;; esac
  for p in $DOCS_FREE; do
    case $p in
      */) case $1 in "$p"*) echo "$p"; return 0 ;; esac ;;
      *) if [ "$1" = "$p" ]; then echo "$p"; return 0; fi ;;
    esac
  done
  return 1
}

# docs_only <rev>: whether HEAD's change since git merge-base HEAD <rev> is documentation only.
# Run it in the repo's top folder. It prints the verdict on the first line, "docs  <why>" or
# "full  <why>", then the files it judged, indented. Returns 0 for docs only, 1 for everything.
docs_only() {
  local rev=$1 base files f mode n=0 used= free p
  if ! git rev-parse --verify --quiet "$rev^{commit}" > /dev/null 2>&1; then
    echo "full  the base $rev isn't a commit in this repo"
    return 1
  fi
  if ! base=$(git merge-base HEAD "$rev" 2> /dev/null) || ! base=$(git rev-parse --short=12 "$base" 2> /dev/null); then
    echo "full  git found no merge base of HEAD and $rev"
    return 1
  fi
  # core.quotePath=false prints other scripts' letters as they are; a name with a quote, a
  # backslash or a control character still comes out quoted, and then matches nothing.
  if ! files=$(git -c core.quotePath=false diff --no-renames --name-only "$base" HEAD 2> /dev/null); then
    echo "full  git diff $base HEAD failed"
    return 1
  fi
  if [ -z "$files" ]; then
    echo "full  nothing changed since $base"
    return 1
  fi
  while IFS= read -r f; do
    n=$((n + 1))
    if ! free=$(docs_under "$f"); then
      echo "full  $f changed since $base, and it isn't documentation (ci/docs-only.sh's DOCS_FREE)"
      printf '%s\n' "$files" | sed 's/^/      /'
      return 1
    fi
    # What HEAD holds there: nothing (deleted), a plain file, or something the rule can't follow.
    if ! mode=$(git ls-tree HEAD -- "$f" 2> /dev/null); then
      echo "full  git ls-tree HEAD failed"
      return 1
    fi
    case ${mode%% *} in
      ''|100644|100755) ;;
      120000) echo "full  $f is a symlink, and the rule can't tell what one points to"; return 1 ;;
      *) echo "full  $f isn't a plain file (git mode ${mode%% *})"; return 1 ;;
    esac
    case " $used " in *" $free "*) ;; *) used="$used${used:+ }$free" ;; esac
  done << EOF
$files
EOF
  free=
  for p in $DOCS_FREE; do
    case " $used " in *" $p "*) free="$free${free:+, }$p" ;; esac
  done
  if [ $n -eq 1 ]; then f=file; else f=files; fi
  echo "docs  only $free changed since $base ($n $f)"
  printf '%s\n' "$files" | sed 's/^/      /'
  return 0
}
