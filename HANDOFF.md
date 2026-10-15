# Handoff

Written 2026-10-15, at the end of the platform step (6b in the contract's order of work). Read this first, then
`docs/architecture.md` (the contract) and the design for whatever you pick up.

**In short**
- The platform layer is built on `main`. Linux is green: `ci/check.sh` passed at `63c45da`.
- **macOS: unverified since `4ae1b32`**, the step's first code commit (last verified at `8aa301e`). The Mac code is
  only type-checked and linted from Linux. The owner's Mac run comes next.
- Then: finish step 7, merge `sounds`, merge `taste`; then 8a, 8b and step 9.

## Where everything is

| Branch | Commit | State |
|---|---|---|
| `main` | this commit | Steps 1–6, the fast note edits (8aa301e), and the platform layer. |
| Platforms | | Linux green at `63c45da` (ci/check.sh). macOS unverified since `4ae1b32`. Windows: designed only. |
| `origin/wip/step7` | 5079233 | Step 7, the engine: slices 1–3 done, slice 4 half done. Doesn't build yet (`core/runtime/src/tests/live.rs:491` expects `Make`). |
| `origin/taste` | 4804247 | The new look (T and S); 8b.1 Tracks done (a0d5f91); 8b.2 song bar unfinished. |
| `origin/sounds` | 916127d | 20 new std sounds (polysynth, FM, drum kit). Its last independent check was cut short. |

The three waiting branches start at 8aa301e, before this step. They exist only on `origin`; each has a detached
worktree in `~/m2-runs/wt/` (`step7`, `taste`, `sounds`).

Designs, all in `docs/designs/`:
- `platform/design.md` (§8: how later steps add to `m2-os`; §13.3: the merges) and `platform/build-notes.md` (what
  each slice built, every check, and "Left for later").
- `step7/design-7.md` (revision 2; §18 build slices, §19 owner questions).
- `step8/design-8a.md`, `design-8b.md`, `taste.md` (the look), `audit-ui.md` (98-shot audit).
- `step6/`: the Tx and write path (C1–C21, cited by the contract).
- `workflows/`: old workflow scripts, as templates. Their paths are the Mac's.

## The check: `ci/check.sh`

One script checks everything, on the server and on the Mac ([PLAT-6](docs/architecture.md#platforms)). It ends with a
short summary. Its first line says `music2 check: PASS` or `FAIL`.

| Row | What it checks |
|---|---|
| toolchain, web build, build | Rust 1.99.0 (pinned), the web UI, every binary |
| app | Linux only: the desktop app on an invisible screen (Xvfb), with `first-song` and `with-plugins`, a screenshot of each |
| rust tests | every Rust test, with a scratch `HOME` (754 at `63c45da`) |
| clippy | the whole workspace; a warning is an error |
| std kernels, std.wasm | the kernels' tests and lints; `std.wasm` rebuilt to the same bytes in another folder |
| mac lint | Linux only: the Mac-only code, type-checked and linted |
| browser suites | 29 suites (1,034 checks), each on a fresh headless server |
| renders | 8 offline renders against `e2e/renders.sha256`, one record for every OS |
| gates | the Mac only (Linux with `--gates`): the timing gates `ci/gates.txt` names (G11 `tx-data` and `watcher` now) |
| repo unchanged | the check changed nothing in the repo |

- Without the test plugins it says FAIL, even when no row failed.
- Edit no tracked file while it runs: its last row compares `git status` before and after (S9's first run failed
  that way).
- A test or suite that failed, then passed on its retry, is listed under "passed only on a retry". The lead decides
  whether it is a known flake (below) or a real failure, and writes which here.

**On the server** (8 minutes with the repo's warm `target/` on a quiet server):

```sh
cd ~/dev/suite2/music2
TW_WAIT=3500 ~/m2-runs/tw check-<label> 'M2_CHECK_OUT=$HOME/m2-runs/check-<label> ci/check.sh'
```

`M2_CHECK_OUT` must be a new folder. It gets `summary.txt`, every log, and the app's two screenshots
(`app-first-song.png`, `app-with-plugins.png`); all logs are also packed in `<folder>.tgz`. Send the owner the
screenshots once per step (this step's: `~/m2-runs/app-shots/platform-63c45da/`).

**On the Mac** (the owner; about 45 minutes, mostly waiting *(estimate)*):

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. The check keeps it awake. |
| 2 | Open Terminal on the Mac itself, not over ssh. Type `cd ~/dev/suite2/music2`, then `git switch main`, then `git pull`. | One test needs your login session. |
| 3 | Type `ci/check.sh` and press Return. | The first run may download Rust and the test browser. |
| 4 | If it stops at once and says your plugins are in the wrong folder, move Surge XT and Airwindows from `/Library/Audio/Plug-Ins/CLAP` to `~/Library/Audio/Plug-Ins/CLAP`, and run it again. | music2's tests need the shared folder empty. Other music apps still find them. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines, from `music2 check:` to the `logs` line, and paste them back. | That is all the lead needs. |

After a green run the lead writes here: "macOS: green at <commit> (ci/check.sh on <model>, <date>)", with G11's numbers
beside the M1's and the machine named (the owner's Mac is an M1 Pro). If a row fails, "what failed" names it: the lead
fixes it on Linux, checks the mac lint, and asks for one more run. The logs are in `~/m2-check/<commit>.tgz` if needed.

**What the Mac run should show changed, on purpose** (platform design §7.8, and the review's fixes):

| Change | On the Mac |
|---|---|
| `examples/resample` exports other bits, once (`libm`) | 3 recorded hashes changed; `e2e/renders.sha256` must pass as on Linux |
| `std.wasm` rebuilt from the kernels' workspace | every kernel's module hash changes once; no render changes |
| The durable sync falls back to `fsync` on a volume that refuses `F_FULLFSYNC`; a folder sync a volume refuses is skipped | APFS unchanged |
| A save on a disk without an atomic swap renames over the file in one step | exFAT and network shares never show a missing file |
| New audio is on disk before a song names it | one more `fsync` per export or bounce |
| A walk that events asked for counts no missed events | a big checkout no longer moves a project to walks |
| Song code: no `eval`, `Function`, `performance`, `WeakRef` or `enum`; `**` through `libm`; each build on a 32 MB thread of its own | such a song gets a clear error; no song today uses them |

## What to do next, in order

1. **The Mac run** (above), at `main`'s head once the lead has pushed. The step is done only when it passes (PLAT-6).
2. **Finish step 7** on `wip/step7`: bring it onto `main` (merge or rebase; notes below) and make it build. First
   platform design §8's bug 1 in `core/play` (sort cpal's errors: an xrun is counted and play goes on). Then finish
   slice 4 (design-7 §18: state loads into kept instances in prepare, restart from the committed blob, export with
   `Hold`, G7e). Then verify every gate, review, confirm, fix. Mac run before it lands on `main`.
3. **Merge `sounds`**: re-run its full checks first (its commit says why), then merge (notes below).
4. **Merge `taste`**: finish 8b.2 (song bar) and build 8b.3 (Mix faders) per design-8b, then merge (notes below).
   Mac run before it lands.
5. Then **8a** (all CPU cores), **8b.4–8b.10**, and **step 9**. Each adds its OS pieces to `m2-os` as platform design
   §8 says (8a's `rt.rs` becomes `mac/rt.rs` and `linux/rt.rs`; 8b.10's MIDI uses `midir`). Mac run at the end of each.

## Merge notes

Platform design §13.3, updated by what the build learned. Never take a side on `Cargo.lock` or `std.wasm`.

| Branch | Where | What to do |
|---|---|---|
| any | `crates/server/src/graph/fsevents.rs` is gone | FSEvents moved to `core/os/src/mac/watch.rs`, the hints and filters to `graph/events.rs`. git sees a delete and an add (`git diff -M40%` pairs it with `mac/watch.rs`). No branch changes that file today; if one does, carry the change by hand. |
| any | `core/kernel/` is now `std/kernels-src/kernel/`; the app test is `app_main_thread.rs` | git follows these renames |
| any | OS code outside `core/os` | `core/os/tests/one_place.rs` fails on it (PLAT-2): move it into `m2-os` |
| `wip/step7` | `docs/architecture.md`: PLANE-5's rule, the gates table | keep both edits |
| | `Cargo.lock` | take `main`'s, then `cargo build` adds step 7's edges; never `cargo update` |
| | `crates/gates/src/store.rs` (new there: Mach `task_info` and QoS, under `cfg`) | move both into `m2_os::process::memory()` and `thread::interactive()` (design §8); G7f's memory rules fail when memory is `None`; records name their machine. Until then `one_place.rs` fails. |
| | `core/runtime/src/audio.rs` | clean; check that the moved `write_new_wav` still pushes the file and syncs its folder (S7) |
| | `core/plugins/src/probe.rs` | nothing: `main_thread::token()` and `is_process_main()` stay as wrappers |
| | `ci/gates.txt` | add design-7 §12's gates for the Mac run |
| | G7f on Linux | wasmtime's reservations run out at about 5 step-7 renderers per process here; any `ulimit -v` stops them sooner |
| `sounds` | `std/kernels/std.wasm` | `std/build.sh`, then `std/build.sh --check`, check the renders, commit once |
| | after the merge | add `examples/sounds` to `PURE`, `e2e/baseline.sh` and both records, recorded on Linux; the next Mac run confirms them. The check's std kernels row now runs the kernels' clippy, which the six new kernels have not met. |
| `taste` | `docs/architecture.md`: WRITE-17's *Today* | keep `taste`'s text, then apply this step's network-volume edit (platform design §6.1) |
| | `e2e/attention.mjs` (the review's D7) | `taste` rewrites the toast-Undo lines; there, wait for the page's revision before clicking the toast's Undo, as `notes.mjs` does |
| | the font | `taste`'s `--font-sans` falls back to DejaVu Sans on Linux. Keep the Mac's system font; bundle Inter (OFL) for Linux and Windows. |
| | after the merge | re-run s8-tracks' "layout.json within a second" on the inotify watcher. Before this step, 14 of `taste`'s 32 suites failed on Linux (Ctrl labels, and others to re-check alone): run them all. README's "You need a Mac" gains Linux. The key table, a menu per OS (GTK), the reveal label and a PC-keys suite are `taste`/8b work. |

## Known flakes

Seen, not fixed:

| Test | What fails | Note |
|---|---|---|
| G10 in `the_live_engine_passes_every_gate` | "bar 64 takes no longer than the same length from bar 1" (1.48 s against 1.31 s) | timing, under load only; passes on its retry. Left alone (PLAT-1). |
| G3 (a note edit within compile + 20 ms) | p95 16–22 ms at n=10 | near its line under load; judged on the Mac |
| `attention-attn` | "the move's toast Undo says it can't any more" | clicks before the undo's revision reaches the page; fixed at the `taste` merge (D7) |
| `live3b` | "its release writes db(-32) into song.ts" | once, at load 45–74; not traced; watch for it |
| `s5-knobs` | page detached after a double-click on a knob at its default (3 of 11 release runs) | from before this step; not seen in its checks |

Fixed in this step:

| Test | Cause |
|---|---|
| `files::…diagnostics_say_where_in_the_project_each_problem_is` (a long checkout path) | QuickJS cut module names to 63 bytes; package modules are now named as people see them (`4ae1b32`) |
| `live3a` ruler drag | the page hears of an API seek at its next 200 ms poll; the suite now waits for the playhead |
| `cell_heard_never_runs_ahead` | the test's own timing: the writer could finish before the reader ran; also a rare hang in its gate |
| `surge_plays_notes` | Surge XT starts notes on its own 32-sample block lines, at a random phase; the test now asks for that |
| `s4-meter-fix`, `s4-clips-fix`, `live3a` reading a missing file | Linux saves moved the file aside for a moment; saves swap in one step since S6 |
| `notes-proj`, `notes` | the file changed before the page had the revision; the suites wait for the revision |
| `cpu_time_grows_while_the_thread_works` | 30 ms of wall time wasn't always 10 ms of CPU under load; it now waits for the CPU clock |

## Left for later

One line each; the details are in `platform/build-notes.md`.

- **The macOS side is type-checked only** until the Mac run: everything moved into `core/os/src/mac/` (the barrier's
  `fsync` fallback, `renamex_np`, `clonefile`, `getattrlist`, `gethostuuid`, FSEvents, the main loop), and the
  review's fixes there (D6's clone error, `sync_dir`'s error list, `c_numbers`).
- FSEvents, unproven: one `FSEventStreamFlushSync` delivers a write made just before it; a burst of 4,196 files is
  named file by file; a link-count change is named (evt-2).
- `ci/check.sh --gates` has never run whole; the Mac run is its first full run.
- Step 8's folder push, after a save's renames, still ignores errors, `EIO` too: the save has landed (D2).
- QuickJS's 256 MB limit counts bytes as each OS's `malloc` does (det-3); a contract deferred row.
- `retry_lock` scans with no journal skipped (`rescan` skips `pending`): after a lost lock and an outside edit, Retry
  could set the session's own completed journal aside as interrupted. From before this step; rare.
- WRITE-15's *Today* names only step 8's push as ignoring errors; a rollback's and the lock's tidy's do too.
- D4's words name inotify's instance limit for `EMFILE`, which is also the process's own file limit.
- An export name over 63 bytes is cut by QuickJS, so its error loses its place.
- Old rustfmt diffs in `core/script` and `core/session`: only new lines were formatted.
- The kill test takes about 130 s of `m2-session`'s test time; split per case, its cases would run side by side.
- `surge_plays_notes` can't see a note passed up to 31 samples late (Surge's blocks); the probe plugin's test can.
- inotify running out of watches after start is untested: lowering the limit needs root.
- `apps/music2-desktop/src/main.rs` still says the plugins' main thread is AppKit's (true on macOS only).
- Windows: `push_tree` opens files read-only, but `FlushFileBuffers` needs write access; platform design §8 lists the
  Unix-only spots a Windows build stops at.
- The real home here holds a stray `~/Library/Caches/music2/hashes.json` from before S6; left as it is.

## Open owner decisions (ask, with a recommendation)

- Step 7 slice 3 went beyond the design twice, both forced by gates: new nodes' audio inputs fade in over
  5 ms (G6 caught a real click at a new delay's first echo), and `text::slide` re-aligns inserted tokens.
- G7f: no Store layout passes the rule's absolute address-space bound (HEAD fails it too). Layout P1, the
  default settings, was kept. G7f part 2's p99 rule still failed in slices 2 and 3.
- `std:chorus`, phaser and tremolo wobble on engine time, so strings and keys after a stop differ from the
  export. Fixing it changes `templates/starter`'s render and needs a std version bump.

## Decisions already taken for the owner

- Step 7's Q1–Q9 as design-7 §19 recommends (a SiteId is a call's `(` point; export is a tape pause).
- 8a: gate 8 on this M1 Pro is "provisional" until a base M1 exists; the device buffer keeps its default.
- 8b: tempo drag lets audio drift until release; a MIDI keyboard slice (8b.10) is added; the level slider lives only on
  the Mix strip. The data plane keeps its 20 ms budget, met by playing the edit first (PLANE-5).
- Platform (owner, 2026-10-14): a new song's name is taken in any capitals, on every computer (Q1). On Linux the
  browser suites tell Chrome it is a Mac; a Ctrl suite comes with `taste`'s key table (Q2). The server keeps its host
  name (Q3). No hosted Mac runners: the owner runs the check on the Mac at the end of each step. The desktop app may
  open here on an invisible Xvfb screen, once per step, with screenshots sent to the owner.

## How the owner wants the work done

- **Quality over speed.** Full checks over lighter ones. For each step: design, attack, build in green slices, verify
  (`ci/check.sh`: every test, clippy, every browser suite, the renders; the Mac's timing gates), multi-lens review,
  skeptic confirmation, fix. Commit and push each step once it passes.
- **Simple yet elegant**, a tasteful professional DAW (good enough for A. G. Cook). Don't be afraid to change
  earlier work when it makes things simpler.
- **Writing to the owner:** they are dyslexic and new to music. Short sentences, plain words, tables,
  "In short" first, no walls of text.
- No built-in AI in the app. No backwards compatibility.

## The server

| What | How |
|---|---|
| The machine | `jerry-ax102`: Ubuntu 24.04, Ryzen 9 7950X3D (32 threads), 124 GB, ext4 on RAID. No sound card. Its drives have power-loss protection, so its timings are a best case. |
| Long jobs | `~/m2-runs/tw <name> '<command>'` runs it in a window of tmux session `m2-sh`, which the owner watches; the log goes to `~/m2-runs/logs/`. It waits 9.5 minutes (`TW_WAIT=<seconds>` for longer), then prints `~/m2-runs/tw-wait <channel> <log>` to wait again. No double quotes or parentheses in the command. |
| Test plugins | `ci/plugins.sh` (once): Surge XT 1.3.4 and Airwindows Consolidated into `~/.local/share/m2-test-plugins/clap`, from the archives in `~/.local/share/m2-test-plugins/dl` (Airwindows' zip is gone upstream: keep that copy). Never into `~/.clap` or `/usr/lib/clap`. |
| The desktop app | Only on an invisible Xvfb screen, as the check's app row opens it. Never on a real screen. |
| Scratch `HOME` | Anything that runs music2 gets a scratch `HOME` and `M2_AUDIO=none`, and then `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Never the real `~/.music2` or `~/Music`. |
| Scratch copies | `git worktree add --detach ~/m2-runs/scratch-<label>`, with its own `target/`; delete it when done (they once filled 390 GB). The repo's own `target/` is about 140 GB. |
| Tools | Rust 1.99.0 (`rust-toolchain.toml`), Node 24, chrome-headless-shell 133.0.6943.141 (the check installs Rust and Chrome when missing), Xvfb. |
| MIDI | `/dev/snd/seq` gives `EACCES` here: tests inject notes and never open a device. |
