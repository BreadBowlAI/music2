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

### B2. The keyboard piano

**In short**

| What | Now |
|---|---|
| Turn it on | ⌘K (Ctrl+K on Linux), from anywhere, even while typing. Or View ▸ Keyboard Piano, or the keyboard button in Notes, beside Hear notes |
| Play | A S D F G H J K L ; ' are the white keys, W E T Y U O P the black ones. A is middle C (C4) at first |
| Octave | Z goes down an octave, X goes up |
| What plays | The selected track. A card at the timeline's lower right shows the keys, their notes, the octave, and "Plays Bass" |
| Its keys | While it is on, those 20 keys only play notes. Their shortcuts (A, S, E, X, Z, K) wait until it is off. Space, C, M and the rest work as always |
| Typing | Typing in a field or the code editor never plays notes. A dialog takes the keys too |
| Off | ⌘K again, the card's ×, or the button. It is always off when the app starts |

**What changed** (web only, and one menu row)

- `lib/playTarget.ts` (new): `playTarget(graph, selected, clips)`: the selected track's notes lane, the lane of
  the clip Notes shows when it is that track's, else the first; null for the Song row, nothing selected, or no
  notes lane (Q4). `usePlayTarget`, `currentTarget`. B3 shares it.
- `lib/piano.ts` (new): `usePiano` `{on, octave, held}`; `NOTE_KEYS` by `e.code` (Q7); `setPiano`,
  `togglePiano`, `letGo`. While on, capture-phase `keydown` and `keyup` on `window`: a piano key with no ⌘ ⌃ ⌥,
  outside a field, the code editor (`.monaco-editor`) and an open `[aria-modal="true"]` is
  `preventDefault` + `stopImmediatePropagation` (so no shortcut runs) and `strike(…, {always: true})` on the
  target; its key up releases that note. A repeat strikes nothing. Z and X clamp the octave to C1–C7. `blur`,
  `visibilitychange`, and a Meta key down or up let every note go (X16). With no target, a key is still the
  piano's and plays nothing. While on it holds an `attach()` of audition's, so the server's answers and a closed
  socket are heard.
- ⌘K (X15): a capture-phase listener of `piano.ts`'s own, in a browser only, before anything else sees it, so
  it works from fields and from Monaco (whose ⌘K chords give way). In the desktop app the menu's
  `CmdOrCtrl+K` sends `piano`, as every ⌘ chord does there. So `App.tsx`'s `chord()` has no ⌘K line: the
  capture listener takes the key first in a browser, and the menu does in the app.
