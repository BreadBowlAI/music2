# ci/gates-auto.sh: the rule ci/check.sh --gates=auto follows. Its verdict is one shell function,
# gates_auto, with small helpers: gates_under, gates_tree, gates_unseen, and gates_record, which
# keeps the record of passes it reads. ci/check.sh sources it, and ci/gates-auto-test.sh tries it on
# a table of cases. Bash 3.2.
#
# In short: the timing gates are skipped only when git proves that HEAD holds, in every file they
# read or build from, exactly what a check on this machine already passed them on. The change is
# every file git diff --no-renames lists between HEAD and its merge base with main. They are skipped
# only when the tree is clean (gates_unseen: nothing uncommitted or untracked, no change git hides,
# no ignored file where they read), that list isn't empty, HEAD holds no symlink, every file on the
# list is under GATES_FREE, and HEAD's gates_tree is one GATES_PASSED records. Anything else runs
# them: another file, a dirty tree, an empty list (main itself), no recorded pass on the same files,
# a git error. A Mac always runs them (ci/check.sh).
# Why the record: the merge base alone proves nothing about the gates. main fast-forwards through
# every commit of a track, so the base can be a mid-track commit whose own check failed, or a
# parallel track's commit that landed later with a fix after it. A diff only shows what changed
# since then.
#
# GATES_PASSED (ci/check.sh sets it; on Linux ~/.cache/m2-check/gates-passed): one line for each
# check whose gates row passed on a clean tree, "<gates_tree> <commit> <date>" (gates_record). It
# is this machine's own record, since timings are a machine's.
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
# Checked again for the docs plan at d2e7d64 (2026-10-25), by hand as above: docs/guide/ is read
# only by apps/m2's build script and tests, and m2-gates doesn't build the m2 package (none of its
# dependencies is m2); m2-server now embeds templates/song-agents.md, which isn't free. No change.
GATES_FREE="web/ docs/ README.md HANDOFF.md"
# GATES_BUILT: build output git ignores that the gates don't read as source: cargo's target/ (the
# gates build into it, and cargo decides what to rebuild), std's kernels' target/ (std.wasm is
# committed, and the runtime reads only std/m2pkg.toml, std/kernels/std.wasm and std/src), and
# e2e/node_modules/ (under e2e/, m2-gates reads only renders.sha256). Any other ignored file outside
# GATES_FREE runs the gates: an ignored .cargo/config.toml, or a file in core/dist/, could change
# what they build.
GATES_BUILT="target/ std/kernels-src/target/ e2e/node_modules/"

# gates_under <file> <paths>: prints the first of <paths> that <file> is, or is under (a path that
# ends in /), and returns 0. Returns 1 when there is none.
gates_under() {
  local p
  for p in $2; do
    case $p in
      */) case $1 in "$p"*) echo "$p"; return 0 ;; esac ;;
      *) if [ "$1" = "$p" ]; then echo "$p"; return 0; fi ;;
    esac
  done
  return 1
}

# gates_tree: one hash of everything in HEAD's tree the gates can read: git ls-tree -r HEAD (each
# path with its mode and blob), less the paths under GATES_FREE. Two commits with the same
# gates_tree hold the same bytes in every file the gates read or build from. Fails on a git error.
gates_tree() {
  local list t
  list=$(git ls-tree -r HEAD 2> /dev/null) && [ -n "$list" ] || return 1
  t=$(printf '%s\n' "$list" | awk -v free="$GATES_FREE" '
    BEGIN { FS = "\t"; n = split(free, p, " ") }
    { for (i = 1; i <= n; i++) if ((p[i] ~ /\/$/) ? (index($2, p[i]) == 1) : ($2 == p[i])) next; print }' |
    git hash-object --stdin 2> /dev/null) || return 1
  case $t in ''|*[!0-9a-f]*) return 1 ;; esac
  echo "$t"
}

