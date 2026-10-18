# Handoff

Written 2026-10-18. Read this first, then [docs/architecture.md](docs/architecture.md) (the contract), then the
design for whatever you pick up. Older versions of this page, with each track's full story, are in git.

## In short

- **Every planned step is built on Linux** (steps 1 to 9), and all of it is on `main` at `3c26fcd`, pushed.
- Each track was reviewed, its findings fixed or listed in [docs/gaps.md](docs/gaps.md), and `ci/check.sh --gates`
  passed on Linux with nothing passed only on a retry. The last one, track `ex` at `6d212a8`: 1,717 Rust tests,
  71/71 browser suites, 12 of 12 renders, the 7 gate runs, 33 minutes.
- **The finish check passed first time** (`ci/check.sh --gates` at `6dfb235`, branch `fa`: the code tidy and this docs pass): 1,717 Rust tests, 71/71 browser suites, 12 of 12 renders, the 7 gate runs, 35 minutes.
- **Track `bus` is ready to land** (branch `bus`, `main` at `c852cb3` merged in): buses, sends and groups, in song
  code (RENDER-34, `@m2/core` 1.5.0, std 0.4.0) and in the Mix view (Add bus, a send knob per bus, an output
  picker). Gap #27 is closed. No sound changed. A song asking for std "0.3" needs `std = "0.4"` (below). Its
  `ci/check.sh --gates` passed at `9c985c3`: 1,751 Rust tests, 72/72 browser suites, 12 of 12 renders, the 7 gate
  runs, 36 minutes. Notes: `docs/designs/step9/build-notes-bus.md`.
- **The Mac ran everything at `e7518f4`** (2026-10-18): 11 of 13 rows. Two tests failed, fixed on branch `mf2`
  (`7482009`, `e505d0b`). G12 failed only because the output was at 44.1 kHz, not 48. G7f passed ab's layout by
  far. 8a's sweep is done. **One more `ci/check.sh` on the Mac is owed, with the output at 48 kHz.**
- What the app does: [README.md](README.md). What is missing: [docs/gaps.md](docs/gaps.md). The checks by ear:
  [docs/mac-run.md](docs/mac-run.md).
- Next: that one Mac run (after `mf2` is merged), then fix what it finds. Then Windows, as its own step, or the top gaps.

## Mac runs owed

| # | Run | What it decides | Time |
|---|---|---|---|
| 1 | `ci/check.sh` on `main` | **Done at `e7518f4`** (2026-10-18), 11 of 13 rows. Two tests failed, fixed on `mf2`: a take's steps looked up by the folder's real path (`7482009`), and the plugin window test asks the host process that owns the window (`e505d0b`, the SIGBUS). G12 failed only its set-up (output at 44.1 kHz). `pl-arrange-fix`'s retry (on Linux too) was a real race, fixed on `mf2`: a marker's Delete or Rename now finds it by its bar. G7f passed one sandbox a box: a block −1,018 µs over S, a callback −2,748 µs, 76 MB against 554, build 28 ms against 205 | 71 min |
| 2 | (dropped) G7f store on the old gain | Not needed any more. It asked whether the Mac's +135 µs a block since step 7 was the 32 MiB store layout (O1). Track `ab` replaced that layout with one sandbox per box, so run 1's G7f rows now judge the new layout. The old std (ABI v0) can't load on `main` anyway. | — |
| 3 | 8a's Mac sweep, `ci/sweep.sh` | **Done at `e7518f4`.** It confirmed `live_count` 4 and `BETWEEN` = park (spin was far worse), so no code changed. Gate 8: 4 threads, 10 minutes, p99 1,308 µs (45%), 0 dropouts; provisional, at 44.1 kHz. Exports on 8 fast cores, 6.4× one thread | about 40 min |
| 4 | By ear, steps 7 to 123 | [docs/mac-run.md](docs/mac-run.md): step 7 (7–17), `sounds` (18–21), `taste-next` (22–31), `8b-ui` (32–38), `polish` (39–50), `next` (51–60), `studio` (61–72), `io` (73–82), `ab` (83–87), `ph` (88–95), `sx` (96–102), `pm` (103–110), `ex` (111–116), `bus` (117–123) | about 2 hours, in parts |