- `lib/audition.ts`: `strike(…, {always})`: the piano plays whatever Notes' Hear notes toggle says.
- `components/KeyboardPiano.tsx` (new), mounted after `<Toasts />` (X30): a portal, 16 px in from
  `[data-testid=timeline]`'s lower right (a `ResizeObserver`). `raised`, 1 px `line`, `rounded-md`,
  `shadow-lift`; caps 22×20 px in `text-xs`, the black ones `gray-700` over the white ones (`canvas`,
  `line-strong`); a held cap in the track's `tint`, `bar` and `ink` (X31); the white keys' notes under them
  (`pitchName`, C's with their octave); "Z  C4–F5  X"; "Plays Bass" or "Select a track with notes to play it";
  a × "Keyboard piano off (⌘K)".
- `components/AuditionTools.tsx`: the `Keyboard` toggle after Hear notes (X13). It is the header's other
  toggles' size, 24 px (`sm`), not 28: every Notes tool is 24.
- `App.tsx`: `<KeyboardPiano />`; `case "piano"`. `Shortcuts.tsx` (X32): View "Keyboard piano on / off: ⌘K", and
  a last group "Keyboard piano": "Play notes: A to ' and W to P", "Octave down / up: Z / X".
- Desktop `menu.rs`: `ITEMS` `piano` ("Keyboard Piano", `CmdOrCtrl+K`) after `metronome`; View: after Show
  Browser; a test `the_keyboard_piano_is_command_k_in_view`.

**Tests**

| Test | Where | Result |
|---|---|---|
| `nx-piano` (new, after `s8-audition`) | 39 checks: off at first (A shows automation); ⌘K; the card's words and place; A is 60 at 0.8 on `bass.notes`, lit in Bass's colour, its Off; W 61, K 72 (the metronome left off); X then A 72 (C5–F6); Z Z then A 48; S, E, Z and A run no shortcut; C, M and Space still do; "asdf" in the song's name and in the code editor plays nothing; the ⌥K sheet takes the keys and lists the rows; repeats send one On; blur and ⌘ let go; Keys and the Song row as targets; the lint of the card; ⌘K from a field; the toggle; the ×; a PC page's Ctrl+K | ok |
| `the_keyboard_piano_is_command_k_in_view` | `music2-desktop` | ok |
| `s8-audition` 51, `pc-keys` 37, `taste` 39, `pl-notes` 102, `nx-click` 20 | moves | ok |

The web build; `music2-desktop`'s tests, clippy and Mac lint.

**Red first** (`nx-piano` on `dist-before`):
`FAIL  off at first: the toggle in Notes' tools is off and there is no card (null)`,
`FAIL  ⌘K turns it on: the toggle is lit and the card shows`,
`FAIL  the card says what plays and the octave: null`.

**What's left**

- Velocity keys and a sustain key (Q7: kept small).
- The desktop app's ⌘K and View ▸ Keyboard Piano are checked on `:97` once, after B3.

**For gaps.md:** #24 (playing live): the computer's keys play the selected track (the keyboard piano).

**Contract:** taste §3.5 (Notes' tools: the keyboard piano toggle after Hear notes), §5 (⌘K is the keyboard
piano, from anywhere; the piano owns its 20 keys while on), the piano card (as built above).

### B3. The MIDI keyboard

**In short**

| What | Now |
|---|---|
| Plug it in | Before or while the app runs. A message says "Keystation 49 is connected. It plays the selected track." |
| The mark | A small keyboard mark shows in the top bar, before the status, while one is plugged in. Its tooltip says "MIDI keyboard: Keystation 49 · plays Bass" |
| Play | Its keys play the selected track, as hard as you press them. The sustain pedal holds notes |
| Unplug it | Its notes stop, the mark goes, and a message says it was unplugged |
| Nothing selected | Keys play nothing. The tooltip says "Select a track with notes to play it" |
| Recording | Not yet: nothing you play is written |

**What changed**

- `m2_os::midi` (B0): `Listening`'s drop no longer joins its own thread (the last owner let go inside an event).
- `crates/server/src/graph/midi.rs` (new): `ControlPlane.midi` (`Midi { keyboards, target }`). `Graph::midi(Event)`:
  `Plugged` joins the list and answers the toast's words; `Unplugged` leaves it, lets go of its notes
  (`audition_gone("midi:<name>")`, one Release) and answers its words; `Bytes` reads the message (`read`):
  `0x9n` with velocity > 0 an On (handle `note + 128 × channel`, pitch the note, velocity `v / 127`), `0x8n` or
  `0x9n` at 0 an Off, held back while the pedal (`0xBn 64`, ≥ 64 down) is down and sent when it lifts; `0xBn 120`
  or `123` let go of all; the rest nothing (Q5). Ons go to the target through `Graph::audition("midi:<name>", …)`;
  Offs need no target, so a note struck before the target changed ends where it sounds. `Graph::midi_target`
  keeps the lane with the song's token: another song opened plays nothing until a window names a lane (X37).
  `Graph::midi_state`: `{devices, target}`, in the state as `midi`.
- `graph/audition.rs`: `Source.keyboard` (a `midi:` source); the watchdog skips it (Q6);
  `expire_auditions_at(now)` for tests.
- `graph.rs`: `pub mod midi;`, `ControlPlane.midi` at the struct's end, `state["midi"]` in `Inner::state`.
- `lib.rs`: `AppState.midi: OnceLock<Listening>` at the struct's end; `spawn_midi` in `start_with` (so the app,
  the headless server and the capture server all listen) unless `M2_MIDI` is `none` or `inject`;
  `AppState::midi(ev)` toasts and sends every window `{"t": "midi", "midi": …}` when a keyboard comes or goes;
  `AppState::midi_changed`; `midi_injects()`.
