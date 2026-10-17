# Build notes: polish

The polish track, by part. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix strips, change instrument, drum names, track IDs. Branch `polish-a`, worktree `wt/pl-a`.

### A1 and A2: M, S and a meter on every Mix strip

**In short**

| What | Now |
|---|---|
| M and S on a strip | Each track's strip has M and S, centred under its pan. They are the header's buttons: the same state, look and tooltips |
| A strip of two tracks ("Kick + Hats") | M and S act on both. When the two differ, the button shows a dash. ⌥-click on S solos exactly those two |
| The strip's menu | It has Mute in song (or Unmute in song, first, when the strip's tracks are muted in the song). One undo step, with a toast |
| A meter on each strip | Two thin bars, left and right, beside the fader. A track's meter shows its sound before the fader, so moving the fader doesn't move it |
| Master's meter | Two wider bars: what the song sends out |
| The clip dot | A small red dot over a track's meter when its sound goes over the top (0 dB). It stays until you click it. Master has none: it ends in a limiter |

**What changed**

- `MuteSolo.tsx`: one `Pair` of buttons for a list of tracks. The header passes one track; `StripMuteSolo` passes a
  strip's tracks. A button's state is on, off or `mixed` (`aria-pressed="mixed"`, drawn as a dash in the on colour).
  A click turns all on when any is off. A drag across M buttons crosses headers and strips alike. A strip's buttons
  carry `data-strip-mute` and `data-strip-solo` (a header's keep `data-mute`, `data-solo`), so header suites don't see
  two buttons per track. A strip of several tracks names no key in its tooltips (the M key acts on the selected track).
- `lib/overlays.ts`: `soloOnly(modules)` solos exactly a list (`exclusive` the first, `solo` the rest).
- `lib/meters.ts`: each meter keeps three levels (the louder side, left, right). `mount(keys, canvas, bars)` draws
  one bar or two, the louder of several tracks per side. Every meter's readout (`aria-valuenow`, and `data-left`,
  `data-right` on a two-bar one) changes on the same frame, 10 times a second, so two meters of one track always say
  the same. A track's peak at 1.0 or more latches its clip (`useClipped`, `clearClips`); a new song clears them.
- `Meter.tsx`: `channels` and `bars` props, a `title`, and `ClipDot` (a 12 px button holding a 4 px `danger` dot; no
  focus, so Space still plays).
- `Mix.tsx`: the M and S row; `StripMeter` in the fader's column (a drag on it still moves the fader), 7 px wide
  (11 on Master), from 9 px under the travel's top to its foot, the clip dot above it. A strip whose level is set in
  code has the meter too, with its words under it. The strip menu: Mute in song, through `muteInSongCall` for all the
  strip's tracks.

**Tests**

| Suite | Checks | Result |
|---|---|---|
| `pl-mix` (new, after `s8-mix`) | 47 | ok |
| `s8-mix` | 80 | ok. Two checks follow the change: Tab now goes pan, M, S, fader; the strip menu has Mute in song (4 rows) |
| `s8-mute` | 152 | ok |
| `s8-meters` | 24 | ok |
| `taste` | 39 | ok |

Web build: ok. No Rust changed.

**Red first** (the new suite on the page from before, `29430e8`):

```
FAIL  the Kick strip has M and S, as every track's strip does ([["kick",false,false],["hats",false,false],…])
FAIL  the Hats strip has a meter, as every strip does (kick:0 hats:0 bass:0 keys:0 master:0)
FAIL  the Hats strip's two bars both read above the floor
```

(The second and third come from a run of the meters part alone: on the old page the M and S part stops the suite.)

**Notes**

- First-song's Bass reaches 0 dB where it enters the duck, so its clip dot lights while the song plays. That is true
  of its sound; the master's limiter keeps the song itself clean.
