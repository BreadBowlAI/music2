# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground.

## Lane A

Audio in the app.

## Lane B

Record what you play.

## Lane C

MIDI files and stems.

## Lane D

Plain-language help. Branch `studio-d`.

### D1. A tooltip on every control

**In short**

| What | Now |
|---|---|
| Hold the mouse over a button, knob, fader, tab or switch | A short line says what it does, and its key |
| A track's header | "Kick: click to show it in the dock below · drag to move it · right-click for more" |
| M and S | "Mute while you listen (M)", "Solo: hear only the soloed tracks (S)" |
| A knob in Device | How to use it comes first. Where it is in your code comes last |
| The key's Major and Minor | "Major: a bright, happy sound", "Minor: a darker, sadder sound" |
| A file in Code | What the file is: "song.ts: the song's main file. It brings the tracks together" |

**What changed**

- `e2e/tiplib.mjs` (new; P0 hadn't landed, so D made it, as the plan's §7.1 gives D the file after P0):
  `tips(sel, keys)` runs in the page. It asks for a tooltip on an icon button, a toggle (`aria-pressed`), a tab, a
  switch, a slider, a knob or fader (`[data-knob]`), a spin button, and every control `keys` names (selector → its
  key as `chord()` spells it, which the tooltip must hold). A text button that says its action and has no key needs
  none ("Done"), nor a segmented choice whose words name it ("Minor") (D19). Menu and list rows and the code
  editor's insides are left out (Q43). A tooltip is the control's own `title`, or an ancestor's that holds no other
  control; a knob's may sit round its slider. A tooltip that only repeats the control's words is listed too.
- Titles, attribute-only hunks (§7.2):
  - `SongTimeline.tsx`: the track header (`[data-module-tab]`, a `role=tab`) says what a click, a drag and a
    right-click do; the Song row says what it shows.
  - `NoteView.tsx`: Length's − and + ("One bar shorter", "One bar longer").
  - `CodeView.tsx`: each file's row says what the file is, in place of its path again.
  - `KeyField.tsx`: Major and Minor say how they sound.
  - Reworded (jargon or the wrong thing first): `MuteSolo.tsx` M ("Mute while you listen (M)") and S ("Solo: hear
    only the soloed tracks (S) · ⌥-click: solo only this track"); `NodeGraph.tsx` a Device knob: "Drag (⇧ for fine)
    · double-click to type a value · ⌥-click to reset · right-click to automate it", then "In your code:
    tracks/bass.ts, line 10 (written as 0.5)" (it started with `tracks/bass.ts:10`).
- `e2e/st-tips.mjs` (new, kind `fix`, 44 checks), its `run.mjs` row after `taste` and its README row.

**Tests.** `st-tips`: the lint's own check (planted controls: an icon with no tooltip, a toggle with none, a
tooltip that repeats "Export", a key not named; a "Done" button and a menu's row pass), then one check per place:
the start screen, New Song, the top bar, the track headers, M's and S's words, the timeline (as it opens, Bass's
automation shown, a clip picked, a marker), the Repeat popover, a marker's name field, the side browser, the dock's
header, a Device knob's words, the Device view of every track and the song, a knob's menu, the effect browser, Notes
and its tools, a note's menu, Mix, Code and its file list, Change track ID, Save As, Settings, the ⌥K sheet, + Track's
list, a toast, the Delete dialog, the key's popover, the mutes and solos popover, the export question, the status
popover with broken code, the keyboard piano's card, a MIDI keyboard's mark (injected), Rename Song (an app page's
menu), a PC page's top bar and dock header (Ctrl+Shift+E, Ctrl+Alt+B, Alt+1), and the taste lint. 44 of 44.
Moved (values only, no count changed): `pc-keys` 37, `taste` 39, `s8-mute` 152, `s5-knobs` 120, `code` 58,
`pl-notes` 102, `pl-mix` 47, `s8-mix` 80, `s8-tracks` 75, `pl-ids` 39, `s8-audition` 51, `nx-effects` 47, all ok.

**Red first** (`dist-before`, the server at `85b069a`): 35 of 44, 9 failed. 13 controls had no tooltip or one that
only repeated its words, on 5 kinds of place; and the words checks:

```
FAIL  the track headers: every control says what it does (5: no tooltip at <div[role=tab][data-module-tab=kick]> "Kick M S"; …)
FAIL  M and S say what they do: Mute (M) | Solo (S) · ⌥-click: solo only this track
FAIL  a knob's tooltip leads with how to use it, then where it is in the code: tracks/bass.ts:10  (written as 0.5) / Drag (⇧ for fine) · …
FAIL  Notes, its tools and keys: every control says what it does (2: no tooltip at <button> "One bar shorter"; no tooltip at <button> "One bar longer")
FAIL  the Code view and its file list: every control says what it does (1: a tooltip that repeats its words ("song.ts") at <button[data-file=song.ts]> "song.ts")
```

The first ten without one: the five track headers (Kick, Hats, Bass, Keys, Song), Length's − and +, song.ts's row
(its path again). Most of the app had its tooltips already (157 titles in `components/` at `85b069a`).

**Guards.** The lint's own first check (it can't be red on the old page: it tests `tips`, which is new).

