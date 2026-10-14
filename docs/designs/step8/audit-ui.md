# music2 UI audit at HEAD cbf5eb9, written before step 8

Read-only audit of the app as built through step 6. Every claim below points at a screenshot in
`/private/tmp/claude-501/-Users-jerryxu-dev-suite2-music2/07b91ec3-d8c4-49ce-985f-b33193c681f7/scratchpad/ux8/shots/`
(98 PNGs, 1440x900, plus five 2x crops named `f05`–`f09`). It has three parts:

- **Part 1:** the critique of a pop producer working at A. G. Cook's level, screen by screen.
- **Part 2:** the critique of a senior product designer for pro tools.
- **Part 3:** the top 20 problems, ranked, each tied to a step-8 item or marked as UI-only.

Defects found while taking the screenshots are listed first, because they are bugs and not matters of taste.

## How the screenshots were taken

- A detached worktree of HEAD at `scratchpad/wt-head`, built with
  `CARGO_TARGET_DIR=scratchpad/target-head cargo build -p m2-server --example headless`, then
  `npm ci && npm run build` in `web/`.
- The headless server ran with `M2_AUDIO=none` and `HOME=scratchpad/home`, on copies of the
  fixtures in `ux8/proj-first` (examples/first-song), `ux8/proj-starter` (templates/starter) and
  `ux8/proj-empty` (templates/empty). There was also a generated 16-track project, `ux8/proj-p16`
  (first-song's notes, cycling through kick/bass/hat/pluck/pad), to see how the window scales.
  Projects were opened with `POST /api/graph/open`. The New Song dialog was driven through the UI
  and created "Hyperpop Demo" in the scratch home.
- Headless Chrome 133 was driven with puppeteer-core from `e2e/node_modules`, with
  `window.__M2__ = { native: false }`, as the e2e suites do. The scripts are `ux8/{lib,a-start,b-first,c-edit,d-states,e-templates,f-detail,g-more}.mjs`.
- **Caveats of browser mode:**
  - The start screen hides **Open Project Folder…** and the ⌘ hints (native only).
  - Export saves without a save dialog.
  - There is no native menu bar.
  - Tauri's WebKit renders fonts and scrollbars a little differently.
  - The 88 px left inset in the top bar is real: it is reserved for the macOS traffic lights.

## Defects found while shooting (fix regardless of ranking)

| # | Defect | Evidence | Severity |
|---|---|---|---|
| D1 | **Stale code diagnostics after switching projects.** After the Starter was open, opening Empty showed "6 type problems: song.ts:5 Cannot find module './tracks/bass'", with red squiggles under Empty's *comments*. On a fresh page load the same project says "Compiles". Monaco models and markers outlive the project (`GraphCanvas` is keyed by session, but the models are global by path). This hits New Song with a song open, Open Project and Open an Example. | `e12` vs `f01` | high: a beginner sees errors that aren't there |
| D2 | **A project whose code doesn't compile opens to "No tracks yet. Tracks… show up here" and 120 BPM.** The song has four tracks and 124 BPM in `data/arrangement.json`. The timeline lies about the song while the banner says the code is broken. | `g02` | medium |
| D3 | **Cable drag lights up targets it will refuse.** Dragging the bass's `wobble` (Hz) highlights every knob, `drive` (dB) included. Dropping it gives the toast "edit rejected: input `amount` expects control<db>, got control<hz>". The knob is labelled *drive*, but the error says `amount`. | `d01`, `d02` | medium |
| D4 | **A single click to focus the piano roll adds a note.** There is no select/draw tool distinction, so the first click into the roll writes a revision (48 → 49 notes, rev 1 → 2). | `b08` → `b09` | medium |
| D5 | **The read-only state says the same thing three times.** It appears in a blue banner, as a red "Needs attention" row (`lock.held`), and as a toast after a refused drag. The text is CLI copy ("close the project in m2 and run this again", "pid 76949"), and the knobs still look and feel editable. | `d15`, `d16` | medium |
| D6 | **Automate names the lane after the binding, not the control.** Automating the kick's *decay* made a lane `out-decay` (the `const out = kick(…)` name leaks into the key), and the toast reads "Automating out decay in a new lane, out-decay". | `c07`, `c08` | low–medium |
| D7 | **Toasts pile up and linger over the work.** Code-diff toasts stay through several later actions and cover the right edge of the graph (the delay node, the outputs) and the zoom bar. | `c13`, `c14`, `c21`, `e06` | medium |
| D8 | **Track order is alphabetical by key.** The Starter's mixer order is kick, clap, hats, bass, chords, hook. The timeline shows Bass, Chords, Clap, Hats, Hook, Kick, so the kick sits at the bottom. Colours are assigned by that index and cycle every 7 (Kick 01 and Pluck 15 are both pink). FMT-19 says `layout.json` holds order and colour, and nothing reads it yet. | `e01`, `f02`, `f04` | medium |
| D9 | **Module tabs overflow behind the view switcher** at 16 tracks ("Kic…" is cut under Graph/Code/Notes), with no scroll affordance. | `f02` | medium |
| D10 | **The UI fonts come from Google Fonts over the network** (`web/index.html`). Offline, which is common in studios and on planes, the app falls back to system fonts, so the design changes. Ship Inter and JetBrains Mono in `web/public`. | `index.html` | low, but it's a pro-tool basic |
| D11 | **Clicking or double-clicking an automation clip does nothing**: no selection, no editor, no message. | `g03`, `g04` | medium (it's the #3 problem below) |
| D12 | **Scrolling the timeline to reveal a new lane tucks the first track's name under the sticky ruler** ("Bass" is half hidden). | `c07`, `d09` | low |

---

## Part 1: the producer (A. G. Cook making a pop record)

The lens: a producer who writes, sound-designs and mixes in one sitting. They loop 4 bars for an hour,
duplicate a chorus in one keystroke, automate everything (filter sweeps, pitch, volume rides),
sidechain hard, and resample. They never take their hands off the keyboard for long, and they judge
a tool by how many seconds stand between an idea and hearing it.

### Start screen (`a01`, `a02`, `a03`, `g01`)
- **No recent songs.** The whole 1440x900 window is used for two buttons. Every working day starts
  by reopening yesterday's song, and here that means File → Open Project Folder… and a folder dialog
  (and in a browser it isn't offered at all). gaps.md #12 is "small"; it should come before most
  of step 8b.
- Open an Example is fine. The With plugins refusal (`g01`) is plain and kind.
- **What I'd want:** a recent list with each song's name, BPM, key, last opened and a mini
  arrangement thumbnail (the template preview already draws one). Enter reopens the newest.

### New Song dialog (`a04`–`a06`, `a08`)
- Fast: ⌘N, type, Enter. This is one of the best screens.
- **Missing:** tempo, time signature and key at creation. A producer knows "140, F minor" before
  anything else. With only two templates, the cards could be smaller, leaving room for a BPM field.

### Top bar / transport (`b01`, `b03`, `f05`)
- **No tempo field, no time signature, no key, no metronome, no record, no count-in, no CPU/DSP meter
  and no master meter.** The tempo is shown, read-only, in 11 px mono in the track column's corner
  ("4/4 · 124 BPM"). Changing it means editing `data/arrangement.json` by hand. Step 8b's tempo bar
  is the fix.
- The position reads "1.1" (bar.beat). There's no sixteenth or tick, so you can't tell where in the beat you are.
- Play is a toggle with a pause glyph, but stop "ends the notes". It's really a stop, so the
  glyph lies. There's no separate stop, and no "return to where play started" option (Ableton's
  and Logic's default behaviour).
- **The close-song ✕ sits right of Redo**, the same size and style. One slip from redo closes the song.
  Nothing is lost, but it's jarring mid-session.
- Export is a top-level button. Fine for a beginner, odd for a pro; it belongs in File (where it
  also is) with a shortcut that doesn't steal ⌘E (see Shortcuts).

### Arrangement / timeline (`b01`, `b07`, `e01`, `f02`, `f04`, `g05`, `g06`, `c22`)
- **You can't add a track.** The Empty template's empty state (`e11`) is a dead end: "No tracks yet
  — Tracks, and the lanes the code declares, show up here." The only way in is uncommenting code.
- **No M/S/arm/volume/pan in the track headers.** Each header spends its space on "Bass / track"
  and "notes / 48 notes" (`f06`). Neither helps while producing.
- **No zoom.** The scale is fixed at 24 px per beat, and rows are fixed at 64 px. You can't see a
  3-minute song, and you can't get in close to edit a hat roll. At 16 tracks only 6 are visible (`f02`).
- **Clips can only be moved along their own lane, snapped to whole beats** (`g05`: a 6-beat drag
  lands on bar 2 beat 3). There's no duplicate (⌘D), copy/paste, resize, split, repeat, or move to
  another track, and no clip names, colours or mute. You build a pop song by copying sections, and
  here you can't.
- Double-click empty space to make a 1-bar clip is good (`c22`), but there's no pencil/draw mode,
  and the new clip lands past the song's end with no hint where the song ends in the timeline.
- No markers or sections (verse/chorus), no locators, and no insert or delete of bars.
- Track order is alphabetical (D8). You can't put drums on top, group tracks or fold them.

### Mixing: there is no mixer (`b02-…-song`, `e02-…-song`, `f03`, `f08`)
- **Mixing happens in the Song module's node graph**, as rows like `hats.db -6 dB` and `hats.pan 0.3`
  on a `mixer` node. In first-song **the kick has no level control at all**: `kick: kick.out` has no
  `db`, and nothing shows a knob for a value the code leaves out of a plain mixer entry. The master
  has only a limiter ceiling.
- **There are no meters anywhere**: not per track, not on the master, not in export. You can't
  see clipping or loudness. The headroom is invisible.
- **There's no mute or solo.** It's the most-used button in every DAW, every minute. Step 8b plans
  it. MON-2 even says "the mixer shows this" (that a muted kick still ducks the bass), and that
  needs a visual.
- At 16 tracks the mixer node is taller than the panel, and its text is about 6 px at the "fit" zoom
  of 50% (`f03`). Mixer rows are named by key (`t01.db`), not by track name.
- Sidechain (`duck`) exists only in code. Search "sidechain" in the effect browser and you get
  "No effects match" (`c12`), even though `duck` is exactly that.

### Sound design: the track graph (`b02-…-bass`, `b02-…-kick`, `e02-…-chords`, `e02-…-clap`, `c21`)
- The **Chords device chain** (pad → widen → chorus → reverb → out, `e02-…-chords`) is the best
  screen in the app: one row, left to right, knobs per device. That is what a producer expects.
- The **Clap graph** (`e02-…-clap`) shows the DSP inside the clap (noise, svf.bandpass, adsr, two
  scales, vca, poly, note) at 50% zoom, unreadable. A producer wants "Clap: tone, decay, room", not
  the circuit. The bass graph shows `time.beats → lfo → xrange → blend`: patching, not playing.
- **Knobs are too small and too coarse.** Each is about 30 px with a 160 px full throw. A 40 px drag
  moved hats.db from −6 to +12 dB, the top of its range (`c02`). There's no fine mode
  (⇧/⌥ drag), no type-in value, no value readout at the cursor while dragging, no ⌥/⌘-click reset
  (double-click only), and no scroll-wheel nudge.
- **No bypass, no reorder, no presets, no A/B, no dry/wet macro** on effects (gaps #21, #26).
- The knob menu (`c05`, `c06`) has only Automate and Reset. A pro expects: type a value, MIDI learn,
  copy/paste value, show automation, and "Edit in code" (the file:line is there, but as a footer label).

### Automation (`b01` bass lanes, `c07`, `c08`, `g03`, `g04`)
- **Automate is a dead end.** It makes a new lane, scrolls to it and flashes it, then you can't draw
  in it (gaps #14), and clicking the existing automation clips does nothing (D11).
- Automation lanes sit above the notes lane, in code order. The producer's mental model is the
  notes first, with automation folded under them on demand.
- There are no automation modes (read/latch/write), no "draw while playing", and no curve tools.
  WRITE-22a (lane leases) is the base for drawing while playing. It should cover control lanes
  from day one.

### Piano roll (`b08`–`b12`, `e03`–`e06`, `c22`, `e15`, `f09`)
- **It is silent.** Drawing, dragging a note two semitones (`e05`) or clicking the keyboard plays
  nothing (gaps #8, step 8b audition). You can't write a melody you can't hear.
- **The keyboard is decoration**: dark bars for black keys, one label per octave (C2), and no click
  to hear.
- No scale or key highlight, and no ghost notes from other tracks (you write the hook against the
  chords you can't see). No quantize, transpose, legato, humanize or triplet grid. The snap stops at
  1/32 and has no swing.
- Drum lanes show piano pitches: the hats roll is a single C#2 row (`b11`), and the empty Drums lane
  opens at C4 for a kick (`e15`). It should show drum names, or fold to the used rows.
- Rows are 16 px with no vertical zoom or overview. The hook's top note is half-cut at the edge (`e03`).
- The good parts: note names drawn on the notes (`e03`), the velocity lollipops, the marquee, the
  "Click in the grid to add a note" empty state (`c22`), the arrow-key nudge with ⇧ for octave/bar,
  and the length field.

### Effect browser (`c09`–`c13`)
- Typeahead plus Enter adds the effect: fast and right. The descriptions are lovely ("Darker:
  removes the highs above the cutoff").
- Names are code identifiers: `lowpass`, `eq3`, `bitcrush`. The browser has std only, no
  installed CLAP plugins (gaps #32), no presets, no favourites/recents and no preview.
- The popover moves when the list filters. It re-anchors from the + button and jumps up the screen
  (`c09` vs `c11`).
- After adding, the toast prints the code diff (`c13`): "tracks/keys.ts: pluck(lane.notes(\"chords\")) →
  reverb(pluck(…), { size: 0.6, … })".

### Code view (`b13`, `b14`, `d03`–`d08`, `e07`, `e08`, `f01`)
- A solid Monaco setup: types, ⌘S, problems strip, changed-on-disk banner, completions (`e08`).
- For a producer it should be a secondary dock that opens when asked, not a co-equal tab that the
  Song module and every track sit behind.
- D1 (stale diagnostics) makes it look broken.

### Needs attention, compile banner, read-only, lost connection (`d04`, `d05`, `d09`–`d12`, `d15`, `d16`, `f10`, `g02`)
- The ideas are right: data that's lost but movable gets a one-click "Move all to Kick (kicks)",
  and Undo works.
- **Each banner pushes the arrangement down by 50–130 px.** The compile banner plus Needs attention
  plus read-only can eat a quarter of the screen (`d15`). The text is long prose followed by an
  internal code (`data.unclaimed`, `lane.invalid`, `lock.held`).
- While the code is broken, the knob menu greys out with a reason (`d06`). That's good.

### Export (`d13`, `d14`)
- The progress ring in the button is elegant, and you keep working.
- There are no options: no range or loop-only export, no stems (gaps #36), no MP3/AAC, no bit depth
  or sample rate, no normalize, no tail control. The done toast doesn't report the peak or loudness
  (the event carries `peak_db`). Once mute/solo exists, MON-4's `overlays.active` warning has to show here.

### Shortcuts (from `App.tsx`, `menu.rs`, README)
- About a dozen exist: Space, Enter/Home, ⌘Z/⇧⌘Z, ⌘N, ⌘O, ⌘S, ⌘E, ⌘D (notes only), ⌘A (notes only),
  arrows, Delete, Esc.
- **Missing, in the order they'd be missed:**
  - mute/solo the selected track (M/S);
  - new track (⌘T / ⇧⌘T in Live);
  - duplicate a clip or track (⌘D everywhere, not only in the roll);
  - split at the playhead (⌘E in Live);
  - loop the selection (⌘L);
  - timeline zoom (⌘= / ⌘−, Z to zoom to selection);
  - metronome (K in Logic);
  - record (R);
  - draw mode (B in Live);
  - switch the bottom dock (Tab, or ⌘1/2/3);
  - quantize (Q);
  - transpose (⌥↑/↓);
  - a command palette (⌘K), which suits a code-first app.
- **⌘E is spent on Export.** Live users reach for ⌘E to split. Put Export on ⇧⌘E (or ⌘B, Logic's
  bounce) and keep ⌘E for split, which is coming with clip editing.
- The Transport menu has no Loop, Metronome or Record, and the View menu has only Full Screen (no
  Mixer, Zoom, or Show/Hide the editor).

---

## Part 2: the product designer (Ableton/Bitwig/Logic/Figma-level polish)

### Hierarchy
- **The loudest pixels on screen are the brand, not the work.** In `f05` the two most saturated
  objects are the "m2" logo tile and the gradient play button, side by side and the same colour.
  Next come the pastel clip bodies. The selected track gets only a slightly whiter header (`b01`).
  The open clip gets a 2 px dark ring. Selection should be the strongest signal in an editor.
- **The chrome stack costs about 210 px before content in the bottom panel**: 64 px top bar, 34 px
  ruler, 8 px split, 44 px tab bar, then a rounded card inside the panel with its own title row,
  legend and 16–20 px padding (`b02-…-kick`). That's 23% of a 900 px screen before a single knob.
  The top bar alone could be 44 px.
- **The song name and "rev 1" are centred in the top bar.** The revision is engine vocabulary. It
  should be hidden, or live in a status line next to the code view's "playing rev 1".

### Typography
- **Ten type sizes** are in use: 9, 10, 10.5, 11, 12, 13, 14, 15, 19 and 30 px. Several are below
  11 px at 1x: tempo marks on the ruler are 9 px, node subtitles 10 px, "shared ×2" 9 px.
  **Pick four**: 11 / 12 / 13 / 15.
- **Monospace is used for things that aren't code**: lane names ("notes", "hits", "cutoff"),
  node titles ("bass", "mixer"), the bar.beat position, the BPM, and "rev". Mono should mean
  "this is literally in your file". Everywhere else, Inter with tabular numbers.
- Italic marks "this control is at its default, not written in the code" (`b02-…-bass`: *4 dB*).
  Italic at 11 px in grey reads as disabled. A small hollow dot (HOME-1: "the UI shows which home")
  is clearer and scales to the other homes: lane, cable, latch.
- **All-caps "TRACKS"** with tracking is the only caps label in the app. Remove it.

### Spacing and alignment
- **Track headers have two competing columns**: the name left-aligned at the top, and the lane
  names right-aligned (and in mono) on each lane row (`f06`). Lane names should sit left-aligned
  under the track name, indented, in the same face.
- **The graph sits in a card inside a panel inside a window** (a rounded `ring-1` section on the
  `ink-950` panel). Drop the card. The canvas is the panel.
- **Small graphs float in a sea of dots.** A 3-node track graph sits centred in about 1400x300 px of
  dotted grid (`b02-…-kick`, `e13`), while large ones are shrunk to 50% (`e02-…-clap`, `f03`).
  "Fit" should have a floor and a ceiling (never below 75%, never above 100%), and a device chain
  should top-align and left-align like Live's device view, not centre.
- **Popovers are placed differently.** The knob menu opens at the pointer and covers the view
  switcher (`c05`). The effect browser jumps as it filters (`c11`). Toasts cover the bottom-right
  of the canvas (`c14`).

### Colour
- **There are five accent systems at once:**
  1. the mauve-to-violet gradient (brand, play, primary buttons, radio checks);
  2. sun-yellow (playhead, loop);
  3. seven pastel track hues;
  4. port kinds (rose audio, blue control, green notes);
  5. semantic banners (amber compile, sky read-only, red errors, and **mauve for warnings** in
     Needs attention, `d09`, which reuses the brand colour to mean "warning").

  Keep three: one neutral UI accent, track colours, and red/amber for problems.
- **Gradients** on the logo, play button, Create button and radio check look like a web template,
  not an instrument. Use flat fills.
- **Play's states are inverted.** Stopped is a bright gradient; playing turns dark grey (`b03`),
  which reads as disabled. In Live and Logic, playing is the *lit* state.
- **Track colours are mixed 50% toward grey, then the clip bodies are drawn at 22–42% alpha** with
  darker marks (`f06`). The note marks in clips are low-contrast brown-on-beige. Muting is good for
  the "light gray, muted" brief, but apply it once, not twice.
- Track colours are assigned by alphabetical index, so they change when a track is added (D8). The
  colour should be chosen and stored in `layout.json` (FMT-19; HOME-3 keeps it out of the code).

### Density
- **The track rows say the same things twice**:
  - "track" under every track name: the timeline only holds tracks, except the Song's own lanes,
    which are labelled "mix";
  - the lane kind *and* a note count ("notes / 48 notes");
  - the key after a rename ("track · hats", `c17`).

  Replace all of that with controls: M, S, a level, and a meter.
- **The node cards say "bass.ts:11" under every title** (`f07`). File and line belong in a hover,
  the knob menu footer (already there), or the code view. They're noise on the canvas.
- The graph's header line ("Bass track | ● Audio ● Control ● Notes | Drag a named output onto a knob
  to connect it") is a legend plus a tutorial, on every module, forever. Show the hint once, in an
  empty state or in onboarding.

### Iconography
- Lucide's line icons are consistent. But **every audio node gets the same waveform glyph** and every
  filter in the browser the same icon (`c09`), so the icons stop meaning anything. Either one icon
  per category, shown once in the category header, or distinctive icons per instrument or effect.
- **Undo, Redo and Close are three identical ghost buttons in a row** (`f05`). Close should be
  separated, or removed (⌘W in the menu bar).
- The "m2" logo is text in a gradient tile, both in the top bar and on the start screen. The top
  bar doesn't need a logo: the window title and the Dock already say which app this is.

### Consistency
- There are three popover styles: the examples menu, the knob menu (tiles with 28 px icon squares and
  two-line descriptions for *two* items) and the effect browser. All use radius 16–22 px with big
  shadows. Pro menus are dense one-line rows, with the shortcut right-aligned and an optional detail
  line.
- A native `<select>` ("Elsewhere…") sits in the attention strip among custom buttons (`d09`).
- Focus rings vary: browser-blue on the Graph tab after Space (`e09`), mauve elsewhere.
- About seven corner radii are in use (6/8/10/12/16/20/22 px). Pick two: 6 for controls, 10 for
  surfaces.
- Naming varies: the timeline says "track", tabs say the label, mixer rows say the key (`t01.db`),
  and the automated lane says the binding (`out-decay`).

### Motion
- **The playhead glides on a 200 ms CSS transition between 200 ms polls**, so it steps and eases
  rather than moving at audio rate. It should be driven by `requestAnimationFrame` from the
  transport clock, interpolated from the last status.
- The "lit" flash on a new lane and the `rise` on popovers are tasteful.
- There are no meters, so nothing on screen moves with the music except the playhead. A playing DAW
  should visibly breathe: meters, the active clip and the playing note in the roll.

### Empty states
- The Empty template's timeline (`e11`) states a fact with no action. It needs a single "Add a
  track" button, which step 8b's add-track builder provides.
- The broken-on-open timeline (`g02`) says "No tracks yet", which isn't true (D2). It should say
  "Waiting for the code to compile" with the track names it last knew.
- "Click in the grid to add a note" (`c22`) is the right pattern. Use it everywhere.

### Things to REMOVE
1. The "m2" logo tile in the top bar.
2. "rev N" next to the song name (move it to the code view's status line).
3. "track" under every track name, and the note counts in lane labels.
4. File:line ("bass.ts:11", "lane · bass.ts:7") under every node title.
5. The graph's legend row and its "Drag a named output…" hint.
6. Module tabs as the primary navigation: the selected track in the timeline *is* the selection.
   Keep "Song" as a header row in the track list.
7. Code diffs in toasts ("song.ts: -6 → 12", "tracks/keys.ts: pluck(…) → reverb(…)"). Toasts
   should name the change in words ("Hats volume −6 → +12 dB", "Added Reverb to Keys") with Undo.
8. Internal codes (`data.unclaimed`, `lane.invalid`, `lock.held`) and "pid" from user-facing text.
   Keep them behind a disclosure for agents and power users.
9. All gradients (logo, play, primary buttons, radio checks).
10. The outer card around the graph canvas.
11. The per-item category icons in the effect browser, and the "esc" key hint.
12. The italic-means-default convention (replace it with a home marker, HOME-1).
13. The zoom bar on graphs that fit at 100%.
14. The native `<select>` in the attention strip.
15. The duplicated read-only message: one banner, nothing in Needs attention, no toast (grey the
    controls instead).

---

## Part 3: top 20 problems, ranked

Ranked by how much each costs a working producer, weighted by how soon it can land.

**"Where"** says what fixes it:
- a step-8b item: a Tx builder or engine feature on the step-7 base;
- **UI**: web/ only, no core change;
- **later**: a gaps.md item outside step 8.

Sizes follow gaps.md (S/M/L).

| Rank | Problem | Evidence | Fix | Where | Size |
|---:|---|---|---|---|---|
| 1 | **There is no mixer: no faders, no mute/solo, no meters.** Levels are graph knobs; the kick has no level control; the master is a ceiling; at 16 tracks it's unreadable. | `b02-…-song`, `e02-…-song`, `f03`, `f08` | A **Mix** view in the bottom dock: one channel strip per track (fader on `mixer({ db })`, pan, M, S, a meter), master strip with a peak/LUFS meter. M/S also in every track header. Mute and solo are overlays (MON-1–4) with a distinct "listening only" look (outlined M/S). "Muted in song" (MON-5) is a different look: the name struck through, a filled M. A muted kick shows its control export still feeding duck (MON-2: "the mixer shows this"). A track with no `db` in the mixer still gets a fader that writes one in (as left-out controls already do, PKG-3). | 8b mixer + mute/solo + muted option; meters need taps (gaps #20) | M + M |
| 2 | **Tracks can't be added, duplicated, deleted or reordered from the UI**, and the Empty template is a dead end. | `e11`, `e14`, `f02` | A "+" at the foot of the track list and ⌘T open the instrument picker (the effect browser's pattern: type, Enter). Track header context menu: Duplicate (⌘D), Delete (⌫, with undo), Rename, Colour. Drag headers to reorder; order and colour go in `layout.json` (FMT-19). | 8b add/duplicate/delete/rename track | M |
| 3 | **Automation is a dead end.** Automate makes a lane you can't draw on; automation clips ignore clicks; the lane is named `out-decay`. | `c07`, `c08`, `g03`, `g04` | Click an automation lane to add a point, drag points, ⌥-drag curves, a pencil while playing (WRITE-22a lane lease on control lanes). Name lanes `<track>-<control label>` ("kick-decay") with the label "Decay". Fold automation lanes under their track; show a parameter picker per lane. | 8b clip editing by id with lane leases (must include control lanes; gaps #14) | M |
| 4 | **Clip editing is move-only, snapped to whole beats.** No duplicate, copy/paste, resize, split, cross-track move, names or mute. | `b07`, `g05`, `g06` | Edges resize, ⌘D duplicates after itself, ⌘C/⌘V at the playhead, split at the playhead (⌘E once Export moves), ⌥-drag copies, a vertical drag moves to another track of the same lane kind. Snap follows the zoom (bar → beat → 1/16) with ⌘ to bypass. A clip name strip on top, and a "loop this clip" action. | 8b clip editing by id (clip ids already exist, FMT-12) | M |
| 5 | **Nothing can be heard while editing notes**: drawing, dragging, keyboard clicks. | `b08`, `e05`, `f09` | Audition as a `Live(Event)` source (MON-7): sound on note-down, drag across pitches, clicking the roll's keys; a speaker toggle in the roll header; a panic button. Later, the same path serves MIDI input. | 8b audition | M |
| 6 | **Tempo, time signature and key can't be set**, and they're shown in 11 px mono in the track column corner. | `b01`, `f06`, `e01` | A tempo field in the top bar: drag to scrub (RENDER-27a lease), type to set, tap tempo. Time signature and key fields beside it; a metronome toggle (K). Set them in the New Song dialog too. Remove the "4/4 · 124 BPM" corner label. | 8b tempo bar with its lease; key/scale is later (#16), metronome is gaps #19 | S + S |
| 7 | **Knobs aren't pro-grade**: about 30 px targets, a 160 px full throw (40 px went −6 → +12 dB), no fine drag, no type-in, no cursor readout, double-click is the only reset. | `c01`, `c02`, `f07`, `f08` | Throw scaled to the range (~400 px full), ⇧ for fine (×0.1), ⌘/⌥-click resets, double-click types a value (units parsed: "-3", "2k"), wheel nudges one step, a value bubble at the cursor during drags. Hit target ≥ 32 px with the whole row draggable. | UI | S |
| 8 | **The bottom panel is organised around code modules**, not the selection. Module tabs duplicate the track list and overflow at 16 tracks; Notes is a disabled tab until a double-click. | `b01`, `f02`, `c18` | The selected track or clip drives a dock with views Device, Notes, Mix, Code (Tab or ⌘1–4 cycle them). Selecting a notes clip shows Notes; selecting a track shows Device. Remove the module tabs; "Song" becomes the master row at the top or bottom of the track list. | UI | M |
| 9 | **The graph shows DSP internals and code locations as the instrument's face.** The clap circuit at 50%; `time.beats → lfo → xrange → blend`; `bass.ts:11` under every node. | `e02-…-clap`, `b02-…-bass`, `f07` | A **Device** view: the instrument card then its effect chain, left to right (the chords chain, `e02-…-chords`, already looks like this), each card showing its declared controls (descriptors carry ranges and labels). The full graph stays as a "Patch" toggle for sound design. File:line goes to hover and "Edit in code". | UI (descriptors exist, step 5) | M |
| 10 | **Feedback is written in code.** Toasts print diffs and type errors; undo says "Undid: song.ts 12 → -6". | `c03`, `c04`, `c13`, `c14`, `d02` | One toast per edit, in words, with Undo: "Hats volume −6 → +12 dB", "Added Reverb after Keys", "Can't connect a frequency (Hz) to Drive (dB)". The edit's descriptor knows the control's label and unit, so this isn't guesswork. Toasts: bottom-centre, one at a time, 4 s. | UI + editor messages (gaps #29) | S–M |
| 11 | **Cable drag offers targets it will refuse** (D3). | `d01`, `d02` | While dragging, dim the knobs whose unit doesn't match and say why on hover. Optionally offer "Connect through a range" (insert `xrange`) for a mismatched unit. | UI (+ editor for the mapping) | S |
| 12 | **Track order is alphabetical; colours are by index** and shift when tracks are added (D8). | `e01`, `f02`, `f04` | Read and write `layout.json` (FMT-19): order from the user (default: the mixer's order, else the import order), colour per track key, a picker in the header menu. | UI + session (layout.json is Presentation, no revision) | S |
| 13 | **No timeline zoom or track height**: a fixed 24 px/beat and 64 px rows; 6 of 16 tracks fit; automation lanes can't fold. | `f02`, `f04`, `b01` | ⌘=/⌘− and pinch for horizontal zoom (pinch already works in the graph), zoom to selection (Z), track height presets (S/M/L, ⌥-scroll), fold/unfold lanes per track, an overview strip under the ruler. | UI (gaps #17) | S |
| 14 | **Track headers hold labels, not controls.** "track", lane kinds and note counts; no M/S/arm/level/meter. | `f06`, `b01` | Header: colour bar, name (inline rename on double-click, wide field), M, S, a mini level slider and meter; lanes indented below with their label and a fold arrow. 44 px default row. | UI with 8b mixer | S |
| 15 | **The piano roll lacks pro tools**: no scale highlight, ghost notes, quantize, transpose, triplets or drum names; 16 px rows, no vertical overview; a click to focus adds a note (D4). | `b09`, `b11`, `e03`, `e15`, `f09` | Draw vs select modes (B), or require a double-click to add. Ghost notes of other tracks (a toggle). Scale and key highlight (once key exists). Quantize (Q), transpose (⌥↑↓ / ⇧⌥↑↓), triplet and swing snap. Drum lanes fold to used rows with names. Labelled keys every C, playable keys. | UI + 8b audition; scale is gaps #16, note tools #15 | M |
| 16 | **Export has no options** and no loudness report. | `d13`, `d14` | Keep one-click Export, plus an "Export…" sheet: range (song / loop / selection), stems per track (bounce exists in `m2`), WAV/AIFF/MP3, 16/24/32-bit float, normalize/peak, tail. Report peak and LUFS in the done toast; show the `overlays.active` warning (MON-4) once overlays exist. | later (gaps #36); the overlay warning is 8b | M |
| 17 | **Status messaging stacks, shifts the layout and repeats itself**: compile + attention + read-only banners, internal codes, CLI wording; "No tracks yet" on broken open (D2, D5). | `d05`, `d09`, `d10`, `d15`, `d16`, `g02` | One status line or popover in the top bar ("2 issues"), opening a panel that holds the fixes. A thin, non-shifting colour edge on the timeline when silent or read-only. Read-only greys out edit affordances (gaps "loose ends"). Codes go in a disclosure. | UI | S–M |
| 18 | **No recent songs**; the start screen is three buttons in an empty window. | `a01`, `a03` | A recent list (`~/.music2/recent.json`) with thumbnails, BPM and key, plus File → Open Recent. Enter opens the newest. Reopen the last song on launch (a setting). | later (gaps #12), but small: pull it into step 8 | S |
| 19 | **The visual system isn't a system yet**: gradients, five accent systems, ten type sizes, mono everywhere, double-muted track colours, playing state looks disabled, seven radii. | `f05`, `f06`, `b03`, `d09` | A token pass in `index.css`: one UI accent (no gradient), track colours muted once, red/amber for problems; type scale 11/12/13/15; mono only for code and literals; radii 6/10; play is lit when playing; ship the fonts locally (D10). | UI | S |
| 20 | **Keyboard coverage is thin, and ⌘E is spent on Export.** | `App.tsx`, `menu.rs` | A shortcut map, applied in the menu bar and the page: M/S on the selection, ⌘T new track, ⌘D everywhere, ⌘E split, ⌘L loop selection, ⌘=/⌘− zoom, K metronome, B draw mode, Tab/⌘1–4 dock views, Q quantize, ⌘K command palette; Export to ⇧⌘E. Transport and View menus list them. A "Keyboard shortcuts" sheet (⌘/). | UI + `menu.rs` | S |

### What step 8 should carry from this audit

- **8a** has no UI of its own, but its gate is "callback ≤ 70% of block budget". Expose a small DSP
  load meter in the top bar (every pro DAW has one), with the buffer size in a Settings sheet
  (gaps #31). It also makes the 64-frame run demonstrable in the app.
- **8b, in this order:**
  1. **mixer with mute/solo and the `muted` option** (rank 1, 14), shipped with the Mix dock view
     and track-header M/S;
  2. **add/duplicate/delete/rename track** (rank 2, 12), shipped with `layout.json` order and
     colour so the new track lands where the user wants it;
  3. **clip editing by id with lane leases** (rank 3, 4), covering control lanes so Automate stops
     being a dead end;
  4. **audition** (rank 5);
  5. **the tempo bar** (rank 6);
  6. automate-a-knob, which already exists: fix its lane naming (D6).
- **Each builder's toast and undo label** should be written in words from the descriptor (rank 10)
  as it lands, not retrofitted.
- **Fix D1 (stale diagnostics) before step 8b's track builders.** Adding a track will create files
  and switch modules, so the stale-model problem gets worse.

### A layout to aim for (1440x900)

```
┌ top bar 44px ─────────────────────────────────────────────────────────────────────────────┐
│ ▶ ■ ● ⟲ | 5.3.2  0:09.4 | ♩ 124  4/4  F min  ◷metro | (song name)        DSP 23%  ▮▮ -6 dB |
├ track list 220px ─────┬ arrangement (zoomable) ────────────────────────────────────────────┤
│ ▌Kick      M S ▬▬ ▮   │ ruler + sections                                                   │
│ ▌Clap      M S ▬▬ ▮   │ clips…                                                             │
│ ▌Bass ▸    M S ▬▬ ▮   │   (automation folded; ▸ shows lanes)                               │
│ …                     │                                                                    │
│ + Add track           │                                                                    │
│ ▌Song (master) ▬▬ ▮   │                                                                    │
├───────────────────────┴────────────────────────────────────────────────────────────────────┤
│ dock: [Device] [Notes] [Mix] [Code]  ← driven by the selection, Tab cycles                  │
│ Device: instrument card → effect cards → (+), 100% scale, top-left aligned; "Patch" toggle   │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
```

It stays light grey and muted, with every tool visible and no modes. It is simple because each
thing appears once (no module tabs, no duplicated status, no code in toasts), and the code stays one
keystroke away.

## Screenshot index

| Shot | What it shows |
|---|---|
| `a01`–`a03` | start screen, hover, examples menu |
| `a04`–`a06` | New Song dialog: Starter picked, Empty picked, a name typed |
| `a07` | new song from Starter, created (toast) |
| `a08` | New Song over an open song |
| `b01` | first-song default (Bass graph) |
| `b02-*` | first-song graphs: song (mixer), bass, hats, keys, kick |
| `b03` | playing |
| `b04` | loop button on |
| `b05`, `b06` | ruler drag to loop, loop set |
| `b07` | clip picked |
| `b08`–`b12` | piano roll: bass, select-all (with the accidental note, D4), keys, hats, tall panel |
| `b13`, `b14` | code view (hats, song) |
| `c01`–`c04` | knob hover, mid-drag, released toast, undo toast |
| `c05`, `c06` | knob menus |
| `c07`, `c08` | Automate: lane made and lit, after |
| `c09`–`c12` | effect browser: open, scrolled, "rev", no match |
| `c13`, `c14` | reverb added (diff toast), pluck → reverb → delay chain |
| `c15`–`c18` | rename in the timeline (field, typed, done), rename from the graph title |
| `c21` | graph zoomed in |
| `c22` | new empty clip opened in the roll |
| `d01`, `d02` | cable drag, refused |
| `d03`–`d08` | code: unsaved errors, saved broken, compile banner on the graph, knob menu while broken, after fix, changed on disk |
| `d09`–`d12` | Needs attention: one problem, two problems, roll read-only on a bad lane file, fixed (toast) |
| `d13`, `d14` | export progress, export done |
| `d15`, `d16` | read-only banner, refused edit |
| `e01`, `e02-*` | Starter timeline and graphs: song, chords, clap, hook, kick, hats |
| `e03`–`e06` | hook roll, note picked, note dragging, chords roll |
| `e07`, `e08` | clap code, autocomplete |
| `e09`, `e10` | Starter playing, scrolled |
| `e11`–`e13` | Empty: timeline, code (with stale problems, D1), graph |
| `e14`, `e15` | Empty after uncommenting the drums, first clip in the roll |
| `f01` | Empty code on a fresh load (no problems: confirms D1) |
| `f02`–`f04` | 16 tracks: timeline, mixer graph, scrolled |
| `f05`–`f09` | 2x crops: top bar, track headers, bass nodes, mixer node, roll header and keys |
| `f10` | lost connection |
| `g01` | With plugins refused |
| `g02` | project broken on open (D2) |
| `g03`, `g04` | automation clip clicked and double-clicked (nothing happens) |
| `g05`, `g06` | clip drag, dropped past the end |
