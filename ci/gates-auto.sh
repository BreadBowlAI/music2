# ci/gates-auto.sh: the rule ci/check.sh --gates=auto follows, as one shell function: gates_auto.
# ci/check.sh sources it, and ci/gates-auto-test.sh tries it on a table of cases. Bash 3.2.
#
# In short: the timing gates are skipped only when git proves that nothing they read or build from
# changed. The change is every file git diff --no-renames lists between HEAD and its merge base with
# main. They are skipped only when the tree is clean, that list isn't empty, HEAD holds no symlink,
# and every file on the list is under GATES_FREE. Anything else runs them: another file, a dirty
# tree, an untracked file, an empty list (main itself), a git error. A Mac always runs them
# (ci/check.sh). The rule trusts that the gates passed at the merge base. That holds because each
# track merges main in and runs its check with the gates before it lands.
#
# GATES_FREE: the paths m2-gates never reads or builds from. Checked at 69c3283 (2026-10-20), by
# hand and by watching it (below). Check again when m2-gates, its songs, or what it builds starts
# reading a new file:
#   web/       m2-gates never starts the HTTP server, so it never serves web/dist. It drives
#              m2_server's Graph, AppState and export directly. No crate it builds reaches web/:
#              none of them has a build script (the only ones are crates/gates' and the desktop
#              app's, and m2-gates doesn't depend on the app), and no include_str!/include_bytes!
#              names a path there.
#   docs/, README.md, HANDOFF.md
#              Named only in comments and messages. Nothing m2-gates builds or runs opens them,
#              nor does the S build (crates/gates/s/build.sh: git archive of 8aa301e,
#              rust-toolchain.toml and crates/gates/s).
#   not e2e/   m2-gates reads e2e/renders.sha256 (G12's offline sweep, crates/gates/src/realtime.rs).
#              ci/gates.txt doesn't run that sweep today, but one line added there would.
#   Not anything else either: the code and its Cargo.toml files, Cargo.lock, rust-toolchain.toml, std/,
#              examples/ and templates/ (the gates' songs and fixtures), crates/gates (the S build,
#              its records), ci/ (gates.txt, check.sh, this file), and any new top-level path.
# How it was checked. By hand:
#   grep -rnE 'include_(str|bytes)!|CARGO_MANIFEST_DIR' core crates/server/src crates/editor crates/gates
#   grep -rnE '"[^"]*(docs/|web/|e2e|README|HANDOFF|\.md)[^"]*"' core crates/server crates/editor crates/gates
#   find . -name build.rs -not -path ./target/\*; git ls-tree -r HEAD | grep -c '^120000'
# By watching it: the release build's dep-info files (target/release/deps/*.d) name 227 files of the
# repo, all under core/, crates/ and examples/first-song; cargo itself reads Cargo.toml, Cargo.lock,
# rust-toolchain.toml and each package's own README; and under strace (-f, every file call) the S
# build and the 7 runs of ci/gates.txt touched the repo only in crates/gates, crates/server,
# examples/with-plugins, std/, target/ and rust-toolchain.toml: nothing under web/, docs/, e2e/,
# README.md or HANDOFF.md.
GATES_FREE="web/ docs/ README.md HANDOFF.md"

# gates_auto <rev>: whether the timing gates can be skipped for HEAD, judged against
# git merge-base HEAD <rev>. Run it in the repo's top folder. It prints the verdict on the first
# line, "skip  <why>" or "run   <why>", then the files it judged, indented. Returns 0 for skip, 1 for
# run. A name git prints in quotes (one with a quote, a backslash or a control character in it)
# never matches GATES_FREE, so it runs them.
gates_auto() {
  local rev=$1 base files dirty tree link f p free used= n=0
  if ! git rev-parse --verify --quiet "$rev^{commit}" > /dev/null 2>&1; then
    echo "run   the base $rev isn't a commit in this repo"
    return 1
  fi
  if ! base=$(git merge-base HEAD "$rev" 2> /dev/null) || ! base=$(git rev-parse --short=12 "$base" 2> /dev/null); then
    echo "run   git found no merge base of HEAD and $rev"
    return 1
  fi
  if ! dirty=$(git status --porcelain --untracked-files=all 2> /dev/null); then
    echo "run   git status failed"
    return 1
  fi
  if [ -n "$dirty" ]; then
    n=$(printf '%s\n' "$dirty" | grep -c '')
    if [ "$n" -eq 1 ]; then f=file; else f=files; fi
    echo "run   the tree isn't clean: $n uncommitted or untracked $f"
    printf '%s\n' "$dirty" | sed 's/^/      /'
    return 1
  fi
  if ! files=$(git -c core.quotePath=false diff --no-renames --name-only "$base" HEAD 2> /dev/null); then
    echo "run   git diff $base HEAD failed"
    return 1
  fi
  if [ -z "$files" ]; then
    echo "run   nothing changed since $base: this is main itself, or a commit already in it"
    return 1
  fi
  if ! tree=$(git ls-tree -r HEAD 2> /dev/null); then
    echo "run   git ls-tree HEAD failed"
    return 1
  fi
  link=$(printf '%s\n' "$tree" | awk 'BEGIN { FS = "\t" } $1 ~ /^120000 / { print $2; exit }')
  if [ -n "$link" ]; then
    echo "run   HEAD holds a symlink ($link), and the rule can't tell what one points to"
    return 1
  fi
  while IFS= read -r f; do
    n=$((n + 1))
    free=
    for p in $GATES_FREE; do
      case $p in
        */) case $f in "$p"?*) free=$p ;; esac ;;
        *) if [ "$f" = "$p" ]; then free=$p; fi ;;
      esac
      if [ -n "$free" ]; then break; fi
    done
    if [ -z "$free" ]; then
      echo "run   $f changed since $base, and it isn't under $(echo "$GATES_FREE" | sed 's/ /, /g')"
      printf '%s\n' "$files" | sed 's/^/      /'
      return 1
    fi
    case " $used " in *" $free "*) ;; *) used="$used${used:+ }$free" ;; esac
  done << EOF
$files
EOF
  # The free paths it touched, in GATES_FREE's order.
  free=
  for p in $GATES_FREE; do
    case " $used " in *" $p "*) free="$free${free:+, }$p" ;; esac
  done
  if [ $n -eq 1 ]; then f=file; else f=files; fi
  echo "skip  only $free changed since $base ($n $f)"
  printf '%s\n' "$files" | sed 's/^/      /'
  return 0
}
