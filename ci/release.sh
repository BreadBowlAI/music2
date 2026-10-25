#!/bin/bash
# ci/release.sh: music2's release files, made and checked as .github/workflows/release.yml does it
# (PLAT-8). Run it on your own machine the same way.
#
#   ci/release.sh version [<tag>]  the version every manifest says (tauri.conf.json, the workspace's
#                                  Cargo.toml, web/package.json), which must be one; with a tag
#                                  (v0.1.0), it must match, and docs/releases/<version>.md (the
#                                  release notes) must be there. Prints the version
#   ci/release.sh build <out>      this computer's release files, built by the Tauri CLI (web's own,
#                                  installed by npm ci there) and put in <out> under their release
#                                  names: on Linux the .deb, the AppImage and m2's .tar.gz; on a Mac
#                                  music2.app, zipped
#   ci/release.sh check <out>      unpacks each file in <out> into a scratch folder and checks it
#                                  carries what the app needs; then runs its m2 from outside the
#                                  source tree, with a scratch HOME and no sound: --version, docs, a
#                                  new song from every template and example, each checked, one
#                                  rendered
#   ci/release.sh app <out> <pics> Linux: opens the .deb's app in <out> on an invisible screen
#                                  (Xvfb), on its start screen and then on a new song, and keeps a
#                                  picture of each in <pics>
#   ci/release.sh sums <dir>       writes <dir>/SHA256SUMS for every other file there
#
# In short: the files are named music2-<v>-macos-arm64.zip, music2_<v>_amd64.deb,
# music2-<v>-linux-x86_64.AppImage and m2-<v>-linux-x86_64.tar.gz. Each finds std, the templates
# and the examples inside itself (crates/server/src/resources.rs). The check proves it: it adds a
# template and an example of its own to the unpacked copy, which only that copy has, and m2 must
# make a song from each.
#
# Signing (Mac): ad hoc (tauri.conf.json), unless APPLE_CERTIFICATE is set (a base64 .p12, with
# APPLE_CERTIFICATE_PASSWORD): then build imports it into a keychain of its own, finds its
# "Developer ID Application" identity (or uses APPLE_SIGNING_IDENTITY), and builds with the hardened
# runtime on and apps/music2-desktop/entitlements.plist. Tauri then notarizes when APPLE_ID,
# APPLE_PASSWORD and APPLE_TEAM_ID are set too. The workflow passes these from the repo's secrets,
# on a tag only.
#
# Builds: cargo --locked, into CARGO_TARGET_DIR (default target/); m2 into target/ always
# (apps/music2-desktop/bundle-m2.sh). Scratch files in M2_RELEASE_SCRATCH (default
# ${TMPDIR:-/tmp}/m2-release), replaced at each run. Bash 3.2.
set -eu
cd "$(dirname "$0")/.."
ROOT=$PWD
SCR=${M2_RELEASE_SCRATCH:-${TMPDIR:-/tmp}/m2-release}
SCR=${SCR%/}
case $(uname -s) in
  Darwin) OS=mac ;;
  Linux) OS=linux ;;
  *) echo "release.sh: music2 is released for macOS and Linux only" >&2; exit 2 ;;
esac

usage() { sed -n '4,22p' "$0" >&2; exit 2; }
fail() { echo "release.sh: $*" >&2; exit 1; }
say() { echo "release.sh: $*"; }

# version [<tag>]: prints the one version, or fails naming the manifests that differ.
version() {
  local conf cargo web
  conf=$(node -p 'require("./apps/music2-desktop/tauri.conf.json").version')
  cargo=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
  web=$(node -p 'require("./web/package.json").version')
  if [ "$conf" != "$cargo" ] || [ "$conf" != "$web" ]; then
    fail "the versions differ: tauri.conf.json $conf, Cargo.toml $cargo, web/package.json $web"
  fi
  if [ -n "${1:-}" ]; then
    [ "$1" = "v$conf" ] || fail "the tag $1 isn't v$conf, the version the app says"
    [ -f "docs/releases/$conf.md" ] || fail "no release notes: docs/releases/$conf.md"
  fi
  echo "$conf"
}

# Paths the build writes.
target() { echo "${CARGO_TARGET_DIR:-$ROOT/target}"; }

