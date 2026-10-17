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

### For the merge (lane A's blocks in files others touch)

| File | Lane A's block | Meets |
|---|---|---|
| `crates/editor/src/lib.rs` | `pub mod swap; pub mod note_names; pub mod rekey;` after `pub mod clips;` | every lane's own `mod` lines: keep all |
| `crates/editor/src/tracks.rs` | `taken`, `channel_keys`, `quoted`, `prop_key` made `pub(crate)` (P0a's X10 would make all of them so) | none |
| `crates/editor/src/timeline.rs` | `TimelineLane::Notes.names` (left out of the JSON when empty), `lane()` takes the revision | lane B's `markers`, lane D's `key` on `TimelineView`: other lines |
| `crates/server/src/lib.rs`, `routes.rs`, `graph.rs` | `mod routes_mix;`, its `.merge(…)`, `mod instrument; mod rekey;` | every lane's own lines: keep all |
| `crates/server/src/graph/view_file.rs`, `routes_view.rs`, `web/src/lib/local.ts`, `GraphCanvas.tsx` | lane B's view store, its commit `2a6d3c1` taken as it is (lane D took it too) | lanes B and D: the same change |
| `apps/music2-desktop/src/menu.rs` | `change_instrument` in `ITEMS` after `rename`, in Track after Rename; test `change_instrument_follows_rename_in_track` | lane B's View and Transport rows, lane C's File rows: other places |
| `web/src/App.tsx` | `case "change_instrument"` after the mute group | lane B's zoom and marker ids, lane D's quantize: other cases |
| `web/src/lib/shell.ts` | `canvas.changeInstrument` after `muteInSong` | lane B's hooks at the end |
| `web/src/components/NoteView.tsx` | `keysW` (and its ref) for `KEYS_W`; `rowName` in the note label and the two tooltips; `Keys` gets `names` and `width` | lane D's menu, tools, Q and key shading (D reads the lane's `names` for X26: the same field) |
| `web/src/components/Tracks.tsx` | `InstrumentPicker`'s change mode, `TrackMenu`'s row, `openChangeInstrument`, the `canvas.changeInstrument` hook | lane C's delete question (`DeleteDialog`, its keys): other functions |
| `web/src/components/CodeView.tsx` | the file list's right-click menu and `<TrackIdDialog />` | lane B's two Monaco chord names (X5): other lines |
| `e2e/lintlib.mjs` | lane B's file, byte for byte | lane B |
| `e2e/run.mjs`, `e2e/README.md` | `pl-mix`, `pl-sounds`, `pl-ids` after `s8-mix`; the `s8-mix` row's words | other lanes' rows at their anchors |
| `e2e/s8-mix.mjs` | the Tab order (pan, M, S, fader) and the strip menu's 5 rows | none |

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

### B1 and B3. Zoom, and follow the playhead

**In short.** You can zoom the timeline in and out. While a song plays, the view moves along with
the playhead. You can turn that off. Both are kept with the song.

| Do this | Mac | Linux | What happens |
|---|---|---|---|
| Zoom in or out | ⌘= / ⌘− (⇧⌘= too) | Ctrl+= / Ctrl+− | Around the playhead when you can see it; else around the middle |
| Zoom around the pointer | pinch, or ⌘-scroll | Ctrl-scroll | The beat under the pointer stays under it |
| Zoom to fit | Z | Z | The selected clips fill the view; with none selected, the whole song. Z again goes back |
| The buttons | right end of the ruler | same | Follow, Zoom out, Zoom in. They stay in view as you scroll |
| Follow the playhead | ⇧⌘F | Ctrl+Shift+F | When the playhead passes 85% of the view, the view jumps so it sits at 15% |

- The zoom goes from 2 to 192 px a beat. Each step is ×1.5. It starts at 24.
- Bar numbers thin out when bars get narrow: every 2nd, 4th or 8th bar under 40, 20 and 10 px.
  Beat lines go under 6 px a beat; bar lines stay.
- The grid's Auto follows the zoom: 1/8 at 24 px a beat, 1/16 at 54.
- Scrolling sideways by hand while it plays pauses following. It starts again when play starts
  again, or when the playhead comes back into view.
