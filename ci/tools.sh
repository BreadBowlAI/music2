#!/bin/bash
# ci/tools.sh: the five tools ci/pr.sh needs beyond rustup and Node, each a pinned release,
# checked against its sha256 below before it is unpacked. The same versions in CI and on your
# machine, so a check that passes here passes there.
#
#   ci/tools.sh [nextest] [deny] [actionlint] [zizmor] [mdbook]   (no names: all five)
#
# In short: each tool goes into its own folder under the cache (M2_TOOLS, default
# ~/.cache/m2-tools), once; later runs find it there. The last line printed is a PATH fragment
# naming those folders, so: PATH="$(ci/tools.sh | tail -1):$PATH". Linux x86-64 and Apple silicon
# Macs; elsewhere it says what to install, and M2_TOOLS=path takes the tools already on your PATH.
# Exit 1 when a download or a checksum fails. Bash 3.2.
#
# Versions (each a release at least two weeks old when it was pinned, 2026-10-21; the sha256 is the
# digest GitHub records for the release file):
#   cargo-nextest 0.9.146  the Rust test runner that splits the tests across CI jobs
#   cargo-deny    0.20.2   licences, advisories and sources of the Rust dependencies (deny.toml)
#   actionlint    1.7.12   the workflow files in .github/workflows
#   mdbook        0.5.4    the guide's website, from docs/guide/ (ci/guide.sh; pinned 2026-10-25,
#                          released 2026-07-06)
#   zizmor        1.30.1   the workflow files' security: injection, kept credentials, too-wide
#                          permissions (ci/pr.sh lint; pinned 2026-10-26, released 2026-09-09)
set -u
NEXTEST=0.9.146
DENY=0.20.2
ACTIONLINT=1.7.12
MDBOOK=0.5.4
ZIZMOR=1.30.1
CACHE=${M2_TOOLS:-${XDG_CACHE_HOME:-$HOME/.cache}/m2-tools}

case "$(uname -s) $(uname -m)" in
  "Linux x86_64")
    NEXTEST_FILE=cargo-nextest-$NEXTEST-x86_64-unknown-linux-gnu.tar.gz
    NEXTEST_SUM=682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428
    DENY_DIR=cargo-deny-$DENY-x86_64-unknown-linux-musl
    DENY_SUM=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
    ACTIONLINT_FILE=actionlint_${ACTIONLINT}_linux_amd64.tar.gz
    ACTIONLINT_SUM=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
    MDBOOK_FILE=mdbook-v$MDBOOK-x86_64-unknown-linux-gnu.tar.gz
    MDBOOK_SUM=3f28de05dafca9d0f2eab99c662116b0e37b89b1d96a08f8f430b9eeae958cd7
    ZIZMOR_FILE=zizmor-x86_64-unknown-linux-gnu.tar.gz
    ZIZMOR_SUM=e65324f4430c2717591937edcec90ccbefaf14c174f8ec9415e03ca875b46e1a ;;
  "Darwin arm64")
    NEXTEST_FILE=cargo-nextest-$NEXTEST-universal-apple-darwin.tar.gz
    NEXTEST_SUM=39785160b3c2f6ed9a765049cf4fa79f3b39aa02eb7598a5a0e2a1a0b9ffb9a8
    DENY_DIR=cargo-deny-$DENY-aarch64-apple-darwin
    DENY_SUM=fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3
    ACTIONLINT_FILE=actionlint_${ACTIONLINT}_darwin_arm64.tar.gz
    ACTIONLINT_SUM=aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f
    MDBOOK_FILE=mdbook-v$MDBOOK-aarch64-apple-darwin.tar.gz
    MDBOOK_SUM=03e8a6d8b13a2971e0b3280affd03b388373c1485e26f73407c3a76b0b1838df
    ZIZMOR_FILE=zizmor-aarch64-apple-darwin.tar.gz
    ZIZMOR_SUM=e28d22b087f9ebb8d99da6e740d348c930f559961c7c3f12badda54f882195a2 ;;
  *)
    echo "ci/tools.sh: no pinned builds for $(uname -s) $(uname -m). Install cargo-nextest $NEXTEST, cargo-deny $DENY,"
    echo "actionlint $ACTIONLINT, zizmor $ZIZMOR and mdbook $MDBOOK yourself (cargo install --locked cargo-nextest@$NEXTEST"
    echo "cargo-deny@$DENY zizmor@$ZIZMOR mdbook@$MDBOOK; go install github.com/rhysd/actionlint/cmd/actionlint@v$ACTIONLINT),"
    echo "put them on PATH, and set M2_TOOLS=path."
    if [ "${M2_TOOLS:-}" != path ]; then exit 1; fi ;;
