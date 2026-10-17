# e2e: browser suites for the music2 UI

Each suite drives the real web UI in headless Chrome against the **headless server**
(`crates/server/examples/headless.rs`), on a fresh copy of a fixture and a scratch `HOME`, and
prints one `PASS`/`FAIL` line per check. Nothing reads or writes the real `~/.music2` or `~/Music`.

## Running

```sh
(cd e2e && npm ci)                   # puppeteer-core 24.43.1 (no browser download)
cargo build -p m2-server --example headless && cargo build -p m2   # the server; the CLI for `export`
(cd web && npm run build)            # the server serves web/dist

M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs graph        # one suite
M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs start empty  # one suite, another fixture kind
M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs all          # every suite, each on a fresh server
CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs all
                                     # the same, with live3b's plugin part (Surge XT, Airwindows) included
```

- `--repo <checkout>` is where `examples/first-song` is copied from (every kind's `<root>/proj`). The
  suites that copy an example themselves get it as `E2E_REPO`: `live3b` copies
  `examples/with-plugins`, `fx-fixes` another `examples/first-song`. Default: this repo. It does
  **not** change what the server serves: `web/dist`, `std`, `templates/` and the projects behind
  "Open an Example" come from the source tree the headless binary was compiled in (its
  `CARGO_MANIFEST_DIR` is built in; `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` override the last
  three, and `run.mjs` passes your environment on). So `newsong` and `fx-fixes` test the binary's
  templates whatever `--repo` says. Use the checkout the binaries were built from; `run.mjs` prints a
  note when the binary wasn't built in `--repo`.
- `--out <dir>` keeps the logs (and, with `--keep`, the scratch folders) there; by default a new temp
  folder, printed at the end. The scratch folder of a run that wasn't `ok` is always kept.
- `all` prints one line per suite and a total. A run is `ok` only when the suite exited 0, printed no
  `FAIL` and no `SKIP` line, and passed exactly the number of checks `run.mjs` expects of it (the
  `expect` counts in its `SUITES`, the same as the table below). A run that passed but printed a
  `SKIP`, or passed another number of checks, is `PART`, and the total says why: part of it didn't
  run (live3b without `CLAP_PATH`), or the suite changed and its count in `run.mjs` and here must
  follow. Exit status: 0 when every run was `ok`, 1 when any failed, 3 when none failed but some
  were `PART`.
- `M2_CLI` is the `m2` binary for `export` (default: `<headless dir>/../m2`). `E2E_CHROME` overrides
  the browser (default: puppeteer's cached `chrome-headless-shell` 133.0.6943.141 for this OS). Off
  the Mac the suites start it through `chrome-as-mac.sh`, which gives pages a Mac user agent, so they
  test the Mac's keys on every OS (`chrome.mjs`). `E2E_TIMEOUT` (seconds, default 600) bounds a suite.
- Hosted CLAP plugins: only `live3b` gets `CLAP_PATH`, and only when you set it. There is no
  default, so your own plugin folder is never used unasked; `CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP`
  uses it. Its with-plugins part needs Surge XT and Airwindows Consolidated in that folder. Without
  them, live3b skips those 20 checks with one `SKIP` line and passes 40 of its 60, and `run.mjs`
  reports it `PART` (with a note at the start when `CLAP_PATH` isn't set). Every other suite runs
  **without** `CLAP_PATH`; `fx-fixes` must.

Fixture kinds (what `run.mjs` prepares in `<root>`, and the suite's arguments):

| kind     | server opens              | suite gets                               |
|----------|---------------------------|------------------------------------------|
| `proj`   | `examples/first-song`     | `<root>` (`<root>/proj`, `<root>/testhome`) |
| `cables` | `fixtures/cables` (one-file song) | `<root>` (`<root>/cables`)         |
| `fix`, `attn` | `examples/first-song` | `<root> <home> <project>`               |
| `empty`  | nothing (start screen)    | `<root> empty`                           |
| `none`   | nothing, scratch HOME     | `<root> <home>`                          |
| `export` | `examples/first-song`     | `<root> <home> <project> <m2 cli>`       |

`<root>` also holds `orig-song.ts`, `keys-orig.ts` (and `cables-orig.ts`): the fixture's files as
they started, which suites compare edits against.

## Byte-identity of renders

`baseline.sh` renders the pure fixtures, ten renders in all (full renders of `examples/first-song`,
`examples/resample`, `examples/sounds`, `templates/starter`, `templates/empty`, and `--from 4 --seconds 6`
of each) into a folder with a `SHA256SUMS`:

```sh
e2e/baseline.sh 933f4de /tmp/base-head          # builds m2 at that commit in a temporary worktree
e2e/baseline.sh --tree . /tmp/base-now          # the working tree as it is
diff /tmp/base-head/SHA256SUMS /tmp/base-now/SHA256SUMS
```

A commit from before the `sounds` merge has no `examples/sounds`: the script skips it and says so.

`renders.sha256` is the one record every OS must match (PLAT-4); `ci/check.sh` renders with
`baseline.sh` and checks against it (`cd <out> && shasum -a 256 -c <repo>/e2e/renders.sha256`).

## Suites

| suite | kind | checks at 933f4de | covers |
|-------|------|------------------:|--------|
| `graph` | proj | 19 | a header to select each module, the Song's pinned last; the Bass graph's nodes and edges; a pan's drags on its Mix strip (slow and fast) write `song.ts` and nothing else, with no toast; an outside edit shows; a broken save keeps the last good graph, with the status pill and "Show in code" in its popover; play and stop |
| `cables` | cables | 12 | dragging a cable from an output onto a knob writes the signal's name there (and makes no toast); ✕ writes the declared default back; a cable into its own declaration is refused with a reason |
| `undo` | cables | 10 | undo and redo of a cable from the toolbar and ⌘Z / ⇧⌘Z, with toasts that say where, never the diff |
| `effects` | proj | 12 | the catalog wraps the Keys' pluck in a delay (settings as knobs), said in words with Undo; removing it unwraps; undo restores the file exactly |
| `layout` | proj | 39 | the timeline above the dock: tracks and the Song's row, automation folded under its track, clips, selection both ways, the split's resize (180px at least), ruler seeks, the playhead, knob drags, playing past the end, Stop back where play started |
| `views` | proj | 30 | the dock's Device / Notes / Mix / Code switcher: its view is the producer's, a selection changes only what it shows, and it never moves; code follows the module; Notes shows the selected clip, a double-click opens it, Esc or a removed clip leaves its empty state |
| `code` | proj | 58 | the code editor: the files API's guards (409, 400), autocomplete, ⌘S, compile errors as markers and in the problems strip, files changed on disk under clean and edited buffers, knobs turned while a file is open |
| `notes` | proj | 52 | the piano roll and notes clips (a click selects, a double-click adds, D4); the roll's edits are one lane write, the timeline's clip edits one `clips` gesture, each checked on disk |
| `s8-audition` | fix | 51 | audition in Notes (design-8b §8.4, NT-2, NT-3): a press on a note sounds it at its own velocity while held (`live` 1, then 0) and lights its key in `press`, never the track's colour; a drag across three pitches sends three Ons and three Offs (the page's WebSocket frames); a held key sounds at 0.8 and a drag along the keys plays each key; the toggle (on by default, `accent-soft`, no focus, kept over a reload) sends nothing when off; what lets go of held notes: the toggle, ⌘., Space with no pointer down (not with one down: a key plays on through play and stop), a closed page within a second; a read-only roll still sounds; the hints: silenced by the Bass solo (and still only Bass's once soloed Hats is taken out of the song, as the pill counts one), muted, muted in the song, Nothing plays this lane (and gone once it plays again); a PC page's tooltip says Ctrl+.; on a server of its own that plays into a file, a key held while stopped is heard |
| `ports` | proj | 46 | node ports, port-to-port edges, cables dragged from an output circle, the effect browser, zoom and Fit |
| `s8-meters` | fix | 24 | meters (design-8b §10.4, taste.md §2.8, §3.4): a `role=meter` in every track header and on the Song row (the output), 3 × 28 px at x = 231 (x = 191 in a 1100 px window's 200 px headers), −60 to +6 dB, all at the floor at rest; 20 to 40 `meters` messages a second while playing, with every track, the output and the load; Kick's, Bass's and the Song's meters move, read through `aria-valuenow`; the Song meter's fill is `fg-muted` below −6 dB and `warn` above (canvas pixels); Kick muted stays at the floor while Bass moves; a second after the stop every meter reads the floor, the server's last messages are silence, then none; the peak hold's tick stays near the top, holds, then falls; the socket closing floors every meter, and they move again once it reopens; an answer for the song cut off on its way (as when the window reloads) leaves the window as it was, with no page error. Prints the real load and late callbacks while playing |
| `dsp` | proj | 33 | std's reverb, compressor and chorus added after the Keys through the effect browser, each with its settings as knobs, and exactly that in the file |
| `start` | proj / empty | 24 / 14 | the graph view, page keys, ⌘S, closing to the start screen, the API catch-all; `empty`: a server started with no project |
| `fixes-graph` | fix | 42 | Enter in the effect browser while playing, a Hz knob at its end, the canvas, feedback loops, one name at two inputs, std helpers as nodes, editor saves and undo, renames, unsaved files, diagnostics columns |
| `fixes-notes` | fix | 37 | the open clip follows its clip or closes; roll writes name their revision; the roll is read only while the code is broken; Enter in a field doesn't jump playback |
| `attention` | attn | 57 | data a key rename left behind: "Needs attention" in the status popover, its one-click move, undo, Delete; per-file picks from a "Move to…" menu; every graph event carries the diagnostics; knob undo after an insert in an earlier file |
| `newsong` | none | 71 | the start screen, New Song from the templates, Open an Example, names and numbering, ⌘N with a song open; the Tempo field (the template's, Tab to it, 5 BPM refused, 140 written into the new song's `data/arrangement.json`) |
| `export` | export | 37 | Export Audio: the button, progress, the done toast, the WAV in `~/Music/music2/Exports`, a second export's name, ⇧⌘E, refusals, and that it equals `m2 render` |
| `fx-fixes` | none | 75 | export progress stays with its song, export names, stale part files swept, reserved and nameless names, long names in the browser's bar and the Mac app's at 1100 to 1440 px, the engine's states in the status pill below every problem, an export's problems said plainly in its one toast, the plugins example's message (needs no `CLAP_PATH`), the Empty template's steps, the Starter's bass knob |
| `s4-clips` | fix | 28 | clips by id in the timeline and the piano roll (FMT-11, FMT-12) |
| `s8-clips` | fix | 94 | the timeline as an editor (8b.5): its grid chip (Auto, the menu, triplets), drags that land on the grid or, with ⌘ held, anywhere, ⌘1 to ⌘4 on the view last pointed at (the timeline's once Notes has gone: the dock hidden, or Mix in it), a click on empty lane space that writes nothing; clip gestures: each one `clips` edit and one undo step, either edge, ⌥-drag copies, a clip moved to another track with the bubble and the guide, lanes that can't take it, ⇧-click and ⌘-click, ⌫, a clip changed elsewhere refused in words; clip commands: the clip and lane menus, ⌘D (onto the next bar), a marquee whose ⌘D keeps its empty bars, ⌘E, ⌘J, ⌘C ⌘X ⌘V at the playhead and the copy events the app's menu runs, Q, ← →, ⌘E in the code editor; a PC page's words |
| `s8-automation` | fix | 45 | automation drawn in place (8b.5): Automate names the lane after the control ("Decay"); a double-click makes a clip holding the lane's value; on the curve a click adds a point and a drag moves it, ⌫ deletes it, Draw mode paints; a click on the body away from the curve picks the clip; Remove automation from the lane's header (the playhead inside the clip: what the curve plays there is written, not the fallback) and from the input it drives (the playhead before the clip: the fallback), Show lane too, with a toast and ⌘Z; a PC page's words |
| `pl-zoom` | fix | 62 | the timeline's view (polish B1 to B3): ⌘= and ⌘− zoom around the playhead, the ruler's Zoom in and Zoom out the same, ⌘-scroll and a Ctrl-wheel (Chrome's pinch) around the pointer, the page never zooms; Z fits the selected clip or the song, Z again goes back; at 24 px a beat Auto is 1/8, at 54 it is 1/16 and a dragged clip lands on sixteenths; the zoom is kept in `.music2/view.json` and back after a reopen; at 4 px a beat every 4th bar is numbered; Zoom out is off at 2; Follow pages to the playhead while playing, again past the view's right, pauses after a scroll by hand until play starts again, the button and ⇧⌘F turn it off and on, off is kept; track heights: a 6 px edge at the foot of a header, a 28 px drag makes Bass 72 px (its clip 64, Keys lower), M, S and the meter keep to the top 44 px, a double-click puts back 44, ⌘↓ and ⌘↑ on the selected track (28 px: centred, the meter 20 tall) or every track, an automation lane's edge to 64, both kept and back after a reopen; a PC page's tooltips; lint zoomed in and out, and tall |
| `s4-attn` | fix | 13 | "Needs attention" (the status pill's popover) in the diagnostic model: a label change keeps every address (ID-10); a key renamed by hand leaves data unclaimed and Relink plays it again (ID-17) |
| `s4-problems` | fix | 18 | a lane file that doesn't read keeps its last data with an Error naming the field (FMT-9); clips without ids are a Notice with a fix (FMT-12); a stray file under `data/` (FMT-7) |
| `s4-meter` | fix | 8 | bars by the time signatures: a 7/8 and a tempo change from bar 5, in the timeline and the roll (FMT-13) |
| `live3a` | fix | 49 | the live engine (step 3a): seeks and loops are transport jumps, notes and effects added while playing don't jump back, stop holds, the roll locks while its lane file doesn't read |
| `live3b` | proj | 60 (40 without plugins) | gesture leases (step 3b), on the Mix strips' faders and pans: drags stay live through slow compiles, outside saves, a held knob, a closed window, broken code and renumbered calls; with `CLAP_PATH` (Surge XT and Airwindows), plugin instances carried across a rebuild, heard in a captured stream, and an export that borrows them: playback paused, "Paused for export" in the top bar's status pill, and play going on from there (step 7) |
| `s5-knobs` | fix | 120 | descriptors (step 5): every declared control is a knob with its declared range and default (a mixer channel's `db` left out too; the mixer's levels and pans are drawn on their Mix strips); turning a left-out one inserts it, undo takes it out; the browser's categories and inserts; `import { delay as echo }`; the kick's fader written into its bare mixer entry as `db(…)` on its first move in the Mix view, and a channel's first pan through `/api/graph/mix/insert` (8b.3b) |
| `s6-builders` | fix | 56 | the first step-6 builders: rename a track's label in place, automate a knob from its menu, move a renamed track's whole folder from Needs attention in the status pill's popover; each one Tx and one undo step |
| `s6-write` | fix | 36 | the step-6 write path: an outside lane write taken in once it settles; a lane written in two halves never shows empty; a project another program holds opens read-only (said once, by the status pill; no edit offered) and writes after Retry; broken code opens in the code view, silent, its tracks shown greyed from `data/` (D2), and plays once a save compiles |
| `s6-fixes` | fix | 19 | step-6 review fixes: outside edits still taken in after a file that existed for one poll; a Tx naming a file the disk spells another way is refused; refusals answer 422 with diagnostics; a renamed folder holding a non-data file is offered file by file |
| `taste` | fix | 39 | the taste lint (taste.md §0): first that it catches a planted offence of each kind; then the font (taste.md §2.1): a Mac page draws its text in the system font and never loads Inter, a page given Linux loads Inter (its FontFace) and draws its text in it (as Chrome's DevTools protocol reports), with tabular figures; then one check per state the UI audit shot and the shell's own (the start screen, New Song, every module's device view, automation unfolded, playing, the loop, the roll, Notes with no clip, Mix, the code view, a fader drag, a toast, the knob menu, a knob's typed value marked unreadable, the effect browser, a cable drag, a browser search, the shortcuts sheet, + Track's instrument list, a track's header menu with its colours, the delete dialog, the status popover for broken code and for Needs attention, an export, read only and its popover, a 1100×700 window): no font size outside 11/12/13/15 px, no radius outside 0/2/4/8 px or a full round, no gradient, no backdrop filter, no monospace outside `[data-code]` |
| `gestures` | fix | 30 | the knob gestures (taste.md §5.2, KN-1) and how values read, on the Mix strips' pans and faders and the Bass's knobs: "−6.0 dB", "R 30"; 300px is a knob's whole range and ⇧ drags a tenth as far, without a jump when pressed mid-drag; a drag on its value turns it too; ⌥-click and ⌫ reset as one edit; a double-click (or Return) types a value ("-3", "L20", "2k"), unreadable text is marked and writes nothing, Escape writes nothing, Tab writes and moves to the next knob or fader; ↑, ⇧↑ and the wheel step it, a run of steps being one edit |
| `g11-play` | fix | 24 | play first, save beside it (PLANE-5): a piano-roll note is written; one the disk refuses (`.music2/tx/` read-only) is taken back with one message saying what wasn't saved, the clip shown as it was and nothing accepted, and an agent's Tx is answered `taken_back` and said in the app; stopped, a refused click's message doesn't say it played; once the disk takes writes, the next edit saves and undo goes back through the saved ones |
| `s8-tracks` | fix | 75 | tracks (design-8b §3): + Track and its instrument list (std's 22 sounds in three groups, Drums, Instruments and Sound design; typeahead; the code it writes, a member export such as `synth.pluck`), an add written byte for byte (its file, its import, its last channel, an empty lane, layout.json) below the selected track, selected and lit, in the first unused colour while every other keeps its own; Space plays at once; ⌘R renames only the name; ⌘T, Enter adds the last instrument again; duplicate copies the channel's duck and gives clips new ids; ⌘⌫ deletes at once and Undo restores every byte; deleting the kick asks, names both ducks and takes them off; a drag reorders and recolour writes only layout.json, with no revision; an outside edit of layout.json shows within a second; one that doesn't read keeps the defaults, + Track works, a reorder refuses in words, Reset layout fixes it; read-only refuses (423); the Empty template's Add a track adds a Kick drum, whose lane takes a clip |
| `s8-mute` | export | 152 | mute and solo, and the load (design-8b §6.6, §6.7, the 8b-ui plan's U1, U2, U3 and U11): M at x = 180 and S at 202 in every 240 px header and none on the Song row; M on Kick lights gray-700, the mutes pill says "1 muted", `.music2/overlays.json` follows within a second, and nothing else is written, no revision, no undo step; Kick's clips at 40%, its name muted; Space still plays after the click; M's tooltip and the pill's list say Kick still drives the Duck; S on Bass silences Hats and Keys (names fg-faint); ⌥-click solos only one; a drag across three Ms mutes all three; three quick clicks end as clicked; Return on a focused M mutes and unmutes and leaves the start point where it was; the M and S keys and ⌃M, ⌃S act on the selected track, never the Song row; the pill's list with each ×, Clear all, and no × on the pill; the design system's sizes; 200 px headers at 1100 px; mute in the song from the header menu writes `{ name: "Kick", muted: true }` in one revision, toasts "Kick muted in the song" with Undo, strikes the name through, lights M, hatches the clips and shows no pill; M then opens the menu with Unmute in song first (with Mute in song's icon, not audition's), the M key and a drag skip it, S lights but mute wins; Undo names it and puts the file back byte for byte ("Undid mute in song"), redo, then Unmute in song; the export's question with Kick muted (Esc cancels; "Mute Kick in the song, then export" writes the option in one revision, clears the listening mute, exports a WAV equal to `m2 render`'s and says "Kick stays muted in the song"; ⌘Z takes the option back; Export anyway gives `m2 render`'s WAV of the song as written and names Kick in its toast; from the keyboard, M reaches nothing behind the question, Tab goes round its two buttons, and Return or Space on a focused Export anyway exports with Kick's code untouched and the start point kept), the solo's form ("Only Bass is soloed", Return exports anyway), a mute and a solo together, and only Cancel and Export anyway, with the reason, while the code doesn't compile; while playing, through the page's own fetch wrapper: a load of 75% says "Heavy load" (warn) and its plain words with the sample rate, a dropout says "Sound dropped out" first, late callbacks count too, it stays 8 s and is gone by 10, neither shows once stopped, and a broken code ranks above both; a long name, "Loading …" and "2 muted · 1 solo" clear of each other at 1100, 1180 and 1280 px, in a page and the Mac app's bar; a reload keeps them; a read-only window mutes in memory only and the pill says "Not saved", its Mute in song row is off and the route answers 423; a PC page says Alt-click |
| `s8-songbar` | fix | 82 | the song bar (design-8b §4): the tempo field (a drag writes `data/arrangement.json` once, on release, with no toast; Shift-drag is fine; typing "128.55 bpm"; 1000 refused in words; no Alt-click reset; a two-decimal tempo fits; undo gives back the bytes); 7/8 from the picker re-bars the ruler; the ruler's right-click adds a tempo change at bar 5, typed at its mark, the field edits the one at the playhead, and the mark's popover removes it; the name renamed in place (music2.toml and the window's title follow, an empty one is refused, undo), and its field clear of the tempo and Undo in the Mac app's bar at 1100, 1180 and 1280 px; Space stops back where play started with no run-on of the playhead, Shift+Space continues; a click on empty lane space moves the playhead while stopped, not while playing; ⌘. calls panic; C and ⌘L; the loop kept on reopening; a tempo too fast for the loop turns it off and says so; a read-only window edits nothing |
| `s8-mix` | fix | 80 | the Mix view (design-8b §5.5, taste.md §3.5): ⌥3 shows a 76 px strip per track in track order, each in its colour, and Master (96 px, graphite) pinned right; a level the code leaves out reads 0.0 dB in grey and says why; 0 dB sits at 75% of the travel and −6 dB at 61%; a drag of Kick's fader is heard while held (a lease, the value bubble) and on release writes `kick: { in: kick.out, db: db(…) }` on the fader's law and nothing else, once, with no toast, named in Undo, and undo puts `kick: kick.out` back; Shift-drag moves a tenth as far; a double-click types −3 ("loud" is refused in words); Alt-click is 0 dB; the foot writes −96 and reads −∞; the wheel moves 0.1 dB a notch as one write and one undo, a sideways scroll doesn't move a fader, and in a narrow dock it scrolls the strips; ↑ steps a focused fader, and a notch below −60 dB is −∞; Tab goes pan, fader, next strip; Kick's first pan writes `pan: 0` only past 4 px, goes on as a lease, writes its value on release, and two undos give back `kick: kick.out`; the master's fader writes `master(…, { db: … })`; no level slider in a header, and "Shakers & Hats" fits one; a strip selects its track and Mix stays; its menu recolours, renames in place and opens song.ts at the channel's line; a track in no channel says so, with Edit in code; the Song's Device view draws the mixer as one compact card, "Mixer · 4 channels · Open Mix ⌥3", with no knobs and one cable from each channel's signal, the master's ceiling still a knob there, and Open Mix opens the Mix view (MX-6); a read-only window's faders and pans don't move |
| `pc-keys` | fix | 37 | the keys and labels on a PC (OS-54, OS-75): the one suite that runs entirely on a PC's page (a Linux user agent and `platform: "linux"`, on every OS); every other suite tests the Mac's keys, and s8-mute, s8-audition, s8-clips and s8-automation each also open one PC page to check their words. The top bar's tooltips (the song bar's tempo and Play too), the dock's tabs, a knob's, the track menu, the roll and the ⌥K sheet (with Shift+Space, Ctrl+L and Ctrl+.) say Ctrl, Alt, Shift, Enter and Backspace, with no Mac key symbol anywhere and no sheet row cut short; Ctrl+T opens + Track; Ctrl+R renames, and Ctrl+Backspace in its field deletes the word before and keeps the track; Ctrl+Z and Ctrl+Shift+Z undo and redo; Ctrl+Backspace deletes the selected track; Alt+1 to Alt+4 switch the views; in the code editor Ctrl+Backspace deletes a word, Ctrl+Z undoes, Ctrl+D adds the next match and Ctrl+S saves. The desktop app on Linux sends these chords through GTK's menu first, which no browser has: the build notes of taste's next part (A3) check that path on Xvfb |

Known failures at 933f4de: none confirmed. `node run.mjs all` passed every suite with the counts
above twice on the debug build, and every suite but one on the release build. That one, `s5-knobs`,
is intermittent: 3 of its 11 release runs failed, twice with the page gone right after "a
double-click on a knob at its default writes nothing" (puppeteer's "detached Frame" once, a hang
until the timeout once) and once with the first page load timing out. All three came while other
heavy jobs shared the machine; the other 8 release runs (4 of them at load averages of 18–25) and
every debug run passed 101/101. Watch it; it is not a reproduced app bug. Rerun with the current
`run.mjs` (debug build, `--repo` the 933f4de checkout, `CLAP_PATH` set): 25/25 `ok`, 901 of 901
checks, nothing skipped. Without `CLAP_PATH`, live3b is `PART` at 40 of 51, exit status 3.

`fixlib.mjs` is the shared setup of the `fix`/`attn` suites; `chrome.mjs` and `env.mjs` hold what
all of them get from `run.mjs`'s environment.

## Where these came from

The suites were written during steps 1–5 in a temporary folder that a reboot wiped; they were
rebuilt from the session transcripts by replaying every Write, Edit and file-touching shell command
in order (the recovery script and its log are not part of the repo). Ported here: paths come from
the arguments and the environment (`E2E_CHROME`, `E2E_REPO`, `E2E_HEADLESS`, set by `run.mjs`)
instead of the old scratch folders, and `chrome.mjs` names the browser for all of them.
