#!/bin/bash
# apps/music2-desktop/bundle-m2.sh: builds the m2 that music2.app carries (FMT-23). `tauri build`
# runs it as tauri.conf.json's build.beforeBundleCommand, after it builds the app and before it
# bundles it; bundle.macOS.files then copies target/release/m2 to music2.app/Contents/MacOS/m2,
# beside the app's own program, where a new song's AGENTS.md finds it.
#
# In short: m2 in release, built into this source tree's target/ whatever CARGO_TARGET_DIR says, so
# it is always where tauri.conf.json looks. On a Mac it is signed ad hoc, as the app is (its
# signingIdentity is "-"). Only for this machine's own kind of CPU: a --target build of the app is
# refused, as m2 would be built for the wrong one. Bash 3.2.
set -eu
cd "$(dirname "$0")/../.."
host=$(rustc -vV | sed -n 's/^host: //p')
if [ -n "${TAURI_ENV_TARGET_TRIPLE:-}" ] && [ "$TAURI_ENV_TARGET_TRIPLE" != "$host" ]; then
  echo "bundle-m2.sh: the app is built for $TAURI_ENV_TARGET_TRIPLE, and m2 only for this machine ($host): build the app without --target" >&2
  exit 1
fi
cargo build --locked --release -p m2 --target-dir target
if [ "$(uname -s)" = Darwin ]; then
  codesign --force --sign - target/release/m2
fi
echo "bundle-m2.sh: target/release/m2 is ready for music2.app/Contents/MacOS/m2"