**Next: one more `ci/check.sh` on the Mac, with the output at 48 kHz** (Audio MIDI Setup), on `main` once `mf2` is
merged. It checks the two fixes (the plugin window's `closed` callback now runs on the next main-queue turn: run on
a Mac for the first time) and judges G12 at its designed set-up. The sweep need not run again. Once `bus` has
landed, the same run covers it too: it adds no Mac-only code. Its by-ear steps are 117 to 123.

After a green run the lead writes in the contract's PLAT-6 and the gates table: "macOS: green at <commit>", with
G7f's numbers. If a row fails, fix it on Linux, check the mac lint, and ask for one more run. The Mac's pasted runs
are kept in `~/m2-runs/mac-runs/` (`check-and-sweep-e7518f4.txt` is the last).

## The owner's Mac command

**In short:** next time only paste 1, the check, in Terminal on the Mac itself, with the output at 48 kHz. Then paste
back what it says. (Paste 2, the sweep, is done.)

**1. The check** (about 45 minutes, mostly waiting):

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. |
| 2 | Open Terminal on the Mac itself, not over ssh. | One test needs your login session. |
| 3 | Type `cd ~/dev/suite2/music2`, then `git fetch`, then `git switch main`, then `git pull`. | This gets everything on `main`. |
| 4 | Type `ci/check.sh` and press Return. | It runs every check and the speed tests. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines, from `music2 check:` to the `logs` line, and paste them back. | That is all the lead needs. |

Before it, set the output to the built-in speakers at 48 kHz (Audio MIDI Setup): at another rate or buffer, G12
says "not the designed set-up" and what to set.

**2. 8a's Mac sweep** (about 40 minutes; it plays nothing aloud). Same set-up as the check (plugged in, lid open,
other apps quit, built-in speakers at 48 kHz):

```sh
cd ~/dev/suite2/music2 && git fetch && git switch main && git pull && ci/sweep.sh
```

Paste back what is in `~/m2-8a.txt`. Its steps: the pool's models (a FAIL: stop, fix on Linux first), `m2-os`'s
real-time tests (a FAIL: fix that code before anything live is judged), `sysctl`, the offline sweep, the live sweep
(the fewest threads whose p99 is within 5% of the best, with no dropout; spin if its p99 is at least 10% lower), and
gate 8 for 10 minutes. The full table: `docs/designs/step8/build-notes-8a.md`, "The Mac command".

**What must pass in the check:** the first line says `music2 check: PASS`; every row says PASS (the gates row too,
not PART); nothing "passed only on a retry". In the gates part: G11; G5 and G7a through the server, heard within
200 ms of the request; G5 and G7b on the engine rig, with G7a's click check; G7e (the export pauses, borrows both
plugins, comes back with no click, the held chord sounds again); G7f store (block within S + 80 µs by the mean,
footprint within S + 64 MB, build within 1.5 × S, the 25% rule, the same music); G7f arena (callback within S's +
320 µs by the mean, the same music, the slowest activation block under 5,333 µs); G12's minute.

**What may fail, and why:**
- G7f, near its bound: ask for one more run. G7f's own error is about ±150 µs a callback from run to run (the A/A
  table in `crates/gates/s/README.md`).
- PART, or an `info machines:` line: the Mac's name differs from the records written by hand. Copy what follows
  `machine: ` over the `machine=` value of the three record lines (`store-record` and `store-p1` in
  `crates/gates/store-S.txt`, `callback-record` in `callback-S.txt`), and ask for one more run.
- Anything else that fails is new.

**What the lead reads in the pasted lines:** the `machine:` line names `MacBookPro18,1, macOS …, Apple M1 Pro,
aarch64, 10 threads`; G7f's two "the same music" lines say ok; "S as recorded" within about 1% (90 µs) of 8,936 µs
a callback; the "starting a turn" lines above −100 µs a callback (−30 a block), else a pass near the bound is unsure.
G7f's S plays its own std, as S can't play the app's.

## Known flakes

Seen, not fixed. Linux timings are information only (PLAT-1): the gates are judged on the Mac.

