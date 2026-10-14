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
  the browser (default: puppeteer's cached `chrome-headless-shell` 133.0.6943.141). `E2E_TIMEOUT`
  (seconds, default 600) bounds a suite.
- Hosted CLAP plugins: only `live3b` gets `CLAP_PATH`, and only when you set it. There is no
  default, so your own plugin folder is never used unasked; `CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP`
  uses it. Its with-plugins part needs Surge XT and Airwindows Consolidated in that folder. Without
  them, live3b skips those 11 checks with one `SKIP` line and passes 40 of its 51, and `run.mjs`
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

`baseline.sh` renders the pure fixtures (full renders of `examples/first-song`, `examples/resample`,
`templates/starter`, `templates/empty`, and `--from 4 --seconds 6` of first-song, starter and
resample) into a folder with a `SHA256SUMS`:

```sh
e2e/baseline.sh 933f4de /tmp/base-head          # builds m2 at that commit in a temporary worktree
e2e/baseline.sh --tree . /tmp/base-now          # the working tree as it is
diff /tmp/base-head/SHA256SUMS /tmp/base-now/SHA256SUMS
```

## Suites

| suite | kind | checks at 933f4de | covers |
|-------|------|------------------:|--------|
| `graph` | proj | 19 | a header to select each module, the Song's pinned last; the Bass graph's nodes and edges; knob drags (slow and fast) write `song.ts` and nothing else, with no toast; an outside edit shows; a broken save keeps the last good graph, with the status pill and "Show in code" in its popover; play and stop |
| `cables` | cables | 12 | dragging a cable from an output onto a knob writes the signal's name there (and makes no toast); ✕ writes the declared default back; a cable into its own declaration is refused with a reason |
| `undo` | cables | 10 | undo and redo of a cable from the toolbar and ⌘Z / ⇧⌘Z, with toasts that say where, never the diff |
| `effects` | proj | 12 | the catalog wraps the Keys' pluck in a delay (settings as knobs), said in words with Undo; removing it unwraps; undo restores the file exactly |
| `layout` | proj | 38 | the timeline above the dock: tracks and the Song's row, automation folded under its track, clips, selection both ways, the split's resize (180px at least), ruler seeks, the playhead, knob drags, playing past the end |
| `views` | proj | 30 | the dock's Device / Notes / Mix / Code switcher: its view is the producer's, a selection changes only what it shows, and it never moves; code follows the module; Notes shows the selected clip, a double-click opens it, Esc or a removed clip leaves its empty state |
| `code` | proj | 58 | the code editor: the files API's guards (409, 400), autocomplete, ⌘S, compile errors as markers and in the problems strip, files changed on disk under clean and edited buffers, knobs turned while a file is open |
| `notes` | proj | 52 | the piano roll and notes clips (a click selects, a double-click adds, D4); every edit is one lane write, checked on disk |
| `ports` | proj | 46 | node ports, port-to-port edges, cables dragged from an output circle, the effect browser, zoom and Fit |
| `dsp` | proj | 33 | std's reverb, compressor and chorus added after the Keys through the effect browser, each with its settings as knobs, and exactly that in the file |
| `start` | proj / empty | 24 / 14 | the graph view, page keys, ⌘S, closing to the start screen, the API catch-all; `empty`: a server started with no project |
| `fixes-graph` | fix | 42 | Enter in the effect browser while playing, a Hz knob at its end, the canvas, feedback loops, one name at two inputs, std helpers as nodes, editor saves and undo, renames, unsaved files, diagnostics columns |
| `fixes-notes` | fix | 37 | the open clip follows its clip or closes; roll writes name their revision; the roll is read only while the code is broken; Enter in a field doesn't jump playback |
| `attention` | attn | 56 | data a key rename left behind: "Needs attention" in the status popover, its one-click move, undo, Delete; per-file picks from a "Move to…" menu; every graph event carries the diagnostics; knob undo after an insert in an earlier file |
| `newsong` | none | 65 | the start screen, New Song from the templates, Open an Example, names and numbering, ⌘N with a song open |
| `export` | export | 37 | Export Audio: the button, progress, the done toast, the WAV in `~/Music/music2/Exports`, a second export's name, ⇧⌘E, refusals, and that it equals `m2 render` |
| `fx-fixes` | none | 51 | export progress stays with its song, export names, stale part files swept, reserved and nameless names, long names, the plugins example's message (needs no `CLAP_PATH`), the Empty template's steps, the Starter's bass knob |
| `s4-clips` | fix | 28 | clips by id in the timeline and the piano roll (FMT-11, FMT-12) |
| `s4-attn` | fix | 13 | the "Needs attention" strip in the diagnostic model: a label change keeps every address (ID-10); a key renamed by hand leaves data unclaimed and Relink plays it again (ID-17) |
| `s4-problems` | fix | 18 | a lane file that doesn't read keeps its last data with an Error naming the field (FMT-9); clips without ids are a Notice with a fix (FMT-12); a stray file under `data/` (FMT-7) |
| `s4-meter` | fix | 8 | bars by the time signatures: a 7/8 and a tempo change from bar 5, in the timeline and the roll (FMT-13) |
| `live3a` | fix | 49 | the live engine (step 3a): seeks and loops are transport jumps, notes and effects added while playing don't jump back, stop holds, the roll locks while its lane file doesn't read |
| `live3b` | proj | 51 (40 without plugins) | gesture leases (step 3b): drags stay live through slow compiles, outside saves, a held knob, a closed window, broken code and renumbered calls; with `CLAP_PATH` (Surge XT and Airwindows), plugin instances carried across a rebuild, heard in a captured stream |
| `s5-knobs` | fix | 101 | descriptors (step 5): every declared control is a knob with its declared range and default; turning a left-out one inserts it, undo takes it out; the browser's categories and inserts; `import { delay as echo }` |
| `s6-builders` | fix | 55 | the first step-6 builders: rename a track's label in place, automate a knob from its menu, move a renamed track's whole folder from the Needs-attention strip; each one Tx and one undo step |
| `s6-write` | fix | 36 | the step-6 write path: an outside lane write taken in once it settles; a lane written in two halves never shows empty; a project another program holds opens read-only (said once, by the status pill; no edit offered) and writes after Retry; broken code opens in the code view, silent, its tracks shown greyed from `data/` (D2), and plays once a save compiles |
| `s6-fixes` | fix | 19 | step-6 review fixes: outside edits still taken in after a file that existed for one poll; a Tx naming a file the disk spells another way is refused; refusals answer 422 with diagnostics; a renamed folder holding a non-data file is offered file by file |
| `taste` | fix | 35 | the taste lint (taste.md §0): first that it catches a planted offence of each kind, then one check per state the UI audit shot and the shell's own (the start screen, New Song, every module's device view, automation unfolded, playing, the loop, the roll, Notes with no clip, Mix, the code view, a knob drag, a toast, the knob menu, a knob's typed value marked unreadable, the effect browser, a cable drag, a browser search, the shortcuts sheet, + Track's instrument list, a track's header menu with its colours, the delete dialog, the status popover for broken code and for Needs attention, an export, read only and its popover, a 1100×700 window): no font size outside 11/12/13/15 px, no radius outside 0/2/4/8 px or a full round, no gradient, no backdrop filter, no monospace outside `[data-code]` |
| `gestures` | fix | 30 | the knob gestures (taste.md §5.2, KN-1) and how values read: "−6.0 dB", "R 30"; 300px is a knob's whole range and ⇧ drags a tenth as far, without a jump when pressed mid-drag; its whole row drags it, its name too; ⌥-click and ⌫ reset as one edit; a double-click (or Return) types a value ("-3", "L20", "2k"), unreadable text is marked and writes nothing, Escape writes nothing, Tab writes and moves to the next knob; ↑, ⇧↑ and the wheel step it, a run of steps being one edit |
| `g11-play` | fix | 24 | play first, save beside it (PLANE-5): a piano-roll note is written; one the disk refuses (`.music2/tx/` read-only) is taken back with one message saying what wasn't saved, the clip shown as it was and nothing accepted, and an agent's Tx is answered `taken_back` and said in the app; stopped, a refused click's message doesn't say it played; once the disk takes writes, the next edit saves and undo goes back through the saved ones |
| `s8-tracks` | fix | 75 | tracks (design-8b §3): + Track and its instrument list (drums first, typeahead, the code it writes), an add written byte for byte (its file, its import, its last channel, an empty lane, layout.json) below the selected track, selected and lit, in the first unused colour while every other keeps its own; Space plays at once; ⌘R renames only the name; ⌘T, Enter adds the last instrument again; duplicate copies the channel's duck and gives clips new ids; ⌘⌫ deletes at once and Undo restores every byte; deleting the kick asks, names both ducks and takes them off; a drag reorders and recolour writes only layout.json, with no revision; an outside edit of layout.json shows within a second; one that doesn't read keeps the defaults, + Track works, a reorder refuses in words, Reset layout fixes it; read-only refuses (423); the Empty template's Add a track adds a Kick, bound as kickTrack, whose lane takes a clip |

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
