# Handoff

Written 2026-10-18. Read this first, then [docs/architecture.md](docs/architecture.md) (the contract), then the
design for whatever you pick up. Older versions of this page, with each track's full story, are in git.

## In short

- **Every planned step is built on Linux** (steps 1 to 9), and all of it is on `main` at `3c26fcd`, pushed.
- Each track was reviewed, its findings fixed or listed in [docs/gaps.md](docs/gaps.md), and `ci/check.sh --gates`
  passed on Linux with nothing passed only on a retry. The last one, track `ex` at `6d212a8`: 1,717 Rust tests,
  71/71 browser suites, 12 of 12 renders, the 7 gate runs, 33 minutes.
- **The finish check passed first time** (`ci/check.sh --gates` at `6dfb235`, branch `fa`: the code tidy and this docs pass): 1,717 Rust tests, 71/71 browser suites, 12 of 12 renders, the 7 gate runs, 35 minutes.
- **The transport works as in GarageBand** (landed `34dcfc4`): Space stops where the song is, and plays on from there; Return goes to bar 1. ⇧Space and Continue are gone. `ci/check.sh --gates` passed at `5707f46`: 1,718 Rust tests, 71/71 browser suites, 12 of 12 renders, the 7 gate runs, 37 minutes, nothing on a retry. Its first run caught the playhead moving a block after a stop; now a stop holds exactly there. By ear: mac-run step 28.
- **Track `bus` landed** (`444a2d0`, 2026-10-19): buses, sends and groups, in song
  code (RENDER-34, `@m2/core` 1.5.0, std 0.4.0) and in the Mix view (Add bus, a send knob per bus, an output
  picker). Gap #27 is closed. No sound changed. A song asking for std "0.3" needs `std = "0.4"` (below). Its
  `ci/check.sh --gates` passed at `7bb44f3`: 1,770 Rust tests, 74/74 browser suites, 12 of 12 renders, the 7 gate
  runs, 34 minutes. Notes: `docs/designs/step9/build-notes-bus.md`.
- **Draw mode (B) draws as GarageBand does** (landed `40685ca`): a press adds a note, a drag sets its length, ⌥-drag paints one a grid step. `ci/check.sh --gates` passed at `ce91acb`: 1,771 Rust tests, 74/74 browser suites, 12 of 12 renders, the 7 gate runs, 35 minutes, nothing on a retry. Its first run caught two old races, now fixed: a take started right after the one before on the same track failed ("File exists"), and the playhead lagged Follow by a frame. By ear: mac-run step 133.
- **The Mac ran everything at `e7518f4`** (2026-10-18): 11 of 13 rows. Two tests failed, fixed on branch `mf2`
  (`7482009`, `e505d0b`). G12 failed only because the output was at 44.1 kHz, not 48. G7f passed ab's layout by
  far. 8a's sweep is done. **One more `ci/check.sh` on the Mac is owed, with the output at 48 kHz.**