# keychain: imports APPLE_CERTIFICATE into a keychain of its own and sets APPLE_SIGNING_IDENTITY;
# writes the Tauri config that turns the hardened runtime on to $SCR/signed.json, and the whole
# config as bundle-m2.sh reads it to $SCR/signed.conf.json.
keychain() {
  local kc pw
  [ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ] || fail "APPLE_CERTIFICATE is set, but not APPLE_CERTIFICATE_PASSWORD"
  kc=$SCR/signing.keychain-db
  pw=$(openssl rand -hex 16)
  security create-keychain -p "$pw" "$kc"
  security set-keychain-settings -lut 21600 "$kc"
  security unlock-keychain -p "$pw" "$kc"
  printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$SCR/certificate.p12"
  security import "$SCR/certificate.p12" -k "$kc" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  rm -f "$SCR/certificate.p12"
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$pw" "$kc" > /dev/null
  # Searched first, then the ones there were.
  eval "security list-keychains -d user -s \"$kc\" $(security list-keychains -d user | tr '\n' ' ')"
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning "$kc" | sed -n 's/.*"\(Developer ID Application: .*\)"$/\1/p' | head -1)
    [ -n "$APPLE_SIGNING_IDENTITY" ] || fail "the certificate holds no \"Developer ID Application\" identity"
  fi
  export APPLE_SIGNING_IDENTITY
  # bundle-m2.sh refuses a certificate Tauri would import itself: it is in a keychain now.
  unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
  node -e '
    const fs = require("fs");
    const [conf, ent, overlay, merged] = process.argv.slice(1);
    const mac = { hardenedRuntime: true, entitlements: ent };
    fs.writeFileSync(overlay, JSON.stringify({ bundle: { macOS: mac } }));
    const c = require(conf);
    Object.assign(c.bundle.macOS, mac);
    fs.writeFileSync(merged, JSON.stringify(c));
  ' "$ROOT/apps/music2-desktop/tauri.conf.json" "$ROOT/apps/music2-desktop/entitlements.plist" "$SCR/signed.json" "$SCR/signed.conf.json"
  export M2_TAURI_CONF=$SCR/signed.conf.json
  say "signing as $APPLE_SIGNING_IDENTITY, with the hardened runtime"
}

# build <out>
build() {
  local out=$1 v bundle extra stage
  v=$(version)
  mkdir -p "$out"
  out=$(cd "$out" && pwd)
  rm -rf "$SCR"
  mkdir -p "$SCR"
  [ -x web/node_modules/.bin/tauri ] || (cd web && npm ci)
  set --
  if [ $OS = mac ] && [ -n "${APPLE_CERTIFICATE:-}" ]; then
    keychain
    set -- --config "$SCR/signed.json"
  fi
  bundle=$(target)/release/bundle
  rm -rf "$bundle"
  if [ $OS = mac ]; then
    (cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --ci --bundles app ${1+"$@"} -- --locked)
    ditto -c -k --sequesterRsrc --keepParent "$bundle/macos/music2.app" "$out/music2-$v-macos-arm64.zip"
  else
    # linuxdeploy is itself an AppImage: unpacked and run, as a CI runner has no FUSE.
    (cd apps/music2-desktop && APPIMAGE_EXTRACT_AND_RUN=1 ../../web/node_modules/.bin/tauri build --ci --bundles deb,appimage ${1+"$@"} -- --locked)
    cp "$bundle/deb/music2_${v}_amd64.deb" "$out/"
    cp "$bundle/appimage/music2_${v}_amd64.AppImage" "$out/music2-$v-linux-x86_64.AppImage"
    chmod +x "$out/music2-$v-linux-x86_64.AppImage"
    # m2 on its own, for the AppImage's users: the .deb's m2 and what it finds beside it.
    extra=$SCR/deb
    dpkg-deb -x "$out/music2_${v}_amd64.deb" "$extra"
    stage=$SCR/tar/m2-$v-linux-x86_64
    mkdir -p "$stage/bin" "$stage/lib/music2"
    cp "$extra/usr/bin/m2" "$stage/bin/"
    cp -R "$extra/usr/lib/music2/std" "$extra/usr/lib/music2/templates" "$extra/usr/lib/music2/examples" "$stage/lib/music2/"
    cp LICENSE-MIT LICENSE-APACHE "$stage/"
    cat > "$stage/README.txt" << EOF
m2 $v: music2's command line, for Linux on x86_64.

Run bin/m2 from this folder, or put the folder's bin on your PATH:
  export PATH="\$PATH:$(printf '%s' "\$HOME")/m2-$v-linux-x86_64/bin"
Keep bin and lib together: m2 finds std, the song templates and the examples in lib/music2.
m2 docs prints the guide. Every new song's AGENTS.md says how to use m2 on it.
The app itself is the AppImage or the .deb beside this file on the release page.
EOF
    tar -C "$SCR/tar" -czf "$out/m2-$v-linux-x86_64.tar.gz" "m2-$v-linux-x86_64"
  fi
  say "built for $OS:"
  ls -l "$out"
}