| Test | What fails | Note |
|---|---|---|
| G7f arena's slowest activation block | over 5,333 µs under load | can fail the gates row here beside other agents |
| G3 (a note edit within compile + 20 ms) | p95 16–22 ms at n=10 | near its line under load |
| G5 and G7a through the server | p95 past its bound | twice, at high load while other tracks built; alone they pass at about 111 ms |
| `taste` | the Bass header's click didn't select it within 30 s | once, three suites at once beside other agents |
| `live3b` | "its release writes db(-32) into song.ts" | once, at load 45–74, before step 7 |
| `s5-knobs` | page detached after a double-click on a knob at its default | from before the platform step |
| `audition_never_waits_for_a_compile`, `midi_never_waits_for_a_compile` | "the compile's span" not found; a subtract overflow | once each, at load 9–10; alone 3 of 3 pass |
| `s8-clips` | "the left edge dragged a bar right: 8+28" | once, beside other tracks' builds; passed on its retry |
| `st-audio-fix` | "and adds nothing": the revision moved after an MP3 was refused | once, passed on its retry |
| `nx-effects-fix` | "and says why: Move left (off)": the tooltip read before it changed | once, passed on its retry |
| `st-errors` | waited over 30 s for the Needs attention row of a missing sound file | once, at load 9; alone it passes |
| `a_second_session_is_read_only` (`core/session`) | its `refresh` after the lock came back | once, under load; alone 3 of 3 pass |
| `m2-os`'s two `fake_sysfs` tests | they share one temp folder | run together they sometimes fail; alone they pass |
| `a_slow_disk_misses_but_never_waits` | failed once on the Mac at `e7518f4` | passed on its retry |
| `s8-mute-export` | failed once on the Mac at `e7518f4` | passed on its retry |

A test or suite that passed only on its retry is listed by `ci/check.sh`. The lead decides whether it is one of
these or a real failure, and fixes the real ones (most were a read a moment too soon).

## What comes next

