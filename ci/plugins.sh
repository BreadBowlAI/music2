#!/bin/sh
# ci/plugins.sh (Linux): the CLAP plugins the with-plugins checks use, Surge XT 1.3.4 and
# Airwindows Consolidated, unpacked into ~/.local/share/m2-test-plugins/clap, where ci/check.sh
# looks. No sudo. Never into ~/.clap or /usr/lib/clap: fx-fixes needs them empty. The archives
# stay in dl/ beside it: Airwindows' nightly zip is deleted upstream once a newer one comes, so
# that copy may be the only one. Every file is checked against ci/plugins.sha256.
# M2_PLUGINS_MIRROR: a folder URL that holds copies of both archives, tried when upstream fails
# (CI names the repo's release "test-plugins"; Airwindows' pinned zip is gone upstream since
# 2026-10-21, and it is MIT, so a copy may be shared). The checksums decide, wherever a file came from.
# A GitHub release's folder (https://github.com/<owner>/<repo>/releases/download/<tag>) is read with
# gh when GH_TOKEN is set (CI hands it the job's read-only token), which works while that repo is
# private too; otherwise with curl, which needs the repo to be public.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SUMS=$ROOT/ci/plugins.sha256
P=${M2_TEST_PLUGINS:-$HOME/.local/share/m2-test-plugins}
SURGE=surge-xt-linux-x64-1.3.4.deb
AW=$(grep -o 'AirwindowsConsolidated-.*-Linux\.zip' "$SUMS")
mkdir -p "$P/dl" "$P/clap"
cd "$P/dl"
# mirror <file>: download it from M2_PLUGINS_MIRROR into <file>.part.
mirror() {
  case $M2_PLUGINS_MIRROR in
    https://*/releases/download/?*)
      if [ -n "${GH_TOKEN:-}" ] && command -v gh > /dev/null; then
        repo=${M2_PLUGINS_MIRROR#https://}
        gh release download "${M2_PLUGINS_MIRROR##*/releases/download/}" --repo "${repo%%/releases/download/*}" \
          --pattern "$1" --output "$1.part" --clobber && return 0
        echo "plugins: gh couldn't get $1 from that release, trying curl" >&2
      fi ;;
  esac
  curl -fsSL --retry 3 -o "$1.part" "$M2_PLUGINS_MIRROR/$1"
}
# get <file> <upstream url>: download it unless it is here, from upstream, else from the mirror.
get() {
  [ -f "$1" ] && return 0
  curl -fsSL --retry 3 -o "$1.part" "$2" ||
    { [ -n "${M2_PLUGINS_MIRROR:-}" ] && echo "plugins: $2 failed, trying the mirror" >&2 && mirror "$1"; } ||
    { rm -f "$1.part"; echo "plugins: couldn't download $1 (upstream${M2_PLUGINS_MIRROR:+ or $M2_PLUGINS_MIRROR})" >&2; exit 1; }
  mv "$1.part" "$1"
}
get "$SURGE" "https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/$SURGE"
get "$AW" "https://github.com/baconpaul/airwin2rack/releases/download/DAWPlugin/$AW"
grep -e "  $SURGE\$" -e "  $AW\$" "$SUMS" | sha256sum -c --quiet
rm -rf unpacked "$P/SurgeXTData" && mkdir unpacked
dpkg-deb -x "$SURGE" unpacked/surge
python3 -I -m zipfile -e "$AW" unpacked/aw
cp "unpacked/surge/usr/lib/clap/Surge XT.clap" "$P/clap/"
cp -R unpacked/surge/usr/share/surge-xt "$P/SurgeXTData"   # Surge finds its data one folder up
find unpacked/aw -name 'Airwindows Consolidated.clap' -exec cp -R {} "$P/clap/" \;
rm -rf unpacked
(cd "$P/clap" && grep '\.clap$' "$SUMS" | sha256sum -c --quiet)
echo "plugins: $P/clap"
