# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Hear it while you drag.

## Lane B

Play in: the metronome, the keyboard piano, the MIDI keyboard. Branch `next-b`, worktree `wt/nx-b`.

### B0. midir, first (X27)

**In short**

| What | Now |
|---|---|
| MIDI keyboards | `m2-os` can hear every MIDI keyboard on the machine, as they are plugged in and out. Nothing uses it yet (B3 does) |

**What changed**

- `Cargo.toml`: `midir = "=0.11.0"` after `cpal` (Q2, X26). `core/os/Cargo.toml`: `midir` optional, behind a new
  feature `midi = ["dep:midir"]`. Only `m2-server` turns it on (`crates/server/Cargo.toml`), so no other crate builds
  midir or the OS MIDI libraries.
- `core/os/src/midi.rs` (new, `#[cfg(feature = "midi")] pub mod midi;`): `listen(on) -> Listening`. A thread
  `m2-midi` reads the list of inputs every second, opens each new one (its own `MidiInput`), closes gone ones, and
  tells `Plugged(name)`, `Unplugged(name)` and `Bytes(name, &bytes)`. "Midi Through" ports are skipped. System
  messages (clock, sysex, active sensing) are ignored. A port that won't open is tried again after 5 s. Names read
  plainly (`pretty`: ALSA's "Keystation 49:Keystation 49 MIDI 1 24:0" is "Keystation 49"), and two keyboards with one
  name get "… 2". Dropping `Listening` stops the thread and closes every connection; nothing is heard after.
- `core/os/src/{linux,mac}/mod.rs`: `midi_allowed()`. On Linux, `access("/dev/snd/seq")` first, because alsa-lib
  writes a line to stderr each time the sequencer won't open (here: `EACCES`), and the listener tries every 5 s. On a
  Mac, always true.

**`Cargo.lock`** gained only new entries (`git diff Cargo.lock`, 173 lines in, 2 changed):

| New | Why |
|---|---|
| `midir 0.11.0` | the crate |
| `coremidi 0.9.2`, `coremidi-sys 3.2.1` | its macOS side |
| `android-build 0.1.4`, `java-locator 0.1.9`, `jni-min-helper 0.3.4`, `dlopen2 0.9.0`, `process_path 0.1.4` | its Android side (never built) |
| `windows-sys 0.52.0`, `windows-targets 0.52.6`, `windows_{aarch64_gnullvm, aarch64_msvc, i686_gnu, i686_gnullvm, i686_msvc, x86_64_gnu, x86_64_gnullvm, x86_64_msvc} 0.52.6` | its Windows side (never built) |

The two changed lines only add a version to a name that now has two versions (`"dlopen2"` → `"dlopen2 0.8.2"` in
`tao`, `"windows_i686_gnullvm"` → `"… 0.53.1"` in `windows-targets 0.53.5`). No existing entry's version moved. It
reuses `alsa 0.11.0`, `alsa-sys`, `libc`, `log`, `bitflags 2`, `objc2`, `block2`, `core-foundation`.

**Tests** (`core/os`, `--features midi`): `listening_never_errors` (here MIDI won't open: no keyboards, no stderr
line; drop stops the thread in under 2 s; nothing heard after), `through_ports_are_skipped`, `names_read_plainly`.
All 41 `m2-os` tests pass with the feature; clippy with and without it; the Mac lint with it (it built
`coremidi-sys` with the stub C compiler).

**Red first:** none; a new module (its tests are guards).

**For gaps.md:** none yet (B3).

**Contract:** PLAT-2's *Today*: `m2_os::midi` (midir; one file, no OS folder; `midi_allowed` per OS).

### B1. The metronome

**In short**

| What | Now |
|---|---|
| The button | A Metronome button sits right after Loop in the top bar. Click it, or press K, or use Transport ▸ Metronome |
| What you hear | While the song plays: a click on each beat, a higher, louder one on each bar's first beat. In 6/8 it clicks the eighths |
| Kept | Each song keeps its own setting. A new song starts with it off |
| Exports | Never have the click |
| Small windows | In the Mac app at its smallest (1100 px), the song's name now goes from the top bar: there is no room for it beside the new button. Taste §3.8 drops it after the time |

**What changed**

- `core/runtime/src/click.rs` (new): `Click`, plain fields. `mix(frames, runs, renderer)` finds the grid points
  (every `4 / unit` quarter notes of the meter in effect) whose frame falls in a moving run, `[at, at + n)`, and
  starts a blip there: a sine, 30 ms, decaying (time constant 6 ms) with a linear fade to 0 at its end; 1,500 Hz at
  −12 dBFS on a bar's first beat (`bar_at(g).1 == 0`), else 1,000 Hz at −16 dBFS; the same in both channels. Off and
  not ringing it returns at once, so the output keeps every bit. Its math is `libm`'s (PLAT-4: `host_math` checks
  `m2-runtime`'s files). Its tests are `click/tests.rs`, a child of `click` (X4: `tests.rs` untouched; a
  `#[path]` would hide the file from `host_math`'s module walk, so it is the plain child path).
- `core/runtime/src/engine.rs`: `Engine.click`, `Engine::set_click`, and one line in `render` after
  `let runs = *cur.last_runs();`. It mixes before the hold's fade, and a held engine renders nothing, so an
  export's hold silences it.
- `core/runtime/src/lib.rs`: `pub mod click;`; `Renderer::arrangement()` (the meters), after `tempo()`.
- `core/play/src/lib.rs`: `Shared.click` (after `running`), `Player::set_click` and `Player::click` (after
  `held`), and in `Callback::render`: `shared` in the destructure and `l.engine.set_click(…)` before `l.fill(…)`.
  So the flag outlives a project: whatever plays next clicks as the player was told.
- Server (X9: no new route): `view_file::KEYS` gains `metronome` (7 keys). `set_view` tells the player when
  `metronome` is among the changes; `view_file::metronome(local)` reads it (off unless `true`). `actor.rs`, two
  one-line blocks: in `Actor::open`, after `local` is read, the player gets the song's; in `settle`'s `Way::Load`
  arm (a new player after a device change), the player gets it from the control plane's `local` (so a read-only
  window's in-memory setting holds).
- Web: `lib/metronome.ts` (new): `useMetronome`, `setMetronome` (`setLocal("metronome", true | null)` and a flush
  at once, not after the usual 300 ms), `toggleMetronome`. `TopBar.tsx`: the 28 px toggle after Loop
  (`Metronome` icon, `accent-soft` when on, tooltip "Metronome: a click on each beat (K)", X36). `App.tsx`:
  `k: "metronome"` in `key()` and `case "metronome"`. `Shortcuts.tsx`: Transport "Metronome on / off: K".
- Desktop `menu.rs`: `ITEMS` `metronome` ("Metronome", no accelerator: K is the page's) after `panic`; Transport:
  Loop, Loop Selection, Metronome, Follow Playhead (X14); the old assertion now reads Metronome then Follow; a test
  `the_metronome_has_its_row_in_transport`.
- Suites moved: `s8-songbar` and `fx-fixes` checked the song's name at 1100 px in the Mac app. With the 32 px, the
  name has 22 px of room there, under the 56 it needs, so it goes (§3.8's order). Each now checks it is gone at
  that one size (same counts), and still clears the tempo and Undo at 1180 and 1280. Their README rows say so.

**Tests**

| Test | Where | Result |
|---|---|---|
| `the_click_sounds_on_each_beat_louder_on_the_bar`, `the_click_follows_the_meter_and_a_tempo_change`, `the_click_lands_on_a_loop_wrap`, `no_click_while_stopped_or_held`, `the_click_off_changes_no_bit` | `core/runtime` (`click::tests`) | ok |
| `the_click_never_allocates` | `core/play/tests/no_alloc_click.rs` (new file, X5) | ok |
| `the_metronome_is_kept_per_song` (set, other song, reopen, a new player after a device change, a read-only window's in memory) | `crates/server` (`view_file_tests.rs`) | ok |
| `an_export_never_has_the_click` (a guard, X25) | `crates/server` | ok |
| `the_metronome_has_its_row_in_transport` | `music2-desktop` | ok |
| `nx-click` (new, after `s8-songbar`) | 20 checks, with a capture server of its own (`M2_AUDIO_CAPTURE`, `M2_MIDI=none`, an empty song) | ok |
| `s8-songbar` 82, `fx-fixes` 75, `pc-keys` 37, `taste` 39 | moves | ok |

All tests of `m2-runtime`, `m2-play`, `m2-os`, `m2-server`, `music2-desktop`; their clippy; the Mac lint of the
five; the web build; the renders 10 of 10.

**Red first**

- `click::tests` with `set_click` a no-op: all five fail, e.g.
  `left: [] right: [0, 24000, 48000, 72000, 96000, 120000, 144000, 168000, 192000]` ("a click on each beat, at its
  frame"), `left: [] right: [0]` ("Play clicks the beat it starts on"). (The bit test fails at "on, it clicks".)
- `the_click_never_allocates` with the callback's `set_click` line out: "the click sounded over a silent song".
- `the_metronome_is_kept_per_song` with `metronome()` answering false: fails at "on at once";
  `a_read_only_window_keeps_view_settings_in_memory` at "a read-only window's metronome clicks too".
- `nx-click` on `dist-before` and the 70f9b61 server:
  `FAIL  the top bar has a Metronome toggle, off for a song that never had it on (null)`,
  `FAIL  it sits right after Loop, 28 px, 4 px from it (… metronome undefined …)`.
  The new page on the 70f9b61 server (the view route refuses `metronome`):
  `FAIL  the song's view.json keeps it: {}`,
  `FAIL  on and playing, a click on each beat, exactly 0.5 s apart, each bar's first the louder ()`.

**Guards** (X25, not red first): `an_export_never_has_the_click`, `the_click_off_changes_no_bit`'s "never on"
half.

**What's left**

- The metronome's level and sound, a count-in (with recording), a click on a cue output (Settings, later).
- An `m2` command for it (Q35, gaps #37).

**For gaps.md:** #19 done: the metronome (count-in left, with recording; its level and sound left).

**Contract:** RENDER-28 (the click: plain fields, no allocation, `no_alloc_click.rs`); RENDER-1 and RENDER-3 (the
click is the live engine's alone: exports never make an `Engine`); FMT-20 (`view.json`'s `metronome`, a view
setting); taste §3.3 (the metronome exists, x 176) and §3.8 (the name goes at the Mac app's 1100 px).

**For 8a's merge** (each a block of its own):

- `core/runtime/src/click.rs`, `core/runtime/src/click/tests.rs`: new files.
- `core/runtime/src/lib.rs`: `pub mod click;` after `pub mod cells;`; `Renderer::arrangement()`, right after
  `Renderer::tempo()`.
- `core/runtime/src/engine.rs`: `use crate::click::Click;` above the `use crate::{…}` line; `Engine.click` at the
  struct's end and `click: Click::default()` at `new`'s end; `pub fn set_click` right after `new`; in `render`, one
  line after `let runs = *cur.last_runs();`: `self.click.mix(&mut frames, &runs, cur);`.
- `core/play/src/lib.rs`: `Shared.click: AtomicBool` after `running`; `Player::set_click` and `Player::click` after
  `Player::held`; in `Callback::render`, `shared` added to the destructure and
  `l.engine.set_click(shared.click.load(Ordering::Relaxed));` before `l.fill(…)`.
- `core/play/tests/no_alloc_click.rs`: new file.

## Lane C

Settings and safety.

## Lane D

Effects and small fixes.

## Merge

## End