# carries <bin> <resources> <app?>: fails unless the unpacked bundle has what the app needs.
carries() {
  local bin=$1 res=$2 app=$3 f n
  for f in "$bin/m2" ${app:+"$bin/music2"}; do
    [ -x "$f" ] || fail "no program $f"
  done
  for f in std/m2pkg.toml std/kernels/std.wasm std/src std/presets ${app:+web/index.html}; do
    [ -e "$res/$f" ] || fail "nothing at $res/$f"
  done
  n=$(ls "$ROOT/templates" | grep -cv '\.md$' || true)
  [ "$(ls "$res/templates" | grep -cv '\.md$' || true)" = "$n" ] || fail "$res/templates doesn't hold the $n templates"
  [ -f "$res/templates/song-agents.md" ] || fail "no $res/templates/song-agents.md"
  for f in "$ROOT"/examples/*/; do
    f=$(basename "$f")
    [ -f "$res/examples/$f/music2.toml" ] || fail "no example $f in $res/examples"
  done
  cmp -s "$res/std/kernels/std.wasm" "$ROOT/std/kernels/std.wasm" || fail "$res/std/kernels/std.wasm isn't the committed std.wasm"
}

# glibc <deb> <program>...: the .deb asks for the glibc its programs need, the newest version
# among their symbols, so apt refuses it on an older system rather than the program failing there.
glibc() {
  local deb=$1 need said
  shift
  need=$(objdump -T "$@" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -t. -k1,1n -k2,2n -k3,3n -u | tail -1)
  said=$(dpkg-deb -f "$deb" Depends | grep -o 'libc6 (>= [0-9.]*)' | sed 's/libc6 (>= \(.*\))/\1/')
  [ "$need" = "$said" ] || fail "$(basename "$deb") says libc6 (>= ${said:-nothing}), and its programs need glibc $need: change bundle.linux.deb.depends in tauri.conf.json"
}

# run <m2> <args>: the m2 at <m2>, from a scratch folder outside the source tree, with a scratch
# HOME, no sound and nothing else from this environment but CLAP_PATH.
run() {
  local m2=$1
  shift
  (cd "$SCR/cwd" && env -i PATH=/usr/bin:/bin HOME="$SCR/home" M2_AUDIO=none M2_MIDI=none ${CLAP_PATH:+CLAP_PATH="$CLAP_PATH"} "$m2" "$@")
}

# songs <bin> <resources> <name>: m2's checks on one unpacked bundle.
songs() {
  local m2=$1/m2 res=$2 name=$3 v id out songs
  v=$(version)
  rm -rf "$SCR/home" "$SCR/cwd"
  mkdir -p "$SCR/home" "$SCR/cwd"
  out=$(run "$m2" --version)
  [ "$out" = "m2 $v" ] || fail "$name: m2 --version said \"$out\", not \"m2 $v\""
  run "$m2" docs | grep -q "music2's guide, built into this m2 ($v)" || fail "$name: m2 docs listed no guide"
  run "$m2" docs install | grep -q '^# Install and run music2' || fail "$name: m2 docs install printed no page"
  # A template and an example only this copy has: found, so m2 reads the bundle's, not a tree's.
  cp -R "$res/templates/starter" "$res/templates/zz-release-check"
  cp -R "$res/examples/first-song" "$res/examples/zz-release-check"
  songs=$SCR/home/songs
  for id in $(ls "$res/templates"); do
    [ -d "$res/templates/$id" ] || continue
    run "$m2" new "$songs/t-$id" --template "$id" > /dev/null || fail "$name: m2 new --template $id failed"
    run "$m2" check "$songs/t-$id" > "$SCR/check.log" 2>&1 || { cat "$SCR/check.log"; fail "$name: m2 check of template $id failed"; }
  done
  for id in $(ls "$res/examples"); do
    [ -f "$res/examples/$id/song.ts" ] || continue
    if ! run "$m2" new "$songs/e-$id" --example "$id" > "$SCR/new.log" 2>&1; then
      if grep -q "which isn't installed\|which aren't installed" "$SCR/new.log"; then
        say "$name: example $id skipped: $(cat "$SCR/new.log")"
        continue
      fi
      cat "$SCR/new.log"
      fail "$name: m2 new --example $id failed"
    fi
    run "$m2" check "$songs/e-$id" > "$SCR/check.log" 2>&1 || { cat "$SCR/check.log"; fail "$name: m2 check of example $id failed"; }
  done
  rm -rf "$res/templates/zz-release-check" "$res/examples/zz-release-check"
  grep -q "^m2 for this song: \`$(cd "$1" && pwd -P)/m2\`" "$songs/t-starter/AGENTS.md" || fail "$name: a new song's AGENTS.md doesn't name this m2"
  run "$m2" render "$songs/e-first-song" "$SCR/first-song.wav" --seconds 4 > /dev/null || fail "$name: m2 render failed"
  [ "$(wc -c < "$SCR/first-song.wav")" -gt 100000 ] || fail "$name: m2 render wrote a short file"
  if [ -d "$songs/e-with-plugins" ]; then
    run "$m2" render "$songs/e-with-plugins" "$SCR/with-plugins.wav" --seconds 4 > /dev/null || fail "$name: m2 render of with-plugins (its plugins hosted by m2 run again) failed"
    say "$name: with-plugins rendered, its plugins hosted"
  fi
  say "$name: m2 $v works from it: docs, every template and example, a render"
}

# check <out>
check() {
  local out=$1 v f u checked=
  v=$(version)
  out=$(cd "$out" && pwd)
  rm -rf "$SCR"
  mkdir -p "$SCR"
  for f in "$out"/*; do
    case $f in
      */SHA256SUMS) continue ;;
      *.deb)
        u=$SCR/u-deb
        dpkg-deb -x "$f" "$u"
        say "$(basename "$f") depends on: $(dpkg-deb -f "$f" Depends)"
        glibc "$f" "$u/usr/bin/music2" "$u/usr/bin/m2"
        carries "$u/usr/bin" "$u/usr/lib/music2" app
        songs "$u/usr/bin" "$u/usr/lib/music2" "$(basename "$f")" ;;
      *.AppImage)
        u=$SCR/u-appimage
        mkdir -p "$u"
        (cd "$u" && "$f" --appimage-extract > /dev/null)
        carries "$u/squashfs-root/usr/bin" "$u/squashfs-root/usr/lib/music2" app
        songs "$u/squashfs-root/usr/bin" "$u/squashfs-root/usr/lib/music2" "$(basename "$f")" ;;
      *.tar.gz)
        u=$SCR/u-tar
        mkdir -p "$u"
        tar -C "$u" -xzf "$f"
        carries "$u/m2-$v-linux-x86_64/bin" "$u/m2-$v-linux-x86_64/lib/music2" ""
        songs "$u/m2-$v-linux-x86_64/bin" "$u/m2-$v-linux-x86_64/lib/music2" "$(basename "$f")" ;;
      *.zip)
        u=$SCR/u-zip
        ditto -x -k "$f" "$u"
        codesign --verify --deep --strict "$u/music2.app" || fail "music2.app's signature doesn't verify"
        codesign --verify --strict "$u/music2.app/Contents/MacOS/m2" || fail "m2's signature doesn't verify"
        carries "$u/music2.app/Contents/MacOS" "$u/music2.app/Contents/Resources" app
        songs "$u/music2.app/Contents/MacOS" "$u/music2.app/Contents/Resources" "$(basename "$f")" ;;
      *) fail "what is $f?" ;;
    esac
    checked="$checked${checked:+, }$(basename "$f")"
  done
  [ -n "$checked" ] || fail "nothing to check in $out"
  say "PASS: $checked"
}

