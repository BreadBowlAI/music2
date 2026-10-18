# Track ex: build notes

Branch `ex`, from `main` at `83fa4e4`, 2026-10-18. The plan is [ex-plan.md](ex-plan.md); what changed while building is written there, under each slice ("As built").

## In short

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

## Renders

12 of 12 match `e2e/renders.sha256` after each slice. No fault happens in them.

## Speed (Linux, information only)

Not measured: how long making a `Namer` takes per plan. It clones one shared pointer and walks the nodes once to name the plugins.

## The final check

`ci/check.sh --gates` passed at `9efece3` (Linux, run with `~/m2-runs/tw`; 34 minutes). Nothing passed only on a retry.

| Row | Result |
|---|---|
| Rust tests | 1,682 passed |
| clippy, and the mac lint | pass |
| browser suites | 71 of 71 (`ex-faults` new) |
| renders | 12 of 12 |
| gates | the 7 runs `ci/gates.txt` names |

Logs: `~/m2-runs/sex/check-9efece3.tgz`. The Mac's run is owed.

## A check by ear

About 10 minutes, on the owner's Mac, with a USB microphone. No test can do it: no test plays a real input or meets the Mac's question.

| # | Do | You should see or hear |
|---|---|---|
| 103 | Click **Open an Example**, then **First song**. **+ Track ▸ Audio track**. Press ⌘, for Settings. Under **Sound input**, pick the USB microphone. Close Settings. Unplug the USB microphone. Press R. | A message: "(the USB microphone) isn't plugged in, so this take records from (the Mac's microphone)…". It records. The pill says "1 problem"; click it: the same words. Press R to end the take. |
| 104 | Plug the USB microphone back in. Press R, speak, press R. | It records from the USB microphone. No message. The problem is gone. |
| 105 | Press R and speak. While it records, unplug the USB microphone. | The song plays on. One message: "(the USB microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. |
| 106 | Quit music2. In the Terminal type `tccutil reset Microphone com.suite2.music2` (started from the Terminal, macOS asks for the Terminal: then `tccutil reset Microphone com.apple.Terminal`). Open music2 again, open the song, click the audio track, press R. When macOS asks to use the microphone, click **Don't Allow**. | The take stops by itself within a moment. A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone. The take was only silence, so it wasn't kept." No clip. The song plays on. |
| 107 | Press R again. | The same "music2 can't use the microphone…" words. Nothing records. Then turn music2 (or the Terminal) back on in System Settings ▸ Privacy & Security ▸ Microphone. |
| 108 | Tell the lead what you saw and heard at 103 to 107. | |
