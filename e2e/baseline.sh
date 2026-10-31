#!/bin/bash
# Renders the pure fixtures from one commit into a folder, for byte-identity checks.
#
#   e2e/baseline.sh <commit> <out-dir>              build m2 at <commit> (a temporary worktree) and render
#   e2e/baseline.sh --tree <checkout> <out-dir>     render with the checkout as it is on disk
#   M2=<m2 binary> e2e/baseline.sh --tree <checkout> <out-dir>    ... with an m2 you already built
#
# Cases (fourteen): the full renders of examples/first-song, examples/resample, examples/sounds,
# examples/sampler, templates/starter, templates/empty and core/session/tests/fixtures/sampler-regions,
# and `--from 4 --seconds 6` of each of them. A fixture the commit doesn't have (examples/sounds before
# the sounds merge, sampler-regions before std 0.5.0) is skipped, and the script says so.
# Each fixture is rendered from a fresh copy, with a scratch HOME, M2_AUDIO=none and the checkout's
# own std and packs. <out-dir>/SHA256SUMS lists the WAVs; compare two folders with
#   diff <a>/SHA256SUMS <b>/SHA256SUMS        (or: cd <b> && shasum -a 256 -c <a>/SHA256SUMS)
# e2e/renders.sha256 is the record every OS must match (PLAT-4): cd <out-dir> && shasum -a 256 -c
# <repo>/e2e/renders.sha256 (ci/check.sh does this).
# CARGO_TARGET_DIR is honoured (default: <out-dir>/.target, removed afterwards); PROFILE=debug builds
# a debug m2 (default release). The repo is the one this script lives in unless REPO is set.
set -euo pipefail

REPO=${REPO:-$(cd "$(dirname "$0")/.." && pwd)}
usage() { sed -n '2,18p' "$0" >&2; exit 2; }
[[ $# -eq 2 || $# -eq 3 ]] || usage
if [[ $1 == --tree ]]; then
  [[ $# -eq 3 ]] || usage
  SRC=$(cd "$2" && pwd); OUT=$3; WT=
else
  [[ $# -eq 2 ]] || usage
  COMMIT=$(git -C "$REPO" rev-parse --verify "$1^{commit}"); OUT=$2
fi
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)
# m2 installs the packs music2 ships into the scratch HOME's store, read-only (PACK-6, PACK-16).
writable() { if [[ -e $1 ]]; then chmod -R u+w "$1"; fi; }
writable "$OUT/home"
rm -rf "$OUT/fx" "$OUT/home" "$OUT"/*.wav "$OUT/SHA256SUMS"
mkdir -p "$OUT/fx" "$OUT/home"

OWN_TARGET=
cleanup() {
  if [[ -n ${WT:-} ]]; then git -C "$REPO" worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; fi
  if [[ -n $OWN_TARGET ]]; then rm -rf "$OWN_TARGET"; fi
}
trap cleanup EXIT
if [[ -z ${SRC:-} ]]; then
  WT=$(mktemp -d "${TMPDIR:-/tmp}/m2-baseline-XXXXXX")
  git -C "$REPO" worktree add --detach "$WT" "$COMMIT" >/dev/null
  SRC=$WT
fi
if [[ -z ${M2:-} ]]; then
  if [[ -z ${CARGO_TARGET_DIR:-} ]]; then OWN_TARGET=$OUT/.target; export CARGO_TARGET_DIR=$OWN_TARGET; fi
  PROFILE=${PROFILE:-release}
  if [[ $PROFILE == release ]]; then (cd "$SRC" && cargo build -q --release -p m2); else (cd "$SRC" && cargo build -q -p m2); fi
  M2=$CARGO_TARGET_DIR/$PROFILE/m2
fi

HAVE=()
for f in examples/first-song examples/resample examples/sounds examples/sampler templates/starter templates/empty \
  core/session/tests/fixtures/sampler-regions; do
  if [[ ! -d $SRC/$f ]]; then echo "skipped $f: ${COMMIT:-$SRC} doesn't have it" >&2; continue; fi
  mkdir -p "$OUT/fx/$(dirname $f)"; cp -R "$SRC/$f" "$OUT/fx/$f"; HAVE+=("$f")
done
# The checkout's own std and packs (M2_PACKS: the packs it ships, PACK-15), whichever m2 renders.
packs=()
if [[ -d $SRC/packs ]]; then packs=(M2_PACKS="$SRC/packs"); fi
r() { env HOME="$OUT/home" M2_AUDIO=none M2_STD="$SRC/std" ${packs[@]+"${packs[@]}"} "$M2" render "$OUT/fx/$1" "$OUT/$2" "${@:3}" >/dev/null; }
for f in "${HAVE[@]}"; do r "$f" "${f//\//-}.wav"; done
for f in "${HAVE[@]}"; do r "$f" "${f//\//-}-from4-for6.wav" --from 4 --seconds 6; done
writable "$OUT/home"
rm -rf "$OUT/fx" "$OUT/home"
(cd "$OUT" && shasum -a 256 *.wav > SHA256SUMS && cat SHA256SUMS)
echo "rendered from ${COMMIT:-$SRC} into $OUT" >&2