# app <out> <pics>: the .deb's app on Xvfb, twice: on its start screen, then on a new song.
app() {
  local out=$1 shots=$2 v u display t
  v=$(version)
  out=$(cd "$out" && pwd)
  mkdir -p "$shots"
  shots=$(cd "$shots" && pwd)
  command -v Xvfb > /dev/null || fail "no Xvfb (apt install xvfb)"
  rm -rf "$SCR"
  mkdir -p "$SCR/home" "$SCR/cwd" "$SCR/run"
  chmod 700 "$SCR/run"
  u=$SCR/u-deb
  dpkg-deb -x "$out/music2_${v}_amd64.deb" "$u"
  cc -O2 -o "$SCR/xshot" ci/xshot.c -lX11 -lpng || fail "ci/xshot.c didn't build (X11 and libpng headers?)"
  Xvfb -displayfd 9 -screen 0 1600x1000x24 -nolisten tcp 9> "$SCR/display" > "$SCR/xvfb.log" 2>&1 &
  XVFB=$!
  trap 'kill $XVFB 2> /dev/null' EXIT
  t=0
  while [ $t -lt 100 ] && [ ! -s "$SCR/display" ]; do sleep 0.1; t=$((t + 1)); done
  display=:$(tr -dc 0-9 < "$SCR/display")
  [ "$display" != : ] || fail "Xvfb didn't start"
  open_app "$u/usr/bin/music2" "$display" start "$shots/start-screen.png"
  run "$u/usr/bin/m2" new "$SCR/home/Music/music2/Release check" --template starter > /dev/null
  open_app "$u/usr/bin/music2" "$display" song "$shots/new-song.png" "$SCR/home/Music/music2/Release check"
  # The app hosts plugins by running itself again (HOST-1): with the test plugins, when CLAP_PATH has them.
  if run "$u/usr/bin/m2" new "$SCR/home/Music/music2/With plugins" --example with-plugins > /dev/null 2>&1; then
    open_app "$u/usr/bin/music2" "$display" with-plugins "$shots/with-plugins.png" "$SCR/home/Music/music2/With plugins"
  else
    say "with-plugins: skipped, as CLAP_PATH doesn't hold its plugins"
  fi
  say "the app opened from the .deb's files on $display: pictures in $shots"
}