1. **The Mac's one more `ci/check.sh`**, at 48 kHz (above), when the owner has time. Fix on Linux what it finds.
2. **Windows, as its own step** ([PLAT-1](docs/architecture.md#platforms), the contract's Deferred row):
   `core/os/src/windows/`, the Windows twin of every piece in `m2-os`, the tests' `cfg(unix)` fixes, and the build
   (MSVC, QuickJS under `cl.exe`, `llvm-rc` for the desktop app). Platform design §8 lists the Unix-only spots.
   Known already: `push_tree` opens files read-only, but `FlushFileBuffers` needs write access.
3. **The top gaps**, if the owner asks for features ([docs/gaps.md](docs/gaps.md), most visible first): the rest of
   recording (#24, #25), audio clips (#18), adding a plugin from the app (#32), the Sampler (#22), presets (#26),
   meters after the fader (#20), sections (#23), scale help (#16).
4. **Open owner decisions** (below): ask them, with the recommendation.

The owner wants the app finished clean, with tokens conserved: no new features unless asked, no polish passes.

## Open owner decisions (ask, with a recommendation)

| Question | What was built | Recommendation |
|---|---|---|
| G7f: O2 named a p99 rule. | The mean of 5 pairs is judged; the p99's estimate is printed beside it. The p99 moved by hundreds of µs from pair to pair, the mean by about 12. | Keep the mean. |
| A plugin that keeps failing. | Started again once. If the new one fails within 10 s, it stays silent until the song is opened again, and you are told once. | Keep 10 s. |
| A plugin stuck for good. | After 30 s (2 minutes in a scan) music2 ends the plugin program and says so (track `ph`). | Keep 30 s; the Mac run may move it. |
| The output device changes while playing (headphones in or out). | Playback stops, and a message says "Press Play to go on". | Keep it: music shouldn't jump to the speakers on its own. |
| A songs folder reached through a link. | Settings and the start screen show it at its real place (`/Volumes/Big/Music/music2`). | Keep it. |
| Space, Return, Space: should the wobble and the drums' noise start again as in the export? | Not now: they carry on from where they stopped, as at any jump. Changing it changes no export. | Keep it. Ask once: "After you stop and go back to bar 1, the wobble carries on from where it was; should it start again as in the export?" |

Decided, and still in force: step 7's O2 to O5 (G7f's bounds, a note held across an export plays again, a new node's
inputs fade in); O1's 32 MiB layout was replaced in track `ab` by one sandbox a box (idea 3), which measured better
on every count ([RENDER-32](docs/architecture.md#engine-shape-step-7)); std's version moves with a render change,
one std bundled, the lockfile waits for its trigger ([FMT-3](docs/architecture.md#manifest-and-versions)); gate 8 on
the M1 Pro is provisional until a base M1 exists; no hosted Mac runners; the desktop app may open here only on an
invisible Xvfb screen, with pictures sent to the owner.

## Your own songs (for the owner)

**In short:** a song you made before std 0.4 needs one line changed. Then it plays as before, or very nearly.

| | |
|---|---|
| Which songs | Songs in `~/Music/music2` that ask for std "0.1", "0.2" or "0.3". |
| What you see | The song opens but stays silent. The yellow bar at the top says the code doesn't compile: "This app has std 0.4. To use it, change the std line in music2.toml to std = "0.4". The song may sound a little different." |
| What to do | Open the song's `music2.toml` in a text editor. Change the std line to `std = "0.4"`. Save. The app takes the change in. |
| The sound | A "0.3" song sounds exactly the same: std 0.4 only adds buses. A "0.1" or "0.2" song: nearly the same. The Lead, 808 sub and Synth bass now hear each note's own velocity. Notes you play on a keyboard sound exactly as before. |

## Where things are

| What | Where |
|---|---|
| The contract, and the order of work | `docs/architecture.md` |
| What each step and track built, decided and left | `docs/designs/step9/` (`<track>-plan.md`, `build-notes-<track>.md` for `io`, `ab`, `ph`, `sx`, `pm`, `ex`, `bus`); `step8/` (taste, 8a, 8b, polish, next, macfix, studio); `step7/`, `sounds/`, `platform/`, `kernel-layout/`, `step6/` |
| Kernel ABI v1 | `docs/kernel-abi.md` |
| The look | `docs/designs/step8/taste.md` |
| Each check's logs and summary | `~/m2-runs/check-<label>/` and `.tgz` (step 9's finals: `check-io`, `-ab`, `-ph`, `-sx`, `-pm`, `-ex`, `-bus`) |
| Pictures for the owner | `~/m2-runs/app-shots/<track>-<commit>/` |
| Logs of long jobs | `~/m2-runs/logs/` |
| The Mac's pasted runs | `~/m2-runs/mac-runs/` |
| Branches | Everything is on `main` but `bus` (ready to land, worktree `~/m2-runs/wt/bus`). A track's branch and worktree are deleted once it lands. `origin/taste` and `origin/sounds` are old, all merged |

Engine and tooling items left by steps 7 and 8 that are not user gaps are in each build notes' "Left" or "For the
lead" sections. The ones a next session most needs:
- The live thread count and park or spin wait for the Mac sweep (row 3): one line each in `core/play/src/engines.rs`.
- G12 isn't built with `--server`, `--export`, `--like-m1` or `--capture resample`.
- The recorded tests aren't in `ci/check.sh`: `cargo test -p m2-play --release --test bit_identity --test live
  --test churn -- --ignored --nocapture` (about 10 minutes).
- Every load of std compiles `std.wasm` (+70 ms a load); a compiled-module cache would win it back.
- K13: a note in the very block after a state change doesn't sound in Surge XT. K14: a plugin that applies a loaded
  state late can still win over the code's values.
- The speed work's cuts 3 and 4 are held (the owner's call, only if the Mac misses G7f).
- Contract *Today* lines that cite old line numbers: RENDER-15, 21, 30, ID-3, ID-12, FMT-20, PKG-4, 8, 10, 11,
  MON-1, ABI-2 and 3. Platform design §3's OS-82, 83 and 84 still say "Designed"; they are built.

## How the work is done

- **Writing to the owner:** they are dyslexic and new to music. "In short" first, short sentences, plain words,
  tables, no walls of text.
- **Pace** (since 2026-10-17): tracks in parallel, one full check per track, quality kept. For each track: plan,
  attack, build in green slices, review, fix (each with a test shown red first), `ci/check.sh --gates`, land.
- **Simple yet elegant**, a tasteful professional DAW. No built-in AI in the app. No backwards compatibility.
- **Worktrees:** each track works in its own, `~/m2-runs/wt/<name>` on branch `<name>`. Never touch
  `~/dev/suite2/music2` or another track's worktree. Never push from a track; the lead lands it.
- **Builds:** `--locked`, never `cargo update`. On a merge, take `main`'s `Cargo.lock` and let `cargo build` add the
  branch's edges; never take a side on `std.wasm`: rebuild it (`std/build.sh`). OS code outside `core/os` fails
  `core/os/tests/one_place.rs` (PLAT-2).
- **Commits:** plain words, ending with the Co-Authored-By and Claude-Session lines the session gives.

## The server

| What | How |
|---|---|
| The machine | `jerry-ax102`: Ubuntu 24.04, Ryzen 9 7950X3D (32 threads), 124 GB, ext4 on RAID. No sound card. Other agents' jobs often run beside yours: Linux timings are information only (PLAT-1). |
| Long jobs | Anything over 2 minutes: `~/m2-runs/tw <name> '<command>'` runs it in a window of tmux session `m2-sh`, which the owner watches; the log goes to `~/m2-runs/logs/`. It waits 9.5 minutes (`TW_WAIT=<seconds>` for longer, `TW_WAIT=1` to wait), then prints `~/m2-runs/tw-wait <channel> <log>` to wait again. No double quotes or parentheses in the command. Never send keys to the tmux sessions `m2`, `1`, `2` or `ssh_tmux`. |
| Scratch `HOME` | Anything that runs music2 gets a scratch `HOME` under `~/m2-runs/`, `M2_AUDIO=none` and `M2_MIDI=none`, and then `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Never the real `~/.music2` or `~/Music`. `ci/check.sh` does this itself. |
| Disk | Scratch `target/` folders once filled 390 GB: share or clean them after each track. Never move a `target/` between worktrees uncleaned (it keeps the old paths). |
| Test plugins | `ci/plugins.sh` (once): Surge XT 1.3.4 and Airwindows Consolidated into `~/.local/share/m2-test-plugins/clap`, from the archives in `~/.local/share/m2-test-plugins/dl` (Airwindows' zip is gone upstream: keep that copy). Never into `~/.clap` or `/usr/lib/clap`. Never delete anything under `~/m2-runs/plugins`. |
| The desktop app | Only on an invisible Xvfb screen, as the check's app row opens it. Never on a real screen. |
| Tools | Rust 1.99.0 (`rust-toolchain.toml`), Node 24, chrome-headless-shell 133.0.6943.141 (the check installs Rust and Chrome when missing), Xvfb. |
| MIDI | `/dev/snd/seq` gives `EACCES` here: tests inject notes and never open a device. |

## The check: `ci/check.sh`

One script checks everything, on the server and on the Mac ([PLAT-6](docs/architecture.md#platforms)). Its first
line says `music2 check: PASS`, `FAIL` or `PART`. PART means no row failed but the gates couldn't judge G7f (the Mac
isn't the one S's records name). Only PASS is a pass.

Rows: toolchain, web build, build, app (Linux: the desktop app on Xvfb with `first-song` and `with-plugins`, a
picture of each), rust tests, clippy (a warning is an error), pool models, std kernels, `std.wasm` (rebuilt to the
same bytes elsewhere), mac lint (Linux: the Mac-only code type-checked), browser suites, renders (against
`e2e/renders.sha256`, one record for every OS), old engine S (with the gates), gates (the 7 runs `ci/gates.txt`
names; on Linux only with `--gates`), repo unchanged.

- Without the test plugins it says FAIL. Edit no tracked file while it runs.

On the server (about 35 minutes with `--gates` and a warm `target/`):

```sh
TW_WAIT=3500 ~/m2-runs/tw check-<label> 'M2_CHECK_OUT=$HOME/m2-runs/check-<label> ci/check.sh --gates'
```

Run it from the worktree being checked. `M2_CHECK_OUT` must be a new folder. It gets `summary.txt`, every log, and
the app's two pictures; all logs are also packed in `<folder>.tgz`.
