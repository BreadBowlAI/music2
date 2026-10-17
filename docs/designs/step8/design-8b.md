# Step 8b design: the producer features

Revision 2. This design is checked against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. It sits on the step 7
design (`.git/m2-step7/design-7.md`, revision 2) and the step 8a design (`design-8a.md` in this folder, revision 2), which
the order of work builds first. It uses three notes in this folder: the survey `understand-8b.md`, the UI audit
`audit-ui.md` (its screenshots are in `scratchpad/ux8/shots/` and are cited by prefix, such as `e11`), and the taste
direction `taste.md` (revision 2). Every UI here follows `taste.md`, cited as "taste §n" and by its change ids (TL-5,
MX-1, …). Revision 2 answers the twenty-five problems raised against revision 1 (§18). The largest changes: a tempo drag
never makes audio skip, toasts only for results off screen, native shortcuts only for ⌘ and ⌃ chords, a −∞ fader foot,
no header level slider, and instrument previews and tap tempo cut.

Nothing was built, run or measured. No file in the repository was changed. Files that another workflow is editing (the
G11 data-plane work) were read as HEAD has them. Every cost marked *estimate* is unmeasured.

Step 8b's line in the order of work:

> **8b:** each a Tx builder or an engine feature on this base: add, duplicate, delete and rename track; the mixer with
> monitoring mute and solo plus the `muted` track option; automate a knob; the tempo bar, with its lease; clip editing by
> id, with lane leases; audition.

Its gates (step 8's row, the parts that aren't 8a), and the CI gate that runs from step 8 on:

> Each producer feature lands without changing core's write API. Muting the kick in a fixture still ducks the bass, with
> the ducking read from `kick.env`, from `follow(kick.out)` in `song.ts`, and through a compressor keyed from the kick's
> audio. Toggling mute while a sine plays makes no sample-to-sample step larger than twice the sine's own largest. An
> export is byte-identical with and without overlays active.
>
> overlays never change an export, from step 8 (MON-4).

---

## 0. The design on one page

**Nine slices, in the order a producer needs them.** Each slice lands green on its own. Each ships with the UI that
`taste.md` gives it. Each adds its own browser suite.

