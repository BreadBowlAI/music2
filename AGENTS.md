# AGENTS.md

**In short:** rules and commands for AI coding agents working on music2. Humans follow the same rules: [CONTRIBUTING.md](CONTRIBUTING.md) is the long version.

## What music2 is

A desktop DAW where a song is a TypeScript program: a Rust server and engine, std's sound kernels in WebAssembly, a React UI in a Tauri shell, CLAP plugins in a separate host process. The app has no AI features; agents work on songs from the outside, through the files and the local API (`m2 routes` lists it, `m2 call` sends one route), and measure what they can't hear with `m2 listen`.

## Read before you change anything

| Page | What it decides |
|---|---|
| [docs/architecture.md](docs/architecture.md) | The contract. Rules have ids (`RENDER-8`); tests name the id they check. A change that breaks a rule edits this page first: in the same pull request as the code, in a commit ahead of it. Where any other doc disagrees, the contract wins. |
| [docs/goals.md](docs/goals.md) | Goals and non-goals: no AI in the app, CLAP only, no backwards compatibility before 1.0. |
| [docs/designs/step8/taste.md](docs/designs/step8/taste.md) | How the UI looks and behaves. |
| [docs/gaps.md](docs/gaps.md) | What is missing. Numbered gaps keep their numbers everywhere. |
| [HANDOFF.md](HANDOFF.md) | Current state, next steps, known flaky tests. |

## Commands

| Do | Command |
|---|---|
| Check before a pull request (the PR tier, as CI runs it) | `ci/check.sh --pr` |
| The full check (every PR-tier check, every test; the timing gates on a Mac) | `ci/check.sh` |
| One CI job's part again | `ci/pr.sh quick`, `advisories`, `clippy`, `rust 1/3` (`2/3`, `3/3`), `suites 1/2` (`2/2`), `renders`; smaller: `lint` (what builds nothing), `models` (the pool's models); `ci/pr.sh changes` says if a change is docs only, and if it touches a `Cargo.lock` |
| The checks' tools (nextest, cargo-deny, actionlint, mdBook) | `ci/tools.sh` |
| Which browser suites a pull request runs | `node e2e/run.mjs all --tier pr --list` |
| Build the UI | `(cd web && npm ci && npm run build)` |
| Run the app | `cargo run --locked --release -p music2-desktop` |
| Rust tests | `cargo test --locked --workspace` (build `m2-plugin-host` first: `cargo build --locked -p m2-plugins --bin m2-plugin-host`) |
| Browser suites | see [e2e/README.md](e2e/README.md) |
| Rebuild std's kernels | `std/build.sh` (`std/build.sh --check` proves the committed `std.wasm`) |
| Test plugins (Linux) | `ci/plugins.sh` |

## Rules

1. **Claim first.** Work on one issue that is assigned to you, or that you claimed in a comment saying you are an agent. One issue, one small pull request.
2. **Test first.** Write the test, watch it fail, then fix. Say so in the pull request.
3. **Never weaken a test.** No loosened bound, no skip, no delete. A test that judges speed or is a known flake moves to the nightly tier unchanged, with its reason: `ci/nightly-tests.txt` (Rust) or `tier: "nightly"` and `why` in `e2e/run.mjs` (browser suites).
4. **Fix the cause**, not the symptom. A retry or a longer sleep is not a fix.
5. **Scratch `HOME` for anything that runs music2**, with `M2_AUDIO=none` and `M2_MIDI=none`. Never read or write the real `~/.music2` or `~/Music`. If you change `HOME`, set `CARGO_HOME` and `RUSTUP_HOME` to the real ones.
6. **Never open the desktop app on a real screen.** Use Xvfb (an invisible screen), as `ci/check.sh` does.
7. **`--locked` builds. Never `cargo update`.** A dependency change is its own pull request.
8. **Merges, not rebases.** Merge `main` into your branch; never force-push shared history.
9. **OS-specific Rust code only in `core/os`** (PLAT-2; `core/os/tests/one_place.rs` checks it).
10. **Plain words** in docs, UI text and commit messages. Every doc starts with "In short".
11. **A big change starts with a design note** under `docs/designs/` and the owner's yes.
12. **Keep `ci/check.sh` working in bash 3.2** (macOS's bash).
13. **Don't edit `std/kernels/std.wasm` by hand** or take a side on it in a merge: rebuild it with `std/build.sh`.

## Pull requests

Use the template. Paste the summary of `ci/check.sh --pr` (from `music2 check --pr:` down). Only `PASS` is a pass; `PART` with `M2_NO_PLUGINS` means the plugin tests didn't run here (say so). Say which agent wrote the change. The one required check is `ci-ok`; a pull request merges with a merge commit, once it is up to date with `main`.

What you send is under MIT OR Apache-2.0 (Apache-2.0, section 5). No CLA.