**Left.** Lanes A, B and C's places join `st-tips` at the End (the Record button and its menu, the take, the
Export dialog, the tempo question, the guide, the volume popover, the drop hint), as the plan says. The read-only
window's banner and a hidden lane's "Show" line aren't walked (text buttons that say their action).

**For gaps.md:** #11: every control has a tooltip in plain words with its key, and `e2e/tiplib.mjs` checks it.

**Contract:** taste.md §5.3 gains: "Every icon button, toggle, tab, switch, slider, knob and fader has a tooltip in
plain words, with its key when it has one; a text button that says its action needs none. `e2e/tiplib.mjs`' `tips`
checks it, and every new suite runs it on what it adds."

### D2. Errors in plain words

**In short**

| What | Now |
|---|---|
| The code has a mistake | The status popover says what is wrong in a plain sentence, where, and how to fix it: "**Cutoff wants a frequency (Hz), but gets a level (dB).** tracks/bass.ts, line 12. Write it in Hz, such as hz(800)." |
| A mistake made through std (a level as a filter's cutoff) | It points at **your** line, not inside std. Show in code opens it there, marked |
| A misspelt name | "`pluk` isn't known in tracks/keys.ts. Did you mean pluck? Import it: `import { pluck } from "std/instruments"`." |
| A plugin, a sound file or a data file that's missing | Needs attention says it plainly, with what to do |
| The exact words a programmer wants | Under **Details**, as before |
| A mistake no rule knows | Its own words, as before |

**What changed**

- `core/ir/src/lib.rs`: `Diagnostic.inner: Option<Box<String>>`, serialized only when set (boxed: an unboxed
  `Option<String>` made `Result<_, Diagnostic>` 144 bytes, over clippy's `result_large_err`).
- `core/session/src/lib.rs`: `build_code` maps `m2_ir::check`'s errors through `at_project_call` (new fn after
  `build_code`, D's block): a `code.check` error with a node, placed outside the project, gets `src` = the innermost
  call in the project among the node's frames (`Module::chain(node.frame)`, a site with `in_project`), as
  `file:line:col` with the column in characters from the site's first byte; its old `src` becomes `inner`. An error
  already in the project, or whose frames reach no project call, keeps its place (Q46).
- `core/script/src/lib.rs`: `#[allow(clippy::result_large_err)]` on `described` and `describe_here` (two lines):
  `m2_ir::Diagnostic` is 128 bytes with `inner` even boxed, and clippy's limit is 128.
- `crates/editor/src/explain.rs` (new, `pub mod explain;` after `rekey` in `lib.rs`): `explain(code, message, &Song,
  &At) -> Option<Plain { words, fix }>`. `Song::of(session)`: the track files (from the folder: a song whose code
  never compiled has none in its revision), what packages export (each `src/*.ts`'s `export const|function|let`,
  then the catalog's components), the tracks' names. `At`: file, line, column, that line's text, the module. Rules
  (D2's table): a unit mismatch, an empty input (the check's "is not connected" and the SDK's "the input in is
  missing"), mono and stereo, a name not known (its import, or the nearest export a slip away: Levenshtein ≤ 2), a
  parser message (9 prefixes of oxc's: "There is a typo on line N of file"), "not a function", an import of nothing
  (the project's `import.outside-code`, and a package module's "file not found" or "Error resolving module": the
  track files, or the package's modules), `pkg.undeclared`, `key.duplicate`, `plugin.missing`, `asset.missing`,
  `lane.invalid`. The unit words in one table. Anything else: `None`.
- `crates/server/src/files.rs`: `Problem` gains `plain`, `fix`, `inner` (each serialized only when set);
  `problem()` fills `inner`, made relative; `diagnostics()` explains each draft error from its place and its line's
  text (the column in characters, before the UTF-16 conversion). `plain_problems(session, problems)` (new): the
  revision's problems as JSON, each with `plain` and `fix` when a rule knows it, from `Song::named` (the tracks'
  names and files: it never reads the packages' sources, as it runs at every publish that has a problem).
- `crates/server/src/graph/actor.rs`: one line where the published `problems` are made: through `plain_problems`.
- Web: `lib/plain.tsx` (new; `.tsx`, not the plan's `.ts`, for `Words`): `plain`, `fix`, `inner` on `Problem` and
  `plain`, `fix` on `Diagnostic` by `declare module "./graph"`; `placeOf` ("tracks/bass.ts, line 12"), `Words`
  (backticks set as code), `technical` (message · code · inner).
  - `Status.tsx`' `Compile`: an item in plain words is the sentence in 600, the place unless the sentence names the
    file, the fix, and its own Details (message · code · inner); others say "file, line N: message". Show in code
    sets `codeAt` to the problem's line, so the Code view opens there (it opened the track's file at the top).
  - `Attention.tsx`: the row's message is the plain words, the fix under it (`problem-fix`); the original message
    first in Details.
  - `CodeView.tsx`' problem list: each row the place (`file:line`, as code), the plain words and the fix; its
    Details under the row (a `div` round the button, as a `details` can't sit in a button).
  - `codeWorkspace.ts`: the editor's marker says the plain words and the fix, then the original.
  - `TrackHeader.tsx` (title only): a track's issue mark says the plain words, the original in brackets.
- `e2e/st-errors.mjs` (new, kind `fix`, 34 checks), its `run.mjs` row after `attention`, its README row.

**Tests.**
- `crates/editor/src/explain_tests.rs`, each on first-song with a file broken and opened as the app opens it:
  `a_unit_mistake_says_what_it_wants`, `an_empty_input` (the SDK's and the check's), `mono_and_stereo`,
  `a_name_not_known_proposes_its_import` (pluk → pluck; pluck and `lfo` unimported; nothing near),
  `a_typo_names_its_line` (two of oxc's messages), `a_missing_file_lists_the_tracks` (a track file; a std module),
  `not_a_function`, `an_undeclared_package`, `two_tracks_with_one_id`, `a_missing_plugin`, `a_missing_asset`,
  `a_lane_file_that_doesnt_read`, `an_unknown_message_keeps_its_words`. 13 of 13.
- `core/session`: `a_check_error_points_at_the_call_in_the_project` (`tracks/bass.ts:6:10`, and through the song's
  own component in `lib/dark.ts`: `lib/dark.ts:6:17`; `inner` in `std/src/filter.ts`).
- `crates/server/src/files.rs`: `diagnostics_carry_plain_words_and_the_technical_text` (and the JSON; one no rule
  knows has none; a revision's `asset.missing` through `plain_problems`); `diagnostics_say_where_in_the_project_each_
  problem_is` moved: its graph error is now at `tracks/lead.ts` line 3, `inner` in `std/src/`.
- `cargo test` of `m2-ir`, `m2-script`, `m2-session`, `m2-editor`, `m2-server`: all ok; their clippy clean.
- `st-errors` 34 of 34. Moved (values only, counts kept): `code` 58 and `graph` 19 (the banner's place is now
  "line N of file" or "file, line N"), `s4-problems` 18 (a row's text includes its Details: `lane.invalid`'s field is
  there now). Also ok: `s4-attn` 13, `attention` 57, `s6-write` 36, `s6-fixes` 19, `taste` 39, `st-tips` 44,
  `start` 24.

**Red first.** The rules a stub (`explain` returning `None`, `at_project_call` the identity): 12 of the 13 explain
tests fail, `no plain words for …` each (`an_unknown_message_keeps_its_words` is a guard), e.g.

```
no plain words for code.check: input `cutoff` expects control<hz>, got control<db>
no plain words for import.outside-code: `./tracks/bas` names no code file of the project (looked for …)
no plain words for asset.missing: data/vox/takes.json: `vox.wav` isn't in assets/, so the clip at beat 0 is silent
```

The session test: `left: Some("std/src/filter.ts:26:40") right: Some("tracks/bass.ts:6:10")`. The files test:
`left: (Some("std/src/filter.ts"), Some(26), Some(40)) right: (Some("tracks/lead.ts"), Some(4), Some(36))`.
`st-errors` on `dist-before` and the server at `85b069a`: 1 of 34, then it stops (no plain item to read):

```
FAIL  the server places it on your own line (std/src/filter.ts:26, line 12 wanted) in plain words: undefined undefined
FAIL  the banner says it plainly, where, and the fix: std/src/filter.ts:26:40 input `cutoff` expects control<hz>, got control<db>
```

**Guards.** `an_unknown_message_keeps_its_words`.

**Left.** A missing plugin is named by its track, not its own name (an uninstalled plugin's name isn't known: only
its id). "Not a function" names the thing only when QuickJS's column lands on the call. A unit mismatch names the
control by its port (`cutoff` → "Cutoff"), not the descriptor's label. `m2 check` prints the new place (your line)
but not the plain words.

**For gaps.md:** #11: the common code and data problems are in plain words with a fix, and a mistake made through std
points at your own line.

**Contract:** DIAG-1 *Today*: plain words are made in `crates/editor/src/explain.rs` from a problem's code and message
(core's words unchanged); `m2_ir::Diagnostic.inner` holds the place inside a package when a check error's `src` is
the call in the project that made the node (`core/session`'s `build_code`).

### D3. Make your first song

**In short**

| What | Now |
|---|---|
| Help ▸ Make Your First Song | A card at the top of the side browser: seven steps to a first song. The browser opens for it |
| The start screen | "New to making music? Make your first song": New Song on Empty, then the card |
| Each step | Ticks itself as you do it: drums, a beat, a bass, bass notes, a listen, a sound changed, an export |
| The step to do now | In bold, with what to press under it. Done steps get a check |
| All seven | "You made a song. It is in your Exports folder." (Show in Finder in the app) |
| The chevron, × | One line, "Your first song · 3 of 7"; × closes it, and its ticks stay |
| Next time | The card and its ticks are kept with the song, on this computer |

**What changed**

- `crates/server/src/graph/view_file.rs`: `KEYS` gains `"guide"` (D's line). Test (`view_file_tests.rs`, at its
  end): `the_guide_is_kept_per_song`.
- `apps/music2-desktop/src/menu.rs`: `ITEMS` `("guide", "Make Your First Song", None)` after `shortcuts`; Help
  starts with it on both menu bars. Test at the module's end: `make_your_first_song_is_first_in_help`.
- `web/src/lib/guide.ts` (new): `STEPS` (D3's words, with D14's: "double-click"; keys by `chord()` and `opt()`),
  `seen(graph)` (a track playing a Drums-group instrument; 4 notes or more in its lanes; a track playing
  `std:synth-bass` or `std:synth-sub`; 3 notes or more), `openGuide`, `closeGuide`, `foldGuide`, `guideDid("sound")`,
  `useGuide`. Kept with `setLocal("guide", {open, folded, done})` (`folded` beside the plan's two). It ticks for as
  long as the page is open: the store's graph as it changes, the clock every 250 ms (Listen: played while the guide
  is open), and `on("export")` for this song's export (no edit to `exporting.ts`). A song opened with its guide open
  shows the browser.
- `web/src/components/Guide.tsx` (new): the card, 240 px (the browser's, less its 8 px margins), `raised`, 1 px
  `line`, `rounded-md`; the current step `fg` 600 with its hint, done `fg-muted` with a `CircleCheck` in `accent`, the
  rest `fg-faint`; the fold and × (tooltips); the done line; the foot, "Start from an empty song…".
- `Browser.tsx`: `<Guide />` first in the column. `StartScreen.tsx`: the link (and the New Song button calls
  `openNewSong()` with no event). `App.tsx`: `case "guide"` at the switch's end, and its import.
  `NewSongDialog.tsx` (not in the plan's list; nobody else's): `openNewSong({template, after})`, the dialog starting
  on that template and running `after` once the new song opens. `Knob.tsx`: `guideDid("sound")` after a knob's
  commit. `lib/instrument.ts` (where `changeInstrument` lives, not `Tracks.tsx`): `guideDid("sound")` after a change.
- `e2e/st-guide.mjs` (new, kind `none`, 36 checks), its `run.mjs` row after `start`'s `empty` row, its README row.
  `st-tips` gains the guide's card (an app page's Help ▸ Make Your First Song): 45.

**Tests.** `st-guide` 36 of 36: an Empty song through the API; on an app page, `__menu("guide")` shows the browser
and the card first in it, seven steps, none ticked, the first current with "Press ⌘T and pick Drum kit.", kept in
`view.json`; then a page does each step as a person does (⌘T Drum kit; a clip and four notes by double-click; ⌘T
Synth bass; three notes; Space; a Device knob dragged; ⇧⌘E, with the Export dialog's Return when lane C's is
there), each step ticking; the done line; `view.json` keeps seven; a reload keeps them; the fold ("Your first song ·
7 of 7", kept) and back; × closes, its ticks kept; the start screen's link opens New Song on Empty, then the new
song with the card, nothing ticked; the card never covers the timeline; a PC page says "Ctrl+T"; the taste and
tooltip lints on the card three times and on the start screen. Rust: `make_your_first_song_is_first_in_help`,
`the_guide_is_kept_per_song`; `cargo test -p music2-desktop` 18, its clippy and the mac lint clean; `m2-server`'s
clippy clean. Moved: `start` 24, `newsong` 71, `pl-projects` 66, `taste` 39, `s5-knobs` 120, `gestures` 30,
`s8-tracks` 75, `s8-mix` 80, `pl-sounds` 45: all ok (`pl-sounds` failed once in a batch beside another, at "one
revision, and song.ts as it was", which reads the revision right after the file; it passed twice alone).

**Red first.** `st-guide` on `dist-before` and the server at `85b069a`: 2 of 36, then it stops:

```
FAIL  Help ▸ Make Your First Song shows the browser, the guide at its top
FAIL  seven steps, none ticked, the first the current one, with what to do: undefined
```

`make_your_first_song_is_first_in_help` with Help's rows the other way round: `left: [Item("shortcuts"),
Item("guide")] right: [Item("guide"), Item("shortcuts")]`. `the_guide_is_kept_per_song` with `guide` out of `KEYS`:
`guide isn't a view setting` (the `set_view` unwrap).

**Left.** Show in Finder on the done line only for an export made while this window was open (after a reload the
line has no button). The guide's steps don't follow a song whose drums or bass are written in code by other names
(a custom component). The desktop app on `:97` (Help ▸ Make Your First Song) isn't checked by eye here: the menu row
is tested in `menu.rs`, and the End's pictures take it.

**For gaps.md:** #11: Help ▸ Make Your First Song, seven steps that tick themselves, in the side browser.

**Contract:** FMT-20's homes: `view.json` gains `guide: {open, folded, done}`. taste.md §5.3: the Help menu lists Make
Your First Song first, then Keyboard Shortcuts.

### D4. Music basics, up to date

**In short**

| Where | Now it says |
|---|---|
| The top | Help ▸ Make Your First Song, the start screen's link, and that every button says what it does |
| Notes | Double-click (or B) adds a note; play with ⌘K or a MIDI keyboard; Record (R) with a bar of clicks first |
| Notes or recordings | File ▸ Import Audio… (⇧⌘I) or a drag; the waveform; move, trim, fade, volume, backwards |
| Time | The tempo and time signature in the top bar; triplets (⌘3); Quantize, Humanize, Swing; the metronome (K) |
| Key | The Key field, and Notes shading the rows not in it |
| The parts of a song | + Track's sounds by their names; Change instrument… |
| Effects, mixing | The browser's drag and the ⋯ menu; M and S, Mute in song, the meters; Automate and drawing points |
| Stems | File ▸ Export… ▸ Stems |

**What changed.** `docs/music-basics.md` only: every "In music2" line says what the app does now, in the file's own
plain style; the lines that said "not in the app yet" for the key, mute and solo, drawing automation, triplets,
quantize and humanize, the tempo and the meters are gone. Code names stay, after the app's words.

**Tests.** None (a doc). Its links checked by hand: `gaps.md#recording-and-midi`, `#notes`, `#automation`,
`#mixing`, `#effects-and-plugins`.

**Left.** It describes lanes A, B and C's recording, audio import and stems as the plan's Decisions have them
(Q17 to Q19, Q7 to Q12, Q39 and Q42, D20, D34). The End checks those lines against what landed and trims any that
didn't (each lane's "If late").

**For gaps.md:** #11: [Music basics](music-basics.md) is up to date, and points at the guide.

**Contract:** none.

## Merge

The lead joins the lanes.

## End

The lead's last checks, docs and pictures.
