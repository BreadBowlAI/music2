#!/bin/sh
# The web engine's probe (docs/designs/live/web-engine.md), every host, both inputs:
#   docs/designs/live/probe/run.sh <out dir>
# <out dir>/sub: noise inputs that end in subnormals; <out dir>/normal: no subnormal inputs.
# Needs node, and for Chrome M2_CHROME (a chrome-headless-shell) and M2_PUPPETEER (puppeteer-core's
# folder, as e2e/node_modules/puppeteer-core); without M2_CHROME, Chrome is left out.
set -e
here=$(cd "$(dirname "$0")" && pwd)
out=${1:?an output folder}
mkdir -p "$out/sub"
cargo run -q --locked --release -p m2-runtime --example web-probe -- "$out/sub" --libm
node "$here/libm.mjs" "$out/sub"
for v in sub normal; do
  mkdir -p "$out/$v"
  flag=; [ $v = normal ] && flag=--normal
  cargo run -q --locked --release -p m2-runtime --example web-probe -- "$out/$v" $flag
  cargo run -q --locked --release -p m2-runtime --example web-probe -- "$out/$v" --flush $flag
  node "$here/node.mjs" "$out/$v"
  if [ -n "${M2_CHROME:-}" ]; then timeout 600 node "$here/chrome.mjs" "$out/$v"; fi
  echo "== $v"
  node "$here/compare.mjs" "$out/$v"
done
