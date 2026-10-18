# Track ex: build notes

Branch `ex`, from `main` at `83fa4e4`, 2026-10-18. The plan is [ex-plan.md](ex-plan.md); what changed while building is written there, under each slice ("As built").

## In short

- **Ready to land** (2026-10-18). Linux green at `6d212a8` (`ci/check.sh --gates`); macOS owed. `main` at `98e1b09` (track `pm`) is merged in.
- **One path for faults.** A part that breaks while sound is made puts a small record (24 bytes, numbers only) in one **fault ring**. The ring never allocates, locks or waits, so the audio thread uses it too.
- **The session names each fault** and lists it as a problem in the status pill's popover, with its track. It is toasted once. It goes when the part plays again.
- **Offline** (an export, `m2 render`, `m2 bounce`), any fault that changed the sound stops the render. The error names the part and the code. No file is written with a part silent.
- **The sound input** reports through the same path: an input chosen but not there, an input unplugged during a take, and the Mac refusing the microphone during a take.
- No sound changed: `e2e/renders.sha256` keeps all 12 lines.
- Linux timings are information only. The Mac's run is owed (PLAT-1).

| Slice | You get |
|---|---|
| 1. The fault ring | sounds and plugins that stop are told through it, as problems with codes |
| 2. Every fault, and offline fails | unreadable files, files that won't decode and failed stretches are told; an export with any fault fails and names it |
| 3. Inputs, and closing | the sound input's three problems; the contract, gaps and this note |

## The codes (DIAG-2)

| Code | Severity | When |
|---|---|---|
| `sound.stopped` | Error | a kernel or a voice trapped, or ran out of memory |
| `plugin.failed` | Error | a plugin reported an error, or didn't come back after an export |
| `plugin.crashed` | Error | the plugin host process crashed or hung (kept, now through the ring) |
| `asset.unreadable` | Error | a file is there but can't be read or decoded while it plays |
| `stretch.failed` | Error | a warped clip's stretch failed |
| `input.missing` | Warning | the input chosen in Settings isn't there; the take records from another, named |
| `input.lost` | Warning | the input was unplugged during a take; the take is kept up to there |
| `input.refused` | Error | the OS refuses the microphone during a take; the take is only silence, so it isn't kept |

## Slice 3, piece by piece

| Piece | Where | Test (each red first) |
|---|---|---|
| Three codes and `Subject::Input(n)`: an input's opening, of no plan | `core/runtime/src/fault.rs` | the three below |
| The player holds the ring (`report_to`). `open_input` reports `input.missing`, opens again each take while the one chosen is missing. `input_refused()`: the OS's answer or the seam's `refuse_input`. The input thread no longer logs "gone" | `core/play/src/inputs.rs` | `a_chosen_input_that_isnt_there_is_told_naming_the_one_used` |
| The take thread asks `input_refused()` each tick: the take is dropped, its part file goes, `input.refused` is reported. An unplug reports `input.lost`; the take's own note for it went | `crates/server/src/graph/record.rs` | `a_take_the_os_refuses_is_dropped_and_said`, `an_input_unplugged_mid_take_is_one_problem` |
| `InputNamer`, one per opening: the words, listed until the next take | `crates/server/src/graph/faults.rs` | the same |
| The inject route gains `refuse` | `crates/server/src/routes_record.rs` | `the_inject_api_unplugs_the_input` (now with `refuse`) |
| Settings' input choice carries its name | `crates/server/src/settings.rs` | kept test |
| The browser suite `ex-faults`: the popover lists a stopped sound, naming Hats; one toast | `e2e/ex-faults.mjs` (10 checks) | red with the session's list stubbed: 3 of 10 |

How each was shown red: the three Rust tests with the change stubbed (`input_refused` false, no `input.missing` or `input.lost` report): all 3 failed, then passed. `ex-faults` with the session's list stubbed (as before slice 1: a toast, no problem): no pill, 3 of 10.

## The words

| Code | What the person reads |
|---|---|
| `input.missing` | "Mic C isn't plugged in, so this take records from Mic A. To record from another input, choose it in Settings ▸ Sound input." |
| `input.lost` | "Mic A was unplugged, so the take ended there. It was kept up to then. The song plays on." |
| `input.refused` | "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone. The take was only silence, so it wasn't kept." |

## Pictures

From the desktop app on the invisible screen `:96`, first-song on a copy of std whose noise kernel traps (`~/m2-runs/app-shots/ex-9167e95/`):

| File | Shows |
|---|---|
| `pill-and-hats-mark.png` | playing: the pill says "1 problem", Hats' header has a red mark |
| `popover-stopped-sound.png` | the pill clicked: "The instrument on Hats stopped working, so music2 made it silent. The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound." |

After the merge of `main` (`~/m2-runs/app-shots/ex-01c5dbc/`, same setup):