- **Track `br` landed** (2026-10-18, `054e338`): add an installed plugin from the app (#32), and presets for std
  sounds and plugins (#26, FMT-22, HOST-8). Linux green at `1fb6caa`; macOS owed (in the next Mac run). Build notes
  `docs/designs/step9/build-notes-br.md`.
- **Checks on Linux are faster, and check no less** (track `cs`): `ci/check.sh --gates=auto` skips the timing gates only when HEAD holds, in every file they read, what an earlier check on this machine passed them on, and the browser suites run 4 at a time; its own check passed at `b5e3a0f` with the gates (`ci/` changed): 1,771 Rust tests, 74/74 browser suites in 2.7 min, 12 of 12 renders, the 7 gate runs, 26 minutes, nothing on a retry. See "The check" below.
- **Ready to open up** (track `os`): MIT OR Apache-2.0, a fast pull request tier on GitHub (`ci/check.sh --pr` runs the same here), a nightly tier, the timing gates every night on a quiet machine (`ci/nightly.sh`), and the docs for contributors and agents (README, CONTRIBUTING, AGENTS, goals, SECURITY). `ci/check.sh --gates=auto` passed at `e405740` with the gates (`ci/` changed): its new lint, advisories and nightly list rows, 1,772 Rust tests, 74/74 browser suites, 12 of 12 renders, the 7 gate runs, 26 minutes, nothing on a retry; `ci/check.sh --pr` passed there too, cold and warm (341 s), nothing on a retry. The commits after it change only docs. The owner's GitHub steps: [docs/maintainers.md](docs/maintainers.md). See "The tiers" below.
- **Track `ag-steps`** (the agents design's track 1, branch `ag`, not merged): every change the app accepts is one undo step with an id and a source; a text file saved in another program is one, shown as it lands with a notice and a glow; undo never takes back your own save with someone else's change, and a step it can't take back can be skipped. `ci/check.sh --gates=auto` at `36b015d`: all but `advisories` passed (1,799 Rust tests, 75/75 browser suites, 12 of 12 renders, the 7 gate runs, 26 minutes, nothing on a retry). `advisories` fails on `main` too: RUSTSEC-2026-0386 (tokio 1.53.2, fixed in 1.53.3) needs a lock bump. Notes: `docs/designs/agents/build-notes-steps.md`.
- **Track `ag-fwd`** (the agents design's track 2, branch `ag-fwd`, not merged): with the song open, every `m2` write goes through the app as one undo step, the agent's, named by `--as` and `--why`; two keys (the page's, and the tools key in `session.json`, written whole); an agent's change to what you hold is refused, on arrival and again just before it is heard, and taken back if you began holding it as it landed (`m2` exits 3); `m2 edit`, `m2 write` and `m2 undo`. Notes: `docs/designs/agents/build-notes-fwd.md`.
- **The review's fixes for `ag-fwd`** (F1 to F12, on `ag-fwd`): an agent's change that would end a drag or a plugin window not saved yet is taken back, whatever file it touched; the tools key can't touch what you hold; plain names; bigger forwarded writes; and `m2 edit` and `m2 write` fix a song whose code doesn't build, in the app, as one undo step. `ci/check.sh --gates=auto` at `0d3cf13`: PASS, 1,859 Rust tests, 75/75 browser suites, 12 of 12 renders, the 7 gate runs, 27 minutes, nothing on a retry. Notes: the build notes' "The review's fixes".
- **Track `ag-see`** (the agents design's track 3, branch `ag-see`, not merged): an agent sees what you look at (`m2 focus`), follows everything that happens in the app (`m2 watch`) and points at things to show you where they are (`m2 point`), and none of it is saved; press V to hear before and after the newest change an agent or another program made, while agents wait. `ci/check.sh --pr` at `841b7ca`: PASS, 1,837 Rust tests, 54/54 browser suites, nothing on a retry. Notes: `docs/designs/agents/build-notes-see.md`.
- **The review's fixes for `ag-see`** (F1 to F11, on `ag-see`): Compare only plays: Before is built in memory and never written, so a quit or a crash on Before leaves the song as it was, and Esc on Before is one undo step, said with Redo; the pill shows only what the server says, one V at a time; while it is open an agent can't close or open a song; `m2 watch` follows one song, ends quietly, and keeps so much; every event says its session; a pointer never selects. `ci/check.sh --gates=auto` at `7177f17`: PASS, 1,891 Rust tests, 76/76 browser suites (3,288 checks), 12 of 12 renders, the 7 gate runs, 27 minutes, nothing on a retry. Notes: the build notes' "The review's fixes".
- **Track `ag-routes`** (the agents design's track 4, branch `ag-routes`, not merged): every route of the app's API is declared once, in one table the router, the read-only 423 and the tools key's 403 are made from; agents get 403 on your controls (play, record, solo, the view, plugin windows, exports, settings, songs); `m2 routes` lists the table and `m2 call` sends any route for agents, so every change the window can make is reachable from `m2`; an agent's set can't commit your drag, and a change taken back answers busy. `ci/check.sh --pr` at `e435c65`: PASS, 1,866 Rust tests, 54/54 browser suites, nothing on a retry. Notes: `docs/designs/agents/build-notes-routes.md`.
- **The review's fixes for `ag-routes`** (F1 to F10, on `ag-routes`): an agent's socket can't end your drags; an agent's import or Sampler file copies nothing while you hold the song, and a refused one leaves nothing (`m2 import` goes through the app's own import); every step is named by the agent's words; MIDI files never go into the song's folder; Look again is yours; each route carries its body limit, and no route outside the table answers. `ci/check.sh --gates=auto` at `31633f5`: PASS, 1,918 Rust tests, 76/76 browser suites (3,288 checks), 12 of 12 renders, the 7 gate runs, 27 minutes, nothing on a retry. Notes: the build notes' "The review's fixes".
- **Track `ag-listen`** (the agents design's track 5, the last, branch `ag-listen`, not merged): `m2 listen` measures what an agent can't hear, for the mix and each track (its own sound, and as it reaches the mix): loudness, true peak, crest, octave bands, stereo and masking; `--with` measures a change before it is made, `--as-heard` hears your listening mute and solo, `--png` draws the bands over time. It renders in its own process at background priority, its plugin host too, one at a time, and its copy taps change no bit. Gate G13 plays p50 while agents listen and edit beside it; its timings are recorded until its first Mac runs are, then judged. All five tracks of the design are built. `ci/check.sh --pr` at `ba42c67`: PASS, 1,899 Rust tests, 54/54 browser suites, 6 min, nothing on a retry; the 8 gate runs of `ci/gates.txt` pass on Linux. [build-notes-listen.md](docs/designs/agents/build-notes-listen.md).
- **The review's fixes for `ag-listen`** (F1 to F17, on `ag-listen`): the meters start warm, so a range that starts on a loud note reads no false true peak; masking is judged moment by moment among what the master adds up, a group bus as one, with how much of the range each pair holds; copy taps live only on an offline type the live engine can't take; mute and solo follow track names into a change, which compares the same bars; `m2 listen` refuses what it doesn't know, never waits long on a busy app, and names unsaved plugin windows; on Linux it runs in `SCHED_IDLE` (it still costs playback what one busy core does: gaps), and G13 listens to a song with plugins too. `ci/check.sh --gates=auto` at `bfe1a4c`: PASS, 1,957 Rust tests, 76/76 browser suites, 12 of 12 renders, the 8 gate runs, 29 minutes, nothing on a retry. Notes: the build notes' "The review's fixes".
- **Two hand-written pieces are crates now** (branch `deps`, not merged): `m2 watch`'s WebSocket client is `tungstenite`'s, which also checks `Sec-WebSocket-Accept` (that gap is closed), and `m2 listen --png` writes with the `png` crate, compressed (first-song's picture: 598 KB to 56 KB, the same pixels). Both were in `Cargo.lock` already: it gains four dependency lines, no crate and no version. `ci/check.sh --gates=auto` at `c928f5d`: PASS, 1,958 Rust tests, 76/76 browser suites, 12 of 12 renders, the 8 gate runs, 31 minutes, nothing on a retry.
- What the app does: [docs/manual.md](docs/manual.md). What is missing: [docs/gaps.md](docs/gaps.md). The checks by ear:
  [docs/mac-run.md](docs/mac-run.md).
- Next: that one Mac run (after `mf2` is merged), then fix what it finds. Then Windows, as its own step, or the top gaps.

## Mac runs owed

| # | Run | What it decides | Time |
|---|---|---|---|
| 1 | `ci/check.sh` on `main` | **Done at `e7518f4`** (2026-10-18), 11 of 13 rows. Two tests failed, fixed on `mf2`: a take's steps looked up by the folder's real path (`7482009`), and the plugin window test asks the host process that owns the window (`e505d0b`, the SIGBUS). G12 failed only its set-up (output at 44.1 kHz). `pl-arrange-fix`'s retry (on Linux too) was a real race, fixed on `mf2`: a marker's Delete or Rename now finds it by its bar. G7f passed one sandbox a box: a block −1,018 µs over S, a callback −2,748 µs, 76 MB against 554, build 28 ms against 205 | 71 min |
| 2 | (dropped) G7f store on the old gain | Not needed any more. It asked whether the Mac's +135 µs a block since step 7 was the 32 MiB store layout (O1). Track `ab` replaced that layout with one sandbox per box, so run 1's G7f rows now judge the new layout. The old std (ABI v0) can't load on `main` anyway. | — |
| 3 | 8a's Mac sweep, `ci/sweep.sh` | **Done at `e7518f4`.** It confirmed `live_count` 4 and `BETWEEN` = park (spin was far worse), so no code changed. Gate 8: 4 threads, 10 minutes, p99 1,308 µs (45%), 0 dropouts; provisional, at 44.1 kHz. Exports on 8 fast cores, 6.4× one thread | about 40 min |
| 4 | By ear, steps 7 to 133 | [docs/mac-run.md](docs/mac-run.md): step 7 (7–17), `sounds` (18–21), `taste-next` (22–31), `8b-ui` (32–38), `polish` (39–50), `next` (51–60), `studio` (61–72), `io` (73–82), `ab` (83–87), `ph` (88–95), `sx` (96–102), `pm` (103–110), `ex` (111–116), `br` (117–125), `bus` (126–132), `dn` (133) | about 2 hours, in parts |
| 5 | `ci/check.sh` on `main` (the same run as row 1's next one) | Also runs `br`'s plugin preset search and preset load on a Mac for the first time (HOST-8) | about 45 min |

**Next: one more `ci/check.sh` on the Mac, with the output at 48 kHz** (Audio MIDI Setup), on `main`. It checks the two fixes (the plugin window's `closed` callback now runs on the next main-queue turn: run on
a Mac for the first time) and judges G12 at its designed set-up. The sweep need not run again. The same run covers
`br`, `gb` and `bus` (`br`'s plugin presets touch the host process; `gb` and `bus` add no Mac-only code). Once
`ag-listen` is on `main`, it also records G13's first Mac run (its dropouts and callback times while agents listen
and edit beside p50): record them, then G13 is judged as G12 is (`crates/gates/src/realtime/live.rs`, `G13_RECORDED`).

After a green run the lead writes in the contract's PLAT-6 and the gates table: "macOS: green at <commit>", with
G7f's numbers. If a row fails, fix it on Linux, check the mac lint, and ask for one more run. The Mac's pasted runs
are kept in `$SCRATCH/mac-runs/` on the server (`check-and-sweep-e7518f4.txt` is the last).

## The owner's Mac command

**In short:** next time only paste 1, the check, in Terminal on the Mac itself, with the output at 48 kHz. Then paste
back what it says. (Paste 2, the sweep, is done.)

**1. The check** (about 45 minutes, mostly waiting):

| # | Do this | Why |
|---|---|---|
| 1 | Plug the Mac in, keep the lid open, and quit other apps. | The speed tests need a quiet Mac. |
| 2 | Open Terminal on the Mac itself, not over ssh. | One test needs your login session. |
| 3 | Type `cd`, a space and the path of your music2 folder (the one you cloned), then `git fetch`, then `git switch main`, then `git pull`. | This gets everything on `main`. |
| 4 | Type `ci/check.sh` and press Return. | It runs every check and the speed tests. |
| 5 | If one line says `running` for more than 30 minutes, press Ctrl-C once. | It stops that check and still prints the summary. |
| 6 | Copy the last lines, from `music2 check:` to the `logs` line, and paste them back. | That is all the lead needs. |

Before it, set the output to the built-in speakers at 48 kHz (Audio MIDI Setup): at another rate or buffer, G12
says "not the designed set-up" and what to set.

**2. 8a's Mac sweep** (about 40 minutes; it plays nothing aloud). Same set-up as the check (plugged in, lid open,
other apps quit, built-in speakers at 48 kHz):

```sh
cd <your music2 folder> && git fetch && git switch main && git pull && ci/sweep.sh
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
| G11 tx-data, 5,000 files in assets/ | p95 6.8 ms past its 5 ms | once, at load 4–10 while other tracks built (track `br`); the rerun passed at 1.6 ms |
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
| `bus-mix` | "with nothing sent, the Reverb's meter stays at the floor" | once, in a full check of `ag-see`, passed on its retry; 18 runs alone and four at a time pass |
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
   recording (#24, #25), audio clips (#18), the Sampler (#22),
   meters after the fader (#20), sections (#23), scale help (#16).
4. **Open owner decisions** (below): ask them, with the recommendation.
5. **Opening the repository to everyone:** the owner's one-time GitHub steps are in [docs/maintainers.md](docs/maintainers.md).

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
| What each step and track built, decided and left | `docs/designs/step9/` (`<track>-plan.md`, `build-notes-<track>.md` for `io`, `ab`, `ph`, `sx`, `pm`, `ex`, `br`, `bus`); `step8/` (taste, 8a, 8b, polish, next, macfix, studio); `step7/`, `sounds/`, `platform/`, `kernel-layout/`, `step6/` |
| Kernel ABI v1 | `docs/kernel-abi.md` |
| The look | `docs/designs/step8/taste.md` |
| Each check's logs and summary | `$SCRATCH/check-<label>/` and `.tgz` (step 9's finals: `check-io`, `-ab`, `-ph`, `-sx`, `-pm`, `-ex`, `-br`, `-bus`) |
| Pictures for the owner | `$SCRATCH/app-shots/<track>-<commit>/` |
| Logs of long jobs | `$SCRATCH/logs/` |
| The Mac's pasted runs | `$SCRATCH/mac-runs/` |
| Branches | Everything is on `main`. A track's branch and worktree are deleted once it lands. `origin/taste` and `origin/sounds` are old, all merged |

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
- **Worktrees:** each track works in its own, `$SCRATCH/wt/<name>` on branch `<name>`. Never touch
  the main checkout or another track's worktree. Never push from a track; the lead lands it.
- **Builds:** `--locked`, never `cargo update`. On a merge, take `main`'s `Cargo.lock` and let `cargo build` add the
  branch's edges; never take a side on `std.wasm`: rebuild it (`std/build.sh`). OS code outside `core/os` fails
  `core/os/tests/one_place.rs` (PLAT-2).
- **Commits:** plain words, ending with the Co-Authored-By and Claude-Session lines the session gives.

## The server

| What | How |
|---|---|
| `$SCRATCH` | The maintainer's scratch folder on the server, outside the repo: worktrees, logs, each check's folder, and the `tw` helper. Paths in this page and in `docs/designs/` use this name (set it in your own shell). |
| The machine | The maintainer's Linux server: Ubuntu 24.04, Ryzen 9 7950X3D (32 threads), 124 GB, ext4 on RAID. No sound card. Other agents' jobs often run beside yours: Linux timings are information only (PLAT-1). |
| Long jobs | Anything over 2 minutes: `$SCRATCH/tw <name> '<command>'` (a small helper kept outside the repo) runs it in a window of tmux session `m2-sh`, which the owner watches; the log goes to `$SCRATCH/logs/`. It waits 9.5 minutes (`TW_WAIT=<seconds>` for longer, `TW_WAIT=1` to wait), then prints `$SCRATCH/tw-wait <channel> <log>` to wait again. No double quotes or parentheses in the command. Never send keys to the tmux sessions `m2`, `1`, `2` or `ssh_tmux`. |
| Scratch `HOME` | Anything that runs music2 gets a scratch `HOME` under `$SCRATCH`, `M2_AUDIO=none` and `M2_MIDI=none`, and then `CARGO_HOME` and `RUSTUP_HOME` set to your real home's `.cargo` and `.rustup`. Never the real `~/.music2` or `~/Music`. `ci/check.sh` does this itself. |
| Disk | Scratch `target/` folders once filled 390 GB: share or clean them after each track. Never move a `target/` between worktrees uncleaned (it keeps the old paths). |
| Test plugins | `ci/plugins.sh` (once): Surge XT 1.3.4 and Airwindows Consolidated into `~/.local/share/m2-test-plugins/clap`, from the archives in `~/.local/share/m2-test-plugins/dl` (Airwindows' zip is gone upstream: keep that copy). Never into `~/.clap` or `/usr/lib/clap`. Never delete anything under `$SCRATCH/plugins`. |
| The desktop app | Only on an invisible Xvfb screen, as the check's app row opens it. Never on a real screen. |
| Tools | Rust 1.99.0 (`rust-toolchain.toml`), Node 24, chrome-headless-shell 133.0.6943.141 (the check installs Rust and Chrome when missing), Xvfb. |
| MIDI | `/dev/snd/seq` gives `EACCES` here: tests inject notes and never open a device. |

## The tiers: what runs where

Every check runs from a script in `ci/`, so whatever CI runs, you run with the same command.

| Tier | Where and when | The command | What |
|---|---|---|---|
| Pull request | GitHub, on every pull request, merge-queue group and push to `main` (`.github/workflows/ci.yml`). One required check: `ci-ok`. | `ci/check.sh --pr` (all of it, one step after another); `ci/pr.sh <step>` (one job) | fmt, clippy, the Rust tests in 3 parts (cargo-nextest), the pool's models, the web build, std's kernels and `std.wasm`, the browser suites in 2 parts, the 12 renders, licences, actionlint, the checks' own cases. RustSec's advisories only when a `Cargo.lock` or `deny.toml` changes. A docs-only change runs the docs job alone. |
| Nightly | GitHub, every day on `main`, only once the repo is public (`.github/workflows/nightly.yml`) | `ci/check.sh --nightly` | the full check without the timing gates (so every PR-tier check and the advisories too), the browser suites one at a time, the tests tagged nightly; and a Mac: the Rust tests, the pool's models, the browser suites, the renders. A failed or timed-out job opens or comments on one issue, "Nightly failed". |
| Timing gates | a quiet maintainer machine, every night, by cron | `ci/nightly.sh --report` | `ci/check.sh --gates` on a fresh clone of `origin/main`; posts the status `nightly/gates`. It runs as an OS user of its own, with a fine-grained token that can only write commit statuses (docs/maintainers.md, part 5). Not set up yet. |
| Full check | your machine; the server with `--gates=auto` | `ci/check.sh` | everything, below: every check of the other tiers. |

- **Which tier a test is in.** A Rust test that judges speed or is a known flake is listed in `ci/nightly-tests.txt`, with why; a browser suite gets `tier: "nightly"` and a `why` in `e2e/run.mjs`. Unchanged either way: it runs every night.
- **Times** (`ci/check.sh --pr` in a clean clone on 8 threads of the server, at `2141096` and `d4681ab`): about 10 minutes cold (607 s), 7.5 warm (447 and 451 s), with music2's own crates rebuilt. On GitHub's 4-core runner each job is one part of that; its real times are still to be measured (gaps.md).
- **What the first real runs found.** A test that hosts Surge XT or Airwindows in this process could pass, then crash as its process exited: a destroy is queued to the plugin main thread, and the exit ran the plugin library's destructors under it. nextest runs each test in a process of its own, so it showed at once (one test crashed 40 runs of 40 alone). Now `test_main_thread()` drains that thread when the test's thread ends (`d4681ab`).
- **Verdicts.** `--pr` says PASS only when CI would pass. A docs-only change needs no plugins. `M2_NO_PLUGINS=1` skips the plugin tests and says PART. A browser suite that was PART fails it, even when another run failed and then passed alone. "What failed" names a Rust test whose process crashed (nextest's `SEGV`). The cases are in `ci/pr-test.sh` and `ci/nightly-test.sh`, which the lint step runs (in quick, and in the full check).

## The check: `ci/check.sh`

One script checks everything, on the server and on the Mac ([PLAT-6](docs/architecture.md#platforms)). Its first
line says `music2 check: PASS`, `FAIL` or `PART`. PART means no row failed but the gates couldn't judge G7f (the Mac
isn't the one S's records name). Only PASS is a pass.

Rows: gates rule (`--gates=auto`'s rule tried on its 51 cases), toolchain, lint (`ci/pr.sh lint`: fmt, actionlint,
licences, the CI scripts' own cases), advisories (`ci/pr.sh advisories`: RustSec's), web build, build, app (Linux: the
desktop app on Xvfb with `first-song` and `with-plugins`, a picture of each), rust tests, nightly list (Linux: every
test `ci/nightly-tests.txt` names is there), clippy (a warning is an error), pool models, std kernels, `std.wasm` (rebuilt to the same bytes elsewhere), mac lint (Linux: the Mac-only code
type-checked), browser suites, renders (against `e2e/renders.sha256`, one record for every OS), old engine S (with
the gates), gates (the 7 runs `ci/gates.txt` names), repo unchanged.

- Without the test plugins it says FAIL. Edit no tracked file while it runs.

**When the gates run.** On a Mac: always (`--no-gates` leaves them out). On Linux: with `--gates`, always; with no
option, never; with `--gates=auto`, unless git proves that HEAD holds, in every file the gates read or build from,
exactly what a check on this machine already passed them on. The rule (`ci/gates-auto.sh`) is mechanical:

- The change is every file `git diff --no-renames` names between `git merge-base HEAD main` (or `--base <rev>`) and
  HEAD. Every file on it must be under `web/`, `docs/`, `README.md` or `HANDOFF.md`, which m2-gates never reads or
  builds from (checked by hand, in its build's dep-info files, and by tracing every file the S build and the 7 runs
  touch: the top of `ci/gates-auto.sh`). `e2e/` isn't one: m2-gates reads `e2e/renders.sha256`.
- The list isn't empty (not main itself), and HEAD holds no symlink.
- The tree is clean: nothing uncommitted or untracked, no change git hides (skip-worktree, assume-unchanged), and no
  ignored file where the gates read (build output in `target/`, `e2e/node_modules/` and std's kernels' `target/` is
  fine).
- **A recorded pass on the same files.** Every Linux check whose gates row passes, on a clean tree, adds a hash of
  every file the gates can read to `~/.cache/m2-check/gates-passed` (the gates row says "recorded in ..."). The skip
  needs HEAD's hash there. The merge base alone proves nothing: main fast-forwards through a track's every commit, so
  the base can be a mid-track commit whose own check failed, or a parallel track's commit that landed later with its
  fix.

Anything else runs them, and the check prints its decision at the start. A skip is the row `gates SKIP only web/,
docs/ changed since <base> (N files); the gates passed at <commit> (<date>) on the same files they read`, and the
first line says "without running the gates"; it is never a PASS. The rule doesn't see what the gates take from
outside the repo (`RUSTFLAGS`, a `.cargo/config.toml` above it): gaps.md.

**The browser suites** run several at a time on Linux (`e2e/run.mjs all --jobs N`, N a quarter of the threads, 1
to 4: 4 on the server; each suite still has its own server,
`HOME`, folder and browser; what they could share was checked, see `e2e/README.md`, "In parallel"), then each failed
run once more alone, as before. On a Mac they run one at a time, as before (`M2_CHECK_JOBS=4` tries 4 there).

Times on the server, warm `target/`:

| part | before | now |
|---|---|---|
| browser suites | 10.5 to 11 min, one at a time | 2.7 min, 4 at a time |
| timing gates, with the S build | 16 min | the same when they run; none when `--gates=auto` skips them |
| rust tests | 6 min | the same |
| the whole check, with the gates | 34 to 40 min | 26 to 27 min (`b5e3a0f`, `e4a6183`) |
| the whole check, gates skipped | (no such run) | 10 min (`2bd6456`, `--base e4a6183`: only `HANDOFF.md` changed; timed under the first rule, which kept no record) |

On the server, a track's check (with a warm `target/`):

```sh
TW_WAIT=3500 $SCRATCH/tw check-<label> 'M2_CHECK_OUT=$SCRATCH/check-<label> ci/check.sh --gates=auto'
```

`--gates` instead runs the gates whatever changed.

Run it from the worktree being checked. `M2_CHECK_OUT` must be a new folder. It gets `summary.txt`, every log, and
the app's two pictures; all logs are also packed in `<folder>.tgz`.
