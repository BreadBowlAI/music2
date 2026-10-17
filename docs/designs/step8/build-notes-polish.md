# Polish: build notes

What each part of the polish track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8c/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix and sounds.

Branch `polish-a`, worktree `wt/pl-a`. Like lane B, lane A started with no P0: it made the few seams it
needs itself, in small blocks of the shared files (listed under "For the merge" at the end of this part).

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

**Track ▸ Change Instrument… (a later commit).** There was no P0a, so lane A added its own row: `menu.rs`
`change_instrument` ("Change Instrument…", no chord) right after Rename in Track on both menu bars, with the test
`change_instrument_follows_rename_in_track` (red: `left: None, right: Some(("Change Instrument…", None))`);
`appCommand`'s `change_instrument` sends the selected track (never the Song row) to `canvas.changeInstrument`, which
`Tracks.tsx` fills: the list at that track's header. A browser page has no menu bar, so the page side is checked in
the desktop app at the End (Xvfb). `cargo test -p music2-desktop`: 9 ok; its clippy clean. The same commit makes
change instrument safe in a file with no imports (the import edit then goes at the top).

**What's left**

- The menu bar's row in the running desktop app: the End's Linux run.
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

### A4, part 2: drum names in Notes

**In short**

| What | Now |
|---|---|
| A drum kit's notes | Each key says its drum: Kick, Snare, Closed hat. The key column is wider (88 px) |
| A note | Its label and tooltip say the drum: "Closed hat · beat 2 · 1/8 · velocity 80%" |
| Other rows | Keep the piano look, with no label |
| Other instruments | Unchanged: C1, C2… in 56 px |

**What changed**

