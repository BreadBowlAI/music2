#!/bin/bash
# ci/cache-left.sh <home>: fails, naming each file, when a test run left anything in the cache
# folder or the Music folder of the scratch HOME it ran with. Bash 3.2.
#
# In short: no test may write music2's cache folder (PLAT-7): only a program whose main keeps it
# does (m2_os::dirs::keep_cache), and a test that runs one gives it a HOME of its own. Nor may a
# test write ~/Music, where your songs, presets and packs are: only a program's start installs
# music2's own packs there (PACK-16), and a test gives its server a store of its own
# (m2_song::pack::test_store) and a program it runs a HOME of its own. So after the Rust tests,
# $HOME/.cache/music2 (Linux), $HOME/Library/Caches/music2 (macOS) and $HOME/Music of the HOME they
# ran with must hold no file. A file there is one a plain `cargo test` would have written in your
# own. ci/pr.sh rust and ci/check.sh run it; empty them first with --clear.
set -u
[ $# -ge 1 ] || { echo "usage: ci/cache-left.sh [--clear] <home>" >&2; exit 2; }
clear=
if [ "$1" = --clear ]; then clear=1; shift; fi
home=$1
if [ -n "$clear" ]; then
  # A pack installed there is read-only (PACK-6).
  if [ -e "$home/Music" ]; then chmod -R u+w "$home/Music"; fi
  rm -rf "$home/.cache/music2" "$home/Library/Caches/music2" "$home/Music"
  exit 0
fi
bad=0
left=$(find "$home/.cache/music2" "$home/Library/Caches/music2" -type f 2> /dev/null | sed "s|^$home/||" | sort)
if [ -n "$left" ]; then
  echo "A test wrote in the cache folder of the HOME it ran with ($home), which no test may (PLAT-7):"
  echo "$left" | head -40
  n=$(echo "$left" | wc -l | tr -d ' ')
  if [ "$n" -gt 40 ]; then echo "... and $((n - 40)) more"; fi
  echo "Give the test a folder of its own, or keep it in memory: only a program's main calls m2_os::dirs::keep_cache."
  bad=1
fi
left=$(find "$home/Music" -type f 2> /dev/null | sed "s|^$home/||" | sort)
if [ -n "$left" ]; then
  echo "A test wrote in the Music folder of the HOME it ran with ($home), which no test may (PLAT-7, PACK-16):"
  echo "$left" | head -40
  n=$(echo "$left" | wc -l | tr -d ' ')
  if [ "$n" -gt 40 ]; then echo "... and $((n - 40)) more"; fi
  echo "Give a test's server a store of its own (graph.keep_store(&m2_song::pack::test_store())), and a program it runs a HOME of its own (m2_song::pack::test_home())."
  bad=1
fi
exit $bad
