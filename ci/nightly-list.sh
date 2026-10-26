# ci/nightly-list.sh: ci/nightly-tests.txt and ci/sleeps-tests.txt as cargo-nextest sees them.
# ci/pr.sh and ci/check.sh source it, so the PR tier and the full check judge the lists the same
# way. Bash 3.2.
#
# In short: nightly_filter turns ci/nightly-tests.txt's lines into one nextest filter (ci/pr.sh
# rust leaves those tests out with it); sleeps_filter does the same for ci/sleeps-tests.txt (left
# out only with M2_LATE_SLEEPS). nightly_list and sleeps_list check that nextest finds exactly the
# tests their file names, so a renamed test can't slip back in unseen. Run them in the repo's top
# folder; the lists need cargo-nextest on PATH, and build the test binaries if they aren't built.

# list_filter <file>: its tests as a nextest filter that matches them (none() when it names none).
list_filter() {
  awk '!/^#/ && NF >= 2 { printf "%s(binary_id(=%s) & test(=%s))", (n++ ? " | " : ""), $1, $2 } END { if (!n) printf "none()" }' "$1"
}

# list_check <file> <dir> <name>: whether nextest finds exactly the tests <file> names. Writes
# <dir>/<name>-want.txt and <dir>/<name>-found.txt, prints the count or the difference, and
# returns 1 when they differ or nextest fails.
list_check() {
  awk '!/^#/ && NF >= 2 { print $1, $2 }' "$1" | sort > "$2/$3-want.txt"
  cargo nextest list --locked --workspace --message-format oneline -E "$(list_filter "$1")" 2> /dev/null | sort > "$2/$3-found.txt" || return 1
  if cmp -s "$2/$3-want.txt" "$2/$3-found.txt"; then
    echo "all $(grep -c '' "$2/$3-want.txt") tests $1 names are there"
  else
    echo "$1 names tests nextest doesn't find (<), or finds them twice (>):"
    diff "$2/$3-want.txt" "$2/$3-found.txt"
    return 1
  fi
}

# nightly_filter, nightly_list <dir>: ci/nightly-tests.txt's tests (the nightly tier's).
nightly_filter() { list_filter ci/nightly-tests.txt; }
nightly_list() { list_check ci/nightly-tests.txt "$1" nightly; }

# sleeps_filter, sleeps_list <dir>: ci/sleeps-tests.txt's tests (they need sleeps on time).
sleeps_filter() { list_filter ci/sleeps-tests.txt; }
sleeps_list() { list_check ci/sleeps-tests.txt "$1" sleeps; }

# sleeps_names: ci/sleeps-tests.txt's test names, comma-separated, for a row that says what was
# left out.
sleeps_names() {
  awk '!/^#/ && NF >= 2 { printf "%s%s", (n++ ? ", " : ""), $2 }' ci/sleeps-tests.txt
}
