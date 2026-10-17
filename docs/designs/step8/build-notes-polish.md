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

## Lane C

Songs and tracks.

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

## End

The lead's last check, docs and pictures.
