#!/bin/bash
# ci/npm-licenses-test.sh: tries ci/npm-licenses.mjs, the npm lockfiles' check, on made-up lockfiles.
# ci/pr.sh runs it in its lint step, so the PR tier and ci/check.sh both do.
#
# In short: a package bundled inside another (npm's "inBundle") has no address or integrity hash of
# its own, and passes only when the package it ships in has both and says it bundles. Dependabot's
# npm writes such entries for @tailwindcss/oxide-wasm32-wasi (pull request #30).
#
# Prints a PASS or FAIL line for each case, then a total. Exit status: 0 when every case passed, 1
# when not, 2 when it couldn't start. Written for bash 3.2, which macOS ships.

set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd) || exit 2
SCR=$(mktemp -d "${TMPDIR:-/tmp}/m2-npm-licenses-test.XXXXXX") || exit 2
trap 'rm -rf "$SCR"' EXIT
printf '{"name":"t","license":"MIT OR Apache-2.0"}' > "$SCR/package.json" || exit 2

REG=https://registry.npmjs.org
# A package from the registry, as npm writes one.
reg() { printf '"version":"1.0.0","resolved":"%s/%s/-/%s-1.0.0.tgz","integrity":"sha512-x","license":"MIT"' "$REG" "$1" "$1"; }
SHIPS='"node_modules/ships":{'"$(reg ships)"',"bundleDependencies":["inner"]}'
INNER='"node_modules/ships/node_modules/inner":{"version":"2.0.0","inBundle":true,"license":"MIT"}'

passed=0
failed=0
n=0
# case_ <what> <packages, as JSON members> <want exit> <want, a line it prints, or "">
case_() {
  n=$((n + 1))
  printf '{"lockfileVersion":3,"packages":{"":{"name":"t"},%s}}' "$2" > "$SCR/package-lock.json"
  out=$(node "$HERE/npm-licenses.mjs" "$SCR/package-lock.json" 2>&1)
  code=$?
  if [ "$code" = "$3" ] && { [ -z "$4" ] || printf '%s\n' "$out" | grep -qF -- "$4"; }; then
    passed=$((passed + 1))
    echo "PASS  $1"
  else
    failed=$((failed + 1))
    echo "FAIL  $1: exit $code (want $3), want a line with \"$4\"; it printed:"
    printf '%s\n' "$out" | sed 's/^/        /'
  fi
}

case_ "a package from the registry passes" \
  '"node_modules/a":{'"$(reg a)"'}' 0 "npm licences: ok"
case_ "a package with no address fails" \
  '"node_modules/a":{"version":"1.0.0","integrity":"sha512-x","license":"MIT"}' 1 "a@1.0.0 comes from nowhere recorded"
case_ "a bundled package inside one from the registry that bundles passes" \
  "$SHIPS,$INNER" 0 "npm licences: ok"
case_ "a bundled package's own bundled package passes" \
  "$SHIPS,$INNER"',"node_modules/ships/node_modules/inner/node_modules/deep":{"version":"3.0.0","inBundle":true,"license":"ISC"}' 0 "npm licences: ok"
case_ "a bundled package still needs an allowed licence" \
  "$SHIPS"',"node_modules/ships/node_modules/inner":{"version":"2.0.0","inBundle":true,"license":"GPL-3.0"}' 1 'its licence "GPL-3.0" isn'"'"'t on the allowed list'
case_ "a bundled package inside one that bundles nothing fails" \
  '"node_modules/ships":{'"$(reg ships)"'},'"$INNER" 1 "node_modules/ships, which ships it, bundles nothing"
case_ "a bundled package inside one with no integrity hash fails" \
  '"node_modules/ships":{"version":"1.0.0","resolved":"'"$REG"'/ships/-/ships-1.0.0.tgz","license":"MIT","bundleDependencies":["inner"]},'"$INNER" 1 \
  "which ships it, has no registry address and integrity hash"
case_ "a bundled package with nothing above it in the lockfile fails" \
  "$INNER" 1 "node_modules/ships isn't in the lockfile"
case_ "a bundled package at the top fails" \
  '"node_modules/inner":{"version":"2.0.0","inBundle":true,"license":"MIT"}' 1 "no package above it ships it"

echo "$passed of $n passed"
[ "$failed" = 0 ]