- `pl-mix` lints through `e2e/lintlib.mjs` (P0b-1's). It runs as `page.evaluate(lint, sel)`.

**What's left**

- Meters after the fader (Q1: needs taps in `core/runtime`).
- The "Still drives Duck on Bass" row on a muted strip (taste.md §3.5 MON-2): the words are in M's tooltip, as on the
  header; the strip doesn't dim.

**For gaps.md:** the Mix view's strips have M, S and meters (pre-fader), and clip dots. Left: post-fader meters.

**Contract:** none.

### A3: change a track's instrument

**In short**

| What | Now |
|---|---|
| Where | Change instrument… in a track's menu (after Rename) and in a track strip's menu in Mix |
| What opens | The + Track list, titled "Change Bass's instrument". What the track plays now has a tick. Type to find one, ↑ ↓ to move, Return to pick, Esc to close |
| What it does | The track's instrument call becomes the new one, reading the same notes. One undo step. A toast says "Bass plays Synth pad now", with Undo |
| What stays | The track's name, colour, notes, mixer channel, effects and mute |
| Its old settings | They go: the new instrument starts at its own sound, as a new track's does |
| When it says no | When a setting comes from other code (a lane, an LFO), when the sound is built from parts, when there are two instruments, or a plugin. The toast says why and has Show code |

**What changed**

- `crates/editor/src/swap.rs`: `swap(s, module, component)`. It finds the track's instrument calls: the outermost
  frames of a component that takes notes first and makes audio (with or without a category: std/instruments' `hat`
  has none), and plugins given notes. Exactly one, in the track's own file, or it refuses in words. Each setting
  after the notes must be a plain value (a number, a unit of one such as `hz(420)`, a string or a boolean). The Tx is
  one `TextEdit` of that file, `Confined` to the call and the imports: two edits around the notes argument (so its
  bytes and the lane node keep their names), and one edit of the import lines (trimmed to what differs, so no node
  moves). The imports: names only the dropped text read go (`sites::unused_imports`, X18), keeping the callee's
  binding when the new one shares it (`drums.kit` → `drums.hats`); then the new binding is imported
  (`import_insertion`: joins an import from its module, else a line after the last import). The planner's
  assertion: every added, removed, rewired or changed node is the track's; no module comes or goes; the track's
  lanes are the same. Labels `("Bass", "Bass plays Synth pad")`.
- `crates/server/src/graph/instrument.rs`: `Graph::swap_instrument(token, module, component)`.
- `crates/server/src/routes_mix.rs` (new, lane A's router): `POST /api/graph/track/instrument {session, module,
  component}`, a write (423 read only). No `revision`: the module key and the ComponentId name what to change.
- Wiring, the P0a lines lane A needed before P0a landed: `pub mod swap;` (editor `lib.rs`), `mod instrument;`
  (`graph.rs`), `mod routes_mix;` (server `lib.rs`), its `.merge(…)` in `routes.rs`.
- `web/src/lib/instrument.ts`: `changeInstrument` (the call, the undo step's name, the toast with Undo; a refusal
  ending "in code." or "in the Code view." gets Show code), `currentInstrument` (what the list ticks), `notesLane`.
- `Tracks.tsx`: `InstrumentPicker` gets a change mode (`swap`): a title row, "Find an instrument…", a 14 px check on
  what the track plays, the foot `tracks/bass.ts · synth.pad(lane.notes("notes"))`. `TrackMenu` gets the row
  (`ArrowLeftRight` icon). `openChangeInstrument(module, anchor)`.
- `Mix.tsx`: the strip menu's row, on one-track strips.

**Tests**

| Test | Result |
|---|---|
| Rust `swap_tests.rs`: `swap_replaces_the_call_and_its_import`, `swap_keeps_a_used_import`, `swap_refuses_a_setting_that_follows_code`, `swap_refuses_two_instruments`, `swap_drops_plain_settings`, `undo_puts_every_byte_back`, `the_change_stays_in_the_track` | 7 ok |
| `cargo test -p m2-editor -p m2-server`; clippy of `m2-editor`, `m2-server`, `m2-script` | ok |
| `pl-sounds` (new, after `pl-mix`) | 27 ok |
| `s8-tracks` 75, `s6-builders` 56, `taste` 39 | ok |
| `s8-mix` | 80 ok. Its strip-menu check now has 5 rows |

**Red first**

```
swap against a stub that refuses:  test result: FAILED. 0 passed; 7 failed
FAIL  the Hats menu has Change instrument…, after Rename (Rename⌘R | Duplicate⌘D | Delete⌘⌫ | Colour› | Mute in song… | Edit in code…)
```

**What's left**

- Track ▸ Change Instrument… in the menu bar: P0a's `appCommand` row calls `canvas.changeInstrument`, which lane A
  fills once P0a lands.
- The plan's "Hi-hat ticked" for first-song's Hats can't be: Hats plays std/instruments' `hat`, which the list
  doesn't offer, so nothing is ticked. After a change, the new one is ticked.

**For gaps.md:** a track's instrument can be changed from its menu or its strip (one undo step); refused in words
when code computes a setting, for sounds built from parts, two instruments or a plugin.

**Contract:** WRITE-10's builder list gains change instrument (`m2_editor::swap`, `POST /api/graph/track/instrument`).
taste.md §2.12 gains `ArrowLeftRight` for change instrument.

### A4, part 1: an instrument names its notes

**In short:** the drum kit now says which drum each note plays (36 is Kick, 38 is Snare). The timeline passes those
names to the page. Part 2 shows them in Notes.

**What changed**

- `core/script/sdk/core.ts`: a descriptor's `editor.notes` (`{ 36: "Kick" }`), checked where it is declared: notes
  are whole numbers 0 to 127 (written as such: `"1.5"` and `"-1"` are refused), each name one line of 1 to 24
  characters. Emitted in the descriptor's JSON. `core/script/src/component.rs`: `Editor.notes: BTreeMap<u8, String>`
  (`serde(default)`).
- `std/src/drums.ts`: the kit names the 20 notes of the plan, each checked against the `std:drum` kernel's General
  MIDI map (`std/kernels-src/src/drums/tests.rs`, `KIT`): 36 Kick, 37 Rim, 38 Snare, 39 Clap, 41 Tom 1, 42 Closed
  hat, 43 Tom 2, 44 Pedal hat, 45 Tom 3, 46 Open hat, 47 Tom 4, 48 Tom 5, 49 Crash, 50 Tom 6, 51 Ride, 54
  Tambourine, 56 Cowbell, 69 Shaker, 70 Shaker 2, 75 Claves. The kernel also plays 35, 40, 57 and 59 (a second kick,
  snare, crash and ride, the same sounds): left unnamed, so no two rows say the same. `drums.hats` names its closed,
  pedal and open notes (42, 44, 46). Kernels don't change, so `std.wasm` doesn't.
- `crates/editor/src/note_names.rs`: `of(rev, module, lane)`: the nodes that read the lane (a `poly`'s events, a
  kernel's or a plugin's input, voices included), each one's outermost frame whose component names its notes; the
  names when every reader is of that one component, else none. So a kit inside a project's own wrapper still names
  them, and a lane read by a kit and a pluck names none.
- `crates/editor/src/timeline.rs`: `TimelineLane::Notes` gains `names` (left out of the JSON when empty). P0a's line,
  made here before P0a landed.

**Tests**

| Test | Result |
|---|---|
| `core/script` `editor_notes_are_read_and_checked` (a read, no names, eight refusals) | ok |
| `core/script/tests/std.rs` `the_drum_kit_names_its_notes` (the 20 names, the hats' 3, no other instrument names any) | ok |
| `crates/editor` `a_drum_kit_lane_names_its_notes`, `a_pluck_lane_has_no_names` | ok |
| `cargo test -p m2-script -p m2-editor`; clippy of both | ok |
| `notes` 52, `s8-audition` 51, `s6-builders` 56, `s8-tracks` 75, `taste` 39 | ok |

**Red first**

```
editor_notes_are_read_and_checked: component "kit" at lib/c.ts:2:18: editor has no field "notes" (fields: category, description, insert)
the_drum_kit_names_its_notes:      left: []  right: [(36, "Kick"), (37, "Rim"), …]
a_drum_kit_lane_names_its_notes:   assertion `left == right` failed: []  left: 0  right: 20
```

## Lane B

Timeline.

## Lane C

Songs.

## Lane D

Notes and automation.

## Merge

The lanes into `polish`.

## End

The full check, docs and pictures.
