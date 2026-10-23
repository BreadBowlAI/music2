# Ground facts: checking the Mac on GitHub Actions, and plugins on Linux

Written 2026-10-14 on the Linux server for the platform layer (task label `ground-ci`). Facts come from GitHub's
docs, the projects' own source, and checks on this server. Sources are numbered [S1]… at the end. Anything
not checked here or in a source is marked *(unverified)*. Nothing was downloaded or installed.

## In short

| Question | Answer |
|---|---|
| Which runner? | **`macos-26`**: an Apple M1 run as a virtual machine, 3 cores, 7 GB memory, 14 GB promised disk, macOS 26.6.2 (the owner's Mac runs 26.6.2 too). Pin it; `macos-latest` moves. |
| Price? | **$0.062 a minute** on a private repo, rounded up per job. Free minutes are spent at list price: the Free plan's 2,000 minutes are worth $12, about **190 Mac minutes a month**. |
| One full run? | **About 40 minutes, $2.50, with no cache; about 28 minutes, $1.75, with a warm one** (estimates from runs on 3 of this server's CPUs). A step with three runs: about 96 minutes, $6. The Free plan's minutes cover about two such steps a month. |
| How does the lead start a run? | **With git only, no token:** `git push -f origin <commit>:refs/heads/check/mac`. The owner can also press "Run workflow" on GitHub. |
| How does the lead read the result? | **With `gh`, which needs a login once.** The repo is private, so reading runs and logs needs a token. Ubuntu's own `gh` (2.45) is broken; install GitHub's package. |
| What does the job prove? | Build, every Rust test (the AppKit test too), clippy `-D warnings`, the web build, all 29 browser suites, and the 8 offline renders against recorded hashes. **Timing gates stay on a real Mac.** |
| Can the AppKit test run there? | **Probably yes.** The runner image logs its user in to a GUI session (autologin), so AppKit has a window server. Confirm on the first run. |
| Will it fit? | **Yes, with two settings.** Our debug `target/` is 25 GB in the main checkout today. With no debug info and no incremental files it measured 2.5 to 4.5 GB here. GitHub promises 14 GB free. Peak build memory was 2.2 GB; the runner has 7 GB. |
| Does the cache save much? | **Less than it seems.** GitHub deletes a cache nobody used for 7 days. A run once per step is often cold. The follow-up run after a fix is warm (about 12 minutes shorter). |
| Plugins on the runner? | **Yes, unpacked into a private folder, never installed.** m2 always searches `/Library/Audio/Plug-Ins/CLAP`, and `fx-fixes` needs the plugins to be missing. So the job unpacks the two packages and names the folder only in `CLAP_PATH`. |
| Plugins on Linux? | Surge XT 1.3.4 ships a `.deb`; Airwindows Consolidated ships only a nightly zip whose name changes. **Both install per user without sudo:** unpack into `$SCRATCH/plugins/` and set `CLAP_PATH`. Never `sudo dpkg -i` (same `fx-fixes` reason). Up to three small X libraries may be missing here; `ldd` will tell. |
| Renders in CI? | **The debug `m2` is enough.** It gave the release `m2`'s bits for all 8 renders here, and it saves a 7–10 minute release build on the runner. |
| A GitHub Linux job too? | **No, not now.** This server is already Ubuntu 24.04 on x86_64, and much faster. |
| Found on the way | `std/kernels/std.wasm` contains the owner's Mac path (`<repo>/core/kernel/src/lib.rs`). A rebuild anywhere else gives other bytes. Remap the path before any "std.wasm reproduces" check. |

---

## 1. The macOS runners

| Fact | Value | Source |
|---|---|---|
| Apple-silicon labels | `macos-15`, `macos-26` (= `macos-latest` today), `macos-14` (deprecated, ends 2 Nov), `xcode-27` (preview) | [S1] [S2] [S3] |
| Intel labels | `macos-15-intel`, `macos-26-intel` | [S1] |
| Machine | 3 cores "(M1)", 7 GB memory, 14 GB SSD; the same for public and private repos | [S1] |
| A virtual machine? | Yes. GitHub: nested virtualization isn't supported "due to the limitation of Apple's Virtualization Framework". So each runner is a VM on an M1 host. Which M1 cores the 3 vCPUs get isn't documented *(unverified)*. | [S1] |
| `macos-26` image | macOS 26.6.2 (25G83), image 20260907.0351.1, Xcode 26.6 | [S3] |
| Already on the image | Rust 1.98.1, rustup 1.29.0, Node 24.20.0 (22 cached), gh 2.100.0, jq, zstd, Chrome 152. `bash` is Apple's 3.2: no bash 4 features in scripts. | [S3] |
| Admin rights | passwordless `sudo` | [S1] |
| GUI session | The image turns on automatic GUI login for the runner user (`configure-autologin.sh`) and closes open windows at build time (`configure-windows.sh`). | [S4] |
| Price (private repo) | macOS 3- or 4-core: **$0.062/min**. Linux 2-core: $0.006. Rounded up to a whole minute per job. | [S5] |
| Free minutes a month | Free org 2,000; Team 3,000; Enterprise Cloud 50,000. Spent at list price, so a Mac minute uses about 10 Linux minutes. | [S6] [S7] |
| Faster Mac | `macos-26-xlarge`: M2 Pro, 5 cores, 14 GB, $0.102/min. Never uses free minutes; needs a card on file. | [S5] [S8] |
| Time limits | 6 hours per job; 35 days per run | [S9] |
| Mac jobs at once | 5 on Free and Team | [S9] |
| Cache | 10 GB per repo. Entries unused for 7 days are deleted. A run sees its own branch's caches and the default branch's. | [S10] |
| Artifact storage | 500 MB (Free), 2 GB (Team) | [S6] |

Notes:
- The macOS price fell from $0.08 to $0.062 on 1 January 2026 [S11] [S12].
- Today's billing docs no longer print a "10x" multiplier. GitHub's 2026 FAQ says included usage is consumed
  "based on list price", the same way for Linux, Windows and macOS standard runners [S7]. So $12 ÷ $0.062 ≈ 193
  Mac minutes on Free, and $18 ÷ $0.062 ≈ 290 on Team.
- Which plan BreadBowlAI is on, and whether it has a payment method, wasn't checked *(unverified)*. Free
  minutes are shared by every Actions job in the organization. Without a payment method, jobs stop once the
  free minutes are used up [S6].

---

## 2. What the job installs

| Need | How | Notes |
|---|---|---|
| Rust 1.99.0, `wasm32-unknown-unknown`, clippy | A `rust-toolchain.toml` at the repo root, then `rustup toolchain install`. With no arguments it installs the file's toolchain (rustup 1.28 and later) [S13]. | The image has 1.98.1. The same file pins the server: its toolchain is already named `1.99.0` and has wasm32, clippy and rustfmt (checked), so nothing changes there. |
| Node 24 | `actions/setup-node` v7.1.0 with `node-version: 24` and the npm cache on both lockfiles | Node 24.20.0 is already on the image. |
| chrome-headless-shell 133.0.6943.141, mac-arm64 | After `npm ci` in `e2e/`: `node_modules/.bin/browsers install chrome-headless-shell@133.0.6943.141 --path ~/.cache/puppeteer` | The mac-arm64 build exists (checked in Chrome for Testing's `133.0.6943.141.json`). The `browsers` CLI comes with puppeteer-core (version 2.13.2 in our lockfile), so it is pinned too. `--path ~/.cache/puppeteer` gives exactly the path `e2e/run.mjs` uses by default on a Mac. |
| Cargo cache | `Swatinem/rust-cache` v2.9.2 with `cache-on-failure: true` | It keeps `~/.cargo` and the dependencies in `target/` (not our own crates, not incremental files). It sets `CARGO_INCREMENTAL=0`. Its key covers rustc, `Cargo.lock`, `rust-toolchain.toml` and `CARGO*` variables [S14]. By default it saves only after a green run; a failing run is the one most often followed by a re-run, so save on failure too. |
| Plugins (optional, on by default) | Surge XT 1.3.4 and Airwindows Consolidated dmgs, unpacked with `pkgutil --expand-full` into a private folder (never installed) | See §8. |

- **Pin every action to a commit.** A tag can be moved; a commit can't. The draft pins:

  | Action | Version | Commit |
  |---|---|---|
  | `actions/checkout` | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
  | `actions/setup-node` | v7.1.0 | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
  | `Swatinem/rust-cache` | v2.9.2 | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` |
  | `actions/upload-artifact` | v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |

- **Organization policy.** If BreadBowlAI allows only GitHub's own actions, `Swatinem/rust-cache` is blocked.
  Then either allow it by name, or use `actions/cache` on `~/.cargo/registry`, `~/.cargo/git` and `target`
  (bigger, and it keeps our own crates too).

---

## 3. Disk, memory and time, measured here

The runner can't be measured from here. So this server played the job on **3 CPUs** (`taskset`), with the
job's settings (`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`), on a plain copy of f126d03 (`git archive`),
with its own target folder. Other agents' jobs ran at the same time (load 8 to 72 on 32 threads), so the
times are rough. Their logs: `$SCRATCH/logs/gci-*.log` (the scratch copy was deleted).

| What (3 CPUs) | Measured here | Notes |
|---|---|---|
| Build the test binaries, cold (`cargo test --workspace --no-run`) | **433 s and 586 s** (two runs) | about 590 crates; almost all of it is dependencies |
| Build them again with dependencies kept (rust-cache's warm case) | **114 s**; clippy then 9 s, a release `m2` 34 s | our 13 crates only |
| Clippy, after the build | 50 s | |
| Release `m2`, cold | 300 s | the draft skips it: the debug `m2` gives the same render bits |
| Run every Rust test | **228 s** | 686 pass, 10 fail: the same 10 as the lead's 32-thread baseline, so 3 CPUs add no failure |
| Every browser suite | **361 s** (the 32-thread baseline took about 343 s) | the same results as the lead's baseline (21 ok, 7 fail, live3b PART): the suites wait on the clock, not the CPU |
| The 8 renders with the debug `m2` | under a minute | all 8 hashes equal the release `m2`'s (`base-f126d03/renders`) |
| `target/` size | **2.5 GB** for the test build; **4.5 GB** with clippy, a second feature set and a release `m2` | today's 25 GB in the main checkout is 16 GB of `deps/` with debug info plus 5.5 GB of incremental files |
| Peak memory of the build | **2.2 GB** in processes (3.5 GB counting file cache) | the runner has 7 GB: enough |

What this means for the runner:
- **Disk fits.** About 4 GB of `target/`, plus the toolchain, the cargo registry, `node_modules`, Chrome and the
  plugins: under 10 GB. The 14 GB GitHub promises is enough, with a little room. The job checks for 10 GB free first.
  Without `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0` it probably would not (debug info is most of the 25 GB).
- **Memory fits** with 3 parallel jobs.
- **Time:** expect the runner to be **1.5 to 2 times slower** than these numbers *(estimate, unverified)*. An
  M1 core compiles more slowly than this Ryzen's, and the runner is a VM; but these runs were slowed by other
  jobs too. The first real run replaces this guess.

---

## 4. Starting a run and reading the result, from this server

### Start

| Way | Needs | Command |
|---|---|---|
| **Push to `check/mac`** (recommended) | git over ssh, which works today | `git push -f origin <commit>:refs/heads/check/mac` |
| Dispatch | `gh`, logged in with Actions write | `gh workflow run mac.yml -R BreadBowlAI/music2 --ref <branch>` |
| GitHub's web page | the owner | Actions → mac → Run workflow |

Rules:
- `workflow_dispatch` works only when the workflow file is on the default branch, `main` [S15]. The "Run
  workflow" button appears only then too.
- A push runs the workflow file **inside the pushed commit**. So that commit must contain
  `.github/workflows/mac.yml`. Branches made before the file (`wip/step7`, `taste`, `sounds`) get it when they
  merge `main`. Until then, push a throwaway commit that adds the file on top.
- Each trigger starts its own paid run. Use one of them per commit. The workflow's `concurrency` group cancels
  an older run on the same branch when a newer one starts.
- Since February 2026 the dispatch API can return the new run's id (`return_run_details`); gh 2.87 and later
  uses it by default [S16].

### Read

The repo is private, so listing runs and fetching logs needs a token with read access to it [S17].

| Command | Gives |
|---|---|
| `gh run list -R BreadBowlAI/music2 -w mac.yml -c <sha> --json databaseId,status,conclusion` | the run for a commit |
| `gh run watch <id> -R BreadBowlAI/music2 --exit-status` | waits until it ends (not with a fine-grained token) |
| `gh run view <id> -R BreadBowlAI/music2 --json jobs --jq '.jobs[].steps[] \| [.name,.conclusion] \| @tsv'` | each check's result |
| `gh run view <id> -R BreadBowlAI/music2 --log-failed` | the failing steps' output |
| `gh run download <id> -R BreadBowlAI/music2 -n mac-logs -D $SCRATCH/mac-<sha>` | every log as files |

The lead's whole loop, run through `$SCRATCH/tw` since it waits up to an hour:

```sh
sha=$(git rev-parse HEAD); R=BreadBowlAI/music2
git push -f origin "$sha:refs/heads/check/mac"
until id=$(gh run list -R $R -w mac.yml -c "$sha" --json databaseId -q '.[0].databaseId') && [ -n "$id" ]; do sleep 15; done
gh run watch "$id" -R $R --exit-status > /dev/null; echo "mac run $id: exit $?"
gh run download "$id" -R $R -n mac-logs -D $SCRATCH/mac-"$sha"   # summary.md, test.log, clippy.log, e2e/…
```

### Which login

| | A. `gh auth login` (recommended) | B. Fine-grained token |
|---|---|---|
| Can do | everything `gh` does, on every repo the owner can reach (scopes `repo`, `read:org`, `gist`) [S18] | only music2: read runs, logs and artifacts (Actions: Read) |
| Set-up time | about 2 minutes | about 5 minutes, plus approving it in the organization (approval is on by default) [S19] |
| Limits | The token is saved as plain text in `~/.config/gh/hosts.yml` (a server has no keyring) [S18]. | `gh run watch` refuses fine-grained tokens [S20]; poll with `gh run view --json` instead. Cancel, re-run and dispatch need Actions: Read and write [S21]. Lifetime at most 366 days by default [S19]. |

Why A is fine here: the ssh key on this server already logs in to GitHub as a user, not as a deploy key
(checked with `ssh -T git@github.com`). So the server can already push anywhere that user can. A `gh` login
adds little power beyond that. B is the least-privilege choice if the owner prefers it.

---

## 5. What the owner does, once

**On GitHub, in a browser (about 5 minutes):**
1. Decide "yes" to Mac runners (an open decision in `HANDOFF.md`).
2. Organization BreadBowlAI → Settings → Billing: look at the plan.
   - The free minutes cover about 190 Mac minutes a month on Free, 290 on Team.
   - To allow more, add a payment method and an Actions budget that stops use when reached (for example $20 a month).
3. Organization → Settings → Actions → General: allow Actions for music2, and allow `actions/*` and
   `Swatinem/rust-cache@*` (or tell the lead to use `actions/cache` instead).

**On the server, in a tmux window (about 5 minutes):**

4. Install `gh` from GitHub's own apt repository [S22]. Ubuntu 24.04's own package is `gh` 2.45.0 (checked
   with `apt-cache policy gh`), which gh's maintainers call broken "due to deprecated GitHub APIs" [S22].

   ```sh
   (type -p wget >/dev/null || (sudo apt update && sudo apt install wget -y)) \
     && sudo mkdir -p -m 755 /etc/apt/keyrings \
     && out=$(mktemp) && wget -nv -O$out https://cli.github.com/packages/githubcli-archive-keyring.gpg \
     && cat $out | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null \
     && sudo chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
     && sudo mkdir -p -m 755 /etc/apt/sources.list.d \
     && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
     && sudo apt update \
     && sudo apt install gh -y
   ```

   Without sudo instead: unpack `gh_2.102.0_linux_amd64.tar.gz` (the latest release, 30 Sep 2026) from
   https://github.com/cli/cli/releases and copy its `bin/gh` to `~/.local/bin`, which is already on `PATH` here.
   It won't update itself.
5. Log in: `gh auth login --hostname github.com --git-protocol ssh --web`. It prints a code. Open
   https://github.com/login/device on any device, type the code, approve. Say no if it offers to upload an ssh key.
   - Or, for option B: create a fine-grained token (Settings → Developer settings → Fine-grained tokens):
     owner BreadBowlAI, only `music2`, Actions: Read. Approve it in the organization. Save it on the server
     as `~/.config/m2/gh-token` (mode 600). The lead then runs `GH_TOKEN=$(cat ~/.config/m2/gh-token) gh …`.
6. Check: `gh run list -R BreadBowlAI/music2 --limit 1` prints without an error.

Then the lead does the rest: it adds the workflow to `main` with the platform layer and pushes `check/mac`.

---

## 6. What the job runs, and why that proves "the Mac is green"

| Check | Command | What it proves on the Mac |
|---|---|---|
| Web build | `cd web && npm ci && npm run build` | The UI builds, and `web/dist` exists for the server and the suites. |
| Build | `cargo build --locked --workspace --all-targets` | Everything compiles, the desktop app too (Tauri uses the image's Xcode SDK). Builds the test binaries, `m2` and the `headless` example once. |
| Rust tests | `cargo test --locked --workspace --no-fail-fast` | All tests, including FSEvents, `F_FULLFSYNC`, `core/plugins/tests/main_thread.rs` (CFRunLoop on the real main thread) and `apps/music2-desktop/tests/appkit_main_thread.rs` (AppKit's own loop). With the plugins on, `CLAP_PATH` is set, so both also run their plugin parts (Surge XT; the AppKit test adds Airwindows). |
| Clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` | Code under `cfg(target_os = "macos")` is linted only when compiling for macOS, so Linux's clippy can't stand in. |
| Browser suites | `node e2e/run.mjs all` with `M2_AUDIO=none` | All 29 suites. Exit 0 means all ok; 3 means some were PART (live3b without plugins). |
| Offline renders | the debug `m2` from the build, `e2e/baseline.sh --tree .`, then `shasum -a 256 -c e2e/renders.sha256` | The 8 renders have the recorded bits. After deterministic math, the same file must pass on Linux too. The debug `m2` gives the release `m2`'s bits (checked here: all 8 equal at f126d03), and it saves a cold release build. The server's `check.sh` keeps using release, so the two cross-check each other. |

**The AppKit test.** It calls `[NSApplication sharedApplication]`, sets the activation policy to
"prohibited" (no Dock icon, no menu bar), then runs `[NSApp run]` until the worker stops it. That needs a
connection to the window server. The image logs the runner user in to a GUI session [S4]. Whether the runner
process itself sits in that session isn't documented *(unverified)*. The first run settles it. If it fails
with a window-server error, the test needs a decision, not a workaround: it is HOST-2's proof.

**Not in the job:**
- **Timing gates** (`m2-gates --server …`). The reference is a real base M1 at 48 kHz on a quiet machine. A
  3-core VM shares its host and has no sound device. Its numbers would mislead, so keep them on the owner's Mac.
- **The desktop app's window.** Never launch `music2-desktop`.
- **Rebuilding `std.wasm`** to check it reproduces: it can't match until the path fix in §10 lands.

**Today's renders on Linux** (the lead's `base-f126d03/renders`): 6 of the 8 match the Mac's hashes in
`HANDOFF.md`. Both `examples/resample` cases differ, as `HANDOFF.md` says (the platform libm).

---

## 7. Minutes and cost per run

Built from §3: CPU-bound steps × 1.5 to 2; the browser suites × 1.2 to 1.5 (they mostly wait); plus the fixed
steps *(estimates; unverified until the first run)*.

| Step | Cold (no cache) | Warm (dependencies cached) |
|---|---|---|
| Start, checkout, Rust 1.99.0, Node, `npm ci` ×2, web build | 4–5 min | 4–5 min |
| Plugins: download and unpack | 1–2 min | 1–2 min |
| Build | 11–20 min | 3–4 min |
| Rust tests | 6–8 min | 6–8 min |
| Clippy | 1–2 min | under 1 min |
| Browser suites | 8–10 min | 8–10 min |
| 8 renders | 1 min | 1 min |
| Cache save, log upload | 1–2 min | under 1 min (an exact cache hit saves nothing) |
| **Total** | **about 35–50 min, say 40** | **about 25–31 min, say 28** |
| **Cost at $0.062/min** | **about $2.50** (up to $3.10) | **about $1.75** (up to $1.90) |

What that means in a month:

| Use | Mac minutes | Cost | Covered by free minutes? |
|---|---|---|---|
| One step: a first run, a re-run after a fix, a final run (cold + 2 warm) | about 96 | about $6 | Free plan (193 min): about 2 steps a month. Team (290): about 3. |
| A run that waited more than 7 days (cache gone) | about 40 | about $2.50 | |
| `macos-26-xlarge` instead (5 cores, $0.102/min, maybe twice as fast) | about 20–25 cold | about $2.00–2.50 | never: larger runners always cost money |

Ways to keep it cheap, already in the draft: one job (each job rounds up to a whole minute), the debug `m2`
for renders (no second build), a newer run cancels an older one, a 120-minute cap, and runs only by hand.

---

## 8. Plugins on the Mac runner (optional, on by default)

`live3b`'s plugin part (11 checks), the AppKit test's with-plugins part and `main_thread.rs`'s Surge part
need Surge XT and Airwindows Consolidated on `CLAP_PATH`. Without them `live3b` is PART and `run.mjs` exits 3.

**Never install them into `/Library/Audio/Plug-Ins/CLAP`.** `search_paths()` always adds that folder and
`$HOME/Library/Audio/Plug-Ins/CLAP` after `CLAP_PATH` (`core/plugins/src/lib.rs`). `fx-fixes` checks the
message "it needs the Airwindows Consolidated and Surge XT plugins, which aren't installed", so it fails if
they can be found there. `run.mjs` hides `CLAP_PATH` from every suite but live3b for the same reason. So the
job unpacks the packages into a private folder and names it only in `CLAP_PATH`.

- **Surge XT 1.3.4** (still the latest stable, 11 Aug 2024; `examples/with-plugins` records 1.3.4):
  `https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/surge-xt-macOS-1.3.4.dmg`
  (417 MB). The dmg holds one product package with parts `Surge_XT_CLAP.pkg` (`Surge XT.clap`) and
  `Surge_XT_Resources.pkg` (the factory data, the same files as Linux's `/usr/share/surge-xt`) [S23].
  `pkgutil --expand-full` unpacks them without installing. `SURGE_DATA_HOME` then points Surge at the data
  (Surge reads it on every OS) [S27].
- **Airwindows Consolidated:** one rolling release, tag `DAWPlugin`, rebuilt nightly. The file name carries
  the date and commit, for example `airwindows-consolidated-macOS-2026-10-11-8a67b80.dmg` (49 MB), so there
  is no fixed URL. Look it up at run time (`gh release download DAWPlugin -R baconpaul/airwin2rack -p '*macOS*.dmg'`)
  and log its sha256 [S24]. Its package holds `Airwindows Consolidated.clap` [S25].
- Files fetched with `curl` or `gh` get no quarantine flag, so macOS loads the bundles *(unverified)*.
- Cost: about 1 to 2 minutes to download and unpack *(unverified)*.
- Reading another owner's public release with the job's own token (`github.token`) should work *(unverified)*.
  If it doesn't, an unauthenticated `curl` of the releases API works, but runner IPs share a 60-an-hour limit.

---

## 9. Plugins on Linux (this server)

### What to download

| Plugin | File (Linux x86_64) | Size | Format |
|---|---|---|---|
| Surge XT 1.3.4 | `https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/surge-xt-linux-x64-1.3.4.deb` | 223 MB | Debian package: `usr/lib/clap/Surge XT.clap` (one shared-object file) and the data in `usr/share/surge-xt/` [S26] |
| Surge XT 1.3.4, same files | `…/1.3.4/surge-xt-linux-x86_64-1.3.4.tar.gz` | 350 MB | a tar of the package's `usr/` tree [S26] |
| Surge XT 1.3.4, plugins only | `…/1.3.4/surge-xt-linux-1.3.4-pluginsonly.tar.gz` | 97 MB | no data; would need `surge-xt-portable-content-1.3.4.tar.gz` (253 MB) too |
| Airwindows Consolidated | `https://github.com/baconpaul/airwin2rack/releases/download/DAWPlugin/AirwindowsConsolidated-2026-10-11-8a67b80-Linux.zip` (name changes nightly) | 32 MB | a plain zip of the build's product folder: CLAP, VST3, LV2, standalone [S25]. Built in an Ubuntu 20.04 image (glibc 2.31), so it runs on 24.04. GitHub lists its sha256 (`9144dc8b…f12c` for this build). |

Use the `.deb`: it is the smallest file that has both the plugin and its data. Both projects publish only on
GitHub releases; Surge's download page links there.

**Unpack it; never `sudo dpkg -i` it.** Installed, Surge lands in `/usr/lib/clap`, one of the CLAP spec's
Linux default folders (survey OS-32). Once Linux's `search_paths()` adds that folder, as macOS's adds
`/Library/Audio/Plug-Ins/CLAP`, every run would find Surge, and `fx-fixes` (which needs it missing) would fail.
The same holds for `~/.clap`: keep it empty.

### How Surge finds its data (from its source)

Surge XT 1.3.4 on Linux looks for its data in this order [S27] [S28]:
1. `$SURGE_DATA_HOME` (or `$PIPELINE_OVERRIDE_DATA_HOME`), if set.
2. A folder named `SurgeXTData` next to the `.clap` file, or in any folder above it ("portable mode").
3. `~/.local/share/Surge XT`, then `~/.local/share/surge-xt` (both under `$XDG_DATA_HOME` if set), then
   `/usr/share/surge-xt`.

Our tests give each run a scratch `HOME`, so step 3 would miss. **Use step 2:** put `SurgeXTData` one folder
above the CLAP folder. Then any `HOME` works and nothing else needs setting. Surge's user folder
(`~/Documents/Surge XT` or `~/.Surge XT`) then lands in the scratch `HOME`.

### Install per user, without sudo (do this when the gates need it)

```sh
P=$SCRATCH/plugins
mkdir -p "$P/clap" "$P/dl" && cd "$P/dl"
# Surge XT 1.3.4: dpkg-deb -x unpacks a .deb as a normal user.
curl -fLO https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/surge-xt-linux-x64-1.3.4.deb
dpkg-deb -x surge-xt-linux-x64-1.3.4.deb surge
cp "surge/usr/lib/clap/Surge XT.clap" "$P/clap/"
cp -R surge/usr/share/surge-xt "$P/SurgeXTData"
# Airwindows Consolidated: look up today's zip; unzip isn't installed here, Python's zipfile is.
url=$(curl -fsSL https://api.github.com/repos/baconpaul/airwin2rack/releases/tags/DAWPlugin \
  | /usr/bin/python3 -I -c 'import json,sys; print([a["browser_download_url"] for a in json.load(sys.stdin)["assets"] if a["name"].endswith("-Linux.zip")][0])')
curl -fLO "$url" && sha256sum *.zip
/usr/bin/python3 -I -m zipfile -e AirwindowsConsolidated-*-Linux.zip aw
find aw -name 'Airwindows Consolidated.clap' -exec cp -R {} "$P/clap/" \;
# Any missing system library shows as "not found":
ldd "$P/clap/Surge XT.clap" "$P/clap/Airwindows Consolidated.clap" | grep -E '^/|not found'
```

Then `CLAP_PATH=$SCRATCH/plugins/clap`. That folder holds exactly `Surge XT.clap` and
`Airwindows Consolidated.clap`, the two names `e2e/live3b.mjs` checks for.

Notes:
- `find … -exec cp -R` copes with the zip's inner folders, whose layout isn't known until it is opened
  *(unverified: CMake zips the product folder by its full build path)*.
- Python's zipfile drops the execute bit. `dlopen` doesn't need it on Linux.
- Copy the `.clap` files; don't symlink them. Surge finds its data from the path it was loaded by.
- Record both files' sha256 next to the folder. Airwindows changes nightly, and the old zip is deleted.

### Runtime libraries

Surge's package declares these needs [S26]. Checked on this server with `dpkg-query`:

| Package | Here? | Likely needed for |
|---|---|---|
| `libc6` (≥ 2.27) | yes, 2.39 | everything |
| `libcairo2`, `libfontconfig1`, `libfreetype6`, `libx11-6`, `libxcb1`, `libxkbcommon0` | yes | drawing and fonts |
| `libxcb-cursor0`, `libxcb-xkb1`, `libxkbcommon-x11-0` | **no** | the editor window *(unverified whether the DSO links them directly)* |
| `xdg-utils`, `xclip` | no | opening links, the clipboard (GUI only) |

Airwindows Consolidated is a JUCE 8 plugin built with `JUCE_USE_CURL=0` and `JUCE_WEB_BROWSER=0` [S25]. JUCE
on Linux usually links freetype and ALSA (`libasound2`, present) and loads X11 only when it opens a window
*(unverified)*.

If `ldd` shows something missing:
- simplest: the owner runs `sudo apt install libxcb-cursor0 libxcb-xkb1 libxkbcommon-x11-0` (three small libraries); or
- with no sudo: `apt-get download libxcb-cursor0 libxcb-xkb1 libxkbcommon-x11-0` (works as a normal user),
  `dpkg-deb -x` each into `$SCRATCH/plugins/lib`, and set `LD_LIBRARY_PATH=$SCRATCH/plugins/lib/usr/lib/x86_64-linux-gnu`
  for plugin runs.

What else Linux needs before these plugins play here is platform-layer work, not packaging: JUCE plugins on
Linux want the host's main loop to serve CLAP's fd and timer extensions (survey-code §5, OS-26 and OS-27).
Whether either plugin runs with no X display at all is *(unverified)*: the first `main_thread.rs` run with
`CLAP_PATH` set will show.

---

## 10. What the repo needs before the first run

For the platform layer to add (small):
1. **`rust-toolchain.toml`**: `channel = "1.99.0"`, `components = ["clippy", "rustfmt"]`,
   `targets = ["wasm32-unknown-unknown"]`, `profile = "minimal"` (survey OS-65).
2. **The eighth render in `e2e/baseline.sh`**: `templates/empty --from 4 --seconds 6`. Today
   `$SCRATCH/check.sh` adds it by hand.
3. **`e2e/renders.sha256`**: the recorded hashes, one file for every OS, named as `baseline.sh` names the
   files (`…-from4-for6.wav`; `HANDOFF.md` writes `…-from4.wav`). The deterministic-math commit changes it once,
   on purpose.
4. **`.github/workflows/mac.yml`** on `main` (the draft below).
5. Later, for a "std.wasm reproduces" check: `std/build.sh` must remap source paths. Today's `std.wasm`
   contains `<repo>/core/kernel/src/lib.rs` (checked with `strings`), so a build in
   any other folder differs. Note that `RUSTFLAGS` replaces the `rustflags` in
   `std/kernels-src/.cargo/config.toml`, so keep its `-zstack-size=65536` when adding `--remap-path-prefix`.

---

## 11. Would a GitHub `ubuntu-24.04` job add anything?

**Not now.** This server is Ubuntu 24.04 on x86_64 with 32 threads; GitHub's private-repo Linux runner has 2
cores and 8 GB [S1]. What a GitHub job would add, and when it might be worth it:

| It would add | Worth it? |
|---|---|
| a clean machine, which proves the list of system packages (this server has GTK, WebKit and much more already) | later, once before a release; about $0.006 × ~60 min ≈ $0.40 a run *(estimate)* |
| arm64 Linux (`ubuntu-24.04-arm`, $0.005/min) | only once Linux arm64 matters. Chrome 133 has no linux-arm64 build (survey OS-73). |
| Ubuntu 26.04 (`ubuntu-26.04`) | when a newer distro is a target |

---

## 12. The draft workflow (`.github/workflows/mac.yml`)

Not a file yet: the platform layer adds it with §10's items 1 to 3.

```yaml
# .github/workflows/mac.yml
#
# In short: proves the Mac is still green, on GitHub's Apple-silicon runner (macOS 26, like the
# owner's Mac). Build, every Rust test, clippy, the web build, every browser suite, and the 8 offline
# renders against e2e/renders.sha256. It never runs by itself. Start it by hand, once per step:
#   from the server:  git push -f origin <commit>:refs/heads/check/mac
#   on GitHub:        Actions > mac > Run workflow (the file must be on main for the button)
# Cost: $0.062 a minute (macOS, private repo). Timing gates are not run here: they belong on a real Mac.

name: mac

on:
  push:
    branches: [check/mac]       # the lead's trigger: needs only git over ssh
  workflow_dispatch:            # the owner's button
    inputs:
      plugins:
        description: Install Surge XT and Airwindows Consolidated (the plugin checks)
        type: boolean
        default: true

# A newer run on the same branch cancels the older one, which would be paid for nothing.
concurrency:
  group: mac-${{ github.ref }}
  cancel-in-progress: true

# Read the code; write nothing back to the repo.
permissions:
  contents: read

defaults:
  run:
    shell: bash                 # = bash -eo pipefail: a failure before "| tee" still fails the step

env:
  CARGO_TERM_COLOR: never
  CARGO_INCREMENTAL: "0"        # CI builds once; incremental files only fill the disk
  CARGO_PROFILE_DEV_DEBUG: "0"  # no debug info, so target/ fits the runner's disk (§3)
  M2_AUDIO: none                # no sound device: the tests and the server must never open one
  CHROME: 133.0.6943.141        # the browser the suites are pinned to
  PLUGINS: ${{ github.event_name == 'push' || inputs.plugins }}

jobs:
  mac:
    runs-on: macos-26           # M1, 3 cores, 7 GB. Not macos-latest: it moves to new versions.
    timeout-minutes: 120        # GitHub's own limit is 6 hours; stop a hung run sooner
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1   # v7.0.1

      - name: Machine and free disk
        run: |
          mkdir -p "$RUNNER_TEMP/logs"
          { sw_vers; uname -m; sysctl -n machdep.cpu.brand_string hw.ncpu hw.memsize
            df -h "$GITHUB_WORKSPACE"; } | tee "$RUNNER_TEMP/logs/machine.txt"
          # m2 always searches this folder (and ~/Library/...). It must hold no plugins, or fx-fixes fails.
          if [ -n "$(ls -A /Library/Audio/Plug-Ins/CLAP 2>/dev/null)" ]; then
            ls -la /Library/Audio/Plug-Ins/CLAP; exit 1
          fi
          # The job needs under 10 GB (§3). Stop early, before paying for a build that cannot fit.
          free_gb=$(df -g "$GITHUB_WORKSPACE" | awk 'NR==2 {print $4}')
          [ "$free_gb" -ge 10 ] || { echo "only ${free_gb} GB free"; exit 1; }

      - name: Rust 1.99.0 (rust-toolchain.toml)
        run: |
          rustup toolchain install    # no arguments: installs what rust-toolchain.toml names
          rustc -Vv

      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6   # v2.9.2
        with:
          cache-on-failure: true    # a red run is the one most often re-run soon after

      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1   # v7.1.0
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: |
            web/package-lock.json
            e2e/package-lock.json

      - name: Web build
        id: web
        run: (cd web && npm ci && npm run build) 2>&1 | tee "$RUNNER_TEMP/logs/web.log"

      - name: Build (everything, the desktop app too)
        id: build
        if: ${{ !cancelled() }}     # the Rust build doesn't need web/dist; only the browser suites do
        run: |
          cargo build --locked --workspace --all-targets 2>&1 | tee "$RUNNER_TEMP/logs/build.log"
          df -h "$GITHUB_WORKSPACE"

      - name: Plugins (Surge XT 1.3.4, Airwindows Consolidated nightly), unpacked, not installed
        id: plugins
        if: ${{ !cancelled() && env.PLUGINS == 'true' }}
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          # Never install into /Library/Audio/Plug-Ins/CLAP: m2 always searches it, and fx-fixes needs
          # the plugins to be missing. Unpack the packages into a private folder; only CLAP_PATH names it.
          d="$RUNNER_TEMP/plugins"; mkdir -p "$d/clap" "$d/dl" "$d/x"; cd "$d/dl"
          curl -fsSLO https://github.com/surge-synthesizer/releases-xt/releases/download/1.3.4/surge-xt-macOS-1.3.4.dmg
          # Airwindows has one rolling release, rebuilt nightly under a new file name.
          gh release download DAWPlugin -R baconpaul/airwin2rack -p '*macOS*.dmg'
          shasum -a 256 ./*.dmg | tee "$RUNNER_TEMP/logs/plugins.txt"
          for img in ./*.dmg; do
            mnt=$(mktemp -d)
            hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$img" >/dev/null
            find "$mnt" -maxdepth 2 -name '*.pkg' | while read -r pkg; do
              pkgutil --expand-full "$pkg" "$d/x/$(basename "$pkg" .pkg)"
            done
            hdiutil detach "$mnt" >/dev/null
          done
          for name in "Surge XT" "Airwindows Consolidated"; do
            # sed, not head: under pipefail, head closing the pipe early would fail the step
            src=$(find "$d/x" -type d -name "$name.clap" -path '*Payload*' | sed -n 1p)
            cp -R "$src" "$d/clap/"
          done
          data=$(find "$d/x" -type d -path '*Resources.pkg/Payload' | sed -n 1p)   # Surge's factory data
          ls -la "$d/clap" "$data" | tee -a "$RUNNER_TEMP/logs/plugins.txt"
          echo "CLAP_PATH=$d/clap" >> "$GITHUB_ENV"
          echo "SURGE_DATA_HOME=$data" >> "$GITHUB_ENV"

      - name: Rust tests (all; the AppKit main-thread test too)
        id: test
        if: ${{ !cancelled() && steps.build.outcome == 'success' }}
        run: cargo test --locked --workspace --no-fail-fast 2>&1 | tee "$RUNNER_TEMP/logs/test.log"

      - name: Clippy -D warnings
        id: clippy
        if: ${{ !cancelled() }}
        run: cargo clippy --locked --workspace --all-targets -- -D warnings 2>&1 | tee "$RUNNER_TEMP/logs/clippy.log"

      - name: Browser suites (all)
        id: e2e
        if: ${{ !cancelled() && steps.build.outcome == 'success' && steps.web.outcome == 'success' }}
        run: |
          cd e2e && npm ci
          chrome=$(node_modules/.bin/browsers install "chrome-headless-shell@$CHROME" \
            --path "$HOME/.cache/puppeteer" --format '{{path}}')
          set +e
          E2E_CHROME="$chrome" M2_HEADLESS="$GITHUB_WORKSPACE/target/debug/examples/headless" \
            node run.mjs all --out "$RUNNER_TEMP/e2e" 2>&1 | tee "$RUNNER_TEMP/logs/e2e.log"
          st=${PIPESTATUS[0]}
          mkdir -p "$RUNNER_TEMP/logs/e2e" && cp "$RUNNER_TEMP"/e2e/*.log "$RUNNER_TEMP/logs/e2e/" 2>/dev/null
          # 0: every suite ok. 3: some PART, which is fine only when the plugins were left out (live3b).
          [ "$st" -eq 0 ] || { [ "$st" -eq 3 ] && [ "$PLUGINS" != true ]; }

      - name: Offline renders (8) have the recorded bits
        id: renders
        if: ${{ !cancelled() && steps.build.outcome == 'success' }}
        run: |
          # The debug m2 from the build step: renders keep their bits at any opt-level (checked on
          # Linux at f126d03), and a cold release build would cost 7-10 more paid minutes here.
          M2="$GITHUB_WORKSPACE/target/debug/m2" e2e/baseline.sh --tree . "$RUNNER_TEMP/renders" \
            2>&1 | tee "$RUNNER_TEMP/logs/renders.log"
          cd "$RUNNER_TEMP/renders"
          shasum -a 256 -c "$GITHUB_WORKSPACE/e2e/renders.sha256" 2>&1 | tee -a "$RUNNER_TEMP/logs/renders.log"

      - name: Summary
        if: ${{ always() }}
        run: |
          {
            echo "| check | result |"
            echo "|---|---|"
            echo "| web build | ${{ steps.web.outcome }} |"
            echo "| build | ${{ steps.build.outcome }} |"
            echo "| plugins | ${{ steps.plugins.outcome }} |"
            echo "| rust tests | ${{ steps.test.outcome }} |"
            echo "| clippy | ${{ steps.clippy.outcome }} |"
            echo "| browser suites | ${{ steps.e2e.outcome }} |"
            echo "| renders | ${{ steps.renders.outcome }} |"
          } | tee -a "$GITHUB_STEP_SUMMARY" "$RUNNER_TEMP/logs/summary.md"
          df -h "$GITHUB_WORKSPACE"

      - name: Keep the logs (14 days)
        if: ${{ always() }}
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9   # v7.0.2
        with:
          name: mac-logs
          path: ${{ runner.temp }}/logs
          retention-days: 14
```

Design notes on the draft:
- **Every check runs even when an earlier one fails** (`!cancelled()`), so one paid run gives the whole
  picture. Steps that need the build skip when it failed. The job is red if any step failed.
- **Logs are files**, so the lead reads them with `gh run download` and greps them like the server's
  `$SCRATCH/` logs. The artifact holds only logs (a few MB), never the e2e scratch folders (500 MB here).
- **No secrets.** The job uses only its own short-lived token, read-only.
- The script lines avoid bash 4 features (the image's `bash` is 3.2).

---

## Open points *(unverified, settled by the first run)*

- Free disk on a fresh `macos-26` runner. GitHub promises 14 GB; others report much more after deleting
  preinstalled tools (Electron cleans to about 134–157 GB on `macos-15`) [S29].
- Whether the runner process sits in the autologin GUI session (the AppKit test).
- How fast the 3 vCPUs are compared with this server's cores (§3 scales by 1.5 to 2).
- Whether `github.token` can download another owner's public release.
- Whether either plugin needs an X display, or more libraries, on Linux.
- The inner layout of the Airwindows Linux zip.

## Sources

- [S1] GitHub-hosted runners reference: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- [S2] runner-images README (labels, macOS 14 deprecated): https://github.com/actions/runner-images
- [S3] macOS 26 arm64 image: https://github.com/actions/runner-images/blob/main/images/macos/macos-26-arm64-Readme.md
- [S4] runner-images build scripts: https://github.com/actions/runner-images/blob/main/images/macos/scripts/build/configure-autologin.sh and `configure-windows.sh` in the same folder
- [S5] Actions runner pricing: https://docs.github.com/en/billing/reference/actions-runner-pricing
- [S6] How Actions use is measured (included minutes, storage): https://docs.github.com/en/billing/concepts/product-billing/github-actions
- [S7] GitHub, 2026 pricing changes for Actions (FAQ: "based on list price"): https://github.com/resources/insights/2026-pricing-changes-for-github-actions
- [S8] Larger runners: https://docs.github.com/en/actions/reference/runners/larger-runners
- [S9] Actions limits: https://docs.github.com/en/actions/reference/limits
- [S10] Dependency caching (10 GB, 7 days, branch scope): https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching
- [S11] Changelog, reduced pricing 2026-01-01: https://github.blog/changelog/2026-01-01-reduced-pricing-for-github-hosted-runners-usage/
- [S12] Third-party summary of the old and new macOS rate: https://www.warpbuild.com/blog/github-actions-price-change
- [S13] rustup changelog (1.28.0: `rustup toolchain install` with no arguments): https://github.com/rust-lang/rustup/blob/main/CHANGELOG.md
- [S14] Swatinem/rust-cache v2.9.2 README: https://github.com/Swatinem/rust-cache/blob/v2.9.2/README.md
- [S15] Events that trigger workflows (`workflow_dispatch` on the default branch): https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows
- [S16] Changelog, the dispatch API returns run ids: https://github.blog/changelog/2026-02-19-workflow-dispatch-api-now-returns-run-ids
- [S17] REST, workflow runs (private repos need a token): https://docs.github.com/en/rest/actions/workflow-runs
- [S18] `gh auth login` manual: https://cli.github.com/manual/gh_auth_login
- [S19] Organization token policy (fine-grained tokens need approval by default; 366 days): https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/setting-a-personal-access-token-policy-for-your-organization
- [S20] `gh run watch` manual (no fine-grained tokens): https://cli.github.com/manual/gh_run_watch
- [S21] Fine-grained token permissions per endpoint: https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens
- [S22] Installing gh on Linux (official apt repo; Ubuntu's 2.45 broken): https://github.com/cli/cli/blob/trunk/docs/install_linux.md
- [S23] Surge XT macOS installer script, 1.3.4: https://github.com/surge-synthesizer/surge/blob/release_xt_1.3.4/scripts/installer_mac/make_installer.sh
- [S24] Airwindows Consolidated releases (`DAWPlugin`): https://github.com/baconpaul/airwin2rack/releases/tag/DAWPlugin
- [S25] Airwindows Consolidated build: https://github.com/baconpaul/airwin2rack/blob/main/.github/workflows/build-daw-plugin.yml, `cmake/basic-installer.cmake`, `src-juce/CMakeLists.txt`, `scripts/installer_mac/make_installer.sh`
- [S26] Surge XT Linux package script, 1.3.4 (layout and `Depends:`): https://github.com/surge-synthesizer/surge/blob/release_xt_1.3.4/scripts/installer_linux/make_deb.sh
- [S27] Surge XT data path search, 1.3.4: https://github.com/surge-synthesizer/surge/blob/release_xt_1.3.4/src/common/SurgeStorage.cpp
- [S28] sst-plugininfra Linux paths (Surge's submodule at 1.3.4): https://github.com/surge-synthesizer/sst-plugininfra/blob/49ee7eb761bc06d0c0a012659f975d59b868e3e8/src/paths_linux.cpp
- [S29] Electron, macOS runner free disk after cleanup: https://releases.electronjs.org/pr/49118
- Surge XT releases (assets and sizes): https://github.com/surge-synthesizer/releases-xt/releases/tag/1.3.4
- Chrome for Testing, 133.0.6943.141 downloads: https://googlechromelabs.github.io/chrome-for-testing/133.0.6943.141.json
- Checked on this server: `apt-cache policy gh` (2.45.0-1ubuntu0.3), `rustup` toolchains and components, `dpkg-query`
  for the plugin libraries, `strings std/kernels/std.wasm`, `ssh -T git@github.com`, the lead's
  `$SCRATCH/base-f126d03/` logs, and the timing runs in §3.
