# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is left. The plan is
`~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground.

## Lane A

Hear it while you drag.

## Lane B

Play in.

## Lane C

Settings and safety. Branch `next-c`, worktree `wt/nx-c`.

Lane C started before P0 had landed: it made what it needs itself, in its own files and small blocks of the shared
ones. Its suites start servers of their own where they need an environment (`M2_AUDIO=fail`), so they don't need
P0's `env` rows in `run.mjs`.

### C1. A song opens with no sound device

**In short**

| What | Now |
|---|---|
| No sound device when a song opens | The song opens. It plays silently: the playhead moves, nothing sounds |
| The status pill | "No sound output", in the warning colour, with a crossed-out speaker |
| Its popover | "No sound device was found, so the song plays silently. Plug one in: it's used from your next action." |
| A device plugged in | Within about 2 s the song moves to it, where it was, stopped. A toast says "Sound output is back. Press Play to go on." |
| A device that is listed but won't open | Not tried at every action: again only when the default device changes, or after 30 s |

**What changed**

- `core/play/src/outputs.rs` (new): `Choice` (the output and buffer size a player opens; C2 uses it), `Output`,
  `outputs()`, `default_output()`, `output_ready()`. `M2_AUDIO` picks how the process meets the hardware: the
  devices, `none` (the silent player, which lists `M2_AUDIO_OUTPUTS`), or `fail` (no output starts: a test seam).
- `core/play/src/lib.rs`: `Player::start()` is `start_on(&Choice::default())`. `start_on` opens the chosen output
  (C2), or fails in today's words under `M2_AUDIO=fail`. `Player::frames()` and `Player::fell_back()` read two new
  `Shared` atomics. `silent()` is `silent_as("none", None)`.
- `crates/server/src/graph/output.rs` (new): `Outputs`, which starts players (the starter and the probe are
  injectable; unit tests keep `Player::silent()`). When a start fails, the silent player stands in, by its epoch, and
  the log says why once. `Graph::look_for_output()` marks an output to try, and `Graph::no_output()`.
- `crates/server/src/graph.rs`: `Inner::device()` asks `Outputs::start` for a player, and lets a real one take over
  from the stand-in (`Outputs::came_back`), where it was, stopped, with the toast. Not while an export holds the
  player. `Activity` gains `output` (the device's name) and `no_output`. `Inner.output`, `Graph::with_outputs`.
  `let_go` and `retire` are untouched (plan X2).
- `crates/server/src/lib.rs`: the `music2-tell` thread calls `look_for_output()` every 2 s.
- `web/src/lib/settings.ts` (new: the status's `output` and `no_output` types), `Status.tsx`: the `no-output` state,
  after the song's problems and before a dropout.
- `crates/server/src/graph/tests.rs`: one line. A device change's new player now says its name (`output: "none"`).

**Tests**

| Test | Result |
|---|---|
| `core/play` `outputs_put_the_default_first`, `a_chosen_device_that_is_gone_falls_back_to_the_default`, `a_fixed_buffer_is_used_when_in_range` (C2's, built here with `Choice`) | ok |
| `m2-server` `graph/output_tests.rs`: `a_song_opens_without_an_audio_device`, `the_silent_player_says_so_once`, `a_fallback_player_plays_on_its_clock`, `a_device_that_comes_back_is_used_from_the_next_action` | ok, three runs of the whole lib in a row |
| `cargo test -p m2-play -p m2-server`, clippy, the mac lint of both | ok |
| `nx-nodevice` (new, kind `none`, 9 checks) | ok |
| `taste` | 39, ok |

**Red first**

- `a_song_opens_without_an_audio_device` and the rest, with `Outputs::start` passing the start's error on (today's
  open): `panicked at output_tests.rs:46:28: the song opens: no audio output device`.
- With `came_back` never taking over: `panicked at output_tests.rs:119:5: the next action loads the song into the
  new player`; `the_silent_player_says_so_once`: `left: 1, right: 2` (never tried again).
- `core/play`, with `silent_pick` ignoring the choice: `outputs_tests.rs:45`: `left: ("Speakers", false), right:
  ("Headphones", false)`; `outputs_tests.rs:59`: `left: Some(480), right: Some(256)`.
- `nx-nodevice` on the new server with `DIST=` the page before (plan X19: no older server runs with
  `M2_AUDIO=fail`): 3 of 9 passed, then it stopped. `FAIL  the status pill says "No sound output": undefined
  (undefined)`; `FAIL  in warn, with the VolumeX mark: undefined`.

**Left**

- The "Settings…" link in the popover comes with C2.
- The look for an output only runs while a song is open; with none open, the next song's open tries again.

**For gaps.md:** #13 done: a song opens with no sound device, plays silently, the pill says so, and a device plugged
in is used from the next action.

**Contract:** RENDER-28 or the player's section: a start that fails starts the silent player instead, and the status
says `no_output`; the server looks for an output every 2 s while it stands in; a start that failed is tried again
when the default output changes or after 30 s.

**For 8a's merge** (`core/play/src/lib.rs`, each a small block):
- `pub mod outputs;` and its `pub use` line.
- `Shared`: `frames: AtomicU32` and `fell_back: AtomicBool`, after `last_callback_ns` (X6).
- `Callback::fill_in`: one line after `last_callback_ns`'s store: `self.shared.frames.store(…)`.
- `Player::start()` calls `start_on(&Choice::default())`; `start_on` and `start_in` replace `start`'s first lines and
  its device and config lines (`outputs::pick`, `outputs::buffer`, the `fell_back` store). `Choice` folds into
  8a's `Options` (an output name beside `frames`); `M2_AUDIO=fail` and `M2_AUDIO_OUTPUTS` go into
  `Options::from_env()` (plan X1). C never reads `M2_AUDIO_FRAMES` itself: `silent_as` keeps today's line for it.
- `Player::silent()` is `silent_as("none", None)`; `silent_as` is today's `silent` with a name and a buffer size.
- `Player::frames()` and `Player::fell_back()`, after `now_ns()`.
- The unused `HostTrait` import went (the device lookups moved to `outputs.rs`).

## Lane D

Effects and small fixes.

## Merge

## End
