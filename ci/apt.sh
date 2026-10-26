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
#   ci/apt.sh key <set>  (CI) the cache key for that set's .debs, as key=... into $GITHUB_OUTPUT,
#                      and M2_APT_DEBS=~/.cache/m2-apt into $GITHUB_ENV, for the steps after it
#
# Found from the build itself (the -sys crates each part builds, and ldd of the binaries), on
# ubuntu-24.04 (2026-10-21). pkg-config, a C compiler, git and curl are assumed (the GitHub runner
# has them).
#
# M2_APT_DEBS=<folder>: keep the .debs there, and install from it what is there already (CI's
# jobs cache it, so a slow Ubuntu mirror costs one download, not one a job). The key names the
# set's packages and the runner's image (ImageOS, ImageVersion), so a new image starts afresh.
# Nothing in the folder is trusted: after apt-get update, apt names each .deb it would fetch,
# with its sha256 from the signed index (--print-uris, asked with an empty folder so every one is
# named), and each .deb in the folder that isn't one of those, byte for byte, is deleted first. An
# old version (another file name), a half or a changed file (another sha256) never reaches dpkg:
# apt downloads it again, and checks that download as always. (apt alone would not do: it takes a
# .deb already in its folder when the size matches, without its hash.) A cache miss is an empty
# folder: everything is downloaded, as without M2_APT_DEBS.
set -eu
KEY=
if [ "${1:-}" = key ]; then KEY=1; shift; fi
case ${1:-} in
  build) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev" ;;
  suites) PKGS="libasound2-dev xvfb" ;;
  check) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev xvfb libx11-dev libpng-dev" ;;
  release) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev xvfb libx11-dev libpng-dev file binutils" ;;
  *) sed -n '2,18p' "$0" >&2; exit 2 ;;
esac
if [ -n "$KEY" ]; then
  . /etc/os-release
  key="apt-$1-$(dpkg --print-architecture)-${ImageOS:-$ID}-${ImageVersion:-$VERSION_ID}-$(printf '%s' "$PKGS" | cksum | cut -d' ' -f1)"
  echo "$key"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then echo "key=$key" >> "$GITHUB_OUTPUT"; fi
  if [ -n "${GITHUB_ENV:-}" ]; then echo "M2_APT_DEBS=$HOME/.cache/m2-apt" >> "$GITHUB_ENV"; fi
  exit 0
fi
SUDO=
if [ "$(id -u)" -ne 0 ]; then SUDO=sudo; fi
$SUDO apt-get update -q
# --no-install-recommends: only what the headers need, so the install stays quick.
if [ -z "${M2_APT_DEBS:-}" ]; then
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $PKGS
  exit 0
fi
debs=$M2_APT_DEBS
want=$(mktemp -d)
trap 'rm -rf "$want"' EXIT
mkdir -p "$debs/partial" "$want/partial"
# Each line: '<url>' <file> <size> SHA256:<hash>. Read as "<hash>  <file>".
apt-get install -y -q --no-install-recommends --print-uris -o Acquire::ForceHash=SHA256 \
  -o Dir::Cache::Archives="$want" $PKGS |
  sed -n "s/^'[^']*' \([^ ]*\) [0-9]* SHA256:\([0-9a-f]*\)\$/\2  \1/p" > "$want/sums"
kept=0
for f in "$debs"/*.deb; do
  [ -e "$f" ] || continue
  sum=$(awk -v n="$(basename "$f")" '$2 == n { print $1 }' "$want/sums")
  if [ -n "$sum" ] && [ "$(sha256sum "$f" | cut -d' ' -f1)" = "$sum" ]; then kept=$((kept + 1)); else $SUDO rm -f "$f"; fi
done
echo "ci/apt.sh: $kept of the $(grep -c . "$want/sums" || true) .debs to install came from $debs"
# APT::Sandbox::User=root: apt's downloader user, _apt, can't write a folder in this user's home.
$SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
  -o Dir::Cache::Archives="$debs" -o APT::Sandbox::User=root $PKGS
# The cache step saves the folder as this user: the files apt made as root become this user's,
# and its lock and half downloads go.
$SUDO rm -rf "$debs/partial" "$debs/lock"
$SUDO chown -R "$(id -u):$(id -g)" "$debs"
