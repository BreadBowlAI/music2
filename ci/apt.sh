#!/bin/sh
# ci/apt.sh (Ubuntu or Debian): the system packages music2's builds and checks need, installed with
# apt-get. CI's jobs run it; on your own machine it shows what to install (it runs sudo).
#
#   ci/apt.sh build    the whole workspace (the desktop app, clippy, the Rust tests): WebKitGTK 4.1
#                      (it brings GTK 3, libsoup 3 and JavaScriptCore), ALSA and D-Bus headers
#   ci/apt.sh suites   the headless server, m2 and the plugin host, for the browser suites and the
#                      renders: ALSA's headers, and Xvfb for ph-window's invisible screen
#   ci/apt.sh check    everything ci/check.sh uses: build's, plus Xvfb and the X11 and libpng headers
#                      for the app row's invisible screen and its screenshots (ci/xshot.c)
#   ci/apt.sh release  what ci/release.sh needs on Linux: check's, plus file (the AppImage's
#                      appimagetool needs it) and binutils (objdump reads the glibc the programs need)
#   ci/apt.sh key <set>  (CI) apt-get update, then the cache key for that set's .debs: key=... and
#                      restore=... (the key's prefix) into $GITHUB_OUTPUT, and M2_APT_DEBS and
#                      M2_APT_SUMS into $GITHUB_ENV, for the steps after it
#
# Found from the build itself (the -sys crates each part builds, and ldd of the binaries), on
# ubuntu-24.04 (2026-10-21). pkg-config, a C compiler, git and curl are assumed (the GitHub runner
# has them).
#
# M2_APT_DEBS=<folder> (absolute): keep the .debs there, and install from it what is there
# already (CI's jobs cache it, so a slow Ubuntu mirror costs one download, not one a job). After
# apt-get update, apt names each .deb it would fetch, with its sha256 from the signed index
# (--print-uris, asked with an empty folder so every one is named): the list "key" keeps in
# M2_APT_SUMS, so the install step needs no second update. The key is the set, the runner's OS and
# that list's checksum: when Ubuntu updates a package, the key is new, the cache step restores the
# newest older entry (restore=) and saves the new set once. Nothing in the folder is trusted: each
# .deb in it that isn't one of the list's, byte for byte, is deleted first. An old version
# (another file name), a half or a changed file (another sha256) never reaches dpkg: apt
# downloads it again, and checks that download as always. (apt alone would not do: it takes a
# .deb already in its folder when the size matches, without its hash.) A cache miss is an empty
# folder: everything is downloaded, as without M2_APT_DEBS. CI's folder is /var/cache/m2-apt,
# outside the runner's home, so apt still downloads as its own unprivileged user, _apt.
set -eu
KEY=
if [ "${1:-}" = key ]; then KEY=1; shift; fi
case ${1:-} in
  build) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev" ;;
  suites) PKGS="libasound2-dev xvfb" ;;
  check) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev xvfb libx11-dev libpng-dev" ;;
  release) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev xvfb libx11-dev libpng-dev file binutils" ;;
  *) sed -n '2,19p' "$0" >&2; exit 2 ;;
esac
SUDO=
if [ "$(id -u)" -ne 0 ]; then SUDO=sudo; fi
# sums <file>: each .deb apt would fetch for $PKGS, as "<sha256>  <file name>". apt prints
# '<url>' <file> <size> SHA256:<hash>.
sums() {
  empty=$(mktemp -d)
  mkdir -p "$empty/partial"
  apt-get install -y -q --no-install-recommends --print-uris -o Acquire::ForceHash=SHA256 \
    -o Dir::Cache::Archives="$empty" $PKGS |
    sed -n "s/^'[^']*' \([^ ]*\) [0-9]* SHA256:\([0-9a-f]*\)\$/\2  \1/p" > "$1"
  rm -rf "$empty"
}
if [ -n "$KEY" ]; then
  . /etc/os-release
  debs=/var/cache/m2-apt
  list=${RUNNER_TEMP:-/tmp}/m2-apt-$1.sums
  # The folder is this user's, so the cache step can restore into it and save it.
  $SUDO mkdir -p "$debs" && $SUDO chown "$(id -u):$(id -g)" "$debs"
  $SUDO apt-get update -q
  sums "$list"
  prefix="apt-$1-$(dpkg --print-architecture)-${ImageOS:-$ID$VERSION_ID}-"
  key="$prefix$(cksum < "$list" | cut -d' ' -f1)"
  echo "$key ($(grep -c . "$list" || true) .debs to fetch)"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then printf 'key=%s\nrestore=%s\n' "$key" "$prefix" >> "$GITHUB_OUTPUT"; fi
  if [ -n "${GITHUB_ENV:-}" ]; then printf 'M2_APT_DEBS=%s\nM2_APT_SUMS=%s\n' "$debs" "$list" >> "$GITHUB_ENV"; fi
  exit 0
fi
# --no-install-recommends: only what the headers need, so the install stays quick.
if [ -z "${M2_APT_DEBS:-}" ]; then
  $SUDO apt-get update -q
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $PKGS
  exit 0
fi
debs=$M2_APT_DEBS
# apt reads a relative Dir::Cache::Archives under /var/cache/apt, so this script would check one
# folder and apt install from another.
case $debs in /*) ;; *) echo "ci/apt.sh: M2_APT_DEBS must be an absolute path, not $debs" >&2; exit 2 ;; esac
list=$(mktemp)
trap 'rm -f "$list"' EXIT
if [ -n "${M2_APT_SUMS:-}" ] && [ -f "$M2_APT_SUMS" ]; then
  cp "$M2_APT_SUMS" "$list"
else
  $SUDO apt-get update -q
  sums "$list"
fi
$SUDO mkdir -p "$debs/partial"
# _apt downloads into partial/ (apt's own sandbox), when it can reach the folder.
if id _apt > /dev/null 2>&1; then $SUDO chown _apt "$debs/partial"; fi
kept=0
for f in "$debs"/*.deb; do
  [ -e "$f" ] || continue
  sum=$(awk -v n="$(basename "$f")" '$2 == n { print $1 }' "$list")
  if [ -n "$sum" ] && [ "$(sha256sum "$f" | cut -d' ' -f1)" = "$sum" ]; then kept=$((kept + 1)); else $SUDO rm -f "$f"; fi
done
echo "ci/apt.sh: $kept of the $(grep -c . "$list" || true) .debs to install came from $debs"
$SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
  -o Dir::Cache::Archives="$debs" $PKGS
# The cache step saves the folder as this user: the files apt made as root become this user's,
# and its lock and half downloads go.
$SUDO rm -rf "$debs/partial" "$debs/lock"
$SUDO chown -R "$(id -u):$(id -g)" "$debs"