# open_app <music2> <display> <name> <png> [<song>]: one run until its UI connects, then a picture.
open_app() {
  local log=$SCR/app-$3.log pid t
  (cd "$SCR/cwd" && exec env -i PATH=/usr/bin:/bin HOME="$SCR/home" DISPLAY="$2" XDG_RUNTIME_DIR="$SCR/run" \
    M2_AUDIO=none M2_MIDI=none WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1 ${CLAP_PATH:+CLAP_PATH="$CLAP_PATH"} \
    "$1" ${5:+"$5"} > "$log" 2>&1 < /dev/null) &
  pid=$!
  t=0
  while [ $t -lt 300 ] && ! grep -q 'UI connected to the native shell' "$log" && kill -0 $pid 2> /dev/null; do
    sleep 0.1
    t=$((t + 1))
  done
  grep -q 'UI connected to the native shell' "$log" || { kill -9 $pid 2> /dev/null; cat "$log"; fail "$3: the UI didn't connect within 30 s"; }
  sleep 5
  DISPLAY=$2 "$SCR/xshot" "$4" || fail "no picture"
  kill $pid 2> /dev/null
  sleep 1
  kill -9 $pid 2> /dev/null || true
  wait $pid 2> /dev/null || true
  if grep -q panicked "$log"; then cat "$log"; fail "$3: the app panicked"; fi
  if grep -q "plugins can't be hosted" "$log"; then cat "$log"; fail "$3: the app couldn't host plugins"; fi
  say "$3: the UI connected in $(( (t + 9) / 10 )) s"
}

# sums <dir>
sums() {
  (cd "$1" && rm -f SHA256SUMS && for f in *; do
    [ -f "$f" ] || continue
    if command -v sha256sum > /dev/null; then sha256sum "$f"; else shasum -a 256 "$f"; fi
  done > "$SCR.sums" && mv "$SCR.sums" SHA256SUMS && cat SHA256SUMS)
}

case ${1:-} in
  version) [ $# -le 2 ] || usage; version "${2:-}" ;;
  build) [ $# -eq 2 ] || usage; build "$2" ;;
  check) [ $# -eq 2 ] || usage; check "$2" ;;
  app) [ $# -eq 3 ] && [ $OS = linux ] || usage; app "$2" "$3" ;;
  sums) [ $# -eq 2 ] || usage; sums "$2" ;;
  *) usage ;;
esac
