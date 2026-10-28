# Contributing to music2

**In short**
- Thank you for helping. People and AI coding agents are both welcome, under the same rules. Agents: [AGENTS.md](AGENTS.md) is the same page, shorter.
- Before you start: read the goals, the contract and the taste guide, then claim an issue.
- Before a pull request: run `ci/check.sh --pr`. It runs exactly what CI runs on a pull request.
- One issue, one small pull request. Show the test failing first. Fix causes; never weaken a test.
- What you send is under MIT OR Apache-2.0, like the rest of music2. There is no CLA.

## Before you start

| Read | Why |
|---|---|
| [docs/goals.md](docs/goals.md) | What music2 is for, and what it will not do. |
| [docs/architecture.md](docs/architecture.md) | The contract: the rules every part is built and reviewed against, each with an id such as `RENDER-8`. |
| [docs/designs/step8/taste.md](docs/designs/step8/taste.md) | The taste guide: how the app looks and feels. |
| [docs/gaps.md](docs/gaps.md) | What is missing, most visible first. The numbered gaps are good places to start. |
| [HANDOFF.md](HANDOFF.md) | Where the work stands, what comes next, and the known flaky tests. |

Then pick your work:

1. **Find an issue**, or open one ([bug](https://github.com/BreadBowlAI/music2/issues/new?template=bug.yml) or [idea](https://github.com/BreadBowlAI/music2/issues/new?template=idea.yml)). Issues for newcomers have the label `good first issue`.
2. **Claim it** before you start: say so in the issue ("I'm taking this"), and say if you are an AI agent. A maintainer assigns it to you. If someone has claimed it already, pick another one or ask there. This keeps thousands of people and agents from building the same thing twice.
3. **A big change** (a new feature, a change to the song format, the SDK or a rule of the contract) starts with a design note under [docs/designs/](docs/designs/README.md). Link it in the issue, and wait for the owner's yes before you write the code.

## Set up

You need Linux (Ubuntu 24.04 on x86_64) or a Mac with Apple silicon, [rustup](https://rustup.rs) and [Node.js](https://nodejs.org) 24. The repo pins Rust itself (`rust-toolchain.toml`).

On Linux, install the libraries that build and run the app, and the ones the full check uses to open the app on an invisible screen. (`ci/apt.sh check` installs what the full check needs, with sudo.)

```bash
sudo apt install pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev xvfb libx11-dev libpng-dev
```

Then build the UI and run the app, as in the [README](README.md#quick-start):

```bash
(cd web && npm ci && npm run build)
cargo run --locked --release -p music2-desktop
```

**The test plugins.** Some tests play two real CLAP plugins: Surge XT 1.3.4 and Airwindows Consolidated.

- On Linux, `ci/plugins.sh` downloads both into `~/.local/share/m2-test-plugins/clap`, checked against `ci/plugins.sha256`. Airwindows' file is gone upstream, so fetch the project's copy: `M2_PLUGINS_MIRROR=https://github.com/BreadBowlAI/music2/releases/download/test-plugins ci/plugins.sh`.
- On a Mac, install both in `~/Library/Audio/Plug-Ins/CLAP`.
- Without them, `ci/check.sh` and the Rust and browser parts of `ci/check.sh --pr` say FAIL, as CI would. A change that is only docs needs no plugins.
- `M2_NO_PLUGINS=1 ci/check.sh --pr` runs without them: the plugin tests skip, and the summary says `PART`, not `PASS`, because CI still runs them. Say so in your pull request.

## Before a pull request

| Command | What it runs | When |
|---|---|---|
| `ci/check.sh --pr` | Exactly the checks CI runs on a pull request, one after another, with each one's time and a total. For a change that is only docs, only the docs check, as in CI. About 10 minutes the first time on an 8-thread machine, and about 7 to 8 after a change. | Before every pull request. Paste its summary into the pull request. |
| `ci/check.sh` | The full check: every check of the pull request tier, every test, every browser suite, and on a Mac the timing gates. | Before a big change lands. The nightly run does this every day, without the timing gates. |
| `ci/pr.sh <part>` | One part of the pull request checks, as one CI job runs it: `quick`, `advisories`, `clippy`, `rust 1/3` (and `2/3`, `3/3`), `suites 1/2` (and `2/2`), `renders`. Smaller parts: `lint` (the checks that build nothing: fmt, actionlint, zizmor, licences, the checks' own cases) and `models` (the pool's models). `ci/pr.sh changes` says whether your change is docs only, and whether it touches a `Cargo.lock`. | When one CI job failed, to run just that part. |
| `ci/check.sh --nightly` | The nightly tier as CI runs it: everything but the timing gates, the browser suites one at a time. | To repeat a failed nightly run. |
| `ci/guide.sh check` | The docs check: every link in every Markdown file (`ci/links.mjs`), then the guide's website, built by mdBook into `target/guide`. `gen` writes the guide's two pages made from code again (`commands.md`, `routes.md`); `serve` previews the site at http://localhost:3000. | After changing a doc. A test fails when `gen` is owed. |
| `ci/tools.sh` | Fetches the five tools the checks use beyond Rust and Node (cargo-nextest, cargo-deny, actionlint, zizmor, mdBook), each checked against its sha256, into `~/.cache/m2-tools`. | `ci/check.sh --pr` runs it for you. |
| `node e2e/run.mjs all --tier pr --list` | Lists the browser suites a pull request runs (`--tier nightly`: the ones only the nightly run has). | To see which tier a suite is in. |

`ci/check.sh`, with or without `--pr`, ends with a summary. Its first line says `PASS` or `FAIL` (for example `music2 check --pr: PASS`), then comes one row for each part. Paste from that line down. The checks change no file in the repo, never open the app on your screen, play no sound, and never touch your `~/Music` or `~/.music2`.

### What runs where

| Tier | When | What |
|---|---|---|
| **Pull request** | every pull request and each push to `main` (and the merge queue, if one is on) | fmt, clippy, the Rust tests in 3 parts, the pool's models, the web build, std's kernels and `std.wasm`, the browser suites in 2 parts, the 12 renders, licences (cargo-deny, npm), the workflow files (actionlint, and zizmor for their security), and the checks' own cases. Security advisories (RustSec) only when a `Cargo.lock` or `deny.toml` changes. Only checks that give the same answer every time, on any day. |
| **Nightly** | every day on `main`, on GitHub's machines, once the repository is public | the full check without the timing gates: every pull request check, with the browser suites one at a time, the tests the pull request tier leaves out, and the security advisories. On macOS too: the Rust tests, the pool's models, the browser suites and the renders, less the few whose clock is a sleep (GitHub's Mac's sleeps run long: `ci/sleeps-tests.txt`, and the suites `e2e/run.mjs` tags `sleeps`). |
| **Timing gates** | every night on a quiet machine of a maintainer's | the gates that time real-time audio. A shared cloud machine can't judge them. |

## The rules

| Rule | In practice |
|---|---|
| **Tests first** | A fix or a feature comes with a test that fails before the change and passes after it. Say in the pull request that you saw it fail. |
| **Fix causes, never weaken tests** | Never loosen, skip or delete a test to make it pass. A test that judges speed, or that fails now and then on correct code, moves to the nightly tier unchanged, with the reason written beside it: a line in `ci/nightly-tests.txt` for a Rust test, `tier: "nightly"` with its `why` in `e2e/run.mjs` for a browser suite. |
| **Never your real songs or settings** | Anything that runs music2 in a test gets a scratch `HOME`, `M2_AUDIO=none` and `M2_MIDI=none`. Never the real `~/.music2` or `~/Music` ([PLAT-7](docs/architecture.md#platforms)). |
| **Locked builds** | Build with `--locked`. Never run `cargo update` in a pull request; a dependency change is a pull request of its own. |
| **Merges, not rebases** | Bring your branch up to date by merging `main` into it. Don't rewrite history others have pulled. |
| **Plain words** | Docs, messages and labels in the app use short sentences and plain words. Every doc starts with "In short". Tables beat walls of text. |
| **The contract first** | A change that breaks a rule of [the contract](docs/architecture.md) changes that page first: in the same pull request as the code, in a commit ahead of it. OS-specific code lives only in `core/os` ([PLAT-2](docs/architecture.md#platforms)). |
| **A design note first** | A big change starts with a note under `docs/designs/`, and the owner's yes. |
| **One issue, one small pull request** | Claim the issue first. Keep the pull request to that one thing. |
| **Plain commit messages** | Say what changed and why, in plain words. |

## How pull requests are reviewed and merged

1. CI runs on your pull request. Its one required check is **ci-ok**: it passes only when every other check passed. A pull request that changes only docs runs a quick docs check instead, and the rest is skipped.
2. A first-time contributor's checks start after a maintainer allows them (a GitHub setting that keeps spam from using the shared machines).
3. A maintainer reviews it. Expect questions: they are about the contract, the taste and plain words as much as the code.
4. Once approved and up to date with `main` (its checks run again on top of the newest `main`), it merges with a merge commit.
5. The expensive and timing-sensitive tests run every night on `main`. If the nightly run fails, it opens an issue called "Nightly failed". A test that passed there only on its retry is named in the issue "Flaky tests", with the number of nights it was named on.

## Building and testing

- `cargo test --workspace` runs the Rust tests. Tests that play plugins need the plugin program built first: `cargo build -p m2-plugins --bin m2-plugin-host`.
- To test the UI with no window and no sound: `M2_AUDIO=none cargo run -p m2-server --example headless -- <song-folder | -> <session.json> <token>`, then point a headless browser at the URL in the session file. `-` opens no song, for the start screen. `<token>` is the page's key, which the session file names `page_token` beside the tools key (`token`, an agent's). Outside the app, the UI reads its key from `window.__M2__ = {native: false, token}`.
- The browser suites are in `e2e/`: each drives the UI in headless Chrome against the headless server, on a fresh copy of a song ([e2e/README.md](e2e/README.md)).
- `std/build.sh` rebuilds std's kernels (it needs the `wasm32-unknown-unknown` Rust target, which `rust-toolchain.toml` adds). The built `std/kernels/std.wasm` is checked in; `std/build.sh --check` proves it rebuilds to the same bytes.
- `cargo run --release -p m2-runtime --example bench` compares WebAssembly and native speed.

## Repository layout

| Folder | What's in it |
|---|---|
| `core/ir` | The graph's compiled form, signal kinds and units, and the checker |
| `std/kernels-src/kernel` | The kit for writing kernels in Rust (`m2-kernel`), a member of the kernels' own workspace |
| `core/song` | The song folder (format 1): the manifest, `DataPath` (every project path), keys, lane and arrangement files in their canonical form, asset hashes, and the problems data can have |
| `core/runtime` | Loads packages, runs kernels (wasmtime), `poly`, `feedback`, audio clips; the renderer |
| `core/script` | Runs the TypeScript (oxc + QuickJS) and the `@m2/core` SDK |
| `core/session` | Compiling, revisions, and the write path every edit takes: checking and writing Tx (the change a Tx makes to the graph, what it may change, the journal), the code editor's saves, taking in outside changes once they settle, the project lock, and undo. It knows no gesture |
| `crates/editor` | Every gesture as a Tx: knobs and their ids, cables, effects, notes, relinks and fixes, renames, automate; the views the app shows; and the offline render that Export and `m2 render` share |
| `core/plugins` | CLAP plugin hosting, in a second program (`m2-plugin-host`) |
| `core/play` | Plays a renderer on the audio device, on real-time threads; streams audio from disk; reads the sound input |
| `core/pool` | The thread pool that renders on several cores |
| `core/os` | Everything that differs between macOS, Linux and (later) Windows (`m2-os`) |
| `std/`, `std/kernels-src` | The standard package, and its kernels' Rust source |
| `apps/m2` | The command line |
| `apps/music2-desktop` | The desktop app (Tauri), on the Mac and Linux: window, menus, dialogs |
| `crates/server` | The app's local server: `graph.rs` (the open song), `files.rs` (the code editor's files), `project.rs` (new songs and the songs folder), `export.rs` (Export), `resources.rs` (finding std, the templates and the examples), `api.rs` (the one route table) and the `routes*.rs` files (the API) |
| `web/` | The UI: `StartScreen`, `NewSongDialog`, `GraphCanvas` (the window's shell), `TopBar`, `Status`, `Browser`, `SongTimeline`, `Dock` (`NodeGraph`, `NoteView`, `CodeView`), `EffectBrowser`, `Shortcuts`; `lib/shell.ts` holds the dock's view and the selection |
| `templates/` | What new songs start from: `starter` and `empty`, each a whole song plus a `template.toml` (its name and description) |
| `examples/` | `first-song`, `resample`, `with-plugins`, `sounds` and `sampler`, each with an `example.toml` for the start screen (and the plugins it needs) |
| `e2e/` | The browser suites |
| `crates/gates` | The timing gates (`m2-gates`) that `ci/check.sh` runs |
| `ci/` | `check.sh` (the full check; `--pr` the pull request tier), `pr.sh` (one CI job's part), `nightly.sh` (the timing gates on a quiet machine), `nightly-tests.txt` (the Rust tests only the nightly tier runs), `sleeps-tests.txt` (the Rust tests GitHub's Mac leaves out, as its sleeps run long) and `nightly-list.sh` (both lists as cargo-nextest sees them), `docs-only.sh` (the docs-only rule), `guide.sh` and `links.mjs` (the docs check and the guide's website), the checks' own cases (`docs-only-test.sh`, `gates-auto-test.sh`, `pr-test.sh`, `nightly-test.sh`), `tools.sh`, `apt.sh` and `plugins.sh` (what the checks need), `gates.txt`, `sweep.sh` (a Mac's thread sweep) |
| `.github/` | The workflows (`ci.yml`, the pull request tier; `nightly.yml`; `pages.yml`, the guide's website), Dependabot's settings, and the issue and pull request forms |
| `.config/nextest.toml` | How cargo-nextest runs the Rust tests in CI |
| `deny.toml` | cargo-deny's rules for the Rust dependencies: the licences allowed, security advisories, crates.io only |
| `docs/` | `guide/`, the user guide (one page a topic; `SUMMARY.md` is its menu; also the website and `m2 docs`); then, for contributors, the goals, the contract, the design notes and the gap list |
| `templates/song-agents.md` | The `AGENTS.md` every new song gets, and the guide's "Working on a music2 song" page |

## Licence of your contribution

music2 is under MIT OR Apache-2.0, at the user's option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). What you send is under the same terms: unless you say otherwise, anything you submit for inclusion is licensed as MIT OR Apache-2.0, with no extra terms or conditions (Apache-2.0, section 5). You keep your copyright. There is no CLA to sign.

Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).