| Slice | What the producer gets | Builders (`crates/editor`) | Engine and core | Waits for | Size *estimate* |
|---|---|---|---|---|---|
| **8b.1 Tracks** | + Track (⌘T) with an instrument list; duplicate, delete, reorder and recolour; colours that never shift | `tracks::{add, duplicate, delete}`, `layout::{order, colour}` | none (std's instruments get `editor` metadata) | taste T and S | 2 weeks |
| **8b.2 Song bar** | tempo, time signature and title in the top bar and the New Song dialog; tempo and meter changes on the ruler; ⌘L, C, ⇧Space, stop returns, panic (⌘.) | `song::{tempo, meter, title}` | `Home::Arrangement` | — | 1 week |
| **8b.3 Faders** | the Mix view: a fader and pan on every channel, with a −∞ foot; a master fader | dictionary-entry insertion; `mix::{view, insert}` | std: the mixer's and master's `db` stop being optional (bit-exact) | — | 1.5 weeks |
| **8b.4 Mute and solo** | M and S on every track, "2 muted · 1 solo", Mute in song, the export warning | `tracks::mute_in_song` | the overlay op, cells keyed by module, `Module.muted`, a non-structural `Muted` delta, sidechain inputs in `Flat`; std's keyed compressor | step 7 slice 2 | 2.5 weeks |
| **8b.5 Clips** | clips edited by id on the timeline (move, copy, resize, split, join, duplicate, paste, quantize, delete); automation drawn in place, and removed; grid, snap, zoom, track heights, the End flag, markers | `clips::{apply, edit}`, automate's naming fix (D6), `automate::remove`, `layout::markers` | none | — (audio clip edits cut tails until step 7) | 2.5 weeks |
| **8b.6 Audition** | notes heard as you draw them, move them and click the keys | — | `Msg::Live`, live room in each notes scheduler, live sources | step 7 slice 1 | 1 week |
| **8b.7 Leases** | a note, clip or automation point heard where it is while you drag it during playback; the tempo heard while you drag it, with audio that lags and never skips | `clips::apply`, reused on the control plane | `SwapLane`, an identity `Activate`, and an audio player's lease offset | step 7 slice 2; 8b.2, 8b.5 | 2 weeks |
| **8b.8 Meters** | meters in headers, strips and the top bar; the DSP load | — | render taps (peaks) | step 7 slice 2; the DSP readout waits for 8a | 1 week |
| **8b.9 Track IDs** | Change track ID… in the Code view, and Restore for an ID renamed by hand | `tracks::rekey`, the `restore` fix | a `Rekeyed` pass in step 7's `continues` | step 7 slice 2, for kept instances | 1 week |

That is about 14.5 weeks of work in all *estimate*. §19 gives the build order.

**What can start before step 7.** 8b.1, 8b.2, 8b.3, 8b.5 and 8b.9 (without its pass) touch no engine code. They can be
built before step 7 or beside it, at the cost of rebasing on step 7's `crates/server` changes (§12, §19). 8b.4, 8b.6,
8b.7 and 8b.8 put code into the runtime that step 7 rewrites, so they wait for it. Built on HEAD's `Plays`, they would be
written twice.

**Three rules hold in every slice:**

1. **Core's write API doesn't change.** That means `save`, `Tx`, its five ops (`TextEdit`, `Write`, `Remove`, `Rename`
   and `Clips`), its five expectations (`Identical`, `Values`, `Confined`, `Seen` and `Journal`), `check`, `commit` and
   undo. Every 8b write is one Tx made of today's ops. A test records the API's schema, so a change to it fails CI
   (§13.1).
2. **A slice that touches `core/*` changes the contract first,** in the same commit (WRITE-9). §14 lists every edit.
3. **Every slice lands green.** That means:
   - `core/session/tests/renders.json` is unchanged, so every pure fixture renders the same bits;
   - every existing test passes with the same meaning;
   - `e2e/run.mjs all` passes, with the suites the slice moves updated in the same PR (taste §0);
   - the taste lint (`e2e/taste.mjs`) passes.

**The engine features, one paragraph each.**

- **Overlays (8b.4).** The checker lists every place a track's audio export enters another module through an input that
  isn't a sidechain input, and the song's output when it reads a track directly (`Flat.overlays`). Every plan, offline
  renders included, puts a built-in overlay op at each of those places. The op multiplies by its own overlay cell
  (mapped from its module's key) and glides to a new value over 10 ms. At 1.0 it is `x * 1.0`, which changes no bit.
  Monitoring mute, solo and mute in song all write those cells. Offline renders only ever see mute in song.
- **Audition (8b.6).** `Msg::Live` carries a note on or off for one lane's slot. The lane's scheduler keeps a small
  table of live notes, takes their ids from the engine-wide counter (design-7 §8.1) and merges them into its buffer at the
  block line. Live notes belong to a source (a window today) and end on release, on panic, when their source goes and
  when their consumer is replaced. They don't end on stop, seek or wrap.
- **Leases (8b.7).** A lane lease evaluates the same clip gesture the commit will run, on the control plane. It sends a
  preview `SwapLane` and writes nothing. A tempo lease prepares an identity `Activate` with the dragged tempo map. It
  keeps one prepare in flight, and it sends a plan only while that plan is still the last one sent. Audio players keep
  their renders and read them at an offset that keeps their position, so audio lags behind the drag and never skips.
- **Taps (8b.8).** The plan holds a list of nodes whose output peaks are kept in atomics. The task that runs a node
  writes its peak, and the server reads the peaks 30 times a second and sends them to the window.

**What 8b leaves out** (§15): override latches (MON-6; 8b.5 gives a lane-driven knob a way to its lane and a Remove
automation), the metronome (gaps #19), key and scale (gaps #16, which has no home in the contract), sends and buses
(#27), plugins in the pickers (#32), audio import (#18), automation curves (a format addition), sections (#23),
instrument previews, tap tempo, and MIDI keyboard input (owner question Q3).

---

## 1. What 8b stands on

### 1.1 The builder pattern

Every 8b gesture is a builder in `crates/editor`, in the shape of `plan.rs` (survey §1.1):

1. plan a Tx on the accepted revision;
2. `check` it;
3. assert on the checked candidate what the gesture means (WRITE-9);
4. `commit` it.

Each builder is reached three ways: a route in `crates/server/src/routes.rs`, a CLI command in `apps/m2`, and the generic
`POST /api/tx/check|commit` with the Tx it would build. A route that writes goes in `WRITES`, so a read-only window answers
423. A CLI command that writes takes the lock (WRITE-18).

### 1.2 Which path each change takes, after step 7

| Change | Delta from what plays | Path (design-7 §8.3) |
|---|---|---|
| add, duplicate, delete a track | structural, `Added` or `Removed` | `Activate` with `continues`: untouched tracks keep their tails |
| rename a key | structural, `Rekeyed` | `Activate` with `continues` and 8b.9's pass |
| mute in song | `Muted` | **a cell write** (8b.4 makes `Muted` non-structural) |
| monitoring mute and solo | none (no revision) | control plane: cell writes |
| fader, pan | `Values` | cell write |
| first pan on a channel, first insert of an optional control | structural | `Activate` |
| tempo, meter | arrangement | identity `Activate` (design-7 §8.4) |
| notes and control clips | lanes | `SwapLane` |
| audio clips | lanes (audio) | identity `Activate` with a `LaneMove` |
| layout (order, colour, markers) | none (no revision, PLANE-3) | nothing reaches the engine |
| lane lease update | none | control plane: preview `SwapLane` |
| tempo lease update | none | control plane: identity `Activate` |
| audition | none | control plane: `Live` |

Before step 7, every row that says `Activate` is a new renderer that cuts every tail (RENDER-25's Today). The slices that
can start before step 7 accept that cost, and step 7 removes it without any change to them.

### 1.3 Three files outside the revision

| File | Class | Who writes it | Holds |
|---|---|---|---|
| `layout.json` | Presentation (shared, in git) | Tx (journaled, undoable, no revision) | track order and colour; markers (§2.1) |
| `.music2/overlays.json` | Local | the server, directly | monitoring mute and solo (§6.5) |
| `.music2/view.json` | Local | the server, directly | loop range, zoom, snap, follow playhead, track heights, folded lanes, the dock's view and height, the browser's width, the audition toggle, the export dialog's "don't ask again" (§2.2) |

A Tx can't name a Local path (WRITE-3a), and the HOME table puts overlays, the loop range and zoom in `.music2/`. So the
two local files are plain atomic writes (write a temporary file in `.music2/`, then rename it), never Tx and never undo
steps.

### 1.4 The UI base

Taste slices **T** (tokens) and **S** (shell) land before any 8b UI (taste §8). S brings:
- the 44 px top bar;
- the dock with Device, Notes, Mix and Code, its view one choice for the window that selection never changes (taste
  §3.7);
- the status pill, with no edge line;
- one-at-a-time toasts above the dock, only for results off screen or structural (taste §2.14);
- track headers without captions or a level slider;
- the side browser;
- native accelerators only for ⌘ and ⌃ chords, with focus routing and the ⌥K sheet (§2.6);
- fixes for defects D1, D2, D4 and D12.

D1 (stale Monaco models after a project switch) must land before 8b.1, because adding a track creates files and switches
modules (audit). Every 8b UI below is described against that shell.

### 1.5 Coordination

- **The G11 workflow** is rewriting `crates/editor/src/notes.rs`, `core/session/src/tx/mod.rs` and
  `crates/server/src/graph/actor.rs` now. 8b.5 moves the piano roll's write onto clip gestures (§7.1), so it rebases on
  G11's `notes.rs` and its `Session::commit_with`.
- **The owner chose C20 option 2** for the data plane: a `SwapLane` is heard once its Tx is checked, and the durable
  write runs beside it (PLANE-5 in the working tree). A lane lease's commit (8b.7) uses that path, so the sound doesn't
  change on release.
- **Step 7** rewrites `core/runtime` and the actor's `Plays` (design-7 §18). The 8b slices that can come first touch
  `crates/editor`, `routes.rs`, the view in `graph.rs`, `web/` and `std/` only.

---

## 2. Shared pieces

### 2.1 `layout.json` (FMT-19)

Nothing reads, writes or watches it today. 8b.1 brings all three.

**Schema** (taste §2.7 rule 1; the HOME table's example changes from `"#e4572e"` to a name):

```json
{
  "order": ["kick", "clap", "hats", "bass", "chords", "hook"],
  "tracks": { "kick": { "colour": "rose" }, "clap": { "colour": "teal" } },
  "markers": [{ "beat": 0, "name": "Intro" }, { "beat": 64, "name": "Chorus" }]
}
```

- `order`: track keys from top to bottom. A key that names no track (say Hook, commented out in `song.ts` to try the
  song without it) is ignored when read and **kept** when the file is written, in its place after the key before it.
  So Hook comes back where it was, in its own colour. Only deleting the track (§3.3) removes its key.
- `tracks.<key>.colour`: one of taste §2.7's twelve names. A `#rrggbb` written by hand or by another tool is read as the
  nearest palette colour by hue, and rewritten as a name only when that track's colour is next written. Entries for
  keys that name no track are kept the same way; their colours don't count as used (rule 3 below).
- `markers`: a name and a beat, sorted by beat, at most one per beat (§7.6). They are presentation: no sound depends on
  them.
- **Unknown keys** are kept as read and written back unchanged. FMT-19 says readers ignore them; a writer that dropped
  them would lose a newer version's layout.
- **The canonical form** is JSON indented by two spaces, with the keys above in that order, then any unknown keys in the
  order read. `layout::canonical` writes it.
- **A file that doesn't parse** (a git merge left conflict markers in it) gives the Notice `layout.invalid` (a new
  code, DIAG-2). It names the file and the error, and the view uses the defaults. A conflict must cost order or colour,
  never the main gestures (R15), so:
  - the track builders (add, duplicate, delete, rekey) leave `layout.json` out of their Tx, and colours stay unfrozen
    until the file reads again;
  - reorder, recolour and markers refuse with "layout.json doesn't read: fix it, or reset the layout";
  - the Notice offers **Reset layout**: a Tx that writes the defaults over the unreadable text (based on it, so it is one
    undo step that brings the text back). Nothing is overwritten silently.

**Code.** `crates/editor/src/layout.rs`. The palette's names and taste §2.7's default order are UI vocabulary, so they
live here and not in core (P10):
- `Layout::read(text)`;
- `Layout::canonical()`;
- `layout::effective(rev, &layout, &channels) -> LayoutView { order, colours, markers, stored: bool }`;
- `layout::write(s, change) -> Tx`.

The server sends the palette's names in the graph state, and `web/src/lib/palette.ts` holds their tones. The taste lint
fails when a name has no tones.

**Effective order and colour** (taste §3.4 and §2.7):
1. **Order:** the stored order, restricted to tracks that exist; then any other tracks, in the mixer's channel order
   (ID-18, §2.3); then any still left, in import order. Never alphabetical (D8).
2. **No colour stored at all:** default colours by position in that order (taste rule 3).
3. **Some colours stored:** a track without one (one an agent added by hand, say) gets the first colour no track uses
   (taste rule 5).
4. **Every builder that writes `layout.json` writes every track's effective colour,** so the first write freezes them all
   (taste rule 4) and adding a track never recolours the others.

`TimelineTrack` gains `colour: String`. The timeline's tracks arrive in effective order.

**Base and watch.** A snapshot doesn't hold `layout.json` (PLANE-3), so `plan::base_of` would give `Absent` and every
second write would conflict.
- **The base:** a new `plan::base_from_disk(s, "layout.json")` reads the file when the builder plans. It gives
  `WithText { sha, text }`, or `Absent` when there is no file. The commit compares and swaps against the disk, as for any
  base.
- **The watch:** the server's watcher already walks the project every 50 ms. It now also compares `layout.json`'s
  `(size, mtime)`. When the file has settled after a change, the server re-reads it and publishes a graph event with the
  new layout, with no revision (PLANE-3). After a commit that wrote it, the server re-reads it at once.
- **Undo:** a Tx that writes only `layout.json` is one undo step with no revision. That is built (`tx/mod.rs`, "Only
  layout.json written").

### 2.2 `.music2/view.json`

```json
{ "loop": { "start": 16, "end": 32, "on": true }, "zoom": 24, "follow": true,
  "snap": { "timeline": { "grid": "auto", "triplet": false, "on": true }, "roll": { "grid": "1/16", "triplet": false, "on": true } },
  "heights": { "bass": 72 }, "folded": { "bass": false }, "dock": { "view": "mix", "height": 300 },
  "browser": 240, "audition": true, "export": { "ask": true } }
```

- **Read** when the project opens. Today the loop range is lost on close (survey §8). From now on the transport starts
  with the stored loop.
- **Written** by `POST /api/graph/view` with a partial object, merged and written at most once per 500 ms. Not in
  `WRITES`: a read-only window keeps its view in memory only.
- **Entries keyed by a track** (`heights`, `folded`) follow it: a rekey renames them (§11.1), and a delete drops them
  with its overlays (§6.5).
- **Unknown keys** are kept, and nothing else reads the file.

### 2.3 Finding a track's channel (ID-18)

`crates/editor/src/mix.rs`, `channels(rev) -> Channels`. ID-18 says to follow "the graph edge from the track's export
into the mixer", and doesn't say how. This is how:

1. **The mixer** is the `song` module's frame whose ComponentId is `std:mixer` (PKG-9), whatever it is imported as.
   - With none, the Mix view says "This song doesn't use std's mixer: its levels are in code" and shows strips with M and
     S only (overlays work in every song, MON-1).
   - With two or more, it says "This song has 2 mixers: set its levels in code" and shows strips the same way.
2. **Strip nodes.** For each const in the mixer's frame whose origin names the slot `channels.<c>.db` or
   `channels.<c>.pan` (PKG-6), the node that reads it belongs to channel `c`. A strip's **head** is its first node in
   signal order: the panner when `pan` is written, else the gain. From 8b.3 every channel has a gain (§5.1), so every
   strip has a head.
3. **The walk.** From each audio export of track `k`, walk downstream over audio edges, through nodes outside the
   mixer's frame (a `duck`, a `compress`), to the first node inside it. That node is the head of channel `c`, so `(k, c)`
   is a pair.
   - A track whose exports reach exactly one channel has that channel.
   - A track whose exports reach no channel has none, and its strip says "Not in the mixer".
   - A track whose exports reach two channels has none, and its strip says "In 2 channels: set its level in code".
4. **A channel reached by several tracks** (`drums: sum(kick.out, snare.out)`) is one strip, named "Kick + Snare". Its
   M and S act on every track in it, and show a mixed state (a dash) when those tracks differ. Each track header keeps
   its own M and S.
5. **For each channel, the lookup also returns:**
   - its key;
   - its `db` knob id;
   - its `pan` knob id, or `insert: true` when `pan` isn't written;
   - the channel property's byte range in `song.ts`, which the builders edit;
   - the mixer call's range.
6. **For each track, it also returns `feeds`:** each place its exports reach another module through a sidechain input
   or a control export, as `(track, component label)`. The track is the fed node's own module, except when that node is
   in `song`: then it is the track or tracks of the channel the node sits on, found by the walk of step 3. First-song's
   duck is written in `song.ts` on the bass channel, so the kick's feeds are `[(bass, "Duck")]`, not `[(song, "Duck")]`.
   MON-2's "the mixer shows this" comes from it (§6.7).

The lookup reads the accepted revision's IR (frames, origins, edges) and runs once per revision, in microseconds at p50
*estimate*.

### 2.4 Words

Each builder writes its own label pair for history, in words from labels and descriptors (taste §1.3, ST-5, gaps #29).
The words name the step in the Undo button's tooltip and in Edit ▸ Undo ("Undo Hats volume").

**A toast appears only when the result is off screen or structural** (taste §2.14): adding, duplicating and deleting a
track, mute in song, a track ID change, automating a knob or removing its automation, export, and refusals. A knob, fader, pan, tempo or meter
change, a title edit, a clip gesture, a note edit, a reorder, a recolour or a marker shows no toast: its result is
already on screen, and a mixing pass is hundreds of fader releases. Undo and redo toast only where the gesture did. No
toast prints a diff, a key or a path.

| Gesture | Tx label (before → after) | Toast | After undo |
|---|---|---|---|
| add a track | "" → "Lead Vox" | "Added Lead Vox (Pluck)" | "Undid adding Lead Vox" |
| duplicate | "" → "Bass 2" | "Duplicated Bass" | "Undid duplicating Bass" |
| delete | "Kick" → "" | "Deleted Kick" (with dependents: "Deleted Kick and 2 Ducks") | "Undid deleting Kick" |
| reorder | "Kick" → "Kick" | — | — |
| recolour | "rose" → "teal" | — | — |
| tempo | "124 BPM" → "128 BPM" ("… from bar 9" when not the first entry) | — (except "Loop turned off: it's shorter than a beat at 128 BPM", §4.1) | — |
| meter | "4/4" → "7/8 from bar 9" | — | — |
| title | "first-song" → "Night Drive" | — | — |
| fader | "−6.0 dB" → "−3.0 dB", named "Hats volume" | — | — |
| pan | "C" → "L 20", named "Hats pan" | — | — |
| mute in song | "Kick" → "Kick, muted in song" | "Kick muted in the song" / "Kick unmuted in the song" | "Undid mute in song" |
| clip gestures | e.g. "clip at bar 9 (2 bars)" → "clips at bars 9 and 10", named "Split" | — | — |
| remove automation | "Cutoff lane" → "1.2 kHz" | "Removed the Cutoff automation" | "Undid removing automation" |
| markers | "" → "Chorus at bar 17" | — | — |
| track ID | "bass" → "low" | "Bass's ID is low now" | "Undid the ID change" |

### 2.5 Routes, CLI and read-only windows

| Route | Body | Writes (in `WRITES`) | CLI (takes the lock) |
|---|---|---|---|
| `POST /api/graph/track/add` | `{session, revision, component, label?, key?, after?}` | yes | `m2 add-track <component> [--name] [--key] [--after]` |
| `POST /api/graph/track/duplicate` | `{session, revision, module}` | yes | `m2 duplicate-track <key>` |
| `POST /api/graph/track/delete` | `{session, revision, module, cascade?: bool}` | yes | `m2 delete-track <key> [--cascade]` |
| `POST /api/graph/track/key` | `{session, revision, module, key}` | yes | `m2 rekey <old> <new>` |
| `POST /api/graph/track/mute` | `{session, revision, modules: [key], muted}` | yes | `m2 mute-in-song <key>… on\|off` |
| `POST /api/graph/layout` | `{session, order?, colour?: {module, colour}, markers?}` | yes | `m2 layout order <keys…>`, `m2 layout colour <key> <name>`, `m2 marker add\|move\|rename\|delete …` |
| `POST /api/graph/tempo`, `/meter`, `/title` | `{session, revision, beat?, bpm}` and so on | yes | `m2 tempo <bpm> [--at <bar>]`, `m2 meter <n/d> [--at <bar>]`, `m2 title <name>` |
| `POST /api/graph/mix/insert` | `{session, revision, channel, control: "pan", value}` | yes | — |
| `POST /api/graph/clips` | `{session, gesture, seen}` (§7.1) | yes | `m2 clips <gesture> …` |
| `POST /api/graph/automate/remove` | `{session, revision, id}` (§7.2) | yes | `m2 automate --remove <project> <control>` |
| `POST /api/graph/lane/lease/{begin,update,commit,cancel}` | §9.1 | begin and commit | — |
| `POST /api/graph/tempo/lease/{begin,update,commit,cancel}` | §9.2 | begin and commit | — |
| `POST /api/graph/overlay` | `{session, mute?: {k: bool}, solo?: {k: bool}, exclusive?: k, clear?: true}` | **no**: local, works read-only in memory | — |
| WebSocket `{"t":"audition",…}` | §8.2 | no | — |
| `POST /api/graph/panic` | `{}` | no | — |
| `POST /api/graph/view` | partial `view.json` | no | — |

WRITE-18's list of commands that take the lock gains `add-track`, `duplicate-track`, `delete-track`, `rekey`,
`mute-in-song`, `layout`, `marker`, `tempo`, `meter`, `title` and `clips` (`automate` takes it already). Each command is
small once its builder exists (gaps #37 asks for `m2 add-track` and `m2 tempo`).

In a read-only window, every edit affordance shows values without drag cursors (taste ST-2). M and S, audition, the view
and the transport still work. Overlays there live in memory only, and the pill's tooltip says "Not saved: this song is
open in another window".

### 2.6 Menus and keys

All keys follow taste §5. `apps/music2-desktop/src/menu.rs` owns every ⌘ and ⌃ chord, and only those.

| Menu | Items added by 8b (slice), with their native accelerators |
|---|---|
| File | Export Audio… ⇧⌘E (taste TB-9; ⌘E moves to Split in 8b.5); Close Song ⌘W (S); Rename Song… (8b.2) |
| Edit | Cut ⌘X, Copy ⌘C, Paste ⌘V, Duplicate ⌘D, Split ⌘E, Join ⌘J, Quantize (8b.5); Delete |
| Track (new) | New Track… ⌘T, Delete Track ⌘⌫, Rename ⌘R (8b.1); Mute ⌃M, Solo ⌃S, Mute in Song (8b.4) |
| Transport | Play, Continue, Go to Start, Loop, Loop Selection ⌘L, Stop All Sound ⌘. (8b.2); Add Marker, Previous Marker, Next Marker (8b.5) |
| View | Zoom In ⌘=, Zoom Out ⌘−, Zoom to Selection, Track Height ⌘↑/⌘↓, Show Automation, Follow Playhead ⇧⌘F (8b.5); Device, Notes, Mix, Code (S, 8b.3) |
| Help | Keyboard Shortcuts (S) |

**Native accelerators are only ⌘ and ⌃ chords** (taste §5.3). A native accelerator fires while the producer types in
Monaco or a rename field, which is why `menu.rs:69` already leaves out Space. So single keys (M, S, C, Q, A, B, Z, X),
Space, ⏎, ⌫ and ⌥ chords (⌥1 to ⌥4, ⌥K) get none. They are the page's own, ignored while a text field or the code editor
has focus, and shown in tooltips and in the shortcut sheet instead of menu rows. The sheet is Help ▸ Keyboard Shortcuts on
⌥K (Logic's Key Commands), so ⌘/ stays Monaco's comment.

**Focus routes the ⌘ chords that text editing also uses.** `appCommand` already sends undo to Monaco's own history when
the code editor has focus (`web/src/App.tsx`). It does the same for each menu chord Monaco also binds: ⌘D (add
the next match), ⌘L (select the line), ⌘. (quick fix), ⌘E (find with the selection), ⌘⌫ (delete to the line start),
⌘↑ and ⌘↓ (start and end of the file). In a rename or type-in field, ⌘⌫, ⌘↑ and ⌘↓ do the field's own edit. Everywhere else
they are the menu's commands.

**⌘D is one item, in Edit, and means the selection's duplicate:** clips when clips are selected, notes in the roll,
otherwise the selected track. ⌘C, ⌘X and ⌘V follow the same rule, and stay native text commands inside text fields. The
header menu, a page menu, shows Duplicate with its key like every page menu.

---
## 3. 8b.1 Tracks: add, duplicate, delete, reorder, recolour

**Why first:** this is gaps.md's #1 ("the biggest wall for a beginner") and #10, and the audit's rank 2. The Empty
template's "No tracks yet" is a dead end today (`e11`). It needs no engine work.

### 3.1 Add a track

**Request:** `component` (a ComponentId such as `std:pluck`), `label?`, `key?`, `after?` (a track key).

**The instrument list.** The list holds every component in the catalog (`Session::catalog`) that has a `notes` input
and an audio output: std's five instruments, and a project's own instruments. Core holds no list (P10, PKG-7).
- std gives each instrument an `editor` field: kick and hat get `{ category: "Drums", description: "…" }`; bass, pluck
  and pad get `{ category: "Instruments", description: "…" }`. Descriptions come from the doc comments ("A punchy bass:
  saw plus a sub square…").
- An `editor` field changes no render, so std's version stays (FMT-3).
- Installed CLAP instruments (gaps #32) are left out. The request already takes `clap:<id>`, so #32 only adds them to
  the list.

**What the builder plans** (`crates/editor/src/tracks.rs`, `add`), for "Pluck" on `examples/first-song`:

1. `ready(s)`.
2. **The mixer.** Find the song's mixer (§2.3). Refuse when there isn't exactly one, or when its channels argument isn't
   an object literal in a project file: "This song mixes its tracks its own way, so a new track would have nowhere to
   go. Add it in the Code view." A track that nothing reads would build, show and be silent, and the builder must never leave
   one like that (survey §2.2).
3. **The label:** the request's label, else the component's label ("Pluck"), made unique among track labels ("Pluck 2").
4. **The key:** the request's, checked by ID-1, else `keys::propose(label, taken)`. `taken` is every module key, `song`,
   every `data/<k>/` folder on disk in any capitals (`Snapshot::taken`), and every `tracks/<k>.ts`. Leaving out the
   unclaimed folders would claim a deleted track's data silently.
5. **The file:** `tracks/<key>.ts`. If that path is taken in any capitals, `tracks/<key>-2.ts`, and so on.
6. **The lane:** `hits` for a component whose `editor.category` is "Drums" (as the fixtures name drum lanes), else
   `notes`.
7. **The texts.** The track file:

   ```ts
   import { track } from "@m2/core";
   import { pluck } from "std/instruments";

   export default track("pluck", { name: "Pluck" }, ({ lane }) => pluck(lane.notes("notes")));
   ```

   - The component's module path and export name come from the catalog (`m2_script::describe` lists components module by
     module).
   - The `name` option is left out when the label equals the key.
   - `song.ts` gets two insertions:
     - `import pluck from "./tracks/pluck";`, after the last default import of a `./tracks/` file, else after the last
       import.
     - The channel `pluck: pluck.out`, appended as the last property of the mixer's object. The key is quoted when it
       isn't an identifier (`"lead-vox": leadVox.out`). It follows the object's indentation and trailing-comma style. In
       a `{}` that holds only comments (the Empty template), it goes before the closing brace at the comments'
       indentation.
   - **The Empty template's comment** (`templates/empty/song.ts`) says today "This song has no tracks yet… Delete the
     //", which is false once + Track has added a Kick, and following it adds a second drum track. 8b.1 rewrites it to
     stay true either way: "Add a track with + Track (⌘T), or write one in code: a `track()` line like the drums one
     below (delete its //), and one line for it in the mixer. Each track needs its own key: lowercase letters, digits and
     hyphens, such as "lead-vox"." The template's renders don't change.
8. **The binding:** the key in camelCase (`lead-vox` → `leadVox`, `808` → `t808`). If that collides with any name the
   file declares or imports, or is a reserved word, add `Track`, then `Track2` and so on. A new helper,
   `sites::free_binding(source, wanted)`. For example, adding a Kick to the Empty template, which imports `kick` from
   `std/instruments`, gives `kickTrack`.
9. **The lane file:** `data/<key>/<lane>.json` with `{"clips": []}` in canonical form. It is optional for playback
   (FMT-9), but writing it puts the lane in `Confined.lanes`, and git and agents can see the folder.
10. **`layout.json`:** the new key goes in `order` right after `after` (the selected track), else at the end. The new
    track gets the first unused colour, and every other track's effective colour is written with it (§2.1).

**The Tx:**

| Op | File | Base |
|---|---|---|
| `Write` | `tracks/pluck.ts` | `Absent` |
| `TextEdit` (2 edits) | `song.ts` | accepted text |
| `Write` | `data/pluck/notes.json` | `Absent` |
| `Write` | `layout.json` | from disk (§2.1) |

`Expect::Confined { ranges: { "tracks/pluck.ts": whole, "song.ts": [the import's insertion point, the whole mixer call] },
lanes: [pluck/notes], modules: [Added(pluck)] }`.

**Why the range is the whole mixer call, not just the new property.** `Confined` admits a node when one of its places
lies inside a declared range. The new strip's nodes (its gain, its panner, one more `add` in std's `sum`) are made by
std's sites, and their only project place is the mixer call (`Facts.spans` holds each call of the frame chain, read in
`delta.rs` `segments`). Declaring only the property's range would refuse every add. Declaring the whole call admits more
than the gesture means, so the planner's assertion takes up the rest (WRITE-9).

**The planner's assertion,** on the checked candidate:
- the delta's modules are exactly `[Added(pluck)]`;
- the delta removes no node and changes no constant. The one exception is the first channel of an empty mixer: std's
  silent stand-in for an empty mixer (`scale(sine(0), 0)` in `mix.ts`) goes;
- the only nodes rewired are the mixer's sum chain and its consumer;
- the new channel is the track's channel by the §2.3 walk.

The channel goes last because occurrence numbers within the mixer's `map` don't shift then, so no existing strip is
renamed in the delta.

**The first test of the slice checks the attribution:** `adding_a_track_is_confined_on_every_fixture`, on first-song,
starter, empty and p16. The import node `import:pluck.out` has no origin, and its `src` is the `pluck.out` expression
(`push` in `core.ts`), which lies inside the mixer call. If any node turns out to have no place in a declared range, the
fix is in the planner's ranges, never in `Confined`.

**The view after:** the timeline shows the new track below the selected one, with its colour. The `lit` flash marks it
(taste §2.13), and it is selected.

**Refused, with words:**

| Case | Message |
|---|---|
| the draft doesn't compile | "The code doesn't compile right now, so nothing can be added. Fix it first." (the status pill offers Show in code) |
| no mixer, or two mixers | as in step 2 above |
| the key is invalid or taken | "lead vox isn't a key: keys are lowercase letters, digits and hyphens, such as lead-vox" (`key.invalid`'s suggestion) |
| read-only | the pill (no toast, taste ST-2) |

### 3.2 Duplicate a track

**What it copies:**
- **The code.** The track's `track()` call (`rename::track_call`) and its file.
  - When the file holds only that track and its imports, the new file is a copy with the key string replaced and the
    label made "Bass 2" (unique, as in §3.1).
  - When the track is written in `song.ts` (the Empty template's way), the statement `const drums = track(…)` is copied
    right after itself, with a free binding.
  - A track made by a helper or a loop (no unique `track()` call in a project file) is refused: "Bass is made by code
    that makes several tracks, so duplicate it in code."
- **The data.** Every lane file under `data/<k>/`, rewritten canonically with every clip id minted fresh (FMT-12;
  random, FMT-11).
  - Every plugin state blob and its sidecar is copied byte for byte to `data/<k2>/<frames>/<plugin-key>.state`.
  - A blob over 1 MiB travels as `Payload::Local`, a staging copy, so the Tx JSON doesn't carry 50 MB.
- **The channel.** It is found by §2.3 and copied with the binding substituted at every read of the old track's
  exports inside the property. The import nodes whose `src` lies in the property's range give the exact positions. So
  `bass: duck(bass.out, kick.env, { depth: db(-9) })` becomes `"bass-2": duck(bass2.out, kick.env, { depth: db(-9) })`.
  A producer expects "duplicate the channel strip" to keep the duck (survey §2.3).
  - When the original reaches no channel, the copy gets a plain one (`"bass-2": bass2.out`), and the toast says
    "Duplicated Bass into the mixer".
- **Reads of other tracks' exports** (`kick.env`) are copied as written. Readers of the old track's exports (another
  track reading `bass.env`) are not duplicated.
- **The layout.** The copy goes right below the original and **keeps its colour**, as Live and Logic do. Every colour
  is frozen.

**The Tx:**
- ops: `Write` the new file (or a `TextEdit` of `song.ts`), a `TextEdit` of `song.ts` (the import and the channel), a
  `Write` per lane and state file (`Absent`), and a `Write` of `layout.json`;
- `Confined { the new file: whole, song.ts: [import, mixer call], lanes: every lane of k2, modules: [Added(k2)] }`.

**The planner's assertion:**
- the new module's graph equals the old one's but for the key;
- its lanes hold the same clips but for their ids;
- the old channel's strip is unchanged.

After step 7 the original's instances are kept and the copy's are fresh. A copied plugin loads the copied blob (slice 4).

### 3.3 Delete a track

**What the builder finds** (survey §2.4, and the code and graph design's rule "require replacements for required
consumers or an explicit cascade"):

1. The `track()` call and its file. The file is removed when it holds only that track and its imports. Otherwise (a
   track written in `song.ts`) the statement that declares it is removed, with a trailing comment on its line.
2. Every import of the track's file, in every project file.
3. Its channel (§2.3). The property is removed with its line, its trailing comma, and any comment lines directly above
   it with no blank line between (`// Sidechain: the bass ducks…`).
4. **Every other read of its exports,** from the IR's import nodes in other modules, outside its own channel. Each is
   classified:
   - **removable:** the read is an argument of an effect call that the effect planner can excise (`duck(bass.out,
     kick.env, opts)` → `bass.out`, the same excise `remove_effect` makes);
   - **blocking:** anything else (`lowpass(x, kick.env)` in a track's code, a helper reading `kick.out`).
5. Its data: a `Remove` for every Lane and PluginState file (blobs and sidecars) under `data/<k>/`. The folder goes when
   it is empty (`stage.rs`). A file under it that isn't data keeps the folder (survey §2.4). Assets are never removed
   (FMT-14).
6. `layout.json`: the key leaves `order` and `tracks`, and every remaining colour is frozen.

**Outcomes:**
- **No other reads:** commit at once.
- **Only removable reads:** without `cascade`, the route answers 409 with the list (`{dependents: [{module: "bass",
  component: "Duck", file, line}]}`), and the UI asks (§3.5). With `cascade: true`, the excises go in the same Tx.
- **Any blocking read:** refused, with the list and "Show in code".

**The Tx:**
- `Expect::Confined { ranges: { the track's file: whole (when removed), song.ts: [the import line, the whole mixer call,
  each excised call], other importers: [their import lines, their excised calls] }, lanes: every lane of k, modules:
  [Removed(k)] }`;
- one undo step that restores every file, binary states included (WRITE-15, WRITE-21).

**The planner's assertion:**
- the delta's modules are `[Removed(k)]`;
- every remaining channel's strip is unchanged, compared by channel key rather than by delta name, because deleting a
  middle channel shifts the occurrence numbers of the strips after it;
- every excised call is gone and its first input is wired where it was read.

**Local state.** When the delete commits, the server drops `k`'s entries from `.music2/overlays.json` and
`.music2/view.json`, and keeps them in memory with the delete's undo step. Undoing that step puts them back, so the
track comes back muted, soloed and sized as it was. A later track that gets the key `k` (⌘T → Pluck after deleting
Pluck) starts unmuted and unsoloed instead of arriving soloed and silencing the song (§6.5).

### 3.4 Reorder and recolour

- **Reorder:** a Tx that writes `layout.json` only. `order` becomes the new full order, sent by the client and checked
  to be a permutation of the existing tracks.
- **Recolour:** a Tx that writes `layout.json` only: `tracks.<k>.colour = <name>`, with every colour frozen.

Neither makes a revision (PLANE-3), and each is one undo step.

### 3.5 UI

**+ Track** (TL-3, BR-1, taste §3.4 "Corner", §3.6):
- **The trigger:** a ghost `ctl-sm` button with `Plus` in the track-header corner, and ⌘T. Either opens the same list
  the side browser shows, as a popover anchored to the button (taste §2.14; one list in two places is the one allowed
  duplicate).
- **Rows:** 28 px, label in `text-sm`, grouped Drums then Instruments, with the description as the detail line.
- **Keys:** typing filters, ↑↓ move, Enter adds.
- **The keyboard-active row** when the list opens is the instrument last added in this window, else the first row.
- **The foot** shows the code it will write, in mono `text-xs` (BR-5): `tracks/pluck.ts · pluck(lane.notes("notes"))`.
- **After adding,** the new track is selected and lit (taste §2.13), and nothing opens: Space, M and S work at once, as
  in Live and Logic. ⌘R or a double-click on the name renames it. The key stays the one proposed from the instrument's
  label (ID-2's "never derives it again").
- **One step on purpose:** ⌘T then Enter adds the keyboard-active row: the same instrument again, or the first row in
  a new window. That is Live's speed. ⌘T, ↓↓, Enter picks another.
- **Other ways in:**
  - dragging an instrument from the side browser onto empty space below the tracks adds it there (BR-1);
  - Enter on an instrument row in the browser adds it below the selected track;
  - in the Empty template, the lanes show a centred primary **Add a track** button with "or write one in code ⌥4" in
    `fg-muted` (TL-3), replacing "No tracks yet" (`e11`).

**The header menu** (right-click, or ⋯ on hover; taste §3.4):
- Rename ⌘R
- Duplicate ⌘D
- Delete ⌘⌫
- —
- Colour ▸: a 6×2 grid of 16 px `rounded-xs` swatches, with a check on the current one (taste rule 6)
- Mute in song (8b.4)
- Show automation A
- —
- Edit in code (detail: `tracks/bass.ts`)

The track's ID isn't in this menu (§11.4).

**Delete:**
- With no dependents, nothing asks: the track goes, and the toast says "Deleted Kick · Undo" (6 s).
- With removable dependents, a dialog (taste §2.14):
  - title: "Delete Kick?";
  - body: "Bass and Chords duck to Kick. Deleting Kick also takes off those 2 Ducks.";
  - buttons: [Cancel] and a danger button [Delete Kick and 2 Ducks]. Enter confirms and Esc cancels.
- With blocking dependents, the dialog says "Kick can't be deleted from here: Keys reads it in its code (tracks/keys.ts,
  line 12)." with [Show in code] and [Close].

**Reorder:** drag a header vertically after a 4 px threshold. A 2 px `accent` line shows the drop between headers, and
the header follows the pointer with `shadow-lift`. The Song row is pinned at the bottom and isn't a drop target. Nothing
is toasted; ⌘Z undoes it.

**Duplicate:** ⌘D with a track header selected (and no clip). The copy appears below the original, selected and lit.

**Colours** come from `TimelineTrack.colour` through `palette.ts`. The index palette (`COLORS`, `colorOf`, `mute()`) is
deleted (taste §6 item 14).

### 3.6 Tests (Rust)

`crates/editor/src/tracks_tests.rs`, on the five fixtures and p16, each naming its rule:

- **Adding:**
  - `adding_a_track_is_confined_on_every_fixture`: the exact text of the file, `song.ts` and the lane file, the layout,
    and the delta (WRITE-10, ID-2, ID-18);
  - `the_song_sounds_the_same_after_adding_a_track`: an offline render before and after is equal by value. A new strip
    of silence adds `+0.0`, which can only turn `-0.0` samples into `+0.0`, so bits are compared with `==` and the test
    says why;
  - `adding_to_the_empty_template_fills_its_mixer`, `a_key_never_takes_an_unclaimed_folder`,
    `bindings_never_collide` (a kick into Empty gives `kickTrack`, `808` gives `t808`) and
    `a_song_without_one_mixer_is_refused` (the gates' sine song, and a song with two mixers);
  - `undo_removes_everything_add_wrote`.
- **Duplicating:**
  - `duplicating_copies_code_data_and_channel`: bass in first-song keeps its duck, every clip id is new and unique
    (FMT-12), and the colour is the same;
  - `duplicating_copies_plugin_state` (with-plugins, Surge's blob byte for byte), and
    `a_track_a_helper_makes_is_not_duplicated`.
- **Deleting:**
  - `deleting_removes_file_import_channel_data_and_layout` (hats);
  - `deleting_the_kick_lists_its_ducks` (409 without cascade), `the_cascade_takes_the_ducks_off` (`bass: bass.out`) and
    `a_blocking_read_refuses`;
  - `a_folder_with_other_files_stays`, and `undoing_a_delete_restores_every_byte` (with-plugins' state blob included,
    WRITE-15).
- **Layout:**
  - `reorder_and_recolour_write_only_layout` (no revision, one undo step, PLANE-3);
  - `the_first_layout_write_freezes_every_colour`, `a_hex_colour_reads_as_the_nearest_name` and
    `unknown_layout_keys_survive_a_write` (FMT-19);
  - `a_missing_tracks_order_and_colour_survive_a_write` (Hook commented out, Bass recoloured, Hook restored in place);
  - `a_layout_that_doesnt_parse_is_never_overwritten`, `the_track_builders_work_while_layout_doesnt_read`, and
    `reset_layout_is_one_undo_step`.
- **The template:** `the_empty_templates_comment_reads_true_after_add` (its text, and the render unchanged).

`crates/server/src/graph/tests.rs`:
- `an_outside_layout_edit_is_published_with_no_revision`;
- `a_layout_write_after_an_outside_edit_conflicts` (its base was read from disk);
- `a_new_track_on_a_deleted_key_starts_unmuted` (solo Pluck, delete it, add a Pluck: nothing is soloed), and
  `undoing_a_delete_brings_its_mute_and_height_back`.

`apps/m2/tests/tracks.rs`: `m2 add-track`, `duplicate-track` and `delete-track` on a copy of first-song, and each refused
while the app holds the lock (WRITE-18).

### 3.7 Browser suite: `e2e/s8-tracks.mjs` (kind `fix`, plus an `empty`-template part made through the New Song API)

About 64 checks *estimate*; the exact count goes into `run.mjs` and the README's table when the suite lands.

- **+ Track:** the button and ⌘T open the list; typeahead and Enter add a Pluck. On disk: `tracks/pluck.ts`, the
  `song.ts` import and channel, `data/pluck/notes.json` and `layout.json`, exactly. The header appears below the
  selected track, in its colour, selected and lit, with no rename open: Space then plays. ⌘R, "Lead Vox" and Enter
  write only the name. A second ⌘T, Enter adds another Pluck.
- **Stable colours:** every other header's colour is unchanged, read before and after.
- **Empty template:** its Add a track button adds a Kick and the track plays its clip.
- **Delete:** ⌘⌫ deletes Hats without asking. Undo restores the five files byte for byte. Deleting Kick shows the
  dialog with both Ducks; confirming leaves `bass: bass.out`; undo restores it.
- **Duplicate:** ⌘D on Bass gives "Bass 2" below it, its clips have new ids, and the duck is copied.
- **Layout:** dragging Kick to the top writes only `layout.json` and no revision; ⌘Z puts it back. Recolouring writes
  the name. An outside edit to `layout.json` reorders the timeline within 1 s. With conflict markers written into
  `layout.json`, + Track still adds a track, reorder refuses with its words, and Reset layout restores the defaults.
- **Local state:** solo Pluck, delete it, then ⌘T, Enter: the new Pluck isn't soloed and the other tracks play.
- **Read-only:** a second window (WRITE-17) shows + Track disabled and the pill, and nothing is written.

### 3.8 Contract edits

- **WRITE-10:** rows for duplicate, delete, reorder and recolour (§14).
- **FMT-19:** the schema, the reader, the watch, the base from disk; keys of tracks that are briefly absent are kept.
- **The HOME table's example:** the colour is a name.
- **ID-18:** the walk of §2.3.
- **The DIAG table:** `layout.invalid` (Notice), with its Reset layout fix.
- **WRITE-18:** the new commands.
- **ID-17's Today:** unchanged until 8b.9.

### 3.9 Stays green because

- No render changes: std's `editor` fields are metadata.
- Track order changes on screen (D8: mixer order, not import order). So the suites that read the timeline's order
  (`layout`, `views`, `s4-clips`) are updated in this PR, with their testids kept (taste §0).
- The index palette's removal changes the colours that `s6-builders`' `barColor` reads, and that suite follows too.

---

## 4. 8b.2 The song bar: tempo, time signature, title, transport

**Why second:** "Setting the tempo is step one of every song" (gaps #6). The audit's rank 6 is the fields; rank 20 is the
keys. It is small, needs no engine work, and the lease comes in 8b.7.

### 4.1 Builders (`crates/editor/src/song.rs`)

- **`tempo(s, at_beat, bpm)`** sets the tempo of the entry in effect at `at_beat` (the playhead, by default).
  - It writes `data/arrangement.json` from `Arrangement::canonical()`, based on the accepted text.
  - `Expect::Confined {}`: check 5 admits an arrangement the Tx writes (survey §5.3).
  - It refuses what FMT-13 refuses (20–999 BPM), with the reader's message.
- **`tempo_change(s, beat, bpm)`** adds an entry; **`remove_tempo(s, beat)`** removes one. The first entry can't be
  removed.
- **`meter(s, at_beat, beats, unit)`** sets the meter in effect at `at_beat`, keeping its beat. A new change is snapped
  to the bar line at or before `at_beat` in the meter before it (FMT-13's rule), so it is never refused for being off a
  bar line. `beats` is 1–64 and `unit` is 1, 2, 4, 8, 16 or 32.
- **`title(s, name)`** is a `TextEdit` of `[project] name` in `music2.toml`, written as a TOML basic string, with
  `Expect::Identical`. It takes the code plane (a compile) and its delta is empty. The folder isn't renamed (gaps #28
  covers that).

Labels and toasts are in §2.4. A tempo change that makes the loop too short turns the loop off, as today (`Timing`), and
the toast says "Loop turned off: it's shorter than a beat at 128 BPM".

**New Song** (NS-3): `POST /api/project/new` takes `tempo?`, and writes it into the new folder's `data/arrangement.json`
when it creates the project. That is creation, not a Tx.

### 4.2 HOME-1 for the arrangement

`m2_session::Home` gains `Arrangement` (serialised `"arrangement"`). The tempo and meter the view returns carry it. This
is a core change outside the write API, so the HOME table and HOME-1 get a row first (§14). The home marker table (taste
§2.10) gains a row: the arrangement has no marker, and its tooltip says "Saved in data/arrangement.json".

### 4.3 Transport leftovers (no engine work)

| What | How |
|---|---|
| **Stop returns to where play started** (taste §5.1) | The page remembers the beat where play started. Stop sends `stop` then `seek(start)`. ⇧Space continues from where it stopped (`seek` to the stopped beat, then `play`). A click in the ruler or on empty lane space moves the start point. |
| **C** toggles the loop; **⌘L** loops the selection | ⌘L takes the span of the selected clips, else the bar at the playhead (`POST /api/graph/loop` with `start` and `end`). |
| **The loop range persists** | in `.music2/view.json` (§2.2), read at open and set on the transport before play. |
| **Panic, ⌘.** (gaps #8) | `POST /api/graph/panic` → `Player::panic()`, which exists (RENDER-10). It is in the Transport menu as Stop All Sound. |

### 4.4 UI (taste §3.3, TB-5, NS-3)

**The tempo field:** 76 px, "124" in `text-base` 500 plus "BPM" in `text-xs` `fg-faint`.
- Drag up or down: 1 BPM per 4 px, or 0.1 with ⇧.
- Double-click types "128", "128.5" or "128 bpm". There is no ⌥-click reset: a stray click would reset the song's
  tempo (taste §5.2).
- **Until 8b.7,** a drag shows the value bubble and commits on release, and its tooltip says so ("Heard when you let
  go"). From 8b.7 a drag is a lease: the number turns `accent`, and the song follows it.
- **No tap tempo** in 8b (§15).
- **With several tempo entries,** the field shows and edits the entry in effect at the playhead. Its tooltip says
  "Tempo from bar 9: 128 BPM".

**The meter field:** 40 px, "4/4".
- A click opens a picker popover: beats from 1 to 16 as a stepper field (up to 64 typed), and a unit segmented control
  of 2, 4, 8 and 16.
- It edits the meter in effect at the playhead.

**The ruler marks:** the existing tempo and meter marks (`s4-meter`) are 11 px `fg-muted` text on the ruler's top half.
- A click opens the same field as a popover at the mark, with Remove (except on the first entry).
- The ruler's right-click menu gains "Add tempo change at 9.1" and "Add time signature change at bar 9".

**The title:** a double-click on the centred song name edits it in place.
- Enter writes, and Esc cancels.
- An empty name is refused ("A song needs a name") with the field's error style (taste §2.9).

**The New Song dialog:** a Tempo field 64 px wide beside the name, filled with the template's tempo. Tab moves to it.

**The corner:** "TRACKS" and "4/4 · 124 BPM" leave the track-header corner (TL-17); the S slice takes that space.

### 4.5 Tests

- `crates/editor/src/song_tests.rs`:
  - `a_tempo_edit_writes_only_the_arrangement`, `the_tempo_at_the_playhead_is_the_one_edited` and
    `a_tempo_out_of_range_is_refused` (FMT-13);
  - `a_meter_change_lands_on_a_bar_line`, `the_first_tempo_entry_stays` and `undo_restores_the_arrangement_bytes`;
  - `a_title_edit_is_identical` (WRITE-6);
  - `every_value_names_its_home` extended with the tempo and meter (HOME-1).
- `crates/server/src/graph/tests.rs`:
  - `the_loop_range_survives_reopening`;
  - `panic_ends_every_note_and_never_touches_the_session`;
  - `a_tempo_too_fast_for_the_loop_turns_it_off`.
- `apps/m2/tests/song.rs`: `m2 tempo 128` and `m2 meter 7/8 --at 9`.

### 4.6 Browser suite: `e2e/s8-songbar.mjs` (kind `fix`, about 40 checks *estimate*)

- **Tempo:** dragging the field writes `arrangement.json` once on release, with no toast; typing "128" writes it;
  ⌥-click changes nothing; undo restores the bytes.
- **Meter:** 7/8 from the picker re-bars the ruler (as `s4-meter` reads it).
- **Ruler:** Add tempo change at bar 5 writes a second entry, and its mark appears.
- **Title:** renaming writes `music2.toml` and the window title follows.
- **Transport:** Space plays and Space stops back at the start beat; ⇧Space continues; C toggles the loop; ⌘L loops a
  selected clip; reopening keeps the loop; ⌘. calls panic.
- **New Song:** with tempo 140, `arrangement.json` says 140.

### 4.7 Contract edits and green

- **Edits:** WRITE-10 rows for tempo, meter and title; the HOME table and HOME-1's `arrangement`; WRITE-18's commands;
  gaps.md #6, #7 and the loop's "What works today" fixed (survey §8).
- **Green:** nothing renders differently. `newsong`'s check on the New Song dialog gains the field; its testids stay.

---

## 5. 8b.3 Faders: the Mix view

**Why third:** the audit's rank 1, with 8b.4. "In first-song **the kick has no level control at all**" (`b02-…-song`).
The faders are plain knob leases and Values Tx, so they need no engine work. Mute and solo follow in 8b.4. The strip
keeps their row empty until then, so nothing moves when they arrive (taste §1.8).

### 5.1 std: every channel and the master get a gain

- **`mixer`:** `db: control.db(0).range(-60, 12)` drops `.optional()`, and the body always applies `gain(x, c.db)`.
  `pan` stays optional (PKG-5's reason: a mono input through the panner at centre isn't bit-exact).
- **`master`:** gains `db: control.db(0).range(-60, 12)`, a gain before the limiter.
- **The foot of a fader is silence, with no kernel change.** Every pro fader's bottom is −∞, and a 0 dBFS hat left at
  −60 dBFS is audible in headphones during a breakdown. std's `db_to_gain` already returns exactly 0 at −96 dB or below
  (`core/kernel/src/lib.rs:234-236`). So the fader's foot writes `db: -96`, which the UI shows as "−∞". `range(-60,
  12)` stays the display hint (PKG-8: a range is a hint, never a clamp). std's doc comment on `mixer` and `master` says
  "−96 dB or lower is silence", and no render changes.

**Bit-exact, as the survey argues and the renders must prove:** `db_to_gain(0) = 10^0 = 1.0` exactly, the gain kernel's
smoother primes to its target, and `x * 1.0 == x`.
- `core/session/tests/renders.json` must be unchanged for every pure fixture, and the slice doesn't land otherwise.
- If it changes, the fallback is the survey's option 2: a channel's first fader move is a structural `Confined` write of
  `db` at the drag's threshold, and the drag continues as a lease once that revision plays. That is the first pan's path
  (§5.3), so it needs no new decision.

**The cost:** one more `gain` kernel instance per channel that didn't write `db` (two in first-song, one in the Starter),
and one more `Store` each after step 7 (RENDER-32). Adding a channel already resets every strip inside the mixer's `map`
(design-7 Q9), and that stays inaudible, since `gain` primes to its target and `pan` holds no state.

### 5.2 Writing a value into a dictionary entry

Today `Insertion::plan` (`crates/editor/src/lens.rs:83`) refuses any slot with a `.`: "it is a dictionary entry's, which
is written with its entry". With `db` inherited on every channel, the channel's fader is a knob with an inherited value.
Its first move must write it:

| Entry as written | After the first move to −3 dB |
|---|---|
| `kick: kick.out`, in first-song, whose other entries write `db(…)` | `kick: { in: kick.out, db: db(-3) }` |
| `hats: { in: hats.out, pan: 0.3 }`, in a file that writes no `db(…)` | `hats: { in: hats.out, pan: 0.3, db: -3 }` |
| `bass: duck(bass.out, kick.env, { depth: db(-9) })`, in first-song | `bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), db: db(-3) }` |

- **The value follows the object's style,** so a code-first song never mixes two spellings in one object: `db(-3)` when
  another entry of the same mixer object writes `db(…)` and the file imports `db`; otherwise a plain number, as PKG-3's
  insertions are. The same rule holds for every 8b insertion of a unit value. The entry's `in` keeps the exact text it
  had.
- **It is a `Values` Tx,** because WRITE-6 says inserting an omitted argument "must still count as `Values`". The gain
  node existed with an inherited constant, and now has a written one.
- **Undo** puts the entry's old text back (WRITE-21: a `TextEdit` located by its carried range and recorded value).
- `Insertion::plan` learns the entry forms: a bare signal is wrapped as `{ in: <text>, <slot>: <v> }`, and an object
  gets a property. The anchor is `Anchor::Slot { site: the mixer call, slot: "channels.kick.db" }`, which already names
  an inherited control (WRITE-6).

### 5.3 The first pan on a channel

`pan` is optional, so an unpanned channel has no panner node and no knob: first-song's Kick and Bass, and every track
+ Track adds. A producer pans while the loop plays, so the first pan must be heard during the drag, as the fader beside
it is.
- **When the first pan drag passes its 4 px threshold,** **`mix::insert(s, at, channel, "pan", 0)`** writes `pan: 0`
  into the entry (wrapped as above when it is bare): one `Confined` Tx with the whole mixer call as its range (§3.1's
  reason). The planner's assertion: the only structural change is one new panner on this channel's strip, and its
  constant is 0.
- **Until that revision plays** (about 200 ms into the drag *estimate*), the value bubble shows the drag. Then the drag
  goes on as a knob lease on the new knob, and the release commits its value as a `Values` Tx.
- **Undo:** ⌘Z once puts the pan back at C; a second ⌘Z takes `pan: 0` out of the code.
- **Inaudible insertion.** At C the panner is exactly unity for a stereo input, and about 1e-7 from it for a
  mono one (`cos(π/4)·√2`, `std/kernels-src/src/lib.rs:414-419`), so the revision that adds it changes nothing anyone
  hears. Making `pan` non-optional instead would change renders (`renders.json`) for every mono channel.

### 5.4 The view: `mix` in the graph state

`m2_editor::mix::view(s, &layout) -> MixView` is sent in the graph state beside `view` and `timeline`:

```json
"mix": {
  "mixer": "ok",                       // or "none" or "several" (§2.3)
  "strips": [
    { "module": "kick", "channel": "kick", "label": "Kick", "colour": "rose",
      "db": "song.ts#4/@channels.kick.db", "pan": null, "pan_insert": true,
      "muted_in_song": false, "feeds": [{ "module": "bass", "label": "Duck" }], "why": null }
  ],
  "master": { "db": "song.ts#3/@db", "ceiling": "song.ts#3/@ceiling" }
}
```

- `db`, `pan` and `master.db` are knob ids from the view, so the web uses the same knob model, leases (`/preview`,
  `/set`) and home markers as every other knob.
- Strips come in the effective track order (§2.1). `feeds` drives MON-2's row (§6.7), and `muted_in_song` the struck
  name (8b.4).
- `every_value_names_its_home` covers the strip: faders `code`, colours `layout`.

### 5.5 UI (taste §3.5 Mix, MX-1, MX-2, MX-5, MX-6, TL-1, §5.2)

**The Mix view** (⌥3, or X): strips 76 px wide with a 1 px `line` between them. Master is 96 px, pinned right after a
12 px gap.

| Row | Height | What |
|---|---|---|
| colour bar | 3 px | the track's `bar`; graphite for Master |
| name | 20 px | `text-sm` 500 |
| pan | 32 + 14 px | a 24 px knob and its value: "C", "L 20" or "R 35" |
| M and S | 24 px | empty until 8b.4 |
| fader | the rest | a 4 px track and a 20×10 px thumb; 0 dB sits at 75% of the travel |
| dB field | 20 px | `text-xs`; "−∞" at the foot |
| sidechain row | 16 px | only when it applies (8b.4) |

**The fader law:** 0 dB at 75% of the travel. Above it, the position is linear up to +12 dB at the top. Below it, the
position is `0.75 × ((db + 60) / 60)²` over the travel above a 4 px foot, so −6 dB sits at 61% and −20 dB at 33%. The
foot itself is −∞ (§5.1).

**Gestures** (taste §5.2):
- dragging along the travel covers the full range, down into the foot;
- ⇧ drags ×0.1;
- ⌥-click resets to 0 dB;
- a double-click on the dB field types a value ("-3", "-3db", "-inf");
- scrolling moves 0.1 dB a notch, and one notch below −60 dB reaches −∞.

**Values and homes:**
- A channel that doesn't write `db` shows 0.0 dB in `fg-muted`, with the tooltip "Default. Not written in your code
  yet: changing it writes it in" (taste §2.10), and its first move writes it (§5.2). There is no marker: an inherited
  value lives in code like a written one.
- An unpanned channel's pan reads "C" in `fg-muted`. Its first drag writes `pan: 0` at the threshold and is heard about
  200 ms in (§5.3).

**Strips without a fader:** a track that reaches no channel gets a strip whose fader area says "Not in the mixer" in
`text-xs` `fg-muted`, with Edit in code.

**Master:** its fader is std's new master `db`; the ceiling stays a knob on the Song's Device card. Master has no M or S
(MON-1).

**The strip menu** (right-click): Mute in song (8b.4), Colour ▸, Rename, Edit in code.

**No level slider in the track headers** (taste §3.4). A 44 px slider over −60 to +12 dB would move about 1.6 dB a
pixel, repeat the Mix fader (taste principle 2), and leave the name 95 px, which cuts "Shakers & Hats". The headers keep
M, S and the meter, and the name gets 140 px. The level is the strip's fader, one keystroke away (⌥3).

**The Song Device view** (MX-6): the mixer becomes one compact card, "Mixer · 6 channels · open Mix ⌥3", instead of the
16-row node (`f03`).

### 5.6 Tests

- **std:** `the_mixer_and_master_gains_change_no_bit`, which is `renders.json` unchanged plus a direct render of every
  pure fixture before and after (RENDER-2, FMT-3). `std_defaults_are_todays` gains the two `db` controls.
- **`crates/editor/src/mix_tests.rs`:**
  - `every_channel_has_a_fader` (first-song's kick and bass);
  - `the_channel_is_found_through_the_duck`, `two_tracks_in_one_channel_share_a_strip` and
    `a_track_in_no_channel_says_why` (ID-18);
  - `a_bare_entry_is_wrapped_on_its_first_move` (Values) and `undo_puts_the_bare_entry_back`;
  - `an_insertion_follows_the_objects_style` (`db(-3)` in first-song, `-3` where nothing writes `db(…)`);
  - `the_first_pan_inserts_a_centred_panner` (Confined, inaudible: a render before and after differs by at most
    1e-6 on a mono channel), and `the_master_fader_writes_master`;
  - `the_fader_foot_is_silence` (`db: -96` renders exact zeros for that channel, and `renders.json` is unchanged);
  - `every_value_names_its_home` (the mix view).
- **`insert_tests.rs`:** the entry forms of §5.2, each checked against `forms.ts` (`planned_call_forms_are_current`,
  PKG-11).

### 5.7 Browser suite: `e2e/s8-mix.mjs` (kind `fix`, about 45 checks *estimate*)

- ⌥3 shows six strips in track order with labels.
- Dragging the Kick fader writes `kick: { in: kick.out, db: db(…) }` and nothing else, with no toast, and undo restores
  `kick: kick.out`.
- ⇧-drag moves a tenth as far; a double-click types −3; ⌥-click writes 0; the foot writes −96 and reads "−∞".
- The first pan drag on Kick writes `pan: 0` once it passes 4 px, and the release writes its value; ⌘Z twice restores
  `kick: kick.out`.
- Track headers have no level slider, and "Shakers & Hats" fits in a header.
- The master fader writes `master(…, { db: … })`.
- The Song Device view shows the compact mixer card.
- A read-only window's faders don't drag.

### 5.8 Contract edits and green

- **Edits:**
  - PKG-5: the mixer's `db` and the master's `db` aren't optional; the reason is that a gain at 0 dB is bit-exact. A
    fader's foot writes −96 dB, which `db_to_gain` makes silence.
  - PKG-3's Today: dictionary entries take insertions, written in the style the object already uses.
  - WRITE-10: rows for "a fader on a channel that doesn't write it" (`Values`) and "insert an optional control"
    (`Confined`, at the first drag's threshold).
  - MON-2's "The mixer shows this" gets its UI in 8b.4.
- **Green:** `renders.json` unchanged; `s5-knobs`' "every declared control is a knob" gains the two faders, and its
  count follows in this PR.

---

## 6. 8b.4 Mute, solo and mute in song

**Why fourth:** "It's the most-used button in every DAW, every minute" (audit). It carries all of gate 8's 8b clauses.
It is built after step 7 slice 2, on `plan.rs` and `PlanInfo` (survey §1.4), and after 8a, whose partitioner already
labels an overlay with its consumer's module (design-8a §3.3).

### 6.1 Where an overlay goes (`core/ir`)

The checker computes the list **`Flat.overlays: Vec<OverlaySite { module: String, consumer: Consumer, channels: u8
}>`**, with `enum Consumer { Input { node: usize, input: u16 }, Out }`. A site is every input of a top-level node, or a
capture inside a voice, that meets all of these:
1. it reads a node of track module `m`;
2. it belongs to a node of another module;
3. its source is an audio output;
4. it **isn't a sidechain input**.

**The song's output is a site too** (`Consumer::Out`) when `Flat.out` reads a track's node directly. Imports resolve
through (`core/ir/src/check.rs` `resolve`), so in an agent's `song(() => lead.out)` nothing in `song` reads `lead.out`:
`Flat.out` is a `PortRef`, not a node input, and without this M on Lead would do nothing, against MON-1's "they work for
every song". Its overlay is a node just before the output; for solo paths its consumer is `song`, and 8a labels it
`song` (design-8a §3.3).

Sidechain inputs are new in `Flat` too: **`Flat.side_inputs: Vec<(usize, u16)>`**. These are the inputs, inside a frame
that declared a sidechain input (PKG-13), that read the signal that frame recorded as received. For example, `follow`'s
kernel reads `kick.out`, and `duck`'s kernel reads `side`. `Flat.sidechain` (the sources) stays as it is.

**Why the checker, not the runtime:** the checker knows port kinds and frames, and the runtime doesn't. Why data, not
nodes: `Flat`'s nodes, which the delta and `same_layout` compare, stay exactly as they are. The same code gives the same
list (it is derived from structure).

### 6.2 The overlay op (`core/runtime`)

`prepare` (design-7 §6) and `Renderer::new` for offline renders put one **`Exec::Overlay { src, cell, cur, step }`**
node at each site. The consumer's input is pointed at it; a voice capture reads it from the outer level.
- **One node and one cell per site:** MON-1 asks for "one overlay gain cell on each consumer port". So two consumers of
  `kick.out` get two overlay nodes with a cell each, and solo can set them apart (§6.4).
- **The block, with `g` = its cell's value:**
  - when `cur == g`, each sample is `x × cur`;
  - otherwise `cur` moves toward `g` by `step` per frame, clamped at `g`, and each sample is `x × cur`.
- **The glide:** `step = 1 / round(0.010 × rate)`, so a full mute or unmute takes 10 ms (480 frames at 48 kHz). All
  channels share `cur`.
- **At `g = 1`** the op is `x * 1.0`, which changes no bit, including for −0.0 and NaN. Every pure fixture renders the
  same bits, so RENDER-2 and RENDER-33 hold. At `g = 0` it gives ±0.0, the same on every run.
- **No box.** The op is built-in, not wasm (core has no DSP, and the planner can't depend on a package). `cur` is
  plan-side and starts at the cell's value in a new plan. A glide in flight at an activation finishes as a step of at
  most what is left of it, which is rare and small (§16).
- **8a:** the node is labelled with its consumer's module (design-8a §3.3), so no buffer is written by two tasks.
- **Step 7's fades see through it.** `continues` joins `Flat` nodes, and overlay nodes aren't in `Flat`. So `prepare`
  compares a consumer's source as `Flat` gives it (the export), not the overlay node. An overlay never makes an edge
  "changed", so it never adds a fade. An edge to a new consumer is new anyway, and fades as any new edge does.

**Cells.** `CellMap.overlays: BTreeMap<String, Vec<(SiteIdx, CellId)>>` maps each track module's key to its sites'
cells (RENDER-23: "mapped from a module key"). `SiteIdx` is the site's index in `Flat.overlays`, which is plan-local and
never identity. Overlay cells step in `Cells::apply`, because the op does its own glide. They are written through
`Player::set_cells` like any cell, so nothing new crosses to the audio thread.

**Their first values** come from the caller of `prepare`, as `OverlayInit { values: Vec<f32> }`, one value per site:

| Render | Value per site |
|---|---|
| offline (Export Audio, `m2 render`, `m2 bounce`, range renders) | 0 if the site's module is muted in song, else 1. Overlays never reach it (MON-4, RENDER-3). |
| live | the server's combined value (§6.4) |

The old one-block ramp of RENDER-22a doesn't apply to overlay cells. The 10 ms glide replaces it (§6.8 says why, and §14
edits the rule).

### 6.3 Mute in song (MON-5)

| Piece | Change |
|---|---|
| SDK (`core/script/sdk/core.ts`) | `TrackOptions.muted?: boolean`; `options()` allows `muted: "boolean"`; `__build` emits it. `@m2/core` 1.x gets a minor version (an addition, FMT-2). |
| IR (`core/ir`) | `Module.muted: bool`, `#[serde(default, skip_serializing_if = "Not::not")]`, so every existing IR's JSON, and so every revision's hash, is unchanged (ID-11). |
| Delta (`tx/delta.rs`) | `ModuleChange::Muted { module, muted }` is produced when `Module.muted` differs; `structural()` leaves it out. |
| Actor (`reach`) | a delta of only `Muted` changes (and values) from what plays is **cell writes**: each site of each changed module gets its combined value, and no `Activate` is sent. |
| Builder (`tracks::mute_in_song(s, at, modules, muted)`) | the `track()` call's options, edited as `rename_label` edits `name`: insert `{ muted: true }` after the key, add `muted: true` to the object, or remove the property (and the object when it holds nothing else). Several modules in one Tx is one undo step (used by the export dialog, §6.6). |

The builder's Tx is `Confined { each options' range (or the insertion point after the key), modules: [Muted{k, …}…] }`.
That is WRITE-10's existing row.

### 6.4 Combining, and the solo rule (`crates/server/src/graph/overlay.rs`)

A site `m → c` is an edge from track module `m` (its source) to module `c` (its consumer's). For each site:

```
on_path(m → c) = solo is empty
              or  some soloed s has  s = m  or  s reaches m      (the edge leaves s, or something s feeds)
              or  some soloed s has  s = c  or  c reaches s      (the edge feeds s, or something that feeds s)
value(m → c)   = 0  if muted_in_song(m) or m ∈ mute or not on_path(m → c);  else 1
```

- **"Reaches"** is over the module graph whose edges are the `Flat.overlays` sites. Sidechain inputs aren't sites, so
  they aren't edges (MON-3's exception).
- **This is MON-3 exactly:** an edge is heard when it lies on a path through a soloed module.
  - Solo a vocal: its sites into a bus are heard, and the bus's sites into the song are heard. A backing vocal's site
    into the same bus is muted, since neither condition holds for it (survey §3.3).
  - Solo the bus: the vocal's site into the bus is heard, but a direct site from the vocal to the song is muted. One
    value per module couldn't tell those two apart, which is why each site has its own cell.
- **Mute wins over solo** on the same track, as in Logic. That includes mute in song: Hook, muted in the song as an
  alternate part, stays silent when soloed. Letting solo override it would make S mean two things. Its S lights, its
  tooltip says "Muted in the song: unmute it to hear it", and the roll says so (§6.7).
- **The root `song`** is never muted or soloed: the route refuses it (MON-1).
- **The strip's state** comes from its module's sites: "muted by a solo" when all of them are 0 and the track isn't muted
  itself.

The server keeps `Overlays { mute, solo }`. On a change, or on a revision that changes `Flat.overlays` or a `muted`, it
recomputes every site's value and writes the cells whose value changed. It does so under the control plane's lock (the
knob leases' lock). A plan built while overlays are active gets their values in `OverlayInit` before it is sent, as a
knob lease's value goes into every new plan (WRITE-22).

### 6.5 `.music2/overlays.json` (MON-4)

The file is `{"mute": ["kick"], "solo": []}`, sorted.
- The server writes it atomically after each change, at most every 200 ms, and reads it at open.
- **Entries whose module is gone** (a track commented out in code) stay in the file and are ignored, so the track comes
  back muted as it was. Clear all removes them.
- **Deleting a track with the builder** drops its entries here and in `view.json` when the delete commits. The server
  keeps them in memory with the delete's undo step, and puts them back only when that step is undone (§3.3).
- **Adding or duplicating a track** drops any entries its new key still has (left by a track an agent deleted in code),
  so a new track never arrives muted or soloed.
- **A `Rekeyed` change** in an accepted delta renames the entry here and in `view.json`, and undo renames it back.
- **A read-only window** (WRITE-17) reads the file at open and never writes it. Its mutes and solos live in memory, and
  its pill says so (MON-4 edit).

### 6.6 Export and the two mutes (DIAG `overlays.active`, survey §3.6)

**Export reads committed files only** (RENDER-3). So its `OverlayInit` comes from the IR alone, and the file is
byte-identical whatever is muted or soloed: MON-4 by construction. The export's result carries the Warning
`overlays.active` whenever any monitoring mute or solo changes what is heard when the export starts.

**The dialog before exporting** (EX-3), shown only when overlays are active:
- **with monitoring mutes:**
  - title: "Kick is muted only while you listen";
  - body: "It will be in the file.";
  - buttons: [Export anyway] and a primary [Mute Kick in the song, then export] ("Mute 3 tracks in the song, then
    export" for several). The button names the lasting change. It writes the `muted` option on each muted track in one
    Tx (one undo step), clears those monitoring mutes, and exports. The done toast says "Exported My Song.wav · Kick
    stays muted in the song", and its action is **Unmute Kick** (a mute-in-song Tx back), so the struck-through Kick is
    never a surprise found later.
- **with a solo:**
  - title: "Only Bass is soloed";
  - body: "The file will have every track.";
  - buttons: [Cancel] and a primary [Export anyway]. Muting the others in the song would be a strange thing to offer.
- **"Don't ask again for this song"**, a checkbox in both, kept in `view.json` (`export.ask`). A producer who keeps the
  kick muted while writing the bass and bounces every few minutes isn't stopped each time. From then on the export goes
  straight ahead, and its done toast carries the warning: "Exported My Song.wav · Kick was muted only while listening,
  so it's in the file".

### 6.7 UI (taste §2.9 mute states, TL-1, MX-1, MX-3, MX-4, TB-7, §5.2)

**M and S** sit in every track header (20 px each, at x = 180 and 202, taste §3.4) and on every strip (24×20 px).
- **States, as taste §2.9 gives them:**
  - muted while listening: a `gray-700` fill with a white M;
  - soloed: an `accent` fill;
  - muted by someone else's solo: the name in `fg-faint`;
  - muted in song: a `gray-700` M, the name struck through, and the clips at 40% with hatching. The strikethrough and
    the hatching already say "saved in the song", so the M carries no extra dot.
  - A muted track's clips draw at 40% with `grayscale(0.6)`.
  - A strip shared by several tracks (§2.3) shows a dash in M or S when its tracks differ.
- **Gestures:**
  - a click toggles;
  - ⌥-click on S solos only this track;
  - dragging across several M (or S) buttons sets each to the first one's new state, as in Logic and Live;
  - the M and S keys (and ⌃M, ⌃S in the Track menu) act on the selected track;
  - on a shared strip, M and S act on every track in it.
- **On a track muted in song:**
  - a click on M opens the header menu with "Unmute in song" first, because that is a Tx (taste §2.9);
  - the M key and drags across M skip it;
  - S lights when clicked but plays nothing (mute wins, §6.4), and S's tooltip says "Muted in the song: unmute it to
    hear it".
- **The Song row and Master** have no M or S (MON-1).

**The overlay pill** (TB-7, taste §3.3): "2 muted · 1 solo" in the top bar while any monitoring mute or solo is on. Its
fill is `accent-soft` and its text `accent-strong`.
- A click opens a 360 px popover listing each muted and soloed track with its own ×, and **Clear all** at its foot.
- The pill has no × of its own: overlays aren't a Tx, so ⌘Z can't bring back a six-track mute and solo set that one
  stray click wiped. Clearing all takes two clicks.
- It is required because MON-4 keeps overlays across reopening: a song can reopen half silent next week (survey §3.6).

**Mute in song** sits in the header menu and the strip menu. Its toasts are in §2.4.

**MON-2 made visible** (MX-3): on the strip of a track that is muted, or silenced by a solo, but whose `feeds` aren't
empty, everything dims except one `text-xs` `fg` row: "Still drives Duck on Bass". The wording uses descriptor labels
and the fed track (§2.3 item 6), so the app never needs std's vocabulary (P10); taste MX-3 says the same now.

**Audition and mute:** audition on a muted track is silent, as in Ableton. The roll's header says so in `text-xs`
`fg-muted` while it is: "Muted · audition is silent", "Muted in the song · audition is silent", or "Silenced by the Bass
solo · audition is silent".

### 6.8 Gate 8's 8b clauses (all in this slice; how each is proven is in §13)

- **The sidechain fixtures,** `core/session/tests/fixtures/sidechain/`: three small projects (kick, bass and a mixer),
  each ducking the bass to the kick a different way:
  - `env`: first-song's form, `duck(bass.out, kick.env, …)`;
  - `follow`: `duck(bass.out, follow(kick.out, { release: ms(150) }), …)` written in `song.ts`;
  - `keyed`: `compress(bass.out, { key: kick.out, threshold: db(-30), ratio: 8 })`.
- **std gets a keyed compressor.** std has no compressor with a key input, and a hosted plugin can't be keyed (survey
  §3.5). So `compress` gains `key?: Audio`, declared `port.audio({ sidechain: true })`.
  - When `key` is given, the body calls a **new kernel `std:compress_keyed`** (inputs `in` and `key`, the same detector
    code). Without it, the body calls the old `std:compress`, unchanged.
  - So no existing render can change. The wasm file's hash does change, so every std instance starts fresh once after
    the upgrade (design-7 §5.2), which is harmless.
- **A track-wrapped sine,** `crates/gates/src/songs.rs`: `tone(hz)` is `track("tone", () => osc.sine(hz))` mixed by
  `mixer({ tone: tone.out })`, at full scale. The gates' sine has no track to mute (survey §3.5).
- **Why 10 ms and not one block.** The gate's limit is twice the sine's own largest step, about `2π f / sr`. A ramp of
  length L adds at most 1/L per sample.
  - At 440 Hz: the sine's own step is 0.0576, so the limit is 0.115. A one-block ramp (1/128 = 0.0078) passes, and so
    does a 10 ms glide (1/480 = 0.0021).
  - At 50 Hz, a bass note: the sine's own step is 0.0065, so the limit is 0.0131. A one-block ramp gives 0.0065 + 0.0078
    = 0.0143, which fails. The 10 ms glide gives 0.0086, which passes.
  - Our own test runs at both frequencies.

### 6.9 Tests

- **SDK and IR:**
  - `muted_is_a_track_option` (`core/script/src/tests.rs`), and `an_unknown_option_is_still_refused`;
  - `a_song_without_muted_has_the_same_ir_hash` (ID-11).
- **Delta:** `muted_is_not_structural` (`tx/delta_tests.rs`), and `a_mute_in_song_toggle_is_a_cell_write` (server: no
  `Activate` is sent, and the cell's stamp is applied).
- **Runtime** (`core/runtime/tests/overlay.rs`):
  - `an_overlay_at_unity_changes_no_bit` (every pure fixture, against `renders.json`);
  - `no_overlay_on_a_sidechain_input` (the `follow` fixture's two consumers of `kick.out`);
  - `an_overlay_glides_over_ten_milliseconds`, and `muted_in_song_silences_an_offline_render`;
  - `overlays_cost_no_allocation` (`no_alloc.rs` with cell writes to overlay cells while playing);
  - `overlays_are_the_same_on_1_and_n_threads` (8a's 1-vs-N harness, with overlays in the plan).
- **The gate** (`core/session/tests/mute_sidechain.rs`), for each fixture (`env`, `follow`, `keyed`):
  - `muting_the_kick_keeps_the_bass_ducked`;
  - `a_monitoring_mute_live_equals_mute_in_song_offline`;
  - the negative control `without_the_sidechain_the_bass_changes`.
- **The gate** (`core/play/tests/mute_click.rs`): `toggling_mute_makes_no_step_larger_than_twice_the_sines`, at 440 Hz
  (the gate) and at 50 Hz, with device buffers of 128, 37 and 512 frames, toggling mid-block.
- **The gate and the CI gate** (`crates/server/src/graph/tests.rs`): `an_export_is_byte_identical_with_overlays_active`,
  which also checks that the export's result carries `overlays.active`.
- **Server:**
  - `overlays_persist_in_music2`, and `a_read_only_window_keeps_overlays_in_memory` (MON-4);
  - `solo_keeps_a_bus_fed_and_its_sources` (a bus fixture: vocal and backing into a bus track), and
    `mute_wins_over_solo`;
  - `the_song_module_cant_be_muted` (MON-1);
  - `a_new_plan_takes_the_overlays_before_it_is_sent`;
  - `a_rekeyed_track_keeps_its_mute`;
  - `a_track_muted_in_song_stays_silent_when_soloed`, and `a_shared_strip_mutes_every_track_in_it`.
- **Checker:** `the_songs_output_reading_a_track_is_a_site` (`song(() => lead.out)`: muting Lead silences it), added to
  `no_overlay_on_a_sidechain_input`'s fixtures.
- **Builder:** `mute_in_song_inserts_and_removes_the_option`, and `several_tracks_mute_in_one_tx`.

### 6.10 Browser suite: `e2e/s8-mute.mjs` (kind `fix`, about 55 checks *estimate*)

- **M on Kick:** the pill reads "1 muted"; Kick's clips draw at 40%; `.music2/overlays.json` says `["kick"]`; no
  project file changed and no revision was made.
- **Solo:** S on Bass mutes the rest in `heard`, and the Kick strip shows "Still drives Duck on Bass". ⌥-click on S
  solos one track exclusively; dragging across three M buttons mutes all three; the popover's Clear all clears them,
  and the pill has no ×.
- **Keys:** M and S act on the selected track; the M key skips a track muted in the song.
- **Reopening** keeps the mutes.
- **Mute in song:** from the header menu, it writes `{ name: "Kick", muted: true }` in `kick.ts`; the name is struck
  through; S on it shows its tooltip and plays nothing; undo removes it.
- **Export:** with Kick muted, the export dialog appears; Mute Kick in the song, then export writes the option and
  exports, the WAV equals `m2 render`'s, and the done toast's Unmute Kick removes the option. Export anyway gives the
  same WAV, and the toast names the warning. With "Don't ask again" ticked, the next export shows no dialog.
- **Read-only:** M works in a read-only window and nothing is written.

### 6.11 Contract edits

- **MON-1:** the sites are `Flat.overlays`, the song's output included when it reads a track directly; the op is
  built-in, in every plan, offline renders included; it is ×1.0 when heard; there is one cell per site, mapped from its
  module's key.
- **MON-2:** the strip row.
- **MON-3:** the path rule up and down the module graph over non-sidechain edges; mute, mute in song included, wins over
  solo.
- **MON-4:** the file's format; a deleted track's entries leave with it and come back with its undo; read-only windows
  keep overlays in memory; the export dialog.
- **MON-5's Today:** built.
- **RENDER-22a:** overlay cells glide 10 ms in the op; there is no host ramp for them.
- **RENDER-23's Today:** built.
- **PKG-13:** the checker records the sidechain inputs (`Flat.side_inputs`).
- **ID-11's Today:** built.
- **WRITE-8:** "a module's `muted` → its overlay cell"; `Muted` isn't structural.
- **gaps.md #4.**

### 6.12 Stays green because

- At unity the overlay is the identity, so `renders.json` holds.
- The IR's JSON is unchanged when nothing is muted.
- `same_layout` compares `Flat`'s nodes, and overlays aren't nodes there.
- Step 7's `continues` joins `Flat` nodes, so overlay nodes (stateless and plan-side) never enter the match.
- 8a's 1-vs-N tests run with overlays.

---
## 7. 8b.5 Clips by id: the timeline as an editor

**Why fifth:** "You build a pop song by copying sections, and here you can't" (audit, ranks 3 and 4; gaps #9, #14 and
#17). Today clips only move along their own lane in whole beats (`g05`), and automation clips ignore clicks (D11). It
needs no engine work: notes and control clips take `SwapLane` today, and audio clips take a new renderer until step 7.

### 7.1 One gesture type with two uses (`crates/editor/src/clips.rs`)

```rust
pub struct ClipRef { pub lane: LaneAddr, pub id: Option<ClipId>, pub at: Option<(f64, u32)> }  // at: (start, nth) for an anonymous clip
pub enum Gesture {
    Create    { lane: LaneAddr, start: f64, length: f64 },
    Move      { clips: Vec<ClipRef>, by: f64, to: Option<LaneAddr>, copy: bool },   // a drag; ⌥-drag copies
    Resize    { clip: ClipRef, start: f64, length: f64 },                          // either edge
    Split     { clips: Vec<ClipRef>, at: f64 },
    Join      { clips: Vec<ClipRef> },                                             // notes clips of one lane
    Duplicate { clips: Vec<ClipRef>, range: Option<(f64, f64)> },                  // range: a marquee's, bar-snapped
    Paste     { clips: Vec<Copied>, lane: LaneAddr, at: f64 },
    Delete    { clips: Vec<ClipRef> },
    Notes     { clip: ClipRef, notes: Vec<Note>, length: Option<f64> },             // the piano roll's edit of one clip
    Points    { clip: ClipRef, points: Vec<(f64, f64)> },                          // automation, drawn in place
    Quantize  { clips: Vec<ClipRef>, notes: Option<Vec<u32>>, grid: f64, ends: bool },
    Set       { lane: LaneAddr, put: Vec<Clip>, delete: Vec<ClipId> },             // the generic form, for agents
}
/// What the gesture makes of the lanes it touches. Pure: the edit and the lane lease (§9.1) both call it.
pub fn apply(data: &DataSet, arr: &Arrangement, g: &Gesture, mint: &mut dyn FnMut() -> ClipId) -> Result<Vec<LaneEdit>, EditError>;
/// One `Clips` Tx per gesture.
pub fn edit(s: &mut Session, g: &Gesture, seen: &Seen) -> Result<Edited, EditError>;
```

**The gesture is the request,** so what it means is decided once, in Rust (WRITE-9). The app, `m2 clips` and agents get
the same meaning, and the lane lease previews exactly what the commit will write. While dragging, the web draws a ghost;
on release it sends the gesture with the snapped numbers it shows.

**`seen` closes WRITE-12's exception.**
- Each clip in the timeline JSON gains `sha`: the first 16 hex digits of the SHA-256 of its canonical line.
- A gesture sends the `sha` of every clip it touches. The builder plans on the accepted data and compares those hashes
  first.
- A clip that changed since the window drew it is `EditError::Changed`, naming the lane and the clip: "The Bass clip at
  bar 9 was changed in another window or by an agent. It shows as it is now; try again."
- The piano roll's whole-lane write, planned at a revision on the session's remembered snapshot (`/api/graph/lane`,
  `write_notes`), is deleted in this slice. The roll sends `Notes` for one clip.

**The Tx:**
- one `Clips { lane, put, delete }` for each lane touched (WRITE-3's "at most one per lane"; a move across lanes has two);
- based on the accepted text of each lane file (`WithText`), or a `Write` with base `Absent` when the lane has no file
  yet;
- `Expect::Confined { lanes }`.

A data-only Tx skips QuickJS (PLANE-1), and an outside edit to another clip of the same lane merges (WRITE-3's rule).
The planner's assertion: the candidate's lanes hold every put clip byte for byte, no deleted id, and every other clip as
it was.

**Rules every gesture keeps:**

| Rule | How |
|---|---|
| ids | A moved clip keeps its id, unless the target lane has it, and then it gets a fresh one (FMT-12). A copy, a paste, a duplicate, a split's second half and a new clip always get a fresh random id (FMT-11). |
| anonymous clips | A gesture names one by `at`, its start and its rank there, within the accepted revision. The builder gives every anonymous clip of each lane it writes an id, as `notes::tidy` does today, so `clip.anonymous` clears. |
| kinds | Notes, control and audio clips never cross kinds ("A notes clip can't go on an automation lane"). Control clips cross only between lanes of the same unit ("Cutoff (Hz) and Drive (dB) can't share clips"). Audio clips cross between tracks. |
| create | A notes clip is empty and one bar long at the meter there. A control clip is one bar long with points `[[0, v], [len, v]]`, where `v` is the lane's value at that beat (its fallback, or the clip there). An audio lane refuses until import exists (gaps #18). |
| resize, right edge | `length`, at least 1/64 of a beat |
| resize, left edge | `start` and `length` change, and every note's `start` (or point's beat) shifts by the opposite amount. Notes keep their place in time. Notes now before the clip's start stay in the file and are silent (playback cuts at the clip, and the lane format allows a negative start), so dragging the edge back brings them back: the trim isn't destructive. An audio clip's `offset` grows by the seconds the edge moved, under the tempo map, and the edge refuses to go past the asset's start. |
| split at `b` | The first clip keeps its id and ends at `b`. A note that starts before `b` stays in it, shortened to end at `b`, as Live does. A note at or after `b` goes to the second clip (fresh id, starting at `b`). A control clip's value at `b` is interpolated, and it becomes the first clip's last point and the second's first, so the curve stays continuous. An audio clip's second half gets `offset + seconds(b − start)`; the first keeps its fade in, the second its fade out. |
| join | Two or more notes clips of one lane become one, from the earliest start to the latest end, keeping the earliest clip's id. Every note keeps its place in time (its `start` shifted into the joined clip), and gaps stay empty. Notes trimmed outside a clip's edges stay outside and silent. Control clips refuse ("Automation clips can't be joined": the fallback would stop playing in their gaps), and audio clips refuse until consolidating renders (gaps #18). It is the inverse of split, Logic's and Live's ⌘J. |
| duplicate | Copies land right after the selection, shifted by its span rounded up: to whole bars, in the meter at the span's start, when the span is a bar or longer; to the grid when it is shorter (exactly, with snap off). A marquee selection (§7.4) uses the marquee's own bar-snapped range as its span, so empty bars inside it are kept. So an 8-bar verse whose last clips end on beat 4 of bar 8, or a fill that crosses into bar 9, still lands on bar 9 or bar 10, never on a beat. |
| paste | Clips keep their offsets from the earliest copied clip and land at `at` (the playhead, snapped). They go on the selected track's lane of their kind (for control clips, of their unit), else on the lanes they came from. Clips from several kinds are pasted only onto their own lanes. |
| quantize | Note starts move to the nearest grid line (`grid` in beats: 1/4 for sixteenths; triplets are ⅔ of that), and ends too when `ends` is set. Strength is 100%. Swing and humanize stay in gaps #15. |

**CLI:** `m2 clips split bass/notes:k3f9q2zt --at 9.3.1`, and `m2 clips duplicate|join|move|delete|quantize …`.
Positions are written as bar.beat.sixteenth by the song's meter, or as beats with `--beats`.

### 7.2 Automation drawn in place (TL-6, D11, gaps #14)

- **Control lanes edit in the timeline.** A point is hit only within 6 px of the curve, so a clip can still be
  selected, moved and copied like a notes clip:
  - a double-click on empty lane space makes a clip;
  - a click within 6 px of the curve adds a point there, and a drag from there moves it (time snapped, value free, ⇧
    for fine); dragging an existing point moves it;
  - anywhere else on the clip's body, a click selects the clip and a drag moves it (⌥-drag copies), for ⌘D, ⌘C and ⌘E;
  - ⌫ deletes the selected points, or the selected clips when no point is selected;
  - in Draw mode (B), dragging paints one point per grid step anywhere on the lane.
  - Each point edit is one `Points` gesture; clip edits are the gestures of §7.1.
- **The value range** comes from the lane's unit (`TimelineLane::Control.min/max`, as drawn today).
- **Curves** (taste TL-6's "⌥-drag bends a segment") need a curve field in the lane format, which is a format addition
  (FMT-2 and FMT-3a). So they are left out of 8b (§15).
- **Automate's lane names** (D6, TL-8):
  - the key comes from the control's slot (`decay`), with the device's label in front only on a clash (`pluck-decay`);
  - the code writes `{ name: "Decay" }` only when the key isn't the control's label in sentence case;
  - the toast says "Automating Kick decay in a new lane";
  - this changes `name_of` in `automate.rs`, which names by the binding today (`out-decay`, `c07`).
- **A knob that a lane drives** can't be dragged until override latches exist (MON-6, §15): HOME-2 forbids a drag from
  editing the fallback. Once automation is easy to draw, this comes up in nearly every session, so 8b.5 gives the knob
  a way out instead of a dead control:
  - a drag shows the value bubble "Follows the Cutoff lane" and changes nothing;
  - its menu (KN-2) offers **Show Cutoff lane**, which unfolds the track and selects the lane, and **Remove
    automation**;
  - **Remove automation** is `automate::remove(s, at, knob)`, the inverse of Automate: one `Confined` Tx that replaces
    `lane.control(key, unit, …)` with a literal of the lane's value at the playhead, in the unit's spelling (what is
    heard now stays), and removes the lane file. The track function's `lane` parameter stays. Undo brings back both.
    Its toast is "Removed the Cutoff automation".

### 7.3 Grid, snap, zoom, heights (TL-11, taste §3.2, §5.1)

- **Grid:**
  - Auto, the finest of bar, beat, 1/8, 1/16 and 1/32 whose lines are at least 12 px apart, is the default;
  - Bar, 1/2, 1/4, 1/8, 1/16, 1/32 and 1/64 are fixed choices;
  - Triplet (⌘3) makes each choice ⅔ as wide; Snap (⌘4) turns snapping off and on; holding ⌘ while dragging skips the
    snap; ⌘1 and ⌘2 make the grid narrower and wider;
  - the timeline and the roll each have their own setting, as in Live, kept in `view.json`. ⌘1 to ⌘4 act on the one
    that last had the pointer or the keyboard focus.
- **Zoom:**
  - from 2 to 192 px per beat, 24 by default;
  - ⌘= and ⌘− step ×1.5 around the playhead, and a pinch zooms around the pointer;
  - Z zooms to the selection (or the whole song), and a second Z goes back;
  - ruler numbers thin to every 2, 4 or 8 bars when bars are narrower than 40 px.
- **Track heights:** 28, 44 or 72 px, with ⌘↑ and ⌘↓ on the selected track or on all of them. A folds automation lanes.
  Both are kept in `view.json`.
- **Follow playhead** (⇧⌘F, View menu), on by default and kept in `view.json`: while playing, the view pages when the
  playhead passes 85% of its width. It pauses while the producer scrolls.
- **The End flag** (TL-12): an "End" flag in the ruler at the song's end, with the lanes after it in `chrome`.

### 7.4 Selection and keys

- **Selecting:**
  - a click selects a clip, ⇧-click adds one, and ⌘-click toggles one;
  - dragging on empty lane space draws a marquee across tracks (a double-click still makes a clip). Its range snaps to
    bars, and ⌘D duplicates that range (§7.1);
  - Esc clears the selection.
- **Nudging:** ← and → move by the grid, ⇧← and ⇧→ by a bar.
- **Editing:** ⌫ deletes, ⌘D duplicates, ⌘E splits at the playhead (the selected clips, else every clip under the
  playhead on the selected track), ⌘J joins the selected notes clips of one lane, ⌘C, ⌘X and ⌘V copy, cut and paste,
  and Q quantizes every note of the selected clips.
- **Opening:** a double-click on a notes clip opens it in Notes (taste §3.7).
- ⌘E is free because the T slice moves Export to ⇧⌘E (taste TB-9), so this slice comes after T.

### 7.5 UI (TL-5, TL-6, TL-15, taste §2.8, §2.9, KN-3)

- **Clip looks and states** follow taste §2.8 and §2.9:
  - a selected clip has a 2 px `fg-strong` outline, with its body at `bar` 35% over `tint`;
  - a dragged clip gets `shadow-lift` at 90% opacity.
- **Edges:** a 6 px hit area at each edge, with an `ew-resize` cursor.
- **While dragging:**
  - a value bubble (taste §2.14) at the pointer shows the snapped position ("9.3.1"), and the target track's name when
    the drag crosses tracks;
  - a 1 px dashed `fg-faint` guide crosses the lanes at the new start;
  - a lane that can't take the clip isn't highlighted, and the bubble says why ("Notes clips only").
- **Automation clips** look as taste §2.8 gives them:
  - the curve is a 1.5 px `ink` line with a 10% fill;
  - points are 6 px circles, and a selected point is `fg-strong` with a 2 px white ring;
  - the fallback is a dashed `line-strong` line drawn only between clips.
- **Menus** (KN-3):
  - on a clip: Cut ⌘X, Copy ⌘C, Paste ⌘V, Duplicate ⌘D, Split at playhead ⌘E, Join ⌘J (several notes clips), Quantize
    Q, Delete ⌫, —, Loop this clip ⌘L, and Open in Notes (for notes clips);
  - on a lane: Add clip, Paste, Clear lane…, Hide lane (for automation lanes);
  - on the ruler: Add marker, Add tempo change, Add time signature change, Loop to next marker.
- **The grid chip** sits in the track-header corner between + Track and the fold-all chevron. It shows `Magnet` (`fg`
  when snap is on, `fg-off` when off) and the grid ("Auto" or "1/16") in `text-xs`. A click opens the grid menu (taste
  §2.14).
- **The Notes header** keeps taste §3.5's tools, with the grid going down to 1/64 and Triplet (NT-7's snap part).

### 7.6 Markers

- **Where they live:** in `layout.json` (§2.1), since they are presentation: no sound depends on them. Each change is a
  layout Tx, undoable, with no revision.
- **The look:** each marker is a 1 px `fg-faint` tick on the ruler's top half, with its name in `text-xs` `fg-muted`,
  cut where the next marker starts.
- **Adding one:** the ruler's menu adds one at the clicked bar line, and Transport ▸ Add Marker adds one at the playhead.
  The name starts as "Marker 1" and opens in rename.
- **Using one:**
  - a click seeks to it; a double-click renames it; a drag moves it (snapped);
  - its menu has Rename, Delete and Loop to next marker;
  - Previous and Next Marker are in the Transport menu, on ⌥← and ⌥→ (check them against Logic's key commands, as taste
    §5 asks).
- **Sections later:** sections (gaps #23: copy a section across every track, insert bars) can start from markers later.
  Whether markers stay in `layout.json` then is decided with sections.

### 7.7 Tests

**`crates/editor/src/clips_tests.rs`**, on notes, control and audio lanes (`examples/resample` has an audio lane):
- **ids:** `a_move_keeps_its_id`, `a_move_onto_a_taken_id_mints_one` and `every_copy_mints` (FMT-12);
- **trims:** `a_left_trim_keeps_notes_in_place` and `trimming_back_brings_the_notes_back`;
- **splits:**
  - `a_split_shortens_the_crossing_notes`, and `a_split_control_clip_stays_continuous`;
  - `a_split_audio_clip_plays_the_same_samples`: the lane renders the same bits before and after;
- **placing:** `a_duplicate_lands_after_the_selection`, `a_duplicated_section_lands_on_the_next_bar` (a verse ending on
  beat 4 of bar 8 lands at bar 9), `a_marquee_duplicate_keeps_its_empty_bars`, and `a_paste_mints_and_keeps_offsets`;
- **joins:** `join_makes_one_clip_and_keeps_every_note_in_time`, and `control_and_audio_clips_dont_join`;
- **quantize:** `quantize_to_sixteenths_and_triplets`;
- **refusals and ids:** `kinds_and_units_never_mix`, and `touching_a_lane_gives_its_anonymous_clips_ids`;
- **conflicts:**
  - `a_gesture_on_a_changed_clip_is_changed` (`seen`, WRITE-12);
  - `an_outside_edit_to_another_clip_merges` (WRITE-3);
  - `undoing_after_an_outside_change_to_the_clip_conflicts` (WRITE-21, which is built, now through gestures).

**Elsewhere:**
- **`automate_tests.rs`:** `the_lane_is_named_after_the_control` (D6), `remove_automation_puts_the_heard_value_back`
  (and removes the lane file; undo restores both), and `a_lane_driven_knob_refuses_a_drag_with_words` (HOME-2).
- **`crates/server/src/graph/tests.rs`:**
  - `a_clip_gesture_takes_the_data_path`: no QuickJS run, and a `SwapLane`;
  - `the_roll_writes_one_clip`;
  - `the_old_lane_route_is_gone`.
- **`apps/m2/tests/clips.rs`:** each gesture, and the lock refusal.

### 7.8 Browser suites

**`e2e/s8-clips.mjs`** (kind `fix`, about 75 checks *estimate*):
- resizing either edge, with the notes kept where they were;
- an ⌥-drag copy gets a new id;
- a vertical drag from Kick's hits to Hats' hits works, and to Keys' chords works too (both notes lanes), but onto an
  automation lane is refused;
- ⌘D, ⌘E, ⌘J, ⌘C and ⌘V at the playhead, and Q;
- a marquee over bars 1 to 8 whose last clip ends on beat 4 of bar 8: ⌘D lands the copy at bar 9;
- the marquee, nudging, zoom keys and Z, track heights, the grid chip, and ⌘ skipping the snap;
- ⌘1 changes the roll's grid while the pointer is over the roll, and the timeline's otherwise;
- the End flag, and adding, renaming, moving and deleting a marker, with undo;
- no clip gesture shows a toast;
- every edit is one `Clips` Tx, the lane file stays canonical, and nothing else changes on disk.

**`e2e/s8-automation.mjs`** (kind `fix`, about 40 checks *estimate*):
- Automate on Kick's decay makes a lane labelled "Decay" with the key `decay`;
- a double-click there makes a clip; a click on the curve adds a point; a drag moves it; ⌫ deletes it; B paints;
- a click on the clip's body away from the curve selects the clip and adds no point; a drag there moves the clip;
- the lane's value at the playhead shows in its header;
- dragging the Decay knob shows "Follows the Decay lane" and writes nothing; Remove automation puts the literal back
  and removes the lane;
- undo restores the file.

### 7.9 Contract edits and green

- **Edits:**
  - WRITE-10: rows for the clip gestures (`Confined { lanes }`), removing automation, and markers (layout only).
  - HOME-2's Today: Remove automation is built; a drag on a lane-driven knob says why it doesn't move until latches.
  - WRITE-12: the notes write's exception is gone; clip gestures carry each touched clip's hash.
  - FMT-12: "a builder gives ids to the anonymous clips of each lane it writes".
  - FMT-19: markers.
  - gaps.md #9, #14 and #17.
- **Green:**
  - No render changes.
  - The `notes`, `s4-clips`, `layout` and `fixes-notes` suites send the new gestures in place of `/api/graph/lane`, and
    are updated in this PR.
  - `s4-problems`' read-only roll on a lane file that doesn't read still holds: a gesture on a lane whose file failed
    is refused, as the notes write was.

---

## 8. 8b.6 Audition: hear notes as you draw them

**Why sixth:** "You can't write a melody you can't hear" (audit rank 5, gaps #8). It needs the engine-wide note ids of
step 7 slice 1 (design-7 §8.1). Live notes survive an edit only once schedulers persist, in step 7 slice 2.

### 8.1 The engine (`core/runtime`, `core/play`)

- **The message:** `Msg::Live { generation: u64, slot: u32, ev: LiveEv }`, with
  `enum LiveEv { On { handle: u32, pitch: f32, velocity: f32 }, Off { handle: u32 }, Release { source: u32 } }`.
  - It goes on the structural queue (bounded and never dropped, RENDER-28) and is read at the block line.
  - A message for another plan's generation is refused and counted, as `SwapLane` is.
  - `Player::live(generation, slot, ev)` sends it.
- **Sources.** Each live note belongs to a source, numbered by the server: a window today. A MIDI keyboard (owner
  question Q3) would be a second kind of source on the same path, so the engine and the table don't change for it.
- **The live table:** each notes lane's scheduler holds `live: [LiveNote; 16]`, entries of `{handle, source, id,
  pitch}`, sized at prepare. Its event buffer has room for its caps plus 16 more Ons and Offs.
- **Merging:** at the block line, after the transport's jump if there is one, the scheduler writes live Offs, then live
  Ons, at frame 0 of the lane's buffer, before the transport's notes.
  - Live ids come from `Renderer.next_note`, so they never meet a scheduled note's id in any consumer (a plugin matches
    Offs by id).
  - A full table refuses an On and counts it (`dropped_ons`). An Off always fits.
- **What ends a live note:**
  - its Off;
  - `Release` (all of a source's notes);
  - Panic;
  - a consumer that an activation replaces, or a lane it removes (design-7's ghost ends every note sounding into it at
    entry, live ones included).
  - **Not** Stop, Seek or a loop wrap: a key held by a hand keeps sounding, as a held MIDI key does in other DAWs.
    RENDER-9's and RENDER-26a's "every sounding note" means scheduled notes (§14).
- **While stopped,** the engine processes (RENDER-6a), and any message wakes an idle engine.
- **Offline renders** never get `Live` (RENDER-3).

### 8.2 The server (control plane)

- **The window sends** WebSocket messages:
  - `{"t":"audition","module":"bass","lane":"notes","on":[{"h":7,"pitch":45,"velocity":0.8}],"off":[6]}`;
  - a keepalive `{"t":"audition","keep":[7]}` every 2 s while a note is held, as knob leases send theirs.
  - The window chooses handles, and the server scopes them by the window's source.
- **The server resolves** `(module, lane)` to a slot in the `PlanInfo` of the plan it last sent, under the control
  plane's lock, and sends `Live`. The queue is FIFO behind the actor's activations, so the plan it names is the one that
  will be playing when the engine reads it (as knob leases aim, WRITE-22). It never takes the session (RENDER-29), and it
  works in a read-only window.
- **A lane nothing reads** is silent, and the server answers once with `{"t":"audition","silent":"Nothing plays this
  lane"}`, which the roll shows.
- **A watchdog** for a window source: after 10 s without a keepalive, or when the window's WebSocket closes, the server
  sends `Release`. A device source would be released when the device disconnects, with no keepalive.

### 8.3 Hearing an instrument before keeping it

Revision 1's instrument previews (a scratch project per component, a cache, and a WAV played through WebKit's own output
beside the engine's device) are cut (§15). Audition is the way to hear an instrument: ⌘T, Enter, a click on a key in
Notes, and ⌘Z if it isn't the one. Previews can come back with presets and packages (gaps #26, #39), routed through the
engine.

### 8.4 UI (NT-2, NT-3, taste §3.5 Notes, §3.6)

- **The Notes header** gets the Audition toggle (`Volume2`; `accent-soft` while on). It is on by default and kept in
  `view.json`.
- **What sounds:**
  - **a note:** a note-down (drawing or selecting one) sounds while the pointer is held;
  - **a pitch drag:** while dragging a note to a new pitch, the old pitch gets an Off and the new one an On, so the drag
    plays each pitch it crosses;
  - **painting:** in Draw mode, each painted note sounds for its length or 250 ms, whichever is shorter;
  - **a key:** a key on the 56 px keyboard sounds while held, and draws in `bar` while it sounds. Velocity follows where
    on the key it was clicked, quiet at the left edge and loud at the right, as Logic's keyboard does. Notes use their
    own velocity.
- **Hints** in the header, in `text-xs` `fg-muted`: "Muted · audition is silent", "Muted in the song · audition is
  silent", "Silenced by the Bass solo · audition is silent" (§6.7), "Nothing plays this lane".
- **Panic,** ⌘., is 8b.2's.

### 8.5 Tests

- **`core/runtime/tests/live.rs`:**
  - `a_live_note_sounds_and_ends`;
  - `live_ids_never_meet_scheduled_ids` (a probe plugin sees no repeated id);
  - `a_live_note_survives_stop_seek_and_wrap`, and `a_replaced_consumer_ends_its_live_notes`;
  - `a_full_table_refuses_and_counts`;
  - `release_ends_one_sources_notes`, and `panic_ends_live_notes`.
- **`core/play/tests/no_alloc.rs`:** gains Live messages while playing and stopped. `bit_identity` is untouched.
- **Server:**
  - `audition_never_waits_for_a_compile`: during a 1.5 s compile, an On is heard before the compile ends, as
    `previews_never_wait_for_a_compile` checks for knobs;
  - `a_closed_window_releases_its_notes`, and `the_watchdog_releases_a_quiet_window`.
- **Gates:** `m2-gates --server audition` (G13): from the WebSocket message to the first block that plays the note, p50
  and p95, on p16 while playing, recorded.

### 8.6 Browser suite: `e2e/s8-audition.mjs` (kind `fix`, about 30 checks *estimate*)

`M2_AUDIO=none` makes no sound, so the suite reads `GET /api/graph/status`. It gains `live`, the number of live notes
sounding, which costs one atomic.

- a note-down gives `live` 1, and releasing it gives 0;
- dragging across three pitches sends three Ons and three Offs, counted from the page's WebSocket frames;
- a key held on the keyboard sounds;
- with the toggle off, nothing is sent;
- closing the page releases every note within 1 s;
- a soloed Bass gives the Keys roll the "Silenced by the Bass solo" hint;
- the panic route works.

### 8.7 Contract edits

- **MON-7:** what a live note is, its source, where it merges, its ids, its room, and what ends it.
- **RENDER-14:** live notes merge at the block line, at frame 0, Offs first.
- **RENDER-28 and the engine shape:** `Live` in the message list.
- **RENDER-9 and RENDER-26a:** jumps end scheduled notes; live notes end only on release, panic or a replaced consumer.
- **gaps.md #8.**

---

## 9. 8b.7 Leases: heard while dragging

**Why seventh:** WRITE-22a and RENDER-27a. Once clip editing (8b.5) and the tempo field (8b.2) exist, a drag while
playing should be heard where it is. Today a note edit is heard only once its Tx is accepted (survey §6.3). This slice
waits for step 7 slice 2: the leases need `PlanInfo.lanes` and the identity `Activate`.

### 9.1 Lane leases (WRITE-22a)

- **`begin {session, revision, gesture, seen}`** resolves on the control plane against the revision the session last
  published, as knob leases do (WRITE-22), and returns `{lease, lanes}`.
  - Notes and control gestures (`Move`, `Resize`, `Notes`, `Points`, `Create`) can be previewed.
  - A gesture on audio lanes answers `previewable: false`: it needs renders, so it is applied on release (design-7 §8.2
    keeps audio swaps inside `Activate`).
- **`update {lease, seq, gesture}`:**
  1. `clips::apply` on the published data, with the gesture (§7.1);
  2. for each lane touched, its engine data: notes data in the tempo map, or control data;
  3. if it fits the scheduler's caps (`PlanInfo.lanes[slot].caps`), `Player::swap_lane(generation, slot, data)` from the
     control plane; if not, nothing until the commit, which grows the lane with an `Activate` (design-7 §8.1).
  - Out-of-order updates are dropped, and the web sends at most one per animation frame.
  - RENDER-26a's swap rules end the notes the preview dropped and reschedule moved ends. A note moved to start before
    the transport isn't chased.
- **`commit {lease, seq, gesture}`** has the actor run `clips::edit`: one `Clips` Tx. With C20 option 2, the accepted
  data's `SwapLane` goes once the Tx is checked. It equals the preview, so nothing heard changes.
- **`cancel`, an idle TTL (10 s, renewed by a 2 s keepalive), or the window closing** swaps the accepted data back.
- **An outside write during the lease:** when a settle accepts new data for a leased lane, the actor swaps in
  `apply(the new data, the gesture)` instead of the bare data. If a clip the gesture touches changed (its `seen` hash),
  the lease ends instead: the accepted data is swapped in, and the toast says "Bass changed elsewhere: your drag was
  dropped".
- **A new plan while the lease lives** (a code edit elsewhere): the actor gives `prepare` the preview's lane data in
  place of the accepted data.

Both of the last two read the live lane leases under the control plane's lock, as a knob lease's value is read when a
renderer is built.

### 9.2 The tempo lease (RENDER-27a)

- **The calls:** `begin {session, entry}` (the tempo entry in effect at the playhead), `update {lease, seq, bpm}`,
  `commit {lease, seq, bpm}` and `cancel`.
- **An update** makes the arrangement with that entry at `bpm`, and prepares an identity `Activate` (design-7 §8.4)
  against the plan last sent:
  - every notes lane gets its data in the new tempo map;
  - control lanes are in beats, so they move unchanged;
  - the transport keeps its beat, and plugins get the new tempo in their transport;
  - **audio lanes keep their renders until the commit, and never skip.** Re-rendering every clip at every step is what
    RENDER-27a exists to avoid, so the renders stay. But the transport's frame is recomputed in the new map, and
    the renders place each clip at `seconds_at(start) × rate` in the old map (`AudioLane::build`,
    `core/runtime/src/audio.rs:319`), while `AudioLane::fill` reads at the transport's frame. Read there, `examples/resample`
    at 1:00 would jump about 0.5 s for every BPM of the drag, up to 60 times a second.
    So each audio player (design-7 §8.2) gains a **lease offset**, `offset: i64`, 0 unless a tempo lease is live, and
    reads its renders at the transport's frame plus it:
    - at the block line where a tempo-lease `Activate` lands, `offset` becomes the player's read position just before
      the line minus the transport's new frame. The read position doesn't move, so the old render plays on at its own
      speed and only drifts off the grid: it lags, as RENDER-27a allows, and never jumps. No crossfade is needed, since
      the material and its position are the same;
    - a jump during the lease (a seek, a loop wrap) sets `offset` to the jump's beat in the renders' tempo map minus the
      transport's frame, so the wrap lands on its beat in the old render, declicked as any jump is. The leased plan
      carries the accepted `TempoMap` for that;
    - commit and cancel bring `offset` back to 0 through design-7 §8.2's swap: `prev` is the render read at its offset
      (`shift` = the offset), `cur` the new renders at the commit or the accepted ones at a cancel, crossfaded over
      5 ms.
- **One prepare in flight.**
  - An update that arrives during a prepare replaces any value still waiting, and the newest value is prepared next.
  - Prepares run on a control-plane thread, outside the session lock.
  - A prepared plan is sent only if, under the control plane's lock, the plan it was prepared against is still the last
    one sent. Otherwise it is prepared again. So activations are still never coalesced, and each names the plan it was
    prepared against (design-7 §7.5).
- **While the lease lives:**
  - the server's `Timing` (seek, the loop check and the status beat) uses the lease's map;
  - a code or data revision accepted during the drag is prepared by the actor with the lease's map.
- **Commit** is one Tx that writes `data/arrangement.json` (8b.2's builder). Its arrangement change is an identity
  `Activate`, which re-renders the audio lanes in the new map (design-7 §8.4) and crossfades each from its leased render
  at its offset.
- **Cancel** is an identity `Activate` back to the accepted map, with the same crossfade back to offset 0.

### 9.3 UI

- **Every timeline and roll drag made while playing** begins a lane lease. While stopped, there is nothing to hear, and
  the gesture commits on release as in 8b.5.
- **The tempo field's drag** is a lease whether playing or not, because the time readout follows it. The number turns
  `accent` while the lease lives (taste §3.3).
- **Nothing else changes on screen:** the sound is the feedback.

### 9.4 Tests

**Server, lane leases:**
- `a_lane_lease_previews_with_no_revision`: the driven player hears the moved note at its new beat before release,
  and `data_rev` doesn't move;
- `the_commit_changes_nothing_heard`, and `cancel_puts_the_accepted_notes_back`;
- `an_outside_write_to_another_clip_keeps_the_preview`, and `an_outside_write_to_the_dragged_clip_ends_the_lease`;
- `a_new_plan_carries_the_preview`, and `a_preview_too_big_for_its_room_waits_for_the_commit`;
- `lane_previews_never_wait_for_a_compile`, and `a_note_dragged_away_from_the_playhead_is_ended` (RENDER-26a).

**Server, the tempo lease:**
- `one_tempo_prepare_in_flight`, and `a_stale_tempo_prepare_is_done_again`;
- `the_tempo_is_heard_at_the_first_block_after_its_prepare`;
- `a_tempo_drag_never_skips_an_audio_lane`: `examples/resample` playing from 1:00, 50 updates from 124 to 128 BPM; the
  largest sample-to-sample step of the output is no larger than in the same span rendered with no drag, and a wrap during
  the drag lands on its beat;
- `a_tempo_commit_is_one_tx`, and `a_tempo_cancel_goes_back` (each crossfading the audio back to offset 0).

**Gates:**
- `m2-gates --server lane-drag`, in the shape of G9, on p16 and p50: 200 updates of one note while playing from 1:00,
  measured from each update to the first block that plays it, p50 and p95; then again with 12 outside saves during the
  drag;
- `m2-gates --server tempo-drag` on p50: from each update to the block that plays it, and the prepare's cost.

### 9.5 Measured first

**What to measure:** the cost of preparing a tempo `Activate` at p50, mostly about 50 lanes of notes data
*(estimate: a few ms)*. A lane lease's update costs one lane's notes data, which G11 already measured as part of its
plan (PLANE-5).

**If the tempo prepare costs over 10 ms,** the updates simply arrive less often: one prepare is ever in flight. The
field shows the dragged value as it moves, and only the sound updates less often.

### 9.6 Browser suite: `e2e/s8-leases.mjs` (kind `fix`, about 30 checks *estimate*)

**Clip drags while playing:**
- the page calls `lane/lease/begin`, then one `update` per frame, then `commit`;
- `GET /api/graph/leases` lists the lane lease while it lives;
- no revision is made until release, and then exactly one;
- the lease is cancelled when the drag is abandoned (Esc), and when the page closes.

**The tempo drag:**
- the number is `accent`;
- the status's tempo follows the drag;
- the drag writes `arrangement.json` once, on release.

### 9.7 Contract edits

- **WRITE-22a:**
  - previews evaluate the gesture the commit will write;
  - an outside write to the lane re-applies the gesture, and one to the clip ends the lease;
  - a new plan carries the preview;
  - audio lanes are applied on release.
- **RENDER-27a:**
  - "at the first block after its plan is prepared";
  - one prepare in flight;
  - all audio keeps its renders until the commit: it lags behind the drag, and never skips.
- **The planes table:** the control row names lane and tempo leases.
- **RENDER-29's Today.**

---

## 10. 8b.8 Meters and the DSP load

**Why eighth:** the audit's rank 1 includes meters ("there are no meters anywhere"), and a playing DAW "should visibly
breathe". The faders and mutes are usable by ear first. This slice is the deferred table's trigger "Mixer meters", and
brings render taps in. It needs step 7 slice 2; its DSP readout needs 8a.

### 10.1 Taps (`core/runtime`)

- **In the plan:** `Plan.taps: Vec<Tap { node: u32, slot: u32 }>` and `TapSlots { peaks: Box<[AtomicU32]> }`, with one
  slot per channel of each tap. `prepare` makes them from a list the caller gives (`TapSpec { key, node }`, the node
  named by its `FNode.name`). They are plan-side and never identity, like cells.
- **After a tapped node runs,** inside its partition's task (8a), the task takes each channel's largest `|x|` over the
  block and `fetch_max`es its bits. Non-negative `f32`s order as `u32`s, so no lock and no allocation is needed. At p50's
  roughly 60 taps that is under 2 µs a block *estimate*.
- `PlanInfo.taps: Arc<TapSlots>` names them by key. The song's output is always tapped, as `out`.
- **Taps read buffers only,** so no render changes. Offline renders have none (export already reports `peak_db`), and
  no test needs them there: the ducking gate compares whole outputs (§13.2).

### 10.2 What is tapped (chosen by the server, from §2.3)

| Key | Node | Why |
|---|---|---|
| `<track>` | the channel's gain node: after pan, after the fader | the strip and header meters show what the track sends into the mix |
| `<track>` (no channel) | its first overlay node: after mute | |
| `out` | the song's output, after master | the master meter and clipping |

Revision 1's `<track>:src` taps, for a dot that pulsed beside "Still drives Duck on Bass", are cut with the dot (§15):
the row's words already say what the dot would.

### 10.3 Sending them

- **The meters message:** 30 times a second while the engine runs, the server reads and resets every tap of the plan
  that plays (`swap(0)`) and sends `{"t":"meters","peaks":{"kick":[l,r],…,"out":[l,r]},"dsp":{"load":0.23,"dropouts":0}}`.
  Nothing is sent while the engine idles (RENDER-6a).
- **DSP**, after 8a, comes from 8a's always-on record (`Player::load()`, design-8a §9.1):
  - `load` is the longest callback in the last second, as a share of the device period;
  - `dropouts` counts new late callbacks, gaps and overloads.
  - That puts gate 8a's 70% budget in the app (taste TB-8).

### 10.4 UI (taste §2.8 meters, §3.3, TL-1, MX-1, TB-8, TB-13)

- **Drawing:** canvas peak meters at 60 Hz, never eased (taste §1.8). The fill is `fg-muted` below −6 dBFS, `warn` from
  −6 to 0, and `danger` at 0 dBFS or more.
- **Peak hold:** a 1 px `fg-strong` tick held 1.5 s, then falling at 12 dB/s. A clip dot (4 px, `danger`) stays lit until
  it is clicked. The scale runs from −60 to +6 dBFS.
- **Sizes:** a track header's meter is 3 px wide; a strip's is 2×3 px; Master's is 2×5 px. The top bar's master meter
  is 64×12 px with a clip dot.
- **The DSP pill:** "DSP 23%" in `text-xs` with a 24×4 px bar. It turns `warn` at 70% or more, and shows a `danger` dot
  for 3 s after a dropout.
  - A click opens a popover with the device, its rate and its buffer size, read-only until the Settings window (gaps
    #31) lets them be chosen.

### 10.5 Tests

- **Runtime:** `taps_change_no_bit`, `a_tap_reports_its_blocks_peak`, `taps_never_allocate` (`no_alloc.rs`) and
  `taps_are_the_same_on_1_and_n_threads`.
- **Server:** `meters_are_sent_while_playing_and_stop_when_idle`, and `a_new_plan_moves_the_taps`.

### 10.6 Browser suite: `e2e/s8-meters.mjs` (kind `fix`, about 15 checks *estimate*)

**It needs a player that renders without a device.** The headless example gains `M2_AUDIO=paced`, for tests only: it
runs the engine in real time and throws its output away. Design-8a lists a paced mode as not built for the gate, and this
one is only for the suites.

**Checks:**
- meters messages arrive about 30 times a second while playing, and stop when stopped and silent;
- muting Kick takes its strip's meter to the floor, while the Bass strip's meter keeps moving with its ducking;
- the DSP pill shows a number.

### 10.7 Contract edits

- **The deferred table:** the row "Render taps, meters, track freezing, bounce as a Tx" splits. Render taps and meters
  are built in 8b. Track freezing and bounce as a Tx stay deferred, with the trigger "the resample loop in the UI".
- **gaps.md #20:** the meters are built; loudness for export is still missing.

---

## 11. 8b.9 Track IDs: Change track ID… and Restore

**Why last:** it is gaps #10's remaining part ("Rename a track's key"). Producers rarely see keys (taste: "the track ID,
only in Details and in Code"), and a label rename is built. ID-17 asks for it, and DIAG-6's Restore shares its builder.

**Words.** The contract's word is "key" (ID-1), and the CLI keeps it (`m2 rekey`). Every musician reads "key" as C minor,
and taste plans a top-bar Key field for musical key and scale (gaps #16). So all UI copy calls a module key the track's
**ID**, and "key" in the UI only ever means the musical key.

### 11.1 The builder: `tracks::rekey(s, at, module, key)`

1. **Checks:** `ready(s)`; the new key follows ID-1; it isn't taken (module keys, `song`, `data/<k>/` folders in any
   capitals).
2. **Ops:**
   - a `TextEdit` of the key string in the `track()` call;
   - a `TextEdit` of the mixer channel's key, only when it equals the old track key (ID-18). The label says so: "bass
     → low (and its mixer channel)";
   - a `Rename` of `data/<old>/` to `data/<new>/` on its directory hash, when the folder exists;
   - a `Write` of `layout.json` with the key renamed in `order` and `tracks`.
3. **Two things stay, because neither is identity (P1):** the file `tracks/<old>.ts` keeps its name, and the binding in
   `song.ts` keeps its name.
   - Renaming the file would put another file in every node's delta name. The delta sees `Rekeyed` only when two
     modules' graphs are equal but for the key (`modules()` in `delta.rs`), so it would read the rename as a removed
     and an added module.
   - A file rename can come later, with a delta that maps renamed files.
4. **Expect:** `Confined { the track file: [the key string], song.ts: [the whole mixer call] (when the channel key
   changes), lanes: every lane of the track, under its old key and its new one, modules: [Rekeyed { old, new }] }`.
5. **The planner's assertion:**
   - the delta's modules are exactly `[Rekeyed]`;
   - every lane plays the same clips under the new key;
   - `data.unclaimed` isn't raised.
6. **After the commit,** the server renames the key in `.music2/overlays.json` and in `.music2/view.json` (§6.5, §2.2),
   so the track keeps its mute, its height and its folded lanes, and the selection follows it.

### 11.2 Restore (DIAG-6)

- **What exists:** `data.unclaimed`, on a whole folder with exactly one module that has no data, offers Relink (move the
  folder to the module's key) and Delete, from `core/session`.
- **What 8b adds:** **Restore**, which names the module by the folder's key again. It is built in `crates/editor`
  (`fixes::restore(s, &diag) -> Option<Fix>`), as DIAG-1 says: the rekey builder without the folder's `Rename`, since
  the folder already has that key.
- **Where it shows:** the server adds it to the problem's fixes, and so does every CLI command that prints problems
  (`m2 fix restore …`).
- **Its code** is `restore`, already in DIAG-1's vocabulary. Relink stays the likely fix (DIAG-6), and Restore comes
  second.

### 11.3 Keeping the track's instances (design-7 §3.4)

`continues` never matches nodes across module keys. When the delta from what plays has `Rekeyed { old, new }`, a third
pass in `core/session/src/tx/continues.rs` joins each unclaimed `new/<rest>` with `old/<rest>` (about 20 lines).
- It runs after the path pass and the name pass.
- Keyed plugins are joined by their state path with the module segment mapped.
- So a key rename keeps the track's kernels, voices and plugins, and no tail is cut.
- Before step 7, HEAD's `CarryKey` includes the module key, so the renamed track's plugins are created again and its
  kernels start fresh, as every rebuild does today.

### 11.4 UI

- **Where it is:** not in the track header's or strip's menus, where a beginner would open it to transpose the bass. It
  is in the Code view's file-tree menu on a track's file, "Change track ID…", where IDs are literal code, and in the CLI
  (`m2 rekey`). It opens a dialog (taste §2.14):
  - title: "Change Bass's track ID";
  - a mono field (`data-code`, since the ID is literal code) holding `bass`;
  - help in `text-sm` `fg-muted`: "The track ID names this track in code and files its data (data/bass/). Changing it
    moves both, as one change you can undo.";
  - the field checks as you type, with `key.invalid`'s suggestion, in ID words ("lead vox isn't an ID: IDs are
    lowercase letters, digits and hyphens, such as lead-vox");
  - [Cancel] and [Change ID].
  - The toast: "Bass's ID is low now".
- **In the status popover,** the `data.unclaimed` item gets "Rename the track back" (Restore) beside "Move all to Kick
  (kicks)", which stays the primary button (taste ST-4).

### 11.5 Tests

- **`crates/editor/src/tracks_tests.rs`:**
  - `rekey_moves_code_data_and_layout`, and `the_channel_key_follows_only_when_it_equals` (ID-18);
  - `a_taken_key_is_refused`, and `undoing_a_rekey_puts_every_byte_back`;
  - `restore_is_offered_and_plays_again`, in the shape of `s4-attn`.
- **After step 7:**
  - `continues_keeps_a_rekeyed_tracks_instances` (session);
  - `a_rekeyed_track_keeps_its_tails`: a G7b-style tap on the renamed track through the window.

### 11.6 Browser suite: `e2e/s8-ids.mjs` (kind `attn`, about 25 checks *estimate*)

- Change track ID… in the file tree's menu renames `bass` to `low` in `bass.ts` and the channel, moves `data/bass/` to
  `data/low/`, and keeps the label, the colour, the mute and the track height.
- The track header's menu has no ID item, and no UI string says "key" for an ID.
- Undo puts every byte back.
- An ID renamed by hand shows Restore, which plays again.
- An invalid ID shows its suggestion in the field.

### 11.7 Contract edits

- **ID-17's Today:** key rename (the UI's "Change track ID…", in the Code view) and Restore are built.
- **ID-18:** the label says when the channel's key followed.
- **DIAG-6's Today:** Restore is built.
- **ID-15 and RENDER-25:** a re-keyed track's instances are matched across the two keys.
- **WRITE-10:** a row for the key rename.

---
## 12. What waits for step 7 and 8a

| Slice | Can start before step 7? | Needs from step 7 | Needs from 8a | Until then |
|---|---|---|---|---|
| 8b.1 Tracks | yes | nothing; slice 2 makes an add or a delete keep every other track's tails | — | an add or delete is a new renderer, which cuts every tail |
| 8b.2 Song bar | yes | slice 2 makes a tempo change an identity `Activate` | — | a tempo change rebuilds |
| 8b.3 Faders | yes | nothing | — | a first pan rebuilds |
| 8b.4 Mute and solo | **no** | slice 2: `prepare`, `PlanInfo` and the cell map in `plan.rs` | the partition label for an overlay node (design-8a §3.3), which is already designed | — |
| 8b.5 Clips | yes | slice 2 makes an audio clip edit keep tails | — | an audio clip edit rebuilds; notes and control edits already swap |
| 8b.6 Audition | after slice 1 | slice 1: engine-wide note ids; slice 2: live notes survive an edit | — | an edit while a key is held ends it |
| 8b.7 Leases | **no** | slice 2: `PlanInfo.lanes`, persistent schedulers, the identity `Activate` | — | — |
| 8b.8 Meters | **no** | slice 2: taps made in `prepare` | the load record for the DSP readout (design-8a §9.1); taps written inside tasks | meters without the DSP readout |
| 8b.9 Track IDs | yes (the builder and Restore) | slice 2: the `Rekeyed` pass in `continues` | — | a re-keyed track restarts its instruments |

The two ways to schedule the slices are in §19.

---

## 13. Gate 8's 8b clauses, and how each is proven

| Clause | Proven by | Slice | Where |
|---|---|---|---|
| Each producer feature lands without changing core's write API | `the_write_api_is_step_6s` (§13.1), and `the_write_bypasses_are_closed` (exists) | added in 8b.1; runs in every slice | `core/song/tests/write_api.rs` |
| Muting the kick still ducks the bass, read from `kick.env` | `muting_the_kick_keeps_the_bass_ducked` on `sidechain/env` | 8b.4 | `core/session/tests/mute_sidechain.rs` |
| … from `follow(kick.out)` in `song.ts` | the same test on `sidechain/follow` | 8b.4 | the same |
| … through a compressor keyed from the kick's audio | the same test on `sidechain/keyed`, with std's keyed compressor | 8b.4 | the same |
| Toggling mute while a sine plays makes no step larger than twice the sine's own largest | `toggling_mute_makes_no_step_larger_than_twice_the_sines`, at 440 Hz; our own test adds 50 Hz | 8b.4 | `core/play/tests/mute_click.rs` |
| An export is byte-identical with and without overlays active | `an_export_is_byte_identical_with_overlays_active` | 8b.4 | `crates/server/src/graph/tests.rs` |
| CI: overlays never change an export | that test in CI, with `an_overlay_at_unity_changes_no_bit` | from 8b.4 | CI |

### 13.1 The write API stays

`core/song/tests/write_api.rs` has two parts:
- **An exhaustive `match`,** with no wildcard arm, over every enum of the write API: `Op` (five ops), `Expect` (five
  expectations: `Identical`, `Values`, `Confined`, `Seen` and `Journal`, the last carried only by the FinishTx and
  DiscardTx fixes, `core/song/src/tx.rs:143-154`), `Base`, `Declared`, `ModuleChange`, `Anchor` and `Pos`. A new op or
  expectation fails to compile until someone changes the test, and the contract with it (WRITE-6: "A new kind of
  expectation needs a design review that changes this page").
- **One recorded Tx JSON per op and per expectation,** `Journal` included, taken from WRITE-3's example and step 6's
  tests. Each must parse and serialise again byte for byte, so no field is renamed or dropped.

Every 8b slice runs it unchanged, and each slice's PR description says so.

### 13.2 The ducking test

Offline renders have no taps (§10.1), so the test compares whole outputs, with no probe. For each of the three
fixtures:

1. **Offline renders.** Render **B**: the kick `muted: true`, which uses the same cells as a monitoring mute. Render
   **A′**: the fixture with the kick's mixer channel at the fader's foot, `kick: { in: kick.out, db: -96 }`, so the kick
   is silent in the mix while every other read of it (`kick.env`, `follow(kick.out)`, `key: kick.out`) is untouched.
   - **Assert:** A′ and B are bit-identical. Both make the kick's mix input `x × 0` with the sign of `x`: B through the
     overlay at 0 and then the gain at 1, A′ through the overlay at 1 and then the gain at `db_to_gain(-96)`, which is
     exactly 0. Any difference can only be in the bass, so equality says that muting the kick left the ducking as it
     was.
2. **The negative control.** Render **C**: B with its sidechain input reading silence (`duck(bass.out, 0, …)`, `follow`
   of a silent signal, `key:` a silent signal).
   - **Assert:** C differs from B, so the bass's ducking is really in the output, and the test can fail.
3. **Live.** The driven player plays the fixture from 0, with a monitoring mute on the kick set before play.
   - **Assert:** its output equals B's offline output bit for bit. That is RENDER-33's form, with the same overlay cells.

### 13.3 The click test

- **The song:** the gates' `tone(440)` (§6.8).
- **The run:** the driven player plays it with device buffers of 128, 37 and 512 frames. It toggles the monitoring mute
  every 1,000 frames, so the toggles land mid-block, and plays 10 toggles in each direction.
- **The limit:** the largest `|y[n] − y[n−1]|` over the run must be at most twice the largest step of the same sine
  rendered with no toggle.
- **Our own variant** runs the same at 50 Hz, where a one-block ramp would fail and the 10 ms glide passes (§6.8).

### 13.4 The export test

1. Export first-song with no overlays.
2. Mute the kick and solo the bass (`.music2/overlays.json`), and export again.
3. Mute the kick in the song through the export dialog's other path, and export a third time. That file must differ,
   because it is a different song: the negative control.

**Assert:** the first two WAV files are identical byte for byte. The second export's result carries `overlays.active`,
and the first's doesn't.

---

## 14. Contract edits

Each edit lands in the slice that changes the code, in the same commit, and is listed in that slice's PR (the
contract's own rule).

| Rule | Edit | Slice |
|---|---|---|
| Planes table, control row | "…cells, transport, audition, overlays, knob, lane and tempo leases" | 8b.7 |
| RENDER-9 | Item 1 reads "every note a lane scheduled". A live note (MON-7) keeps sounding through a jump. | 8b.6 |
| RENDER-14 | Live notes merge into the lane's buffer at the block line, at frame 0, Offs first, with ids from the engine-wide counter. | 8b.6 |
| RENDER-22a | "An overlay cell's change glides linearly over 10 ms, in the overlay op; the host doesn't ramp it." Its *Why* adds the 50 Hz arithmetic. | 8b.4 |
| RENDER-23 *Today* | Overlay cells are built, one per site, mapped from the module's key. | 8b.4 |
| RENDER-26a | A live note is ended only by its release, its source going, panic, or an activation that replaces its consumer. | 8b.6 |
| RENDER-27a | "Each update reaches notes, control lanes, time and plugins at the first block after its plan is prepared. The control plane keeps one prepare in flight and sends the newest value, against the plan last sent. Audio keeps its renders until the commit, warped or not, read at an offset that keeps its position: it lags behind the drag, and never skips." | 8b.7 |
| RENDER-28, Engine shape | `Live(generation, slot, LiveEv)` joins the messages (with design-7's `Activate`, `SwapLane`, `Transport`, `Panic` and `Hold`). | 8b.6 |
| RENDER-29 *Today* | Lane leases, the tempo lease and audition never take the session lock. | 8b.7 |
| ID-11 *Today* | `Module.muted` is built. | 8b.4 |
| ID-15, RENDER-25 | "A module that a delta re-keys is matched across its old and new keys." | 8b.9 |
| ID-17 *Today* | Key rename ("Change track ID…" in the Code view; the UI says "ID", never "key") and Restore are built; the track's file keeps its name. | 8b.9 |
| ID-18 | Following the edge means: from the export, downstream over audio edges, to the first node inside a `std:mixer` frame; the channel is the one whose control that node reads. No channel, or two, means no fader. | 8b.1 |
| FMT-12 | "A builder gives ids to the anonymous clips of each lane it writes." | 8b.5 |
| FMT-19 | The schema (`order`, `tracks.<k>.colour` as a palette name, `markers`); unknown keys kept on write; entries for keys that name no track kept, and removed only by deleting the track; read and watched by the server; Tx bases read from disk. | 8b.1, 8b.5 |
| WRITE-8 | "A module's `muted` → its overlay cells; it isn't structural." | 8b.4 |
| WRITE-10 | the rows below | each |
| WRITE-12 | The notes write's exception is gone: clip gestures carry each touched clip's hash, and the revision-based lane write is deleted. | 8b.5 |
| WRITE-18 | The new commands take the lock: `add-track`, `duplicate-track`, `delete-track`, `rekey`, `mute-in-song`, `layout`, `marker`, `tempo`, `meter`, `title`, `clips`, and `automate --remove`. | each |
| WRITE-22a | Previews evaluate the gesture the commit will write; an outside write to the lane re-applies it, and one to the clip ends the lease; a new plan carries the preview; audio lanes are applied on release. | 8b.7 |
| DIAG table | `layout.invalid` (Notice): `layout.json` doesn't parse; the view uses defaults; the track builders leave the file out, reorder, recolour and markers refuse; its fix is Reset layout. | 8b.1 |
| DIAG-6 *Today* | Restore is built (`crates/editor`). | 8b.9 |
| PKG-3 *Today* | A dictionary entry takes an insertion: a bare entry is wrapped as `{ in: …, <slot>: … }`, and the value is written in the style the object already uses. | 8b.3 |
| PKG-5 | The mixer's `db` and the master's `db` aren't optional: a gain at 0 dB is bit-exact. A fader's foot writes −96 dB, which `db_to_gain` makes silence. `pan` stays optional. | 8b.3 |
| PKG-13 | The checker records sidechain inputs (`Flat.side_inputs`): the inputs inside a frame that read the signal the frame recorded for its sidechain input. | 8b.4 |
| HOME table | The colour example becomes `{"tracks": {"kick": {"colour": "rose"}}}`; the layout row adds markers; a new row for `data/arrangement.json` (tempo, meter). | 8b.1, 8b.2, 8b.5 |
| HOME-1 | `home` gains `arrangement`. | 8b.2 |
| HOME-2 *Today* | Remove automation is built: one `Confined` Tx that puts the lane's value at the playhead back as the literal and removes the lane file. A drag on a lane-driven knob says why it doesn't move; latches aren't built. | 8b.5 |
| MON-1 | The sites are listed by the checker (`Flat.overlays`), the song's output included when it reads a track directly; the op is built-in and in every plan, offline renders included; it is exactly ×1.0 when heard; there is one cell per site, mapped from its module's key. | 8b.4 |
| MON-2 | "The mixer shows this": the strip of a silenced track with feeds says so (§6.7). | 8b.4 |
| MON-3 | An edge is heard when it lies on a path through a soloed module (§6.4); mute, mute in song included, wins over solo. | 8b.4 |
| MON-4 | The format of `.music2/overlays.json`; a deleted track's entries leave with it and come back only with its undo; a read-only window keeps overlays in memory; the export dialog, which can be told not to ask again for a song. | 8b.4 |
| MON-5 *Today* | built | 8b.4 |
| MON-7 | What a live note is: its source, its lane slot, its room, its ids, and what ends it. | 8b.6 |
| Deferred table | "Render taps and meters" are built in 8b; "track freezing, bounce as a Tx" stay deferred, with the trigger "the resample loop in the UI". | 8b.8 |
| `docs/gaps.md` | #1, #4, #6, #8, #9, #10, #14 and #17 move to "What works today"; #20 partly; the stale #5 and #7 are fixed (survey §8). | each |

**WRITE-10's new rows:**

| Gesture | Expectation |
|---|---|
| add a track (made precise) | `Confined { the new file, the import and the mixer call in song.ts, the new lane, a new module }` |
| duplicate a track | `Confined { the new file, the import and the mixer call in song.ts, its lanes, a new module }` |
| delete a track | `Confined { its file, its imports, the mixer call and every excised call, its lanes, the module removed }` |
| change a track's key; restore | `Confined { the key, the mixer call when the channel key follows, its lanes, the module re-keyed }` |
| reorder, recolour, a marker, Reset layout | writes only `layout.json`: no revision, so `Identical` |
| tempo, meter | `Confined {}`: the Tx writes the arrangement |
| the song's title | `Identical` |
| a fader whose channel doesn't write it | `Values` |
| insert an optional control (a channel's first pan, at the drag's threshold) | `Confined { the mixer call }` |
| a clip gesture, join included | `Confined { the lanes it touches }` |
| remove automation | `Confined { the lane expression, the lane removed }` |

---

## 15. Not in 8b

| What | Why not now | Where it goes |
|---|---|---|
| Override latches (MON-6, HOME-2): dragging a knob that a lane or a cable drives | It needs an override slot on lane-driven and cable-driven inputs in the engine, and "Write to code" has no settled meaning. 8b.5 gives the minimum instead: the drag says why, and the knob's menu has Show lane and Remove automation (§7.2) | session-only latches next (MON-6 allows them when the key isn't saved), from 8b.4's overlay op on the control's input; then with plugin windows (HOME-5) |
| Instrument previews (revision 1's §8.3) | a scratch project per component, a cache, and a second audio output through WebKit that bypasses the engine's device, for five std instruments | audition does the job (§8.3); previews come back with presets and packages (gaps #26, #39), through the engine |
| Tap tempo | one more popover for a rare gesture; typing or dragging the tempo does it | later, if asked for |
| MIDI keyboard input (CoreMIDI notes into the selected track, no recording) | not in 8b's list; the contract's deferred table puts it "after audition, on the same path" | owner question Q3: a small 8b.10 on 8b.6's sources |
| Metronome and count-in (gaps #19) | a click generator in `core/play` is engine work | its own small step; its top-bar button is placed (taste §3.3) |
| Key and scale (#16), and the top bar's key field | no home in the contract (survey §5.6) | an owner decision first |
| Sends, return tracks, group buses (#27) | structural edits of `song.ts` beyond one channel | later |
| Plugins in the + Track and effect lists (#32) | the host has to describe them | later; the add request already takes `clap:<id>` |
| Swapping a track's instrument (gaps #1's "later") | not in 8b's list | a small `Confined` builder after 8b.1, shaped like `add_effect` |
| Audio import, waveforms, audio tracks (#18) | prepared renders and import | the audio turn; joining audio clips comes with consolidation there |
| Swing, humanize, legato, transpose tools (#15) | tools, not 8b items | later; quantize is in 8b.5 |
| Automation curves | a field in the lane format (FMT-2) | with `m2 migrate` (FMT-3a) |
| Sections: copy a section, insert bars (#23) | large | later; markers start it |
| Freezing, bounce as a Tx | the deferred row | its trigger |
| Recent songs (#12) | not a builder; small | a UI slice beside 8b (taste SS-2) |
| The export sheet, stems, loudness (#36) | not 8b | later; the export dialog (§6.6) is the only export UI 8b adds |
| A Settings window (#31) | not 8b | later; the DSP popover is read-only until then |
| An undo history list, preferences, custom shortcuts | not 8b | later; every 8b builder already has its words (§2.4) |
| Mute and solo inside a track, or on `song` (MON-1) | the deferred row | `key()` frames, on its trigger |

---

## 16. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | `Confined` admits more than the gesture means for mixer edits, since the whole mixer call is declared | Every planner asserts on the candidate: strips compared by channel key, nothing removed or changed outside them. Tests run on every fixture and p16. |
| R2 | An import node or a strip node has no place inside a declared range, so `Confined` refuses add track | The first test of 8b.1 checks it on every fixture. The fix is in the planner's ranges, never in `Confined`. |
| R3 | The mixer's `db` at 0 dB isn't bit-exact | `renders.json` decides before 8b.3 lands; the fallback is a structural write at the first drag's threshold, the first pan's path (§5.1, §5.3). |
| R4 | A mute glide in flight when an activation lands steps instead of gliding | At most what is left of 10 ms, only when an edit lands during a toggle. `cur` could later carry by site. |
| R5 | Solos persist, and a song reopens half silent | the overlay pill, Clear all in its popover, and the export dialog |
| R6 | A producer expects mute to leave the export | the export dialog's "Mute Kick in the song, then export", its done toast's Unmute, and mute in song's distinct look |
| R7 | A lane lease and the actor race on the same lane | both read the leases under the control plane's lock; the actor re-applies the gesture on every swap of a leased lane; tests for outside writes and new plans (§9.4) |
| R8 | A tempo lease's prepare is slow at p50 | one in flight; measured first (§9.5); the sound updates less often, the field doesn't |
| R9 | A tempo lease's plan races the actor's plans | sent only while the plan it was prepared against is still the last one sent, else prepared again |
| R10 | Audio drifts off the grid during a tempo drag | allowed in a preview only, and it never skips: the lease offset keeps each player's position (§9.2); the commit re-renders and crossfades |
| R11 | Native accelerators fire while the producer types in Monaco or a field | only ⌘ and ⌃ chords are accelerators; focus routes Monaco's own chords to Monaco (§2.6) |
| R12 | Builders often refuse hand-written songs (no mixer, a track a helper makes) | every refusal says why and offers Edit in code; every fixture and template fits |
| R13 | Collisions with the G11 workflow and with step 7 | 8b.5 after G11 lands; the engine slices after step 7 slice 2; the early slices touch only the editor, routes, web and std |
| R14 | A sidechain input is missed: a component reads its sidechain through another node first | `side_inputs` are the frame's nodes that read the recorded signal directly; a test for each std component with a sidechain input. A miss can only silence a path that should have stayed heard, never un-duck a track, because a sidechain read through a node is a read of that node. |
| R15 | `layout.json` conflicts in git between two people | presentation only: a conflict costs order or colour, never sound or the track builders; unknown keys and absent tracks' entries survive; Reset layout |
| R16 | Meter messages load the WebSocket at 50 tracks | about 50 × 2 floats per message, about 12 KB/s *estimate*, sent only while the engine runs |
| R17 | A window dies with notes held | the 10 s watchdog, and panic |
| R18 | A 409 on delete surprises an agent | the body lists the dependents with their files; `--cascade` |
| R19 | A deleted track's mute, solo or height lands on a new track with the same key | dropped at the delete, kept in memory for its undo, and dropped again when a track is added on that key (§6.5) |

---

## 17. Pointers

| What | Where (new files marked) |
|---|---|
| track builders | `crates/editor/src/tracks.rs` (new) and `tracks_tests.rs` (new); `rename.rs` (`track_call`, reused) |
| `layout.json` | `crates/editor/src/layout.rs` (new); `plan.rs` (`base_from_disk`); `crates/server/src/graph/watcher.rs` |
| the channel lookup and the Mix view | `crates/editor/src/mix.rs` (new); `lens.rs` (dictionary entries) |
| song settings | `crates/editor/src/song.rs` (new) |
| clip gestures | `crates/editor/src/clips.rs` (new); `notes.rs` (the whole-lane write goes) |
| automate's lane names, and Remove automation | `crates/editor/src/automate.rs` (`name_of`, `remove`) |
| Restore | `crates/editor/src/fixes.rs` (new) |
| routes, `WRITES`, the WebSocket | `crates/server/src/routes.rs`, `crates/server/src/lib.rs` |
| overlays, leases, meters, local state on the server | `crates/server/src/graph/{overlay.rs (new), lease.rs, actor.rs}` |
| `muted` in the SDK | `core/script/sdk/core.ts` (`TrackOptions`, `options`, `__build`) |
| `Module.muted`, `Flat.overlays`, `Flat.side_inputs` | `core/ir/src/lib.rs`, `core/ir/src/check.rs` |
| `Muted` not structural | `core/session/src/tx/delta.rs` (`structural`) |
| `Home::Arrangement` | `core/session/src/lib.rs` |
| the `Rekeyed` pass | `core/session/src/tx/continues.rs` (step 7's) |
| the overlay op, `Live`, taps, the lease offset | `core/runtime/src/{plan.rs, lib.rs, sched.rs, engine.rs, cells.rs, audio.rs}`; `core/play/src/lib.rs` |
| std | `std/src/{mix.ts, instruments.ts, fx.ts}`; `std/kernels-src` (`compress_keyed`) |
| the Empty template | `templates/empty/song.ts` (its comment) |
| the CLI | `apps/m2/src/main.rs`, `apps/m2/tests/{tracks,song,clips}.rs` (new) |
| menus | `apps/music2-desktop/src/menu.rs`; `web/src/App.tsx` (`appCommand`'s focus routing) |
| the web | `web/src/components/{SongTimeline, NoteView, GraphCanvas}.tsx`; taste S's `Dock.tsx` and `Browser.tsx`; `Mix.tsx` and `TrackHeader.tsx` (new); `web/src/lib/{graph.ts, palette.ts}` |
| test fixtures | `core/session/tests/fixtures/sidechain/{env,follow,keyed}/` (new); `crates/gates/src/songs.rs` (`tone`) |
| gate tests | `core/song/tests/write_api.rs`, `core/session/tests/mute_sidechain.rs`, `core/play/tests/mute_click.rs` (new) |
| browser suites | `e2e/s8-{tracks,songbar,mix,mute,clips,automation,audition,leases,meters,ids}.mjs` (new); `e2e/run.mjs` `SUITES`; the README's table |
| the headless server's paced player (tests only) | `crates/server/examples/headless.rs` (`M2_AUDIO=paced`) |

---

## 18. Attack response

Twenty-five problems were raised against revision 1. The ones that change the UI changed `taste.md` too (its §7).

| # | Problem | Verdict | Why | Where it changed |
|---|---|---|---|---|
| B1 | Tempo lease: audio lanes skip on every drag update | **Accept, with a simpler fix** | Confirmed: `AudioLane::fill` reads at the transport's frame, which each update recomputes. Rather than a swap with a crossfade per update, each audio player reads its unchanged renders at an offset set at the update's block line so the read position doesn't move: the same render at the same position needs no crossfade. Jumps re-anchor the offset on the beat; commit and cancel crossfade it back to 0 through design-7's `shift`. RENDER-27a reads "lags, never skips"; the test is on `resample` | §0, §9.2, §9.4, §9.7, §14 RENDER-27a, R10 |
| B2 | "Change key…" in the track menu reads as a musical key | **Accept** | A beginner would open it to transpose the bass, and taste plans a musical Key field. It leaves the header and strip menus for the Code view's file-tree menu; UI copy says "track ID"; the CLI keeps `m2 rekey` | §0, §2.4, §2.6, §3.5, §11, §14 ID-17 |
| B3 | Local state keyed by track key carries over to a new track that reuses the key | **Accept** | A soloed, deleted Pluck would make the next Pluck silence the song. A builder delete drops the key's overlay and view entries, keeps them for its undo, and an add drops any left by an outside delete; a rekey renames them in `view.json` too | §2.2, §3.3, §3.6, §3.7, §6.5, §11.1, R19 |
| B4 | Native menu shortcuts take keys away from the code editor and text fields | **Accept** | Confirmed in `menu.rs` (Space already left out) and `App.tsx` (⌘D taken first). Accelerators only for ⌘ and ⌃ chords; single keys, ⏎, ⌫, Space and ⌥ chords are the page's; Monaco's chords go to Monaco when it has focus; the sheet moves to ⌥K; Duplicate is one item, in Edit | §2.6, taste §5.3, R11 |
| B5 | A toast after every fader, knob, tempo and clip edit, drawn over the Mix strips | **Accept** | One mixing pass is hundreds of releases. Toasts only for results off screen or structural; the words name the undo step instead; toasts sit above the dock | §2.4, §4.6, §5.7, §7.8, taste §2.14 and ST-5 |
| B6 | A channel's first pan is silent until release | **Accept** | The insertion `pan: 0` commits as the drag passes 4 px, and the drag goes on as a lease once that revision plays, about 200 ms in. Making `pan` non-optional is **rejected**: a mono channel's panner at C isn't bit-exact, so every render with a mono channel would change | §5.3, §5.5, §5.6, §5.7, §14 WRITE-10 |
| B7 | On automation clips, a click both adds a point and selects the clip | **Accept** | Points are hit within 6 px of the curve; elsewhere a click selects and a drag moves or copies the clip; Draw mode paints anywhere | §7.2, §7.8, taste TL-6 and §5.2 |
| B8 | Duplicating a section lands off the bar grid | **Accept for duplicate; paste stays at the playhead** | A span of a bar or more rounds up to whole bars, a shorter one to the grid, and a marquee uses its own bar-snapped range. Paste already lands at the snapped playhead the producer chose, so it needs no bar offset rule | §7.1, §7.4, §7.7, §7.8 |
| B9 | Drawn automation makes its knob dead (latches deferred) | **Accept the minimum; keep latches deferred** | A drag says "Follows the Cutoff lane"; the knob's menu offers Show Cutoff lane and Remove automation, the inverse of Automate, one `Confined` Tx. Session-only latches stay the next step after 8b: they need an override on the control's input and Re-enable in the pill | §7.2, §7.7, §7.8, §14 HOME-2, §15 |
| B10 | After + Track, the rename field takes Space, M and S | **Accept** | No rename opens; the track is selected and lit; ⌘R renames. The list's keyboard-active row is the last instrument added, else the first row | §3.5, §3.7 |
| B11 | The fader's floor is −60 dB, not −∞ | **Accept, with no kernel change** | `db_to_gain` already returns exactly 0 at −96 dB or below (`core/kernel/src/lib.rs:234-236`), and a range is only a hint (PKG-8). So the foot writes −96 and shows "−∞"; no render changes and std's version stays | §5.1, §5.5, §5.6, §5.7, §14 PKG-5 |
| B12 | The header level slider is imprecise, a duplicate, and squeezes the name | **Accept** | Removed; the name gets 140 px; the level is the Mix fader | §0, §2.3, §5.5, §5.7, taste §3.4 |
| B13 | An unreadable `layout.json` blocks the track builders; layout writes drop tracks that are briefly absent | **Accept** | The builders leave an unreadable file out of their Tx; only reorder, recolour and markers refuse; Reset layout is the fix. Entries for absent keys are kept, and only deleting a track removes its key | §2.1, §3.6, §3.7, §14 FMT-19 and DIAG, R15 |
| B14 | Hollow "default" markers on most values | **Accept** | An inherited value's home is code, like a written one: `fg-muted` text and its tooltip say it; markers stay for lane, latch, cable and state | §5.5, taste §2.10 |
| B15 | The export dialog appears on every export and doesn't say it edits the song | **Accept** | "Don't ask again for this song" in `view.json`; the button says "Mute Kick in the song, then export"; the done toast offers Unmute Kick | §6.6, §6.10, §14 MON-4, taste EX-3 |
| B16 | The overlay pill's × clears every mute and solo with no undo | **Accept the popover-only form** | Clear all moves into the pill's popover, two clicks away. A local undo for overlays would be a second undo history | §6.7, §6.10, taste §3.3, TB-7, §5.2 |
| B17 | Mute in song combined with solo, the M key and dragging across M unspecified | **Accept; mute keeps winning** | Solo doesn't override a mute in song, so S means one thing; S's tooltip and the roll say why it's silent; the M key and drags skip such tracks; the roll gets the "Silenced by the Bass solo" hint | §6.4, §6.7, §8.4, taste §2.9 |
| B18 | Remove from 8b: previews, tap tempo, the pulsing dot, the status edge, the M code dot, ⌥-click tempo reset | **Accept all six** | Each repeats a signal or adds a subsystem without a decision; audition hears an instrument; the `<track>:src` taps go with the dot | §0, §4.4, §6.7, §8.3, §10.2, §15, taste §1, §2.6, §2.9, §3.3, §5.2 |
| B19 | No overlay site where the song's output reads a track export directly | **Accept** | `Flat.out` is a `PortRef`; it becomes `Consumer::Out` with an overlay just before the output, labelled `song` | §0, §6.1, §6.9, §14 MON-1, design-8a §3.3 |
| B20 | The ducking gate test needs taps that offline renders don't have | **Accept, without a probe** | The test compares whole outputs: the kick muted in the song against the kick's channel at −96 dB, bit-identical when the ducking is untouched, with a negative control. No test-only API | §10.1, §13.2 |
| B21 | Mix view spec gaps: shared strips, and the module named for MON-2's row | **Accept** | A shared strip's M and S act on all its tracks, with a mixed state; `feeds` names the channel's track when the fed node is in `song` | §2.3, §6.7, taste §3.5 |
| B22 | Selecting a track can swap the dock from Mix to Device | **Accept** | The dock's view is one choice for the window; only a double-click on a notes clip opens Notes; Device only at first launch | taste §3.7 |
| B23 | Code the builders leave behind reads wrong or mixes styles | **Accept** | The Empty template's comment is rewritten to stay true; insertions follow the object's unit style | §3.1, §5.2, §5.6, §14 PKG-3 |
| B24 | Inconsistencies between `taste.md` and the design | **Accept** | 1 BPM per 4 px everywhere; EX-3 is one Tx; ⌘1 to ⌘4 act on the editor last pointed at or focused; Follow playhead on by default in `view.json`; five expectations, `Journal` in §13.1 | §0, §2.2, §7.3, §13.1, taste §5.1, §5.2, EX-3 |
| B25 | Gaps in a first session: Join and a MIDI keyboard | **Accept Join; shape audition for MIDI; MIDI itself is an owner question** | Join is a small pure `Clips` gesture on ⌘J (notes clips; control and audio clips refuse with words). Live notes now belong to a source, so a device is a second kind with no engine change; CoreMIDI input is scope the owner decides (Q3) | §2.6, §7.1, §7.4, §7.5, §8.1, §8.2, §15, §20 Q3 |

---

## 19. Build slices

**Before any slice:** taste slices T (tokens) and S (shell), with defect D1, land first (§1.4, taste §8).

**Every slice lands green** (§0): `renders.json` unchanged, every test with the same meaning, `e2e/run.mjs all` with the
suites the slice moves updated in the same PR, the taste lint, and `the_write_api_is_step_6s` unchanged. A slice that
touches `core/*` changes the contract first, in the same commit (§14).

| Slice | What lands | Waits for | Its suite | Size *estimate* |
|---|---|---|---|---|
| 8b.1 Tracks | §3: `tracks::{add, duplicate, delete}`, `layout.rs` with the absent-key and unreadable-file rules, local state on delete and add, the Empty template's comment, + Track with no auto-rename | taste T and S | `s8-tracks` | 2 weeks |
| 8b.2 Song bar | §4: `song::{tempo, meter, title}`, `Home::Arrangement`, the transport leftovers, with no tap tempo and no ⌥-click reset | — | `s8-songbar` | 1 week |
| 8b.3 Faders | §5: std's non-optional `db`, the −96 foot, entry insertions in the object's style, the first pan at the drag threshold, the Mix view without a header slider | — | `s8-mix` | 1.5 weeks |
| 8b.4 Mute and solo | §6: overlays (the song's output included), `muted`, the solo rule with mute winning, `overlays.json`, the export dialog, the gate's sidechain, click and export tests | step 7 slice 2; 8a's labels | `s8-mute` | 2.5 weeks |
| 8b.5 Clips | §7: gestures by id with join and bar-rounded duplicates, automation hit-testing, Remove automation, grid, zoom, heights, markers | the G11 workflow's `notes.rs`; taste T (⌘E) | `s8-clips`, `s8-automation` | 2.5 weeks |
| 8b.6 Audition | §8: `Msg::Live` with sources, the live table, the roll's audition and hints | step 7 slice 1 (slice 2 for notes that survive an edit) | `s8-audition` | 1 week |
| 8b.7 Leases | §9: lane leases, the tempo lease with the audio lease offset | step 7 slice 2; 8b.2, 8b.5 | `s8-leases` | 2 weeks |
| 8b.8 Meters | §10: taps without `:src`, the meters message, the DSP pill | step 7 slice 2; 8a for the DSP readout | `s8-meters` | 1 week |
| 8b.9 Track IDs | §11: `tracks::rekey` in the Code view's menu, Restore, the `Rekeyed` pass | step 7 slice 2, for kept instances | `s8-ids` | 1 week |

About 14.5 weeks in all *estimate*. §12 says what each slice costs when built before step 7.

**Two ways to schedule it** (owner question Q1):

- **A, the order of work as written:** step 7, then 8a, then 8b.1 to 8b.9 in order. Nothing is rebased, and nothing is
  ever cut that step 7 would keep.
- **B, producer features sooner:**
  1. taste T and S, then 8b.1, 8b.2, 8b.3 and 8b.5, in a workflow beside step 7;
  2. then 8a;
  3. then 8b.4 and 8b.6 to 8b.9.

  The owner gets add track, tempo, faders and clip editing about two months sooner *estimate*. The costs: rebasing the
  early slices onto step 7 slice 2's `actor.rs` and `graph.rs`, and structural edits that cut tails until step 7 lands.
  8b.5 must also wait for the G11 workflow's `notes.rs`.

If the owner answers yes to Q3, **8b.10 MIDI input** follows 8b.6: midir note input to the selected track's lane as a
device source (§8.1), no recording, released when the device disconnects; about 1 week *estimate*.

---

## 20. Owner questions

Only the decisions this design can't make on its own. Each has the design's answer. Revision 1's other questions are
now decided in the text, where each has its reason.

| # | Question | This design's answer |
|---|---|---|
| Q1 | Build 8b.1–8b.3 and 8b.5 beside step 7 (B), or after 8a as the order of work says (A)? | A by default. B if you want add track, tempo, faders and clip editing about two months sooner, at the cost of rebases and cut tails until step 7 lands (§19) |
| Q2 | While you drag the tempo, should audio clips play on at their old speed and drift off the grid until you let go (never skipping), instead of re-rendering at every step? The contract allows it today only for warped audio | Yes: re-rendering every clip at every step is what RENDER-27a exists to avoid, and the drift lasts only the drag (§9.2) |
| Q3 | Should step 8 take notes from a MIDI keyboard (CoreMIDI into the selected track, no recording), as a small slice 8b.10 after audition? | Yes: most producers write with one, and 8b.6's sources already fit it, so it costs about a week |
| Q4 | 8b leaves out instrument previews (audition does the job) and tap tempo. Agreed? | Yes: both can come later, previews through the engine with presets and packages (§15) |