# gates_unseen: whether the files on disk can differ from HEAD's where the gates read them. A skip
# and a record both need that ruled out. Returns 0, printing nothing, when they can't: no
# uncommitted or untracked file, no file whose changes git hides (skip-worktree, assume-unchanged),
# and no ignored file but those under GATES_FREE or GATES_BUILT (ignored includes what
# .git/info/exclude or a global excludes file hides). Otherwise it prints why, then up to 20 of the
# files, indented, and returns 1.
gates_unseen() {
  local out kind n f
  if ! out=$(git status --porcelain --untracked-files=all 2> /dev/null); then
    echo "git status failed"
    return 1
  fi
  kind=dirty
  if [ -z "$out" ]; then
    if ! out=$(git ls-files -v 2> /dev/null); then
      echo "git ls-files failed"
      return 1
    fi
    # H: an ordinary tracked file. h, S, s: assume-unchanged, skip-worktree, both.
    out=$(printf '%s\n' "$out" | grep -v '^H ')
    kind=hidden
  fi
  if [ -z "$out" ]; then
    # Every ignored file, one by one, but those under GATES_FREE and GATES_BUILT: git doesn't walk
    # the folders a pathspec leaves out, so a large target/ costs nothing. (--directory would list
    # an untracked folder that holds only ignored files as well as the files in it.)
    set -- .
    for f in $GATES_FREE $GATES_BUILT; do set -- "$@" ":(exclude)$f"; done
    if ! out=$(git ls-files --others --ignored --exclude-standard -- "$@" 2> /dev/null); then
      echo "git ls-files failed"
      return 1
    fi
    kind=ignored
  fi
  if [ -z "$out" ]; then return 0; fi
  n=$(printf '%s\n' "$out" | grep -c '')
  if [ "$n" -eq 1 ]; then f=file; else f=files; fi
  case $kind in
    dirty) echo "the tree isn't clean: $n uncommitted or untracked $f" ;;
    hidden) echo "git hides changes to $n $f (skip-worktree or assume-unchanged)" ;;
    *) echo "$n $f git ignores where the gates can read (not under $(echo "$GATES_FREE $GATES_BUILT" | sed 's/ /, /g'))" ;;
  esac
  printf '%s\n' "$out" | head -20 | sed 's/^/      /'
  return 1
}

# gates_record <tree> <commit>: after the gates passed in a check that began at <commit>, with
# gates_tree <tree> and nothing gates_unseen finds, add "<tree> <commit> <date>" to GATES_PASSED.
# It records only when that still holds now: nothing gates_unseen finds, and HEAD's gates_tree is
# still <tree>. Prints what it did, and returns 1 when it recorded nothing.
gates_record() {
  local why now
  if [ -z "${GATES_PASSED:-}" ]; then
    echo "not recorded: GATES_PASSED isn't set"
    return 1
  fi
  if ! why=$(gates_unseen); then
    echo "not recorded: $(printf '%s\n' "$why" | head -1)"
    return 1
  fi
  if ! now=$(gates_tree) || [ "$now" != "$1" ]; then
    echo "not recorded: the files the gates read aren't what they were when the check began"
    return 1
  fi
  if ! { mkdir -p "$(dirname "$GATES_PASSED")" && echo "$1 $2 $(date +%Y-%m-%d)" >> "$GATES_PASSED"; } 2> /dev/null; then
    echo "not recorded: $GATES_PASSED couldn't be written"
    return 1
  fi
  echo "recorded in $GATES_PASSED"
}

# gates_auto <rev>: whether the timing gates can be skipped for HEAD, judged against
# git merge-base HEAD <rev> and GATES_PASSED. Run it in the repo's top folder. It prints the verdict
# on the first line, "skip  <why>" or "run   <why>", then the files it judged, indented. Returns 0
# for skip, 1 for run. A name git prints in quotes (one with a quote, a backslash or a control
# character in it) never matches GATES_FREE, so it runs them.
gates_auto() {
  local rev=$1 base files why tree link f p free used= n=0 passed=
  if ! git rev-parse --verify --quiet "$rev^{commit}" > /dev/null 2>&1; then
    echo "run   the base $rev isn't a commit in this repo"
    return 1
  fi
  if ! base=$(git merge-base HEAD "$rev" 2> /dev/null) || ! base=$(git rev-parse --short=12 "$base" 2> /dev/null); then
    echo "run   git found no merge base of HEAD and $rev"
    return 1
  fi
  if ! why=$(gates_unseen); then
    printf '%s\n' "$why" | sed '1s/^/run   /'
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
    if ! free=$(gates_under "$f" "$GATES_FREE"); then
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
  # The proof that the gates pass on what HEAD holds where they read: a recorded pass on the same.
  if ! tree=$(gates_tree); then
    echo "run   git couldn't hash HEAD's tree"
    return 1
  fi
  if [ -n "${GATES_PASSED:-}" ] && [ -r "$GATES_PASSED" ]; then
    passed=$(awk -v t="$tree" '$1 == t { w = $2 " (" $3 ")" } END { if (w != "") print w }' "$GATES_PASSED" 2> /dev/null)
  fi
  if [ -z "$passed" ]; then
    echo "run   only $free changed since $base ($n $f), but the gates haven't passed on this machine on the same files they read"
    printf '%s\n' "$files" | sed 's/^/      /'
    return 1
  fi
  echo "skip  only $free changed since $base ($n $f); the gates passed at $passed on the same files they read"
  printf '%s\n' "$files" | sed 's/^/      /'
  return 0
}
