#!/bin/bash
# apps/music2-desktop/bundle-m2.sh: builds the m2 that music2.app carries (FMT-23). `tauri build`
# runs it as tauri.conf.json's build.beforeBundleCommand, after it builds the app and before it
# bundles it; bundle.macOS.files then copies target/release/m2 to music2.app/Contents/MacOS/m2,
# beside the app's own program, where a new song's AGENTS.md finds it.
#
#   apps/music2-desktop/bundle-m2.sh              build m2, and on a Mac sign it
#   apps/music2-desktop/bundle-m2.sh --sign-args  only print what it would sign m2 with, one
#                                                 argument of codesign a line (nothing: unsigned)
#
# In short: m2 in release, built into this source tree's target/ whatever CARGO_TARGET_DIR says, so
# it is always where tauri.conf.json looks. Only for this machine's own kind of CPU: a --target
# build of the app is refused, as m2 would be built for the wrong one.
#
# Signing: Tauri signs its own programs, but not what bundle.macOS.files brings in, and then seals
# the bundle; so m2 is signed here first, as Tauri signs the app: the identity APPLE_SIGNING_IDENTITY
# names, else bundle.macOS.signingIdentity ("-" is ad hoc; none, no signing); with a real identity a
# secure timestamp; the hardened runtime unless bundle.macOS.hardenedRuntime is false, with
# bundle.macOS.entitlements, the app's (m2 compiles std's kernels as the app does). A certificate
# Tauri imports itself (APPLE_CERTIFICATE) is refused: Tauri puts it in a keychain of its own only
# as it bundles, after this runs. The settings are read from tauri.conf.json (M2_TAURI_CONF, for the
# tests in tests/bundle.rs), with Node, which the app's build needs anyway. Bash 3.2.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
conf=${M2_TAURI_CONF:-$here/tauri.conf.json}

# sign_args: what codesign signs m2 with, one argument a line.
sign_args() {
  if [ -n "${APPLE_CERTIFICATE:-}" ]; then
    echo "bundle-m2.sh: APPLE_CERTIFICATE is set, and Tauri imports it into a keychain of its own only as it bundles, after m2 is signed here: put the certificate in your login keychain, set APPLE_SIGNING_IDENTITY, and unset APPLE_CERTIFICATE" >&2
    return 1
  fi
  node -e '
    const path = require("path");
    const conf = process.argv[1];
    const mac = ((require(conf).bundle || {}).macOS) || {};
    const id = process.env.APPLE_SIGNING_IDENTITY || mac.signingIdentity;
    if (!id) process.exit(0);
    const args = ["--force", "--sign", id];
    if (id !== "-") args.push("--timestamp");
    if (mac.hardenedRuntime !== false) {
      args.push("--options", "runtime");
      if (mac.entitlements) args.push("--entitlements", path.resolve(path.dirname(conf), mac.entitlements));
    }
    console.log(args.join("\n"));
  ' "$conf"
}

if [ "${1:-}" = --sign-args ]; then
  sign_args
  exit
fi
[ $# -eq 0 ] || { sed -n '7,9p' "$0" >&2; exit 2; }

cd "$here/../.."
host=$(rustc -vV | sed -n 's/^host: //p')
if [ -n "${TAURI_ENV_TARGET_TRIPLE:-}" ] && [ "$TAURI_ENV_TARGET_TRIPLE" != "$host" ]; then
  echo "bundle-m2.sh: the app is built for $TAURI_ENV_TARGET_TRIPLE, and m2 only for this machine ($host): build the app without --target" >&2
  exit 1
fi
# Asked before the build, so a refusal costs nothing.
args=$(sign_args)
cargo build --locked --release -p m2 --target-dir target
if [ "$(uname -s)" = Darwin ] && [ -n "$args" ]; then
  set --
  while IFS= read -r a; do set -- "$@" "$a"; done << EOF
$args
EOF
  codesign "$@" target/release/m2
fi
echo "bundle-m2.sh: target/release/m2 is ready for music2.app/Contents/MacOS/m2"