esac
# M2_TOOLS=path: the tools already on PATH, whatever their versions (said, not refused).
if [ "${M2_TOOLS:-}" = path ]; then
  if [ $# -eq 0 ]; then set -- nextest deny actionlint zizmor mdbook; fi
  path=
  for t in "$@"; do
    case $t in nextest) b=cargo-nextest; want=$NEXTEST ;; deny) b=cargo-deny; want=$DENY ;; actionlint) b=actionlint; want=$ACTIONLINT ;;
      zizmor) b=zizmor; want=$ZIZMOR ;; mdbook) b=mdbook; want=$MDBOOK ;;
      *) echo "ci/tools.sh: no tool $t (nextest, deny, actionlint, zizmor, mdbook)" >&2; exit 2 ;; esac
    if ! w=$(command -v $b); then echo "ci/tools.sh: M2_TOOLS=path, but $b isn't on PATH" >&2; exit 1; fi
    case $("$w" --version 2> /dev/null | head -1) in *"$want"*) ;; *) echo "ci/tools.sh: $w isn't version $want: CI uses $want" >&2 ;; esac
    path=$path${path:+:}$(dirname "$w")
  done
  echo "$path"
  exit 0
fi

sha256() { if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

# get <folder> <url> <sha256> <binary>: unpack the release at <url> into <folder> unless <binary>
# is already there. The archive is checked before anything in it is used.
get() {
  dir=$CACHE/$1
  if [ -x "$dir/$4" ]; then return 0; fi
  tmp=$CACHE/.part-$1-$$
  rm -rf "$tmp" && mkdir -p "$tmp" || return 1
  if ! curl -fsSL --retry 3 -o "$tmp/archive.tar.gz" "$2"; then
    echo "ci/tools.sh: couldn't download $2" >&2
    rm -rf "$tmp"
    return 1
  fi
  got=$(sha256 "$tmp/archive.tar.gz")
  if [ "$got" != "$3" ]; then
    echo "ci/tools.sh: $2 has sha256 $got, not $3: not used" >&2
    rm -rf "$tmp"
    return 1
  fi
  mkdir -p "$tmp/x" && tar -xzf "$tmp/archive.tar.gz" -C "$tmp/x" || { rm -rf "$tmp"; return 1; }
  # Each release keeps its binary somewhere else in the archive: take it by name.
  bin=$(find "$tmp/x" -type f -name "$(basename "$4")" | head -1)
  if [ -z "$bin" ]; then
    echo "ci/tools.sh: $2 holds no $4" >&2
    rm -rf "$tmp"
    return 1
  fi
  rm -rf "$dir" && mkdir -p "$dir" && mv "$bin" "$dir/$4" && chmod +x "$dir/$4" || { rm -rf "$tmp"; return 1; }
  rm -rf "$tmp"
}

if [ $# -eq 0 ]; then set -- nextest deny actionlint zizmor mdbook; fi
mkdir -p "$CACHE" || exit 1
path=
for t in "$@"; do
  case $t in
    nextest)
      get nextest-$NEXTEST "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$NEXTEST/$NEXTEST_FILE" \
        $NEXTEST_SUM cargo-nextest || exit 1
      path=$path${path:+:}$CACHE/nextest-$NEXTEST ;;
    deny)
      get deny-$DENY "https://github.com/EmbarkStudios/cargo-deny/releases/download/$DENY/$DENY_DIR.tar.gz" \
        $DENY_SUM cargo-deny || exit 1
      path=$path${path:+:}$CACHE/deny-$DENY ;;
    actionlint)
      get actionlint-$ACTIONLINT "https://github.com/rhysd/actionlint/releases/download/v$ACTIONLINT/$ACTIONLINT_FILE" \
        $ACTIONLINT_SUM actionlint || exit 1
      path=$path${path:+:}$CACHE/actionlint-$ACTIONLINT ;;
    zizmor)
      get zizmor-$ZIZMOR "https://github.com/zizmorcore/zizmor/releases/download/v$ZIZMOR/$ZIZMOR_FILE" \
        $ZIZMOR_SUM zizmor || exit 1
      path=$path${path:+:}$CACHE/zizmor-$ZIZMOR ;;
    mdbook)
      get mdbook-$MDBOOK "https://github.com/rust-lang/mdBook/releases/download/v$MDBOOK/$MDBOOK_FILE" \
        $MDBOOK_SUM mdbook || exit 1
      path=$path${path:+:}$CACHE/mdbook-$MDBOOK ;;
    *) echo "ci/tools.sh: no tool $t (nextest, deny, actionlint, zizmor, mdbook)" >&2; exit 2 ;;
  esac
done
echo "$path"