- In the Notes roll (lane D's part), ⌘= and ⌘− ask the roll first (`editorHooks.zoom`).

**What changed**

- `web/src/lib/zoom.ts` (new): the zoom and the follow flag, over `lib/local` (`zoom`, `follow`).
- `SongTimeline.tsx`: `PPB` (24, used in 49 places) is the zoom now, read with `useZoom()` in each
  component and `zoomNow()` in the edits. `zoomTo` keeps a beat in place; the lanes keep room for
  it (`spare`), so a zoom near the song's end still holds the beat. A wheel with ⌘ or Ctrl, and
  WebKit's `gesture*` events (with `preventDefault`), zoom around the pointer. `ViewTools`: the
  ruler's group, `sticky right-0` with its own 1 px edge (X25). Following pages on each frame from
  the clock; the lanes are a view wider while it follows, so a page can always put the playhead
  at 15%.
- Keys and rows (P0a's, for lane B's ids): `App.tsx` `chord()` (=, +, −, ↑, ↓, ⇧F) and `key()` (Z,
  ⌥←, ⌥→ with `preventDefault`); `appCommand` cases; `shell.ts` `canvas.zoom`, `zoomFit`,
  `follow`, `height`, `marker`; `graph.ts` `editorHooks.zoom`; `CodeView.tsx`'s two Monaco names
  for ⌘↑ ⌘↓ (X5); `menu.rs` rows Zoom In, Zoom Out, Zoom to Fit, Taller Tracks, Shorter Tracks
  (View, first), Follow Playhead, Add Marker, Previous Marker, Next Marker (Transport), and a test.
  `Shortcuts.tsx`: rows for zoom and follow.
- `e2e/lintlib.mjs`: the taste lint as a function (P0b-1's), for the new suites.

**Tests.** Suite `pl-zoom` (kind `fix`, 43 checks, after `s8-automation`). The menu rows:
`music2-desktop` `zoom_heights_follow_and_markers_have_their_rows` (9 menu tests pass). Moved
suites, all green with their counts: `taste` 39, `s8-clips` 94, `s4-clips` 28, `s8-automation` 45,
`s8-songbar` 82, `layout` 39, `views` 30, `gestures` 30, `pc-keys` 37, `code` 58. Clippy of
`m2-server` and `music2-desktop`: clean.

**Red first** (the page and server of `29430e8`):
`FAIL the ruler has Zoom in and Zoom out`, `FAIL ⌘= zooms in: clip width 766 → 766`, then
`No element found for selector: [data-testid=zoom-in]`.

The buttons are named "Zoom the timeline in" and "Zoom the timeline out" for screen readers: the
Device view's graph has its own Zoom in and Zoom out (`ports` clicks those by name).

**Left.** A trackpad pinch is checked only as Chrome's Ctrl-wheel; the Mac's `gesture*` events
need the Mac run. Follow is a plain menu row, not a ticked one.

**For gaps.md.** Zoom (#9) and follow the playhead are done.

**Contract.** None beyond the view store's.

### B2. Taller tracks

**In short.** You can make a track taller or shorter. Drag the bottom edge of its name box. Or use
the keys. Automation lanes have the same edge, so they are easier to draw on.

| Do this | Mac | Linux | What happens |
|---|---|---|---|
| Drag a header's bottom edge | | | 28 to 160 px, in 4 px steps |
| Double-click that edge | | | Back to the usual 44 px (32 for a lane) |
| Taller / Shorter Tracks | ⌘↓ / ⌘↑ | Ctrl+↓ / Ctrl+↑ | Steps 28, 44, 72, 120: the selected track, or every track when none is selected |

- A track's other notes rows take its height. An automation lane goes from 24 to 160 px.
- In a row 44 px or taller, the name, ⋯, M, S and the meter stay in its top 44 px, as they were.
  In a shorter row they are centred, and the meter is the row's height less 8 (X24).
- Kept per song in `view.json`: `heights: {"bass": 72, "bass.cutoff": 64}`. One at its usual
  height isn't kept.
- The Song's row at the foot keeps its usual heights.
- In the code editor ⌘↑ and ⌘↓ are the editor's own (to the top and foot on a Mac, a line's
  scroll on Linux); in a text field, to its start and end.

**What changed.** `web/src/lib/heights.ts` (new): the store over `lib/local`, the steps and the
snap. `SongTimeline.tsx`: each track's rows take their heights; `HeightEdge`, a 6 px
`row-resize` strip at the foot of a track's header and an automation lane's; `LaneLabel` keeps to
the row's top; `canvas.height` for ⌘↑ ⌘↓. `TrackHeader.tsx`: its parts in a box the row's top
44 px tall (X24).

**Tests.** `pl-zoom` grows to 62 checks: the edge (6 px, at the foot, its tooltip); a 28 px drag
makes Bass 72 px, its clip 64 px tall and Keys 28 px lower; M and S 12 px and the meter 8 px from
the top, the meter 28 tall (X35); kept in `view.json`; a double-click back to 44, and nothing
kept; ⌘↓ and ⌘↑ on Kick; ⌘↑ on Hats to 28, M centred, the meter 20 tall; with none selected ⌘↓
makes every track 72; Bass's Cutoff lane dragged to 64; both kept, and back after a reopen; lint.
Moved suites green: `s8-mute` 152, `s8-meters` 24, `layout` 39, `s8-clips` 94, `s8-tracks` 75,
`s8-automation` 45, `taste` 39.

**Red first** (the page of `fddb62e`): `FAIL Bass's header has an edge to drag: 6 px at its foot`,
then `failed to find element matching selector "[data-height-edge="bass"]"`.

**Left.** The roll's own row height (its vertical zoom) is lane D's and later.

**For gaps.md.** Track heights (#17's thin lanes too) are done.

**Contract.** None beyond the view store's.

### B5. Markers

**In short.** You can put markers on the ruler: Intro, Verse, Chorus and so on. They show where
each part of the song starts. They are saved in the song, so they are shared.

| Do this | How |
|---|---|
| Add one | Right-click the ruler: "Add marker at bar 9". It opens as a field called "Marker 1", with a short list: Intro, Verse, Pre-chorus, Chorus, Bridge, Drop, Outro |
| Name it | Type and press Return, or pick from the list. A click away keeps "Marker 1". Esc adds nothing |
| Go to it | Click it. Or ⌥← and ⌥→ (Alt+← and Alt+→) for the one before or after |
| Move it | Drag it: it lands on the grid. Hold ⌘ (Ctrl on Linux) to place it freely |
| Rename it | Double-click it |
| Its menu | Right-click it: Rename, Delete, Loop to next marker (to the next marker, or the song's end) |

- Each change is one undo step, with no toast. Undo names it: "Undo Chorus at bar 17", "Undo moving
  Chorus", "Undo renaming Verse", "Undo deleting Marker 1".
- A new marker is written once, when its name is kept (X19): one Undo takes it away.
- Two markers on one beat are refused: "A marker is at bar 9 already". The ruler's row says so too.
- While `layout.json` doesn't read, no markers show, and adding one is refused: "layout.json doesn't
  read: fix it, or reset the layout".
- Transport ▸ Add Marker adds one at the playhead's bar.

**What changed**

- `crates/editor/src/markers.rs` (new): `Marker {beat, name}`, `read(&Layout)` (well-formed entries
  by beat), `on_disk(dir)`, and `set(s, list)`: the whole list, checked, sorted, written into
  `Layout.markers` by `canonical()`, as one layout-only Tx with its label from the change. Whole
  beats are written without `.0`. `layout.rs`: `layout_tx`, `commit` and `unreadable` are
  `pub(crate)`.
- `timeline.rs`: `TimelineView.markers`, in `timeline()` and `last_known()`; `actor.rs` publishes
  it in the timeline's head.
- `crates/server/src/graph/markers.rs` (new): `Graph::markers`. `routes_view.rs`:
  `POST /api/graph/markers {session, markers}`, a write (423 read-only), with its own `writes`
  layer, as `routes_8b.rs`.
- `web/src/lib/markers.ts` (new): the call, the next free "Marker N", the one before and after, the
  open field (`useMarkerEdit`), the list drawn until the timeline shows it.
  `web/src/components/Markers.tsx` (new): the flags, their drag, menu and the name field.
  `SongTimeline.tsx`: the ruler draws them; `canvas.marker` (Add, Previous, Next); ⌥→ twice goes on
  to the second marker even before the clock has heard of the first. `SongBar.tsx` `RulerMenu`:
  the Add marker row. `Shortcuts.tsx`: ⌥← ⌥→.
- `e2e/s8-songbar.mjs`: its check of the ruler menu's rows names the third row.

**Tests.** Rust, `crates/editor/src/markers_tests.rs`: `markers_are_written_sorted_into_layout_json`
(and each change's label, no step when nothing changes, no key when none are left),
`a_second_marker_on_a_beat_is_refused`, `a_name_of_two_lines_is_refused` (and empty, 41
characters, a beat before 0; 40 é is fine), `markers_keep_unknown_keys_and_the_key`,
`undo_puts_the_old_list_back`, `an_unreadable_layout_refuses`. `m2-editor` 257 tests and
`m2-server` 220 pass; clippy of both clean. Suite `pl-arrange` (kind `fix`, 44 checks, after
`pl-zoom`). Moved suites green: `s8-songbar` 82, `layout` 39, `s8-tracks` 75, `taste` 39,
`pc-keys` 37.

**Red first.** Rust, with `set` a stub that refuses: all six fail (`markers_tests.rs:18:88`,
`:52:5`, `:67:9`, `:82:56`, `:97:40`, `:118:5`). The suite, on the page and server of `29430e8`:
`FAIL the ruler's menu has Add marker at bar 5: Add tempo change at 5.1 | Add time signature
change at bar 5`.

**Left.** Transport ▸ Add Marker is checked only on the desktop app (no browser suite reaches the
menu bar). A marker keeps no fields but its beat and name. Markers don't move with a tempo or
meter change.

**For gaps.md.** Markers on the ruler (#23's markers) are done; sections (copy a chorus, insert
bars) are left.

**Contract.** FMT-19: `markers` is `[{ "beat", "name" }]`, sorted, one a beat, names one line of 1
to 40 characters, written by `POST /api/graph/markers`, not `/api/graph/layout` (X45). WRITE-10's
route list gains it.

### B4. Repeat a clip

**In short.** You can repeat a clip a few times in one go. Right-click it and pick Repeat…. Say
how many more times. The copies follow one after another.

| What | How |
|---|---|
| Where | The clip menu, after Duplicate |
| The question | "Repeat [3] more times" and a Repeat button. Return repeats; Esc closes |
| How many | 1 to 64. Anything else: "Repeat 1 to 64 times", and the button is off |
| What it makes | Copies right after the clip (or the selected clips, or a marquee's bars), each a new clip with its own ID. They are selected after |
| Undo | One step for all of them: "Undo repeating a Bass clip". No toast |

**What changed.** `crates/editor/src/clips.rs`: `Gesture::Duplicate` gains `times` (1 to 64,
`REPEATS`; one when not given); each copy lands right after the one before. `apps/m2/src/edits.rs`:
`m2 clips duplicate` sends no `times` (one line; `m2` commands for it are left, gaps #37).
`web/src/lib/clips.ts`: the type. `SongTimeline.tsx`: ⌘D and Repeat… share `copies(times)`; the
clip menu's Repeat… row; `RepeatPopover`. `e2e/s8-clips.mjs`: its clip menu check names the new
row.

**Tests.** Rust, `clips_tests.rs`: `duplicate_three_times_puts_three_copies_after` (spans, fresh
ids, the copies play the first one's notes, one undo), `repeat_of_a_marquee_range_keeps_its_bars`,
`sixty_five_times_is_refused` (0 and 65 refused, 64 made). `m2-editor` 260 tests pass, `m2`'s pass;
clippy of `m2-editor`, `m2-server`, `m2` clean. `pl-arrange` grows to 54 checks. Moved suites
green: `s8-clips` 94, `s4-clips` 28, `taste` 39.

**Red first.** Rust, with `times` ignored (one copy): the three fail, `left: [(0.0, 4.0), (4.0,
4.0)]`, `right: [(0.0, 4.0), (4.0, 4.0), (8.0, 4.0), (12.0, 4.0)]`. The suite on the page of
`61bcea4`: `FAIL the clip menu has Repeat… after Duplicate: cut, copy, paste, duplicate, split,
quantize, delete, loop, open`.

**Left.** No key for Repeat… (⌘D repeats once).

**For gaps.md.** Repeat a clip (#23's loop-a-clip-n-times) is done.

**Contract.** The clips route's `duplicate` gesture takes `times` (1 to 64).

### Lane B: the last run

Every browser suite but `live3b` (it needs plugins) ran on lane B's tip, in three batches: all
green with their counts, but `ports` (2 failed: it clicked the timeline's new Zoom in, by the same
name as the graph's). The buttons got their own names; `ports` 46 and `pl-zoom` 62 then passed.
Rust: `m2-editor` 260, `m2-server` 220, `m2`, `music2-desktop`'s menu tests; clippy of all four
clean. The full `ci/check.sh` is the End's.

**For the merge: the shared files lane B touched** (each in a small block of its own):

| File | What |
|---|---|
| `web/src/App.tsx` | `chord()`: =, +, −, ↑, ↓, ⇧F (and `page` for ⇧⌘=); `key()`: Z, ⌥←, ⌥→; `appCommand` cases for lane B's nine ids |
| `web/src/lib/shell.ts`, `web/src/lib/graph.ts` | `canvas.zoom`, `zoomFit`, `follow`, `height`, `marker`; `editorHooks.zoom` (lane D fills it) |
| `web/src/components/CodeView.tsx` | Two `MONACO_CHORDS` names, `shorter` and `taller` (X5) |
| `web/src/components/GraphCanvas.tsx` | `closeSong` sends waiting view settings first |
| `web/src/components/SongBar.tsx` | `RulerMenu` only: `onMarker` and its row |
| `web/src/components/Shortcuts.tsx` | Rows at the ends of Transport and View |
| `apps/music2-desktop/src/menu.rs` | Lane B's nine rows and their test |
| `crates/server/src/graph.rs`, `graph/actor.rs` | `Published.local`, the state's `local`; `mod markers`; the timeline's `markers` |
| `crates/server/src/lib.rs`, `routes.rs` | `mod routes_view`; its router merged |
| `crates/editor/src/lib.rs`, `layout.rs`, `timeline.rs` | `mod markers`; `layout_tx`, `commit`, `unreadable` made `pub(crate)`; `TimelineView.markers` |
| `apps/m2/src/edits.rs` | `times: None` in the duplicate gesture |
| `e2e/run.mjs`, `e2e/README.md` | `pl-zoom` and `pl-arrange` after `s8-automation` |
| `e2e/s8-songbar.mjs`, `e2e/s8-clips.mjs` | One check each names the new menu row |
| `e2e/lintlib.mjs` | The taste lint as a function (P0b-1's; lane A has the same) |

## Lane C

Songs: recent songs, Save As, Rename Song, the delete question's keys, view settings kept.

There was no P0 when this lane started: lane C built the seams it needs itself, in its own files
where it could (`lib/modal.ts`, `DeleteDialog.tsx`), and in small blocks of the shared ones.

### C5. The delete question's keys

**In short.** The delete question now works like the export question. Return presses the button
with the focus ring. Tab moves the ring. No key gets through to the song behind it.

| Key | Before | Now |
|---|---|---|
| Return | always deleted | presses the focused button (Delete at first) |
| Space | played the song | presses a focused button; else nothing |
| Tab, Shift+Tab | left the dialog | go round its two buttons |
| M | muted a track behind it | nothing |
| Esc | cancelled | cancels |

**What changed**
- `web/src/lib/modal.ts` (new): `useModalKeys(root, { onEscape, onDefault, active })`, the keys of
  every modal dialog. It is `ExportAsk.tsx:47-66`'s logic, fixed for dialogs with fields (plan X14):
  Tab goes round every control (buttons, fields, boxes); a field keeps its own keys (Space types a
  space); Return in a field or a box, or with the focus on nothing, runs `onDefault`; Return on a
  button presses that button. No key reaches the page.
- `ExportAsk.tsx` uses it, with the same behaviour (`s8-mute`'s 152 checks pass).
- `web/src/components/DeleteDialog.tsx` (new): the delete question, moved out of `Tracks.tsx`
  (`joined`, `said`, `DeleteDialog`), on `useModalKeys`. Cancel got `data-testid=delete-cancel`.
  `Tracks.tsx` only exports `Asking` and `what` and imports the dialog: lane A's parts
  (`InstrumentPicker`, `TrackMenu`) are not touched.

**Tests.** New suite `pl-tracks` (kind `fix`, 20 checks), after `s8-tracks` in `run.mjs`: Delete has
the focus at first; Space and M with the focus on nothing do nothing behind it; Tab and Shift+Tab go
round the two buttons; Return on Cancel and Space on Cancel delete nothing and leave the start
point at beat 8; Esc cancels; Return on Delete deletes Kick and its Duck; ⌘Z puts every byte back;
the dialog passes the taste lint. Moved and green: `s8-tracks` 75, `s8-mute` 152, `taste` 39.

**Red first** (the page at `29430e8`):
```
FAIL  Space with the focus on nothing does nothing: nothing plays, nothing is deleted
FAIL  Tab and Shift+Tab go round the two buttons, never behind the question: BODY → browser-toggle → BODY → delete-confirm
FAIL  Return on Cancel deletes nothing: kick.ts is gone
```

**Left.** Nothing.

**For gaps.md.** The delete question's keys are fixed (8b-ui's loose end).
**Contract.** None.

### C1. View settings kept

**In short.** The grid and snap of the timeline and of Notes, and the audition switch, are now kept
with each song. Open the song again, even after quitting the app: they are as you left them. A new
song starts with the usual settings.

| Setting | Before | Now |
|---|---|---|
| Grid and snap (timeline, Notes) | this browser's storage: lost at each app launch | the song's `.music2/view.json` |
| Audition on or off | the same | the same file |

**What changed** (P0b-2's view store, built here as there was no P0)
- Server: `view_file::set(dir, changes)` merges a map into `.music2/view.json` (null removes an
  entry). It and the loop's `write` hold one lock, and each temporary file has its own name (X15).
  `Graph::set_view(session, set)` takes only `zoom`, `follow`, `heights`, `hidden`, `grid` and
  `audition`, refuses `loop` and any other name in words, and in a read-only window keeps them in
  memory only. The state carries them as `local` (read from the file when the song opens, without
  the loop) and `local_stamp` (moves when the server changes them itself; lane A's track ID change
  will use it, X20).
- Route: `POST /api/graph/view {session, set: {key: value | null}}` in `routes_project.rs` (not in
  `routes_view.rs`, which is lane B's). Not a write: no 423 in a read-only window.
- Page: `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `getLocal`, `setLocal(key, value)`
  and `flushLocal()`. Seeded from `graph.local` when the song or the stamp changes. Each change is
  sent at once; changes made while one is on its way go together after it. `pagehide` sends what
  waits with `keepalive`. `flushLocal` is for Save As and Rename (X37).
- `grid.ts` keeps both views' grids in `local.grid` (`{song: {pick, triplet, on}, notes: {…}}`);
  `audition.ts` keeps `local.audition` (`false` when off; on is the default, so it is removed).
  Neither reads `localStorage` now (no backwards compatibility).
- `e2e/s8-clips.mjs`: the two helpers that read the roll's grid from `localStorage` read the state's
  `local.grid.notes` now.

**Tests**
- Rust `crates/server/src/graph/view_file_tests.rs`: `view_settings_merge_and_keep_the_loop` (a
  set beside the loop, a null, the loop route keeping the settings, `loop` and an unknown name
  refused, another session refused, read again at the next open with unknown entries kept in the
  file, a read-only window writing nothing).
- New suite `pl-projects` (kind `none`, 15 checks so far), after `newsong`: it starts its own
  servers on a copy of first-song. Server 1: the roll's grid 1/8, the timeline's snap off, audition
  off, and `view.json` holds them. Server 2 (another port, another browser context): all three are
  as set. Another song opened in the same window starts with the defaults; back to the first, its
  settings come back.
- Moved and green: `s8-clips` 94, `s8-audition` 51, `notes` 52, `taste` 39, `newsong` 71.
- `cargo clippy -p m2-server --all-targets -D warnings` clean. `cargo test -p m2-server`: 218 of 219
  in the full run; `graph::tests::loading_is_published_while_a_plugin_is_created` failed once under
  the machine's load (other tracks building) and passed three times alone. It times a plugin's
  creation; nothing here touches it.

**Red first**
- Rust, with `set_view` a stub that keeps nothing:
  ```
  panicked at crates/server/src/graph/view_file_tests.rs:33:5: assertion `left == right` failed
    left: Object {}
   right: Object {"audition": Bool(false), "grid": Object {"notes": Object {"on": Bool(true), "pick": Number(0.125), "triplet": Bool(false)}}}
  ```
- `pl-projects` on the page and server at `29430e8` (with the old std):
  ```
  FAIL  .music2/view.json holds them: null
  FAIL  the roll's grid is 1/8 after a new launch: 1/16
  FAIL  audition is still off: true
  ```

**Left.** The comment above `s8-clips`' reload check still says "the app starts over"; it no longer
does (one line, lane B's part of that file). Zoom, follow, heights (lane B) and hidden lanes (lane D)
can use the same store: `useLocal("zoom", 24)` and `setLocal("zoom", 36)`.

**For gaps.md.** View settings are kept per song in `.music2/view.json` (8b-ui Q7 closed).
**Contract.** `docs/architecture.md` FMT-20 / the HOME table: `view.json` holds `loop` and the page's
settings `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition`, written by
`POST /api/graph/view`.

### C2. Recent songs

**In short.** The start screen lists the songs you opened lately, newest first. Click one to open
it. The app opens your last song when it starts, if it was still open when you quit. The File menu
has Open Recent.

| Where | What you see |
|---|---|
| Start screen | "Recent": up to 20 songs. Each row: the name, and "124 BPM · opened yesterday". Return opens the newest |
| A song whose folder is gone | its name in grey, "Moved or deleted", and an × to take it off |
| File ▸ Open Recent ▸ | the 10 newest songs, a line, Clear Menu |
| Starting the app | the song that was open when you quit opens again |
| If the app closed while opening it | it starts on the start screen and says: "music2 closed while opening Night Drive last time, so it didn't open it again." |

**What changed**
- `crates/server/src/recent.rs` (new): `Recent` over `~/.music2/recent.json`
  (`{"songs": [{"path", "opened"}], "open", "reopening"}`): `list()` (name from `music2.toml`,
  first tempo from `data/arrangement.json`, `missing` when `song.ts` is gone), `paths()`,
  `opened(dir)` (first, once, 20 at most, `open` true), `closed()`, `forget(dir)`, `clear()`,
  `moved(old, new)` (for Rename, C3), `to_reopen()` → `Song`, `Crashed` or `Nothing`, and the
  crash guard `reopening(dir)` / `reopened()`. Written whole through a temporary file; one change
  at a time. Folders are kept canonical, so `a/../b` and `b` are one song. Times are UTC RFC 3339,
  without a new dependency (a days-to-date function).
- `AppState` (lib.rs): the list is kept only when asked, `keep_recent(file)` (a `OnceLock`, not a
  `ServeOptions` field: no struct literal in another crate changes, `apps/m2/tests/render.rs`
  among them, which is 8a's). `open_song(dir)` opens and records; `opened()` records the song open
  now; `closed()` marks it closed (X29: one way in for every open). `graph_open` uses `open_song`,
  `project_new` calls `opened()`, `graph_close` calls `closed()` (X22).
- Routes (`routes_project.rs`): `GET /api/project/recent`, `POST /api/project/recent/forget {path}`.
- `examples/headless.rs`: keeps the list in `$HOME/.music2/recent.json` (the suites' scratch HOME)
  and opens its song through `open_song`.
- Desktop: `main.rs` keeps the list in `~/.music2/recent.json`; with no folder given it reopens
  `to_reopen()`'s song off the main thread with the guard, or toasts why not; the title thread also
  watches `recent.json`'s time and builds the menu again when it moves. `menu.rs`: File ▸ Open
  Recent ▸ after Open Project Folder…, built by `recent_rows` (a pure function, tested); item ids
  `open_recent:<folder>` (the folder, not an index, so a list that changed since can't open the
  wrong song) and `clear_recent`. `actions.rs`: Open Recent asks about unsaved code first
  (`keep_code`), as Open does; a folder that is gone says so. `plugin_host::open_project` records
  the open.
- Page: `web/src/lib/projects.ts` (new: `recentApi`, `openedWords`, `bpmWords`, its own `request`);
  `StartScreen.tsx`'s `Recent`: a 560 px column of 44 px rows under the folder hint, the folder in
  each row's tooltip, the page scrolls when the list is long. Return opens the newest only while no
  button or field has the focus and no dialog is open (X43).

**Tests**
- Rust `crates/server/src/recent_tests.rs` (temporary folders): `opened_puts_it_first_once`,
  `the_list_keeps_twenty`, `a_crash_while_reopening_is_not_tried_again`,
  `a_song_closed_before_quitting_doesnt_reopen` (X22), `forget_and_clear` (with `moved`),
  `a_list_that_doesnt_read_is_empty`, `times_are_written_in_utc`. `menu.rs`:
  `open_recent_lists_ten_and_clear_menu`.
- `pl-projects` (now 30 checks): three songs made with New Song (120, 96 and the starter's 116 BPM)
  are listed newest first with "… BPM · opened today"; the folder is in the tooltip; a 560 px
  column of 44 px rows; the lint; `recent.json` holds them, and Close Song set `open` false;
  Return opens the newest and `open` is true again; a click opens Alpha, which goes first; a folder
  removed by the suite says "Moved or deleted" in a fainter grey, opens nothing, and its × takes it
  off the list and the file.
- Moved and green: `start` 24 (and 14 on `empty`), `newsong` 71, `fx-fixes` 75, `taste` 39.
- `cargo test -p m2-server` 226 passed; `cargo test -p music2-desktop` (9, and `app_main_thread`
  ok); clippy of both with `-D warnings` clean.

**Red first**
- Rust, with every change of `Recent` a stub that keeps nothing and `to_reopen` always `Nothing`:
  ```
  recent::tests::opened_puts_it_first_once panicked at recent_tests.rs:53:5
    left: []
   right: ["Night Drive", "Chorus Idea", "Sunday"]
  recent::tests::a_crash_while_reopening_is_not_tried_again panicked at recent_tests.rs:84:5
    left: Nothing
   right: Song("/tmp/m2-recent-2458152-0/Night Drive")
  ```
  and `recent_rows` stubbed empty: `open_recent_lists_ten_and_clear_menu`: `left: 0, right: 12`.
- `pl-projects` on the page and server at `29430e8`:
  ```
  FAIL  the start screen lists recent songs
  FAIL  newest first, each with its tempo and when it was opened: []
  ```

**Left.** No picture of each song in the list (taste SS-2's thumbnail; plan §10). The desktop's
reopen at launch and Open Recent are not run here: the End's Linux run on `:96` (two launches with
the same HOME) and the Mac run check them, and whether the menu can be built again while it is
open (plan §11).

**For gaps.md.** Recent songs, Open Recent and reopen at launch are done (#28's "the last song
opens at launch"); the thumbnail is left.
**Contract.** `docs/architecture.md`'s HOME table: `~/.music2/recent.json` (the recent list, the
`open` flag, the crash guard), written by the app and the headless server only.

### C3. Save As, and Rename Song with its folder

**In short.** File ▸ Save As… (⇧⌘S) saves a copy of the song under a new name and opens the copy.
The first song stays as it was. File ▸ Rename Song… changes the song's name, and its folder too if
you tick the box.

| Command | What it does |
|---|---|
| Save As… (⇧⌘S, Ctrl+Shift+S) | "Save a copy of Night Drive": the name ("Night Drive 2"), the folder, [Cancel] [Save Copy]. The copy opens. A toast: "Now working on Night Drive 2. Night Drive is kept as it was." |
| Rename Song… | "Rename Night Drive": the name, and "Also rename its folder" (on). With the box on, the song closes and opens again, so Undo can't go back past it (the dialog says so) |
| A folder name that is taken | "A folder called Summer is already there. Pick another name." Nothing changes |
| Read only | Save As works (it only reads the song). Rename Song says why it can't, in a toast (X21) |

**What changed**
- `project.rs`: `save_as_start(graph)` (the folder the song is in, and the first free "<name> N";
  "Night Drive 2" suggests "Night Drive 3"), `save_as(state, {session, name, dir?})` (refuses a
  taken folder in any capitals before anything is made, copies, opens the copy with `open_song`,
  removes a copy that doesn't open), `copy_into` (the copy: `fill`, as New Song's, so hidden files
  such as `.music2/` and `.git` stay behind and `.gitignore` and the name are written), and
  `rename(state, {session, name, folder})`: the folder name is checked first; then the title Tx
  (one undo step); then, with `folder`, close, `fs::rename` in the same parent, the recent list's
  `moved`, and `open_song` from the new folder. A folder rename that fails leaves the new name and
  reopens the song where it was, with a `warning` (X21). A change of capitals only is allowed.
- `graph/projects.rs` (new, `mod projects;` in `graph.rs`): `Graph::dir_of(session)` and
  `Graph::copy_song`, which copies on the session's actor, between edits.
- Routes (`routes_project.rs`): `GET /api/project/save-as` (where it starts), `POST
  /api/project/save-as {session, name, dir?}` (not a write: read-only windows have it),
  `POST /api/project/rename {session, name, folder}` (a write: 423 read-only).
- Page: `SaveAsDialog.tsx` and `RenameSongDialog.tsx` (new), both on `useModalKeys`, mounted in
  `App.tsx`; `appCommand` `save_as` opens Save As and `rename_song` opens Rename Song (it called
  the top bar's quick rename before; the double-click on the name stays that). `chord()`: ⇧⌘S is
  `save_as`, also from a field. `flushLocal()` before each. Save As shows unsaved code as New Song
  does ("Save Copy Anyway"), and Choose Folder… in the app (the shell's `song_folder`).
- Desktop `menu.rs`: File ▸ Save As… (`CmdOrCtrl+Shift+S`) after Save; Rename Song…'s comment.
  `Shortcuts.tsx`: "Save a copy (Save As)" at the end of File.

**Tests**
- Rust `crates/server/src/project_tests.rs`: `save_as_copies_the_song_without_its_local_folder`,
  `save_as_opens_the_copy_and_leaves_the_original` (and another folder, a taken name, another
  session), `rename_moves_the_folder_and_the_name` (the name only, then with the folder, its
  `.music2/` and the recent list following, then capitals only), `rename_refuses_a_taken_folder`
  (and an empty name), `rename_in_a_read_only_window_is_refused` (and Save As works there).
  `menu.rs`: `save_as_is_shift_command_s_after_save`.
- `pl-projects` (now 58 checks; `PL_ONLY=c1|c2|c3` runs one part, for red runs): ⇧⌘S opens Save As,
  "Save a copy of First Song", "in ~/Music/music2", "First Song 2" focused and selected, the lint,
  Space and M reach nothing; Return makes "First Song 2": `song.ts`, `tracks/` and `data/` byte for
  byte, no view settings, its name in `music2.toml`, open, the song itself as it was, both in Recent,
  the toast. A PC page: Ctrl+Shift+S opens it and the sheet says "Ctrl+Shift+S". In an app page
  (the shell's `menu` event faked, as `newsong` fakes its invoke): Rename Song… opens "Rename First
  Song 2" with the box on and its line, the lint, Space and M reach nothing; a taken folder is
  refused in words and nothing changes; "Night Drive" moves the folder and the name, the song opens
  from there, Recent follows; with the box off only the name changes, one undo step; Save As's
  Choose Folder… puts the copy in the folder picked.
- Moved and green: `taste` 39, `pc-keys` 37, `s8-songbar` 82, `pl-tracks` 20, `start` 24,
  `newsong` 71. `cargo test -p m2-server` 231 passed; `-p music2-desktop` 10 and `app_main_thread`
  ok; clippy of both clean.

**Red first**
- Rust, with `save_as` and `rename` refusing at once:
  ```
  save_as_copies_the_song_without_its_local_folder: called `Result::unwrap()` on an `Err` value: Save As isn't built yet
  rename_refuses_a_taken_folder: left: "Rename isn't built yet"
                                 right: "A folder called Summer is already there. Pick another name."
  ```
- `PL_ONLY=c3 pl-projects` on the page and server at `29430e8`:
  ```
  FAIL  ⇧⌘S opens Save As
  FAIL  the name “First Song 2”, focused and selected: []
  FAIL  Space and M reach nothing behind it
  ```

**Left.** `canvas.renameSong` (`shell.ts`, set in `SongBar.tsx`) is no longer called by
`appCommand`; the top bar's double-click doesn't use it either way. The End can drop the hook (two
lanes' files). `m2` commands for Save As and Rename (gaps #37, plan Q29).

**For gaps.md.** Save As and Rename Song (with its folder) are done.
**Contract.** `docs/architecture.md`: `POST /api/project/save-as` (a copy without `.music2/`,
opened; allowed read-only) and `POST /api/project/rename` (the title Tx, then the folder; Undo
starts over after a folder rename) in the routes list.

### Lane C: review and the app on Linux

**Fixes after reading the lane's diff again**
- Rename Song: a folder that appears between the name check and the rename is not replaced (on
  Linux and macOS, `rename` replaces an empty folder); the song's own folder in other capitals
  still is (a Mac's disk counts them as one name). No test: it needs a folder made inside that
  moment.
- `local.ts`: when the song's settings are read again while a change is on its way, a setting the
  page took out (null) stays out, rather than reading as null.
- `pl-projects` stops the servers it started also when a check throws (a red run left five
  running before). Checked: a run on an empty page throws, and no server is left.
- After these: `pl-projects` 58, `pl-tracks` 20, `s8-audition` 51, `s8-clips` 94, `taste` 39;
  `cargo test -p m2-server` 231 and `-p music2-desktop` 10 (and `app_main_thread`); clippy clean.

**The desktop app on Linux** (the debug build, on the invisible screen `:96`, a scratch HOME,
`M2_AUDIO=none`; a smoke check, not a suite; the End's run is the full one):

| Launch | What happened |
|---|---|
| `recent.json` says the song was open | the song opened by itself; `reopening` was null again after |
| `reopening` left set, as after a crash | the start screen, the toast "music2 closed while opening first-song last time, so it didn't open it again.", and `open` false |
| File menu | Open Recent ▸ (first-song, a line, Clear Menu) after Open Project Folder…; Save As… Shift+Ctrl+S after Save; the menu was built again after the reopen wrote the list |

## Lane D

Notes and automation: the note menu and tools, the song's key, automation on its own scale, hiding
one automation lane.

There was no P0 when this lane started: lane D made the seams it needs itself, in its own files
where it could, and in small blocks of the shared ones (each named below).

### D1. A menu on notes, and note tools

**In short.** Right-click a note in Notes. A menu opens. It can move notes up or down, put them on
the grid, make them longer, loosen them up, or swing them. Each one is one undo step.

| Row | Key | What it does |
|---|---|---|
| Up / Down a semitone | ↑ / ↓ | moves the notes one key |
| Up / Down an octave | ⇧↑ / ⇧↓ | moves the notes 12 keys |
| Quantize to 1/16 | Q | puts each note's start on the grid |
| Legato | — | each note lasts until the next one starts |
| Humanize | — | moves timing (up to 10 ms) and loudness (up to 8%) a little |
| Swing: Light · Medium · Heavy | — | every second note a little later (55%, 60%, 66%) |
| Duplicate | ⌘D | copies the notes after them |
| Delete | ⌫ | takes the notes out |

- The menu acts on the selected notes. With none selected, it acts on every note: its first line
  says "All 12 notes". Then it shows no arrow keys, since those move only selected notes (X28).
- Right-click on a note that isn't selected selects it, alone. On empty grid the selection stays.
- Read only (the code doesn't compile): the menu opens, every row greyed, its tooltip saying why.
- Esc closes it. The roll keeps the keyboard all along.

**What changed**
- `web/src/lib/noteTools.ts` (new, pure): `transpose`, `quantize`, `legato`, `humanize`, `swing`.
  Each takes the notes and the picked indices and gives the notes after, or null when nothing
  changes. Swing **sets** each off-beat note to its swung place (X27): twice is the same as once,
  and Light after Heavy moves them back. A note is off-beat when it starts from a quarter step
  before its pair's middle to just past Heavy's place (1.35 steps in), so a change of amount finds
  them. A swung note keeps its end, so it never runs into the next note. Pairs are eighths when the
  roll's grid is 1/8 or wider, else sixteenths (Q26).
- `web/src/components/NoteMenu.tsx` (new): the menu, on `Popover` and `MenuRow` (taste.md §2.14),
  the Swing row a segmented control, the tooltips in X41's words.
- `web/src/components/NoteView.tsx`: the grid's right-click; Q (the selection, or every note);
  each tool one lane write with its own undo name ("transposing Bass notes", "quantizing Bass
  notes to 1/16", "making Bass notes legato", "humanizing Bass notes", "swinging Bass notes": the
  house's "Undo renaming the song" style, not the plan's "Transpose Bass notes"). The arrow keys
  ignore ⌥ (X1). A Ctrl-click on a Mac starts no gesture: it is a right-click there (X7). The
  grid's tooltip ends "right-click for note tools". Hooks: `editorHooks.quantize` (X2) and
  `editorHooks.zoom` (X3, the roll's own zoom ×1.5) while the roll has the keyboard.
- Shared blocks: `lib/graph.ts` declares the two hooks; `App.tsx`'s `quantize` asks
  `editorHooks.quantize` first (X2). `Shortcuts.tsx`: two rows at the end of Edit.
- `crates/editor/src/notes.rs` (outside the plan's list; no lane owns it): `describe` gave "4 notes
  → 4 notes" for any edit of as many notes either side. The page names an undo step only when the
  two sides differ, so every multi-note edit in the roll (a tool, a drag of several notes) had an
  unnamed Undo. Now it adds what tells them apart: their pitches ("4 notes, A1 to C2 → 4 notes, A2
  to C3"), where the first plays, their lengths or their velocities; else ", changed".

**Tests**
- New suite `pl-notes` (kind `fix`, 61 checks with D1), after `s8-audition`: see `e2e/README.md`.
- Rust: `notes::tests::an_edit_is_described_by_what_it_changed` gains five cases.
- Moved and green: `notes` 52, `fixes-notes` 37, `s8-audition` 51, `pc-keys` 37, `taste` 39,
  `s8-clips` 94, `s4-clips` 28, `g11-play` 24, `undo` 10. `m2-editor`: 251 tests and clippy.

**Red first** (the page and server at `29430e8`; then the page without X1 and X7; then the
server without the label change):
```
FAIL  right-click on a note opens its menu
FAIL  its rows, in order, with their keys: []
FAIL  ⌥→ and ⌥← with notes selected move no note: they are the markers' keys
FAIL  a Ctrl-drag on a note on a Mac moves nothing: Ctrl-click is a right-click there
FAIL  Undo names it: Undo (⌘Z)
  left: "4 notes → 4 notes"
 right: "4 notes, A1 to C2 → 4 notes, A2 to C3"
```

**Left.**
- `App.tsx`'s `zoom_in` and `zoom_out` (lane B's or the lead's) must ask `editorHooks.zoom?.(dir)`
  first, so ⌘= and ⌘− zoom the roll while it has the keyboard (X3). The hook is there.
- Edit ▸ Quantize through the menu bar is checked only in the app (no browser suite has the menu
  bar): the End's Linux run.

**For gaps.md.** Note tools are done: a right-click menu with transpose, quantize (Q), legato,
humanize and swing. Left: swing amounts other than three, groove templates.
**Contract.** None.

### D2. The song's key

**In short.** The top bar has a Key field after the time signature. Click it and pick a key, like
"A minor". Notes then shades the rows that are not in the key. Nothing is locked. The key is saved
in `layout.json`, so it goes with the song.

| What | How |
|---|---|
| Pick a key | Click "Key", choose Major or Minor, then a root (C, C♯, D, E♭, E, F, F♯, G, A♭, A, B♭, B) |
| Change Major / Minor | With a key set, Major or Minor changes it at once |
| Take it out | No key |
| Undo | ⌘Z: "Undo key A minor" |

**What changed**
- `crates/editor/src/songkey.rs` (new): `SongKey { root, scale }`, `read(&Layout)`, `of(dir)`,
  `set(s, Option<SongKey>)`. One Tx of `layout.json` alone (no revision, one undo step), as
  reorder is: `Expect::Identical`, based on the file on disk. Roots read by any name (`C#`, `C♯`,
  `Db`, `D♭`) and are written in ASCII (`C#`, `Eb`, …) so a hand or a script can write them; the
  label uses ♯ and ♭ ("E♭ major"). Scales: major and minor. The key is kept in `Layout.other`, so
  `layout.rs` didn't change: a new key goes right after `markers`, and an old one stays in its
  place. Refused while `layout.json` doesn't read (R15). Setting the key it has writes nothing.
- `TimelineView.key` (`timeline.rs`, its three places), published in the timeline's head
  (`graph/actor.rs`). The watcher already republishes when `layout.json` changes, so a key written
  by hand shows.
- `crates/server/src/graph/songkey.rs`: `Graph::song_key`. `crates/server/src/routes_notes.rs`
  (new router, merged in `routes.rs`): `POST /api/graph/song-key {session, key: {root, scale} |
  null}`, a write (423 read only). Named `song-key`, apart from a track's ID (X42).
- Web: `lib/songKey.ts` (the call, the undo name, the pitch classes in a key, `TimelineView.key`
  by `declare module`); `KeyField.tsx` (the field and its picker); `TopBar.tsx` mounts it in the
  tempo group and **measures** the name's room (Q23): twice the distance from the bar's middle to
  the nearer group, less 8 px a side, from the free space between the groups (a `ResizeObserver`).
  Under 56 px the name goes. `SongBar.tsx`: `SongTitle`'s field keeps to that room (160 to 480 px
  when there is room). `NoteView.tsx`: `Lanes` shades the pitch classes out of the key (the grid
  says which in `data-shaded`); a lane that names its drums (lane A's `names`) is never shaded by
  the key (X26).

**Tests**
- Rust `crates/editor/src/songkey_tests.rs`: `a_key_is_written_into_layout_json`,
  `no_key_removes_it`, `markers_and_unknown_keys_stay`, `a_scale_that_isnt_major_or_minor_is_refused`,
  `undo_puts_it_back`, `an_unreadable_layout_refuses`.
- `pl-notes` grows to 98 checks (see `e2e/README.md`). The test key is E minor (X32): F♯'s row
  plain, F's shaded, the other way round from the black keys.
- Moved and green: `s8-songbar` 82, `fx-fixes` 75, `taste` 39, `layout` 39, `notes` 52. `m2-editor`
  257 and `m2-server` 218 tests, clippy on both.

**Red first** (the tests against a stub `set` that refuses; the suite on D1's page):
```
test songkey::tests::a_key_is_written_into_layout_json ... FAILED
test songkey::tests::undo_puts_it_back ... FAILED
FAIL  the top bar has a Key field
FAIL  with no key, the roll shades the black keys' rows: F♯ (66) shaded, F (65) plain
```

**Left.**
- More scales, scale lock, chords (scale help, gaps #16).
- X26 is in the code but untested here: no lane names its drums until lane A lands.

**For gaps.md.** #6 (key and scale): the song's key is done, major and minor, in the top bar and
`layout.json`; Notes shades the rows out of it. Left: other scales and scale help (#16).
**Contract.** FMT-19: `layout.json` gains `key: {"root": "A", "scale": "minor"}` (ASCII roots),
written by `POST /api/graph/song-key`, not by `/api/graph/layout`. The timeline view gains `key`.

### For the merge (lane D's blocks in files others touch)

| File | Lane D's block | Meets |
|---|---|---|
| `web/src/App.tsx` | `quantize` asks `editorHooks.quantize` first (X2) | P0a's keys, B's zoom ids: add `editorHooks.zoom?.(dir)` first there (X3) |
| `web/src/lib/graph.ts` | `editorHooks.quantize`, `editorHooks.zoom` | — |
| `web/src/components/Shortcuts.tsx` | two rows at the end of Edit | — |
| `web/src/components/SongTimeline.tsx` | `laneScale` in `pointsFrom`, the hit test, `LaneClips`, `Curve`; hidden lanes in `TrackRows`, `songH`, the reveal effect, the header menu's row | lane B's zoom (`PPB`) and heights (`rowHeight`, `songH`): keep both; hidden lanes count in B's sums |
| `web/src/components/TopBar.tsx`, `SongBar.tsx` (`SongTitle`) | the Key field; the name's measured room | lane C's Save As / Rename (if they touch the title) |
| `web/src/components/NoteView.tsx` | the menu, Q, X1, X7, the key's shading in `Lanes` | lane A's `RollKeys` / `rowNames` (drum names): `Lanes` reads the lane's `names` for X26 |
| `crates/editor/src/timeline.rs`, `graph/actor.rs` | `TimelineView.key` and its head field | lane B's `markers`: both fields |
| `crates/server/src/lib.rs`, `routes.rs`, `graph.rs` | `mod routes_notes`, its merge line, `mod songkey` | every lane's own lines |
| `web/src/lib/local.ts`, `routes_view.rs`, `view_file.rs` | lane B's commit, as it is | lane C's own store: keep one |
| `e2e/s8-automation.mjs` | the header menu's rows include Hide this lane | — |

### The view store, taken from lane B

D4 keeps hidden lanes in the song's `view.json`, through the view store. There was no P0b-2, and
lane B had built it (`2a6d3c1`), so this lane took that commit as it is (`git cherry-pick -x`,
`74fd748`): the same files and lines, so merging both lanes meets no new conflict there. Lane C
built a store of its own with the same calls (`useLocal`, `getLocal`, `setLocal`, `flushLocal`,
`POST /api/graph/view`): the lead keeps one.

### D3. Automation on the control's own scale

**In short.** An automation lane for a frequency (a filter's cutoff) now draws like the knob turns:
low notes get as much room as high ones. The middle of the lane is about 632 Hz, not half of
20 kHz. Other lanes look as before.

| Lane's unit | Scale | Lines between points |
|---|---|---|
| Hz | log, 20 Hz to 20 kHz, wider if a point is outside | curved, as they play (straight in Hz) |
| any other | linear, as before (the server's range) | straight |

**What changed**
- `web/src/lib/laneScale.ts` (new): `laneScale(lane)` → `{log, level, value, round, path}`. By the
  lane's unit only (X17: an input a lane drives has no knob, and first-song's cutoff feeds a
  `blend`, so the knob's range was never there to use). On the log scale a value is rounded to 3
  figures (632, 1.99 kHz); on a linear one as before. `path` cuts each sloped piece into 16.
- `SongTimeline.tsx` (P0b-1's extraction, done here): the curve, the hit test, a click, a drag
  and Draw mode's paint go through the lane's scale. Draw mode fills in between pointer moves
  along the drawing, so along the scale.
- `lib/points.ts`: `hit` takes the drawn path, so a press finds the curved line where it is drawn.

**Tests** — new suite `pl-lanes` (kind `fix`, 11 checks), after `pl-notes`: see `e2e/README.md`.
Moved and green: `s8-automation` 45, `s8-mix` 80, `taste` 39.

**Red first** (D2's page):
```
FAIL  1 kHz is drawn where a log scale over 20 Hz to 20 kHz puts it: 228.2 (log 221.6, linear over the points 228.2)
FAIL  the line between them curves, as it plays straight in value: 3 vertices (3 straight)
FAIL  at half height it writes about 632 Hz: 7520 (the log scale at that pixel: 632; linear would be about 7515)
```

**Left.**
- Automation lanes are 32 px tall, so a pixel is a big step on a 20 Hz to 20 kHz scale. Lane B's
  taller lanes help. Typing a point's value is not there yet.
- Seconds (an attack, a delay time) still draw linear: the plan's X17 keeps them as they were.

**For gaps.md.** Automation of a frequency draws on a log scale. Left: other log units (times),
typing a point's value, curves between points (Q28).
**Contract.** None.

### D4. Hide one automation lane

**In short.** Right-click an automation lane's name and pick "Hide this lane". The lane goes away.
A small line under the track says "1 hidden lane · Show". Show brings it back. The song keeps this
for next time.

| Way | What happens |
|---|---|
| Hide this lane (the lane's header menu) | that lane goes; the line shows under the track's lanes |
| Show (on the line) | every hidden lane of the track comes back |
| A, or the track's chevron | shows the track's lanes, never a hidden one |
| Automate, or "Show … lane" on the input a lane drives | that lane comes back |
| Close and open the song | still hidden: `.music2/view.json` keeps `hidden: ["bass.drop"]` |

**What changed**
- `web/src/lib/hiddenLanes.ts` (new): `useHidden(module)`, `useAllHidden()`, `hide`, `show`, over
  the view store's `hidden` key; a lane made or asked for (`laneMade`, `lib/events.ts`) shows.
- `web/src/components/HiddenLanes.tsx` (new): `HiddenRow`, the 20 px line, Show a link button.
  Only while the track's lanes show (X44).
- `SongTimeline.tsx`: the track's rows leave hidden lanes out and end with the line; the Song row's
  height (`songH`) counts both; a lane asked for is revealed once it shows; the lane header's menu
  gains "Hide this lane", between Remove automation and Hide automation.
- `e2e/s8-automation.mjs`: its check of the header menu's rows names the new row (still 45 checks).

**Tests** — `pl-lanes` grows to 28 checks (see `e2e/README.md`). The plan's "Automate on Drop's
knob" can't be done in first-song (Drop is a lane read through a cable, not a knob), so the suite
automates Bass's Drive, hides that lane, and brings it back with "Show Drive lane": the same event.
Moved and green: `s8-automation` 45, `s8-clips` 94, `layout` 39, `views` 30, `s8-tracks` 75,
`start` 24, `s8-mute` 152, `taste` 39, `pl-notes` 98.

**Red first** (D3's page):
```
FAIL  the Drop lane's header menu: Remove automation, Hide this lane, Hide automation (unautomate,hide)
FAIL  Hide this lane takes Drop's row away, and only it
FAIL  a line under Bass's lanes says so: null
```

**Left.**
- A track's lanes all hidden still shows its chevron and, unfolded, only the line. Fine, but a
  "show all" for the whole song isn't there.
- When a track's ID changes, its `hidden` entries don't follow yet (lane A's rekey, X20).

**For gaps.md.** #17's rest: one automation lane can be hidden, kept per song.
**Contract.** FMT-20: `view.json`'s `hidden` lists hidden lanes as `module.lane`.

## Merge

The lead's merges of the four lanes.

**In short.** All four lanes are now in `polish`. Every lane was green, so each was merged whole.
Two lanes had built the same thing (the place view settings are kept): one was kept. A few tests
needed a small change to match.

| Step | What |
|---|---|
| `main` | `d7207d2` (docs only) was already in every lane; `polish` moved up to it first |
| Lane A, `5836e8c` | Merged as it is |
| Lane D, `208c886` | Both lanes' `mod` lines and router merges kept. NoteView takes A's row names and D's grid name |
| Lane C, `8f5b181` | The view store: see below. Both lanes' menu tests kept |
| Lane B, `2e3461d` | The timeline publishes `markers` (B) and `key` (D). A track's rows use B's heights and D's hidden lanes. B's `routes_view.rs` (the view route and markers). `editorHooks.zoom` was declared by both B and D: kept once |

**The view store: lane B's is kept.** Lanes A and D had already taken B's commit `2a6d3c1`, and
lane C had built its own with the same calls (`useLocal`, `getLocal`, `setLocal`, `flushLocal`,
`POST /api/graph/view`).

| Lane C's part | Now |
|---|---|
| `view_file.rs`, `Inner.local`, `local_stamp` | Dropped: B's `Published.local` holds the settings |
| `POST /api/graph/view` in `routes_project.rs` | Dropped: B's route in `routes_view.rs` (two routes on one path would stop the server) |
| `view_file_tests.rs` | B's two tests kept; C's one test dropped (B's cover the same: merge, null, refused loop and names, another session, reopen, read only) |
| `web/src/lib/local.ts` | B's, with its store exported as `useLocalStore`, which C's `grid.ts` and `audition.ts` follow |
| `local_stamp` | Not needed: lane A's ID change moves `heights` and `hidden` in the page (`lib/rekey.ts`), with B's and D's key shapes (`bass`, `bass.cutoff`) |

B's store sends a change 300 ms after the last one, where C's sent it at once. Two `s8-clips` checks
that C wrote read the state's `local` straight after a change. They now wait for it to land. Red
in the first sweep: `FAIL after a click in the timeline, ⌘1 is the timeline's again: 1/2, the
roll's 0.25` and `FAIL and writes nothing: /api/graph/seek,/api/graph/view`. Green after: 94/94.
`s8-songbar`'s "7 beats, each an eighth" now waits for the field to say 7/8 too, not only the file:
on a busy machine the page heard of the change after the check read it (failed once in five runs).

**Also.** `TrackIdDialog` uses `useModalKeys` (X14), as every other dialog does now.

**A fix: a name being typed is never written by losing its room.** `s8-songbar` failed "Escape
wrote nothing" twice in ten runs: the song had the half-typed name. Lane D's measured room (Q23)
hides the name when the groups beside it leave under 56 px. In the app's smallest window that is
close (86 px in the Mac app at 1100), so a status pill showing for a moment hid it. Hidden, the field
lost the keyboard, and losing it writes what is in it. Now the name's box stays while it holds the
field (`has-[input]:flex`), so only Return writes and Esc never does.

| | |
|---|---|
| Test | `pl-notes` (99 now): with the long name at 1440, type "Half typed", take the name's room away (the tempo group made very wide), then Esc: the field kept the keyboard and the name is as it was |
| Red first | `FAIL a name being typed stays while its room goes, and nothing is written: Half typed` |
| Green | `pl-notes` 99, `taste` 39, `s8-songbar` 82, `fx-fixes` 75, `s8-mute` 152 |

**Checks** (the server and `m2` built from `98e3b11`, copied to `bin/pl-98e3b11`):
- Rust: `cargo test --locked --no-fail-fast` of `m2-script` (56, 1, 8), `m2-editor` (280),
  `m2-server` (232), `m2` (its tests), `m2-session` (154), `music2-desktop` (12). Clippy
  `-D warnings` on all six: clean. Log `~/m2-runs/logs/pl-rust1-*`.
- The web build (it type-checks): green.
- Every browser suite but `live3b` (it needs plugins: the End's), twice: four batches at once,
  then two. All passed but `s8-clips` (the two checks above), and three that passed when run again:
  `pl-mix` 46/47 once (a meter read silence, −60, while playing, as the Rust tests ran beside it),
  `newsong` once (⌘N right after a new song opened found no dialog in 30 s), and `s8-songbar`.
  Logs `~/m2-runs/logs/pl-suites-*`, `pl-sweep2-*`, `pl-flaky*`.
- On the tip, after the fixes below: every suite but `live3b` green with its count, in three
  batches at once (logs `pl-final-a-*`, `pl-final-b-*`, `pl-final-c-*`), and `s8-songbar` six
  more times and `newsong` three more, all green (`pl-final-sb-*`). `start` with no song open:
  14/14.

**Left for the End.** `live3b`; the Linux app on `:96`; `ci/check.sh`; the docs each lane listed.
`canvas.renameSong` (set in `SongBar.tsx`, declared in `shell.ts`) is no longer called: File ▸
Rename Song… opens lane C's dialog, and a double-click on the name still renames in place. The End
can drop it.

## End

The lead's last check, docs and pictures.

### Review fixes

The End's review read the tip with two lenses: how it behaves, and its tests, docs and taste. It
found nine things. PL-T2 is the same bug as PL-R4, so they are fixed once. Each fix is its own
commit, with a test that failed first.

#### PL-R2. Save As into the song's own folder

Save As took any folder, even the song's own folder or its `data/`. The copy then copied itself,
one level deeper each time, until the path was too long (about 285 levels) and the copy failed with
a cryptic error. Now Save As refuses a folder inside the song's own before it makes anything:
"Pick a folder outside Night Drive's own folder." Both paths are resolved first, so `./` or a link
doesn't get round it. A folder beside it whose name starts the same ("Night Drive B") is fine.

| | |
|---|---|
| Test | `project_tests::save_as_refuses_a_folder_inside_the_song`: the song's folder, its `data/`, and a path with `./` in it are refused in words, nothing is made, the song stays open; "Night Drive B" beside it works |
| Red first | `left: "making /tmp/m2-save-as-3218415-0/songs/Night Drive/Copy"`, `right: "Pick a folder outside Night Drive's own folder."` |
| Green | `m2-server` `project::` 30/30 |

#### PL-R1. Rename Song with its folder threw away unsaved code

With "Also rename its folder" on (the default), Rename Song closes the song and opens it again.
That ends the code editor, so any edit not yet saved was lost without a word. Every other way a
song closes asks first. Now:

| Where | What |
|---|---|
| The dialog | While the box is on and the code editor has unsaved edits, it says so, as Save As does: "A file has unsaved edits in the code editor. Renaming the folder closes the song without saving them." The button says **Rename Anyway**. With the box off nothing closes, so no warning |
| The server | `POST /api/project/rename` with `folder` refuses while the code editor has unsaved edits, before anything is written: "song.ts has unsaved edits in the code editor. Renaming the folder closes the song without saving them. Save first, or rename anyway." The request's `anyway: true` goes ahead (the dialog sends it after its warning) |

| | |
|---|---|
| Rust test | `project_tests::rename_with_its_folder_asks_about_unsaved_code`: refused with one file and with two, nothing written; the name alone works; `anyway` renames the folder |
| Rust red first | ``called `Result::unwrap_err()` on an `Ok` value: Object {"dir": String("/tmp/m2-save-as-3233989-0/songs/Summer"), "name": String("Summer"), "warning": Null}`` |
| Suite | `pl-projects` C3, in the app: an unsaved edit in the code editor, then Rename Song…: the warning and Rename Anyway; box off, no warning; Esc changes nothing; the route refuses without `anyway`; Rename Anyway renames |
| Suite red first (the page and server of `5e74f50`) | `FAIL with unsaved code it says the edits would be lost, and the button says Rename Anyway: null \| Rename` and `FAIL the server refuses it too, unless asked to go ahead: 200 undefined` |
| Green | `m2-server` `project::` 31/31; `pl-projects` (see the last run below) |

#### PL-R3. A marker moved inside one bar had no Undo name

A marker moved to another beat of the same bar ("Chorus at bar 9" to "Chorus at bar 9") had the
same words on both sides. The page names an undo step only when the two sides differ, so the move
got no name. Undo then showed the step before it, and every name after was one step off. Now a
move inside one bar says the beats: "Chorus at bar 9 beat 1" to "Chorus at bar 9 beat 2.5" (as
notes say their place). A move to another bar still says the bars.

| | |
|---|---|
| Rust test | `markers_tests::a_move_inside_one_bar_says_the_beats` |
| Rust red first | `left: ("Chorus at bar 9", "Chorus at bar 9")`, `right: ("Chorus at bar 9 beat 1", "Chorus at bar 9 beat 2.5")` |
| Suite | `pl-arrange` (55 now): after the ⌘-held move inside bar 9 and its Undo, Undo names the drag before it, "Undo moving Chorus" |
| Suite red first (this page, the server of `5e74f50`) | `FAIL then Undo names the drag to bar 9: Undo Verse at bar 11 (⌘Z)` |
| Green | `m2-editor` `markers::` 7/7, clippy clean; `pl-arrange` (see the last run below) |

#### PL-R4 (and PL-T2). A Ctrl-click on a marker on a Mac moved the playhead

On a Mac a Ctrl-click is a right-click. WebKit sends it as a press with button 0 and Ctrl, then the
menu's event (X7). The marker flag took the press as a click: the menu opened, and the playhead
also jumped to the marker. A Ctrl-drag moved the marker. The roll had the guard (X7); the new flags
didn't. Now the flag ignores a press with Ctrl on a Mac, as the roll does. On Linux, Ctrl-drag still
places a marker off the grid (X8).

| | |
|---|---|
| Suite | `pl-arrange` (57 now), on its Mac page: a Ctrl-press, the menu's event, the release: the menu opens and the playhead stays at 0; a Ctrl-drag moves no marker |
| Red first (the page and server of `5e74f50`) | `FAIL a Ctrl-click on a marker on a Mac opens its menu (true) and leaves the playhead where it was: beat 39.999988888888886` and `FAIL and a Ctrl-drag on a Mac moves no marker: [{"beat":16,"name":"Chorus"},{"beat":44,"name":"Verse"}]` |
| Green | this page with the old server: both pass (the run that showed PL-R3 red); `pl-arrange` (see the last run below) |
| Not checked | A real Mac. Linux Chromium doesn't turn a Ctrl-click into a right-click, so the suite sends the menu's event itself. On the Mac's list |

#### PL-T1. The song's name field shrank to 18 px while it was typed in

The Merge's fix kept the name's field when its room went, but the field still shrank to that room:
you typed into a box 18 px wide and couldn't see the name. Now the field keeps the width it had
when typing began (it may still grow), drawn over the groups beside it until the room comes back.
The bar's box holds the room in a CSS variable (`max-w-(--room)`), and drops it while it holds the
field (`has-[input]:max-w-none`, `z-10`). The narrower name in the Mac app at 1100 px (86 px) is by
design (Q23), so it stays.

| | |
|---|---|
| Suite | `pl-notes` (100 now): the field's width after the room goes is at least its width when typing began (at least 120 px), and it is the topmost thing at its own middle |
| Red first (the page and server of `5e74f50`) | `FAIL the field keeps its width, on top of its neighbours: {"w":160,"top":true} then {"w":18,"top":true}` |
| Green | this page: `{"w":160,"top":true} then {"w":160,"top":true}`; `pl-notes`, `s8-songbar`, `fx-fixes` (see the last run below) |

#### PL-T3. Two decisions had no test: X3 and X26

Two plan decisions were in the code with nothing to catch them going: X3 (⌘= and ⌘− zoom the roll
while it has the keyboard) and X26 (a drum lane is never shaded by the song's key). Lane D had left
X26's test until lane A's drum names landed, and the Merge's list didn't carry it on. Both have
checks now. No code changed: each check was shown red on a page built with its line taken out.

| | |
|---|---|
| `pl-notes` (102 now) | With the roll focused, ⌘= makes its notes 1.5 times longer (a note is drawn 1 px short, so its length is its width + 1) and leaves the timeline's clip as wide; ⌘− puts them back |
| `pl-sounds` (44 now) | With the Drum kit's clip in Notes and E minor written into layout.json: its rows stay shaded as the black keys' (`1,3,6,8,10`); Keys' roll is shaded by E minor (`1,3,5,8,10`), so the key did land |
| Red first (a page with `editorHooks.zoom?.(dir) \|\|` and `!drums` taken out) | `FAIL with the roll focused, ⌘= makes its notes 1.5 times wider and leaves the timeline: note 9.5 → 9.5 px, clip 766 → 1150 px` and `FAIL the Drum kit's rows stay shaded as the black keys', not by E minor: 1,3,5,8,10` |
| Green | `pl-notes`, `pl-sounds` (see the last run below) |

#### PL-T4. Four suites ran weaker copies of the taste lint

`pl-projects`, `pl-tracks`, `pl-lanes` and `pl-notes` each had their own copy of the lint. The
copies in `pl-projects` and `pl-tracks` never looked at text fields, so the Save As and Rename Song
name fields were never checked for font size. `pl-lanes` checked two corners and no mono or
backdrop rule; none checked `::before` and `::after`. All four now run `e2e/lintlib.mjs`, as
`taste.mjs`, `pl-mix`, `pl-ids`, `pl-sounds`, `pl-zoom` and `pl-arrange` do (plan 1.3, X16).

| | |
|---|---|
| Red first | The old copy and lintlib on one page: a Save As dialog whose name field is 14 px. `old inline lint: []`, `lintlib: ["font-size 14px at <input>"]` |
| Green | On the tip, every state these suites lint passes lintlib: `pl-tracks` 20/20, `pl-lanes` 28/28, `pl-notes` 102/102, `pl-projects` 65 |
| Also | `pl-projects`' count goes 58 → 65 here: PL-R1's commit added 7 checks and missed it |
| Left | `s8-mute` (8b's suite) still has its own copy |

#### PL-T5. Two new controls broke the one focus ring

taste.md §2.11 says one focus ring everywhere, and §2.9 says a list row is `accent-soft` under the
pointer and the same with the keyboard. Two new places broke that:

| Where | Was | Now |
|---|---|---|
| Change track ID's field | Its own pale-blue ring (`focus:ring-2 focus:ring-accent-range`), no outline | The one ring, as every other dialog's field |
| A recent song on the start screen | Keyboard focus looked like the grey hover; a "Moved or deleted" row showed no focus at all | `accent-soft` under the pointer and with the keyboard, as the Examples menu's rows; a gone song's row too |

The marker's name field and Repeat's count keep the house's in-place field look (an accent border,
no ring), as the track name and the top bar's fields had before polish.

| | |
|---|---|
| Suites | `pl-projects` (66 now): Tab to Alpha's row and to gone Beta's row: both `rgb(224, 234, 251)`. `pl-ids` (39 now): the field's outline is `solid 2px` accent and it has no box shadow |
| Red first (this page before the fix) | `FAIL Tab to a row lights it accent-soft, a gone song's too: ["rgb(230, 232, 235)","rgba(0, 0, 0, 0)"]` and `FAIL it shows the one focus ring, as every dialog's field does (review PL-T5): {"outline":"none 0px rgb(56, 98, 167)","shadow":"… rgb(200, 218, 248) 0px 0px 0px 2px …"}` |
| Green | `pl-projects` C2: `["rgb(224, 234, 251)","rgb(224, 234, 251)"]`; `pl-ids` 39/39. A first try also gave the field `transition-colors`: the outline then faded in from the text colour (`solid 2px rgb(51, 55, 60)` when read) and the danger border check read it half way, so it was taken out |

#### Also: the unused rename hook is gone

The Merge left `canvas.renameSong` for the End to drop: nothing called it since File ▸ Rename Song…
opens lane C's dialog. It is gone from `SongBar.tsx` and `shell.ts`. A double-click on the name still
renames it in place (`s8-songbar`, `pl-notes`).