| File | Shows |
|---|---|
| `popover-after-merge.png` | the same popover and Hats' red mark, with pm's "Plays up to 2 notes at once." on Kick's card |

The review's fix (r2) has no picture: it is something that no longer shows. Its test proves it.

## Renders

12 of 12 match `e2e/renders.sha256` after each slice. No fault happens in them.

## Speed (Linux, information only)

Not measured: how long making a `Namer` takes per plan. It clones one shared pointer and walks the nodes once to name the plugins.

## The review

It found 5 things, all small.

| # | What it found | What was done |
|---|---|---|
| r1 | a stopped sound or plugin may go untold when two plans are sent within one 300 ms read | a line in `docs/gaps.md` |
| r2 | another song opened without closing kept the last song's input problems listed | fixed (`bd4ab45`): `Told::sent` takes every listed problem away when the session changes. Test `another_song_lists_none_of_the_last_songs_problems`: red on the old code, then green |
| r3 | a plugin that fails a second time is left silent with no new message | a line in `docs/gaps.md` |
| r4 | `fault::Once` was dead code | deleted (`9569f8d`), with its test and the plan's mentions |
| r5 | `m2` loads the test plugin `probe` when `M2_PROBE` is set, in the shipped program too | a line in `docs/gaps.md` |

Contract settled: an input's problem is listed until the next take, **or until another song opens** (RENDER-30a, the three `input.*` rows).

## The merge

`main` at `98e1b09` (track `pm`: std 0.3.0, the wheels, MIDI files in the app) was merged in at `01c5dbc`. Three conflicts: `exchange.rs`'s imports, RENDER-10's Today line, and HANDOFF. `pm` took the by-ear steps 103 to 110, so this track's are now 111 to 116.

The first full check after the merge failed one test: the CLI's crash test asked for std 0.2. `6d212a8` makes it ask for 0.3.

## The final check

`ci/check.sh --gates` at `6d212a8` (Linux, run with `~/m2-runs/tw`). Nothing passed only on a retry. The summary, word for word:

```
music2 check: PASS
commit   6d212a8405c9 on ex (clean)
machine  Ubuntu 24.04.3 LTS, x86_64, 32 threads
tools    rustc 1.99.0, node v24.21.0, 33 min
plugins  /home/jerry/.local/share/m2-test-plugins/clap (the builds ci/plugins.sha256 records)
toolchain        PASS  rustc 1.99.0
web build        PASS  
build            PASS  
app              PASS  the UI connected: first-song in 1 s, with-plugins in 1 s
rust tests       PASS  1717 passed, 0 failed
clippy           PASS  
pool models      PASS  29 passed, 0 failed
std kernels      PASS  tests and clippy
std.wasm         PASS  rebuilt to the same bytes in another folder
mac lint         PASS  the macOS code type-checks and lints
browser suites   PASS  71/71 suites ok
renders          PASS  12 of 12 match e2e/renders.sha256
old engine (S)   PASS  built for G7f from a clean copy of 8aa301e, in /home/jerry/.cache/m2-check/s-8aa301e
gates            PASS  the 7 runs ci/gates.txt names
repo unchanged   PASS  
```

Logs: `~/m2-runs/sex/check-6d212a8.tgz`. Before the review it passed at `9efece3` (1,682 Rust tests; logs `~/m2-runs/sex/check-9efece3.tgz`). The Mac's run is owed.

## A check by ear

About 10 minutes, on the owner's Mac, with a USB microphone. No test can do it: no test plays a real input or meets the Mac's question.

| # | Do | You should see or hear |
|---|---|---|
| 111 | Click **Open an Example**, then **First song**. **+ Track ▸ Audio track**. Press ⌘, for Settings. Under **Sound input**, pick the USB microphone. Close Settings. Unplug the USB microphone. Press R. | A message: "(the USB microphone) isn't plugged in, so this take records from (the Mac's microphone)…". It records. The pill says "1 problem"; click it: the same words. Press R to end the take. |
| 112 | Plug the USB microphone back in. Press R, speak, press R. | It records from the USB microphone. No message. The problem is gone. |
| 113 | Press R and speak. While it records, unplug the USB microphone. | The song plays on. One message: "(the USB microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. |
| 114 | Quit music2. In the Terminal type `tccutil reset Microphone com.suite2.music2` (started from the Terminal, macOS asks for the Terminal: then `tccutil reset Microphone com.apple.Terminal`). Open music2 again, open the song, click the audio track, press R. When macOS asks to use the microphone, click **Don't Allow**. | The take stops by itself within a moment. A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone. The take was only silence, so it wasn't kept." No clip. The song plays on. |
| 115 | Press R again. | The same "music2 can't use the microphone…" words. Nothing records. Then turn music2 (or the Terminal) back on in System Settings ▸ Privacy & Security ▸ Microphone. |
| 116 | Tell the lead what you saw and heard at 111 to 115. | |
