# ci/nightly-list.sh: ci/nightly-tests.txt as cargo-nextest sees it. ci/pr.sh and ci/check.sh
# source it, so the PR tier and the full check judge the list the same way. Bash 3.2.
#
# In short: nightly_filter turns the file's lines into one nextest filter (ci/pr.sh rust leaves
# those tests out with it). nightly_list checks that nextest finds exactly the tests the file
# names, so a renamed test can't slip back into the PR tier unseen. Run both in the repo's top
# folder; nightly_list needs cargo-nextest on PATH, and builds the test binaries if they aren't
# built.

# nightly_filter: ci/nightly-tests.txt as a nextest filter that matches those tests (none() when it
# names none).
nightly_filter() {
  awk '!/^#/ && NF >= 2 { printf "%s(binary_id(=%s) & test(=%s))", (n++ ? " | " : ""), $1, $2 } END { if (!n) printf "none()" }' ci/nightly-tests.txt
}

# nightly_list <dir>: whether nextest finds exactly the tests ci/nightly-tests.txt names. Writes
# <dir>/nightly-want.txt and <dir>/nightly-found.txt, prints the count or the difference, and
# returns 1 when they differ or nextest fails.
nightly_list() {
  awk '!/^#/ && NF >= 2 { print $1, $2 }' ci/nightly-tests.txt | sort > "$1/nightly-want.txt"
  cargo nextest list --locked --workspace --message-format oneline -E "$(nightly_filter)" 2> /dev/null | sort > "$1/nightly-found.txt" || return 1
  if cmp -s "$1/nightly-want.txt" "$1/nightly-found.txt"; then
    echo "all $(grep -c '' "$1/nightly-want.txt") tests ci/nightly-tests.txt names are there"
  else
    echo "ci/nightly-tests.txt names tests nextest doesn't find (<), or finds them twice (>):"
    diff "$1/nightly-want.txt" "$1/nightly-found.txt"
    return 1
  fi
}