- `routes_8b.rs`: `POST /api/graph/midi/inject {device, plugged?, bytes?}`, only with `M2_MIDI=inject` (else 404,
  "no such API"); `ws_text` takes `{"t": "midi", "module", "lane"}` (`module: null` clears) and tells every window.
- Web: `lib/midi.ts` (new): sends the target from `playTarget.ts` whenever the selection, the clip Notes shows or
  the song changes, and again on each (re)connect; `useMidi` from the state's `midi` and the `midi` events.
  `components/MidiStatus.tsx` (new): a 28 px `KeyboardMusic` mark in `fg-muted`, its tooltip naming the keyboards
  and the track (or "Select a track with notes to play it" when the lane has gone or none is named).
  `TopBar.tsx`: `<MidiStatus />` before `<Status />`. `App.tsx`: imports `lib/midi`.

**Tests**

| Test | Where | Result |
|---|---|---|
| `a_midi_key_plays_the_selected_track`, `note_off_and_velocity_zero_end_it`, `velocity_is_the_keys` (with the channels and what is ignored), `channels_dont_share_a_key`, `the_sustain_pedal_holds_offs_until_it_lifts` (struck again under the pedal too), `all_notes_off_lets_go` (CC 123 and 120), `unplugging_a_keyboard_lets_go_of_its_notes`, `a_held_midi_key_outlives_the_watchdog`, `no_track_selected_plays_nothing` (and a lane that has gone), `a_new_target_leaves_held_notes_where_they_sound`, `the_target_belongs_to_its_song`, `midi_never_waits_for_a_compile`, `an_export_has_no_midi_note` | `crates/server` (`graph/midi_tests.rs`, new) | ok |
| `nx-midi` (new, after `nx-piano`) | 24 checks on a server of its own with `M2_MIDI=inject` (P0's `env` for suites hadn't landed; this way it needs none), and the 404 on the usual one | ok |
| `s8-audition` 51, `taste` 39, `nx-piano` 39, `nx-click` 20, `pc-keys` 37 | moves | ok |

All `m2-server` (249) and `m2-os` tests; clippy (`m2-os` with and without `midi`); the Mac lint of `m2-os`,
`m2-server` and `music2-desktop`; the web build.

**Red first**

- `a_held_midi_key_outlives_the_watchdog` with `Source.keyboard` always false (today's watchdog): "timed out
  waiting until the quiet window is let go" (both notes went).
- `nx-midi` on the 70f9b61 server: `FAIL  a keyboard plugged in (404)`, `FAIL  a toast says so: []`,
  `FAIL  the top bar's mark names it and what it plays: null`,
  `FAIL  Bass selected, the window tells the server: MIDI plays bass.notes (null)`.

**Guards:** `an_export_has_no_midi_note` (exports never had a live note), `midi_never_waits_for_a_compile` (MIDI
never took the session).

**What's left**

- Recording what you play, a count-in, quantize on input (gaps #24's rest).
- A MIDI input picker; pitch bend, the mod wheel, aftertouch, program change, clock (Q5).
- Notes placed inside a block by their time stamp (`midi_instant`): a note lands at the next block line.
- The Mac by-ear run: whether CoreMIDI lists a keyboard plugged in while the app runs (`m2-midi` polls from a
  thread with no run loop; Not checked in the plan).

**For gaps.md:** #24 (playing live): a MIDI keyboard plays the selected track, with velocity and the pedal;
recording, a count-in and quantize left.

**Contract:** MON-7 (a keyboard is a source, `midi:<name>`, never let go by the watchdog); PLAT-2 (`m2_os::midi`,
only the server's `midi` feature); the state's `midi` and the `midi` event; `M2_MIDI` (`none`, `inject`) and
`POST /api/graph/midi/inject`; taste §3.3 (the MIDI mark before the status pill).

## Lane C

Settings and safety.

## Lane D

Effects and small fixes.

## Merge

## End