- `web/src/lib/rowNames.ts` (new): `pitchName`, `namesOf(lane)`, `rowName(lane, pitch)` (the lane's name for the
  note, else the pitch's), `keysWidth(lane)` (88 or 56).
- `NoteView.tsx` (no P0b-1, so in place, in small blocks): `KEYS_W` becomes `keysW` from the lane, read through a
  ref in the effects (X12); the note labels, note tooltips and velocity tooltips say `rowName`; `Keys` takes `names`
  and `width`: on a named lane each named row says its name (`text-xs`, `fg-muted`), no C does, and black keys are
  16 px wide so every name sits on the white of its key. Its memo compares the two new props (an empty lane's names
  are one shared object, so it holds).

**Tests**

| Test | Result |
|---|---|
| `pl-sounds`, 14 more checks: a Drum kit track added and a beat written; the timeline names its notes; its clip in Notes says Kick on 36, Snare, Closed hat, Open hat; unnamed rows say nothing, not even C4; the column is 88 px; a note on 42 says Closed hat (label and tooltip), a velocity bar says Kick; every name fits on the white of its key; the lint; Keys' roll still says C2 and C3 in 56 px | 41 ok |
| `notes` 52, `s8-audition` 51, `fixes-notes` 37, `taste` 39 | ok |

**Red first** (the drum part alone, on the page from before and the server with the names):

```
FAIL  key 36 says Kick: C2
FAIL  the key column is 88 px (56)
FAIL  a note on 42 says Closed hat ({"title":"F#2 · beat 2 · 1/8 · velocity 80%","label":"F#2"})
```

**What's left:** folding the rows to the drums used (taste NT-5).

**For gaps.md:** Notes names a drum kit's rows (Kick, Snare…), from the kit's own descriptor; any instrument can name
its notes (`editor.notes`). Left: folding to the drums used.

**Contract:** PKG-9's descriptor gains `editor.notes`. The timeline's notes lane gains `names`.

### A5, part 1: change a track's ID

**In short**

| What | Now |
|---|---|
| Where | In the Code view, right-click a track's file in the file list: "Change Bass's track ID…". Not in the track's menu: a beginner never needs it there |
| The dialog | The ID in a code font. It is checked as you type ("lead vox isn't an ID…", "low is taken…"). [Cancel] [Change ID] |
| What it does | The track's ID changes in its code, in its mixer channel when that has the same name, its data folder moves, and layout.json follows. One undo step. A toast says "Bass's ID is low now", with Undo |
| What stays | Its name, colour, mute and solo, and it stays selected. Its sounds restart |

**What changed**

- `crates/editor/src/rekey.rs`: `check(s, module, key)` (the words the dialog shows as you type) and `rekey(s, module,
  key)`. Ops: the key string in the `track()` call (in its own quotes); the channel's key when it equals the track's
  (ID-18), and the label then says "(and its mixer channel)"; `Rename data/<old>/ → data/<new>/` when the folder
  exists; `layout.json` with the key renamed in `order` and `tracks` when it reads (R15). The file and the binding keep
  their names (design-8b §11.1 step 3). Refused in ID words: not an ID (with one like it), taken by a track or a data
  folder, a channel of that name, `song`, the same ID.
- **Its expectation differs from design-8b §11.1 step 4.** A rename moves every node of the track, so the track's
  file is declared whole, and each read of the track in another file (its innermost call, `duck(…, kick.env)`), since
  an import of a module is named by its key. Core only takes ranges in a file the Tx edits ("ranges are declared in
  song.ts, which the Tx doesn't touch"): so when a reading file isn't edited (a channel with a name of its own), the
  builder checks the Tx first (`Expect::Seen` probe, R26), asserts on that delta, and commits it as seen (WRITE-11).
  Both ways run the same assertion: the modules are exactly `[Rekeyed]`; every node that changes is the track's, an
  import of it, or its followed channel's; no value changes; every lane plays the same clips under the new key.
- `crates/server/src/graph/rekey.rs`: `Graph::rekey` and `Graph::rekey_check` (reads only). `routes_mix.rs`: `POST
  /api/graph/track/rekey {session, module, key}` (a write) and `POST /api/graph/track/rekey/check` (not a write).
- `web/src/lib/rekey.ts`: the dialog's store, `checkId`, `changeId` (undo step "changing Bass's ID", toast with
  Undo), `tracksIn(graph, path)`. The window follows a track whose ID changed (the change, its undo, a rename by hand):
  when one track ID goes and one comes in the same song, `selected`, `module`, `clips` and `open` move to the new one
  (X20). Not before the graph has the new ID, or the timeline picks another track.
- `web/src/components/TrackIdDialog.tsx`: the dialog, with its own modal keys for now (Return in the field presses
  Change ID, Tab and ⇧Tab go round every control, Esc cancels, no key reaches the page). It moves onto P0b-1's
  `useModalKeys` once that lands.
- `CodeView.tsx`: the file list's right-click menu, and the dialog mounted.

**Tests**

| Test | Result |
|---|---|
| Rust `rekey_tests.rs`: `rekey_moves_code_data_and_layout`, `the_channel_key_follows_only_when_it_equals`, `a_taken_key_is_refused`, `an_invalid_key_says_what_an_id_is`, `undoing_a_rekey_puts_every_byte_back` | 5 ok |
| `cargo test -p m2-editor -p m2-server`; clippy of both | ok |
| `pl-ids` (new, after `pl-sounds`) | 35 ok |
| `code` 58, `s8-tracks` 75, `s8-mute` 152, `attention` 57, `taste` 39 | ok |

**Red first**

```
rekey against a stub that refuses:  test result: FAILED. 0 passed; 5 failed
FAIL  a right-click on tracks/bass.ts offers Change Bass's track ID… ()
```

**What's left:** see part 2.

**For gaps.md:** #10's rest is done: a track's ID can be changed from the Code view, one undo step. Left: Restore
(DIAG-6), and keeping a re-keyed track's sounds playing (design-8b §11.3).

**Contract:** ID-17's Today: the key rename is built (Code view, "Change track ID…"). WRITE-10: a row for it, with
the expectation above (Confined, or checked and committed as seen when a file reading it isn't edited).

### A5, part 2: a track's view settings follow its new ID

**In short:** a track keeps its height, its lanes' heights and its hidden lanes when its ID changes, and again when
that is undone.

**What changed**

- Lane B's view store (`2a6d3c1`, the same commit lane D took) is in this branch, as there was no P0b-2.
- `web/src/lib/rekey.ts`: when the window follows a track from one ID to another (the change, its undo, a rename by
  hand), it also renames the track's entries in the view settings: `heights` (`bass`, `bass.cutoff`) and `hidden`
  (`bass.cutoff`), through `setLocal`, so `.music2/view.json` follows within 300 ms. **This differs from X20**, which
  put the follow in the server's actor with a `local_stamp` for the page to seed again: the page owns these
  settings (it writes them, 300 ms after a change), so a server-side rename would be written back over by the
  page's own copy unless `local.ts` (lane B's) learned to seed again. Following in the page covers the same three
  cases with no change to lane B's store. What it doesn't cover: a re-key while no window has the song open (an
  agent's Tx): `view.json` then keeps the old ID's settings, which simply stop applying.
- The dialog keeps its own modal keys: lane C's `useModalKeys` (`web/src/lib/modal.ts`) has the same rules. After the
  merge, `TrackIdDialog` can take it in a line.

**Tests:** `pl-ids` 3 more checks (38): the song opens again with Bass's height (72), its Cutoff lane's (64) and that
lane hidden, from `view.json`; after the change `view.json` has them under `low`, after Undo under `bass` again.

**Red first** (on the page from before, `837150f`):

```
FAIL  its height, its lane's and its hidden lane follow it in view.json ({"heights":{"bass":72,"bass.cutoff":64,"kick":120},"hidden":["bass.cutoff"]})
```

## Lane B

Timeline: zoom, follow the playhead, track heights, markers, repeat a clip. Branch `polish-b`,
worktree `wt/pl-b`.

There was no P0 when this lane started. Lane B built the seams it needs itself: the view store
(P0b-2), its keys and menu rows (P0a's, for its own ids only), in its own files where it could and
in small blocks of the shared ones.

### View settings kept per song (P0b-2's view store)

**In short.** The app can now keep how you look at a song: the zoom, the track heights and so on.
They are kept in the song's own `.music2/view.json`, on this computer only. They come back when the
song opens again.

| What | How |
|---|---|
| Where | `.music2/view.json`, beside the loop it already kept. Never shared, never an undo step |
| Which settings | `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition`. Any other name is refused |
| Read-only window | The settings work, but only in memory: the file is the writer's |
| Two writes at once | One lock in the server, and each write has its own temporary file: no write loses another's |

**What changed**

- `crates/server/src/graph/view_file.rs`: `set(dir, changes)` merges a map in (null takes a key
  out), under one lock with a temporary name per write; `write` (the loop) goes through it.
  `local(dir)` reads the page's keys. `Graph::set_view(session, changes)` lives here.
- `graph.rs`, `actor.rs`: the published project holds `local`, read from `view.json` at the open;
  the state carries it as `local`.
- `crates/server/src/routes_view.rs` (new): `POST /api/graph/view {session, set}`. Not a write.
- `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `getLocal`, `setLocal(key, value)`,
  `flushLocal()`. Seeded from the state's `local` when the song changes. Sent 300 ms after the last
  change; sent at once before Close Song and on `pagehide` (X37).

**Tests.** Rust, `crates/server/src/graph/view_file_tests.rs`:
`view_settings_merge_and_keep_the_loop` (a set, a null, a refused `loop` and unknown key, another
session refused, kept over a reopen) and `a_read_only_window_keeps_view_settings_in_memory`.
`cargo test -p m2-server --lib view_file`: 2 passed. Clippy of `m2-server`: clean.

**Red first.** With `set_view` a stub that refuses:
`panicked at crates/server/src/graph/view_file_tests.rs:43:95` and `:75:87` (the set is refused).
The old server answers the route 404.

**Left.** Lane C's grid and audition, and lane D's hidden lanes, can use `useLocal` and `setLocal`
with their keys. A song opened from the desktop's Open while a setting waits its 300 ms loses it.

**For gaps.md.** View settings are kept per song (Q16): zoom, follow and heights so far.

**Contract.** FMT-20 and the HOME table: `view.json` also keeps `zoom`, `follow`, `heights`,
`hidden`, `grid`, `audition`, set through `POST /api/graph/view`.

## Lane C

Songs and tracks.

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
