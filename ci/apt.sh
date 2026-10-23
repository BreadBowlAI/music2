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
#
# Found from the build itself (the -sys crates each part builds, and ldd of the binaries), on
# ubuntu-24.04 (2026-10-21). pkg-config, a C compiler, git and curl are assumed (the GitHub runner
# has them).
set -eu
case ${1:-} in
  build) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev" ;;
  suites) PKGS="libasound2-dev xvfb" ;;
  check) PKGS="libwebkit2gtk-4.1-dev libasound2-dev libdbus-1-dev xvfb libx11-dev libpng-dev" ;;
  *) sed -n '2,13p' "$0" >&2; exit 2 ;;
esac
SUDO=
if [ "$(id -u)" -ne 0 ]; then SUDO=sudo; fi
$SUDO apt-get update -q
# --no-install-recommends: only what the headers need, so the install stays quick.
$SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends $PKGS
