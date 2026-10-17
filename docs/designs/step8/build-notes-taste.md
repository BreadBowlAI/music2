# Taste's next part: build notes

What each slice of taste's next part did, and what its checks said. The plan is
`~/m2-runs/taste/plan.md`, with its Attack and the lead decisions T1 to T8, which override it. Each slice
adds its own section. All runs are on Linux (jerry-ax102), in the worktree `~/m2-runs/wt/tn`, branch
`taste-next`, made from `09b2dfd`: `main` (`cbac57c`, step 7) plus the `sounds` merge's slices 1 and 2.
The `sounds` merge's slices 3 (std 0.1 to 0.2) and 4 (docs) are not in it yet.

The owner asked for speed as well as quality. So each slice runs the tests of the crates it touched
and the browser suites it can affect. The full `ci/check.sh` runs at the end of step A (after A3) and
in the final phase.

## A1: the merge of `a0d5f91`

**In short**
- Taste's new look and 8b.1 Tracks are merged onto step 7 and `sounds`. The 10 conflicts were the ones
  the plan named, and each was resolved as it says.
- + Track now offers `sounds`' 22 instruments, in three groups: Drums, Instruments, and Sound design
  (the two with every knob). std's first five instruments leave the list (lead decision T3).
- "Paused for export" and "Loading …" are now states of the status pill, below every problem (T2).
- The Mac app's top bar is checked too: a long name ran over the tempo there. Fixed.
- An export's notes and its done toast are one toast now, so Show in Finder stays.
- The watcher looks at `layout.json` only when an event names it. An idle project is again watched
  with no stat.
- A new live test: adding and deleting a track keeps a ringing tail.
- Checks: every crate touched passes; clippy is clean; all 32 browser suites pass, live3b with the test
  plugins; 10 of 10 renders match. No render record moved.

### Slice 0

| What | Result |
|---|---|
| `git merge-tree --write-tree 09b2dfd a0d5f91` | 10 conflicts, the ones the task names: `delta.rs`, `delta_tests.rs`, `graph.rs`, `graph/tests.rs`, `architecture.md`, `gaps.md`, `e2e/README.md`, `fx-fixes.mjs`, `instruments.ts`, `GraphCanvas.tsx` |
| `sounds`' L3 (a `time` node per component call, placed under its frame chain) | In the base (`02aab27`), so break 1's test meets it |

### What was built

| Commit | What |
|---|---|
| `d5a4a1f` | The merge. Only the resolutions of plan §2.2: `delta.rs` keeps step 7's `scope(w, …)` and adds taste's `within`; both sides' tests and `Inner` fields kept; WRITE-17 is taste's text with the platform step's network-volume words; `gaps.md`'s top three from both; `instruments.ts` has taste's `category` and `sounds`' `description`; `GraphCanvas.tsx` is taste's. |
| `f98fdba` | Break 2 (T2, attack C, break 18). The engine's states in the status pill; the playhead holds still while an export holds playback; the Mac app's bar (below). fx-fixes: 75 checks. live3b reads the pill. |
| `ad3b1d9` | Break 3. An export's notes follow the file in its one done toast. |
| `cf3115b` | Break 4 (D7). attention waits until the page has the outside edit (its Undo names nothing) before a toast's Undo. 57 checks. |
| `c03ac4b` | Break 1 (T3, breaks 16 and 17). + Track and `sounds`' instruments (below). |
| `028b442` | Break 19 (and 8). `layout.json` watched by its events. |
| `90035ac`, `2b4c273` | Break 10. The live test of a ringing tail (the second commit only satisfies clippy). |
| `7d5cd3a` | README: a Mac, or Linux (Ubuntu 24.04), with OS-66's packages; the status line no longer says tracks are added in code. |
| `06b30bc` | `e2e/README.md`: live3b's and s8-tracks' rows. |
| `94ea671` | Break 14. views: a window starts on the top track. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| How + Track leaves out std's first five | The list holds an instrument only when it has a category. The five lose theirs. | PKG-7's text already said a category lists one. A project's own instrument needs a category to be listed now; PKG-7 says so. |
| How "the two every-knob ones last" is done | A third category, Sound design, which the list puts after Drums and Instruments | It uses PKG-7's free string and the list's existing rule (unknown categories after the two). No new SDK field. The design's two groups stay first. |
| Their labels | Synth becomes **Polysynth**, FM becomes **FM synth** | "Synth" and "Synth pad", "FM" and "FM keys" differ by one word. |
| "No two rows that differ by one word or letter" | Read as: no label is another with a word added or taken away, and no two differ by one letter | Those are the attack's pairs (Kick and Kick drum, Hat and Hats, Pad and Synth pad). "Synth pad" and "Synth pluck" stay: their second words differ. A test holds the rule. |
| The order inside a group | Where the package declares them | It gives T3's order without moving code in `sounds`' files: Drum kit, the single drums, FM metal; FM keys, FM bell, Supersaw, Lead, Synth pad, Synth pluck, 808 sub, Synth bass, Strings; FM synth, Polysynth. |
| The Mac app's bar | There the time shows from 1280 px, not 1180, and from 1280 px the name keeps 548 px clear, not 480 | The app's bar starts 80 px in. Measured: its left side ends at 496 px once the time shows, so a long name crossed the tempo at every width. With the time hidden it ends at 432. The bar styles itself from `data-app="mac"` on the root (`main.tsx`), so fx-fixes lays a page out as the app's, with no test hook. taste.md §3.8 says so. |
| fx-fixes' widths | 1100, 1180, 1280 and 1440 px, both bars | **The HANDOFF's "960" becomes 1100**: taste's smallest window is 1100 × 700, so no app window is 960 px wide. |
| The pill's popover | "Paused for export: the export is using the song's plugins. It plays on from here when the export is done." "Loading plugins: <names>. The song plays on meanwhile, and your change joins it once they're ready." | Plain words, one line each. A long plugin list is cut short in the pill; its title has it whole. |
| views' "for the selected module (Bass)" | Not a flake. Fixed in the suite. | Since 8b.1 tracks show in the mixer's order (D8), so a window starts on Kick. It failed every run. |

### Break 1: + Track and `sounds`' instruments

- `tracks::instruments` takes a plain export when there is one, else a member of an exported object.
  A track file then imports the object and calls the member:
  `import { synth } from "std/synth";` and `synth.pad(lane.notes("notes"))`.
- The 22 got a category in `std/drums.ts`, `fm.ts` and `synth.ts`. Each passes taste's SDK rule:
  std loads, and every test that loads it passes.
- Tests:
  - `every_listed_instrument_adds_confined_and_the_song_sounds_the_same`: all 22 added to first-song,
    each `Confined`, each playing from the lane its group says. Then a 2 s render compares equal by
    value to the one before (the attack's `f32 ==`). `synth.strings`, whose chorus reads `time`, is
    `Confined` with L3.
  - `the_instruments_are_stds_sounds_in_order`: the 22 in order, with groups, lanes and labels. It
    checks that the first five are gone and that no two labels are near.
  - The builder tests, the server's test, the CLI's tests and s8-tracks now use `sounds`' instruments.
    The plan said the server's 5-name test would list 27; with T3 it lists 22.
  - PKG-11 (break 17): every track file's `track(…)` call is in `forms.ts`, which tsc checks.

### Break 19: `layout.json` and WRITE-19

- Before, taste stat'd `layout.json` on every watcher tick, 20 a second, against WRITE-19's "no stat
  in between".
- Now the watcher looks at it when an event names it, or a tick walks everything, and then every
  tick until a change holds still. By walks alone, every tick, as before.
- `an_idle_project_is_watched_without_a_stat` now also checks that `layout.json` isn't stat'd, and
  that an outside edit of it still shows by its event, before the next walk.
- FMT-19's *Today* says so.

### Break 10: a ringing tail

`adding_and_deleting_a_track_keeps_a_ringing_tail` (`crates/server/src/graph/tests.rs`):
- A pad chord plays in the driven player. While its release rings, the test adds a track, then
  deletes the first channel's track. That makes the pad's gain new; it crossfades.
- After each, no step in the output is bigger than twice the tail's own. The tail is as loud as the
  same song's with no edit, within 1 dB.

Measured:

| Edit | Tail's own step | Largest step around the edit | After the edit |
|---|---|---|---|
| Add a track | 0.0129 | 0.0111 | the same level as with no edit, to the last digit |
| Delete the first channel | 0.0044 | 0.0057 | 0.00996 against 0.01003 with no edit |

### Checks

| Check | Result |
|---|---|
| `cargo test -p m2-session`, `-p m2-editor`, `-p m2-server`, `-p m2-script`, `-p m2`, after the merge | all pass: 151, 182 (+3), 171, 53 (+1, +7), and m2's 12 |
| The same after the breaks | all pass. m2-editor 183 (the new test), m2-server 172 (the new test) |
| `cargo clippy --workspace --all-targets -D warnings` | clean (after `2b4c273`) |
| The web build (`npm run build`: tsc, the SDK's check, the forms' tsc, vite) | passes |
| Every browser suite (`run.mjs all`, with the test plugins) | 31 of 32 at `2b4c273`, 1,211 of 1,212 checks; views failed (break 14). After `94ea671`, views 30 of 30 alone. live3b 60 of 60, its plugin part included (break 9): no selector needed fixing beyond the pill. |
| The suites each break touches, alone | fx-fixes 75/75, export 37/37, attention 57/57, s8-tracks 75/75 (twice), views 30/30 |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.json`, `std.wasm`, `std/m2pkg.toml`, `Cargo.lock` | unchanged since `09b2dfd`. `std/build.sh --check`: the same bytes. |

### Numbers (Linux, information only, PLAT-1)

| What | Here | Before |
|---|---|---|
| s8-tracks: an outside edit of `layout.json` shows | 87 ms, on inotify, before and after break 19 | 172 ms on taste, before the platform step |
| G11 `watcher`, idle (`m2-gates --server tx-data watcher`, release, load about 13) | 0.12% of a core: 2 walks in 10 s, 196 ticks that looked at nothing | 0.10% (the platform step on Linux) |
| G11 `watcher`, an outside write played | p50 161 ms, p95 239 ms in place; p50 166 ms, p95 204 ms as an atomic save | p50 159 ms, p95 213 ms; p50 166 ms, p95 173 ms |
| G11 `tx-data`, to the first block that plays it, p95 | 8.9 ms idle; 8.4 ms with another writer; 9.3 ms with 5,000 assets | 8.5 ms (platform step); 9.0 to 9.2 ms (step 7's check) |

Both G11 gates pass. The machine was shared (load 12 to 15), so the p95s say little. The full
`--gates` run comes at the end of step A.

### Not in this slice

| Break | Where |
|---|---|
| 5, 24: keys and labels per OS | A3 |
| 6, 7, 11, 12, 15, 20, 21, 22: the WIP song bar's fixes | B1 |
| 13: the mixer knobs in the Device view | B5 |
| 23: "loop turned off" during a hold | B2 |

### Left

- The full `ci/check.sh --gates`: at the end of step A. Nothing here ran the mac lint row; no
  macOS-only code changed.
- HANDOFF: its taste merge-notes rows are done now (WRITE-17, D7, the engine note, s8-tracks on
  inotify, the README). The engine note went into the pill (T2), not under the name. Its known-flake
  row for attention's toast Undo can go. Left for A-end's HANDOFF edit.
- The owner's screenshots (+ Track's list with the 22) come from the app on Xvfb at A-end.
- The Mac run judges the pill's place and the Mac app's bar for real. This slice only lays a page out
  as the app's.
- Scratch: `~/m2-runs/tn-scratch` (the suite runner `suite.sh`, suite logs, renders). Logs:
  `~/m2-runs/logs/tn-a1-*`.

## A2: fonts

**In short**
- Off the Mac, the app's UI is now in Inter. Before, Linux showed DejaVu Sans, a wide fallback.
- The Mac keeps its own font (SF). Nothing changes there.
- Inter ships with the app, in `web/public/fonts/`, with its licence (`OFL.txt`) beside it. Nothing
  is fetched from the network.
- The taste suite checks the font each page draws in: 39 checks, 4 new.
- The app on an invisible screen (Xvfb) draws its UI in Inter: a before and after picture below.
- Checks: the web build, taste 39/39, fx-fixes 75/75. No Rust changed. No render record moved.

### What was built

| File | What |
|---|---|
| `web/public/fonts/InterVariable.woff2` | Inter 4.1's variable font, from the rsms/inter release (`Inter-4.1.zip`, `web/InterVariable.woff2`), unchanged. 352,240 bytes, sha256 `693b77d4…429a8e3`. |
| `web/public/fonts/OFL.txt` | The release's `LICENSE.txt`: Inter's copyright line and the SIL Open Font License 1.1. It names no Reserved Font Name, so the file ships as it is. |
| `web/src/index.css` | An `@font-face` for "Inter Variable" (weights 100 to 900), with the version and sha256 in a comment. `--font-sans` is `"Inter Variable", sans-serif`. `:root[data-os="mac"]` keeps taste's `system-ui, -apple-system, "Helvetica Neue", sans-serif`. Mono is as it was (code only). |
| `web/src/main.tsx` | Sets `data-os="mac"` on the root on a Mac (`isMac`), before the first render. |
| `e2e/taste.mjs` | 4 checks (below). `run.mjs` and `e2e/README.md`: 35 becomes 39. |
| Docs | `taste.md` §2.1 (the font per OS, a table), its §2.15 token line and §0's lint list. Platform design OS-76: built. README: a Credits section (Inter, OFL). |

Vite copies `web/public/` into `web/dist/` as it is, so the font and its licence reach the app's
bundle (`tauri.conf.json` takes all of `web/dist/`). The attack's point: a licence in
`web/src/assets/` would never have reached it.

### The new checks in `taste.mjs`

| Check | How |
|---|---|
| A Mac page keeps the system font | `data-os` is `mac`; the body's font list starts with `system-ui`; Inter's FontFace was never loaded; and the text was drawn in a system font, not the page's own file. |
| A Linux page loads Inter | A second page, given `platform: "linux"`. Its FontFace "Inter Variable" has status `loaded` (T8). |
| A Linux page draws its text in Inter | Its font list starts with Inter, and Chrome's DevTools protocol (`CSS.getPlatformFontsForNode`) says the text was drawn in Inter, from the page's own file. The attack's point: `document.fonts.check()` says true for a font the page never loaded. |
| Inter's figures are tabular | "0000" and "1111" are both 32.00 px wide. Without `tabular-nums` they are 32.00 and 20.00, so the check can tell. |

Tried once with the font file taken out of `web/dist`: the three Linux checks fail (the FontFace
says `error`, the text is drawn in Liberation Sans, the figures differ), and the Mac check passes.

What the checks printed: the Mac page's "BPM" was drawn in DejaVu Sans (the system's, on this
machine); the Linux page's in Inter Variable (the page's own file). On the Mac run, the Mac page
should say the Mac's system font.

### The app on Linux

The worktree's `music2-desktop` (debug), opened on first-song on an invisible Xvfb screen (a scratch
HOME, `M2_AUDIO=none`), once with this slice's `web/dist` and once with the commit before's
(`MUSIC2_WEB_DIR`). WebKitGTK draws the UI in Inter: narrower letters than DejaVu Sans, Inter's
shapes. The GTK menu bar keeps the system's font (A3 rebuilds that menu). Pictures:
`~/m2-runs/tn-scratch/a2/` (`before/`, `after/`, and `cmp-list.png`, the browser's list enlarged,
before on top).

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| Which file | The release's `InterVariable.woff2` as it is, not cut down to Latin | The plan's "latin" is read as the upright face (no italic: taste uses none). Unchanged, its sha256 matches the release, so anyone can check it. It also has Greek and Cyrillic, for song names. 352 KB beside Monaco's several MB. |
| The family name | "Inter Variable", as the plan says | The release's own CSS calls it "InterVariable". Only our CSS and the check use the name. |
| `font-display` | `block` | The file is local, so it loads at once. `block` avoids a flash of DejaVu Sans as the window opens. |
| `data-os` off the Mac | Not set | Only the Mac needs a rule; every other OS takes the default, Inter. |
| The font's URL | `/fonts/InterVariable.woff2` | The app's server and the headless one both serve `web/dist` at the root. |

### Checks

| Check | Result |
|---|---|
| Web build (`npm run build`: tsc, the SDK's check, the forms' tsc, vite) | passes; `web/dist/fonts/` holds both files |
| `taste` | 39 of 39, twice; 36 of 39 with the font file taken out (the 3 Linux checks, as they should) |
| `fx-fixes` (the top bar's fit checks, as a guard) | 75 of 75 |
| Rust | nothing changed; `music2-desktop` rebuilt for the pictures |
| `renders.sha256`, `renders.json`, `std.wasm`, `Cargo.lock` | unchanged |

Every other browser suite gives its page `platform: "macos"`, so it keeps the system font as before:
this slice changes nothing they see.

### Left

- The full `ci/check.sh` at the end of step A. Its app row's `app-first-song.png` should show Inter, as
  the pictures above do.
- HANDOFF's merge-notes row "the font" is done now. Left for A-end's HANDOFF edit, with A1's rows.
- The Mac run: the app should look as it did before this slice (SF), since the Mac's WebKit page has
  `data-os="mac"`.
- Code (Monaco, `[data-code]`) on Linux stays in the system's monospace (DejaVu Sans Mono). The plan
  keeps mono as it is.

## A3: keys per OS

**In short**
- Off the Mac, every label now says the keys a PC has: Ctrl, Alt, Shift, Enter, Backspace. Before,
  Linux showed ⌥, ⇧ and ⌫ in many places (the ⌥K sheet, tooltips, menus).
- One helper spells every key (`web/src/lib/native.ts`). No label writes a key by hand any more.
- The menu bar is one per OS. On Linux: no app menu, Quit in File, About in Help, "Show in Folder".
  The Mac's is as it was.
- On Linux, Ctrl+Backspace in a text field deletes the word before, and keeps the track. Before, in
  the app, it deleted to the line's start (the Mac's ⌘⌫).
- A new browser suite, `pc-keys` (36 checks), runs as a PC on every OS, the Mac included (T8).
- The app on an invisible screen (Xvfb) passed the four key checks T8 adds: pictures below.
- Checks: the crates touched pass, clippy is clean, pc-keys 36/36, and the four suites nearest the
  change pass. No render record moved.

### What was built

| Where | What |
|---|---|
| `web/src/lib/native.ts` | `chord(key, { shift, alt })` (⇧⌘E or Ctrl+Shift+E), `opt` (⌥3 or Alt+3), `shifted` (⇧Return or Shift+Enter), `keyName` (⌫ or Backspace, Return or Enter), `mod` (⌘ ⇧ ⌥ or Ctrl Shift Alt, for "Shift-drag" and "Ctrl + scroll") and `revealLabel` (Show in Finder or Show in Folder). `kbd` and the sheet's own `chord` are gone. |
| Every label | The top bar, the dock's tabs, + Track, the track menu, a track's name, the start screen, the code view's Save, the roll, the graph's zoom, a knob's tooltip and the ⌥K sheet use the helper. |
| `App.tsx` | `deleteBack`: ⌘⌫ in a text field deletes to the line's start on a Mac and the word before elsewhere, as one typing step the field's undo takes back. The menu's Delete Track runs it in a field. Off the Mac the page also runs it for Ctrl+Backspace in a field, before the field sees the key (below). |
| `CodeView.tsx` | The menu's Ctrl+Backspace in the code editor runs Monaco's `deleteWordLeft` off the Mac (`deleteAllLeft` on a Mac, as before). |
| `apps/music2-desktop/src/menu.rs` | `layout(mac)`: the menu bar as data, one per OS, built by one loop. Tests check both layouts on any OS: every item once in each; the Mac's app menu and Window menu; off the Mac Quit ends File, About ends Help, and no row the system greys out or that does nothing. About off the Mac shows the version and the app's icon. |
| `e2e/pc-keys.mjs` | The new suite (below). `fixlib.mjs` `open(…, { pc: true })`: the real Chrome, a PC's user agent and platform (`chrome.mjs` `pc`), and `platform: "linux"`. `run.mjs` and `e2e/README.md` list it. |
| `templates/empty/song.ts`, `mix.rs` | Words every OS shares lose their Mac keys: the Empty template's comment says "+ Track" and "save" with no ⌘T and ⌘S; + Track's refusal says "Add it in the Code view." (it said ⌥4). A test holds the template to no ⌘. Same line count, so no code line moves. |
| Docs | README: the key table has a Linux column; Show in Folder named. taste.md §5.1 and §5.3: keys per OS, ⌃M and ⌃S Mac only, the menus per OS. PLAT-2 names the helper and the menus. Platform design: OS-54, OS-55 and OS-75 are Built. design-8b: the refusal's words. |

### The new suite: `pc-keys`

Its page has a PC's user agent and platform (set with `page.setUserAgent` before the page loads)
and `platform: "linux"`, on every OS. So Monaco takes a PC's keys even on the owner's Mac (the
attack's point G). 36 checks:

| Part | Checks |
|---|---|
| The page | Chrome says Linux; the page has no Mac class |
| Labels | the top bar's tooltips (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Shift+E, Ctrl+Alt+B, Enter); the dock's tabs (Alt+1 to Alt+4); a knob's ("Shift for fine", "Alt-click"); the track menu (Ctrl+R, Ctrl+D, Ctrl+Backspace, Alt+4); the roll's ("Backspace deletes · Ctrl+D duplicates"); 12 rows of the sheet; no sheet row cut short |
| No Mac symbol | in text, titles, labels and placeholders (outside code): the window, the sheet, + Track's list, the track menu, Notes, Code |
| Keys | Alt+K, Escape; Ctrl+T; Ctrl+R; Ctrl+Backspace twice in the rename field (a word each, the track kept); Enter; Ctrl+Z; Ctrl+Shift+Z; Ctrl+Backspace on Hats deletes it, Ctrl+Z brings it back; Alt+3, Alt+1, Alt+2, Alt+4 |
| Code editor | Ctrl+Backspace deletes a word; Ctrl+Z undoes it; Ctrl+D adds the next match (typing changes both); Ctrl+S saves the file |

Tried against the sheet before this slice: 3 checks fail (its Mac symbols, and a row cut short).

### The app on Linux (Xvfb): the GTK path

On Linux, GTK runs a menu's shortcut before the focused field sees the key, so in the app the page
gets Ctrl+Z, Ctrl+D and Ctrl+Backspace as menu commands even while typing. No browser has that
path. Checked by hand: the worktree's debug `music2-desktop` on first-song, on an invisible Xvfb
screen, a scratch HOME, `M2_AUDIO=none`; keys and clicks sent with XTest by `~/m2-runs/tools/xkey`
(new, outside the repo, as plan §4.5 says).

| Check (T8) | Result |
|---|---|
| Ctrl+Backspace in a rename field deletes a word and keeps the track | "Kick big drum" became "Kick big "; `tracks/kick.ts` and every other file unchanged |
| Ctrl+Z works in a type-in field | typing then Ctrl+Z put the field back ("Kick big drum"); nothing written |
| Ctrl+D in Monaco adds the next match | "// abc abc", the last abc selected, Ctrl+D, typing "xyz": "// xyz xyz" |
| Alt+3 shows Mix | yes (with the focus outside the code editor: see below) |
| Also | Ctrl+Backspace in Monaco deleted one word ("end"), not the line; Ctrl+S saved "// xyz xyz" to `tracks/kick.ts`; Alt+4 showed Code; Ctrl+Backspace on Hats deleted it and Ctrl+Z brought it back; Alt+K showed the sheet with every row whole; the menus as designed; About opens GTK's dialog with the version and icon |

Pictures: `~/m2-runs/tn-scratch/a3/` (`01-file.png` the File menu, `03-help.png` Help, `05-rename.png`,
`06-field-undo.png`, `08-code-word.png`, `09-code-ctrl-d.png`, `10-mix.png`, `12-sheet.png`,
`13-about.png`). The script is `a3/app.sh` and `a3/do.sh`.

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| Who deletes the word on Ctrl+Backspace in a field | The page (`deleteBack`), on every PC path: from GTK's menu in the app, and from the key in a browser, before the field sees it | In the app GTK takes the key first. In a browser, Chrome built for the Mac doesn't delete a word on Ctrl+Backspace, so pc-keys would fail on the owner's Mac. One function, tested by pc-keys on both. |
| Edit menu off the Mac | No Cut, Copy, Paste or Select All rows | muda's GTK rows only work with libxdo, which isn't built in: they did nothing. The web view has the keys itself (Ctrl+C, Ctrl+V). Nothing in a menu that does nothing. |
| Window menu off the Mac | None, so no Shift+Ctrl+W | GTK greys out Minimize and Maximize. The window's close button and Ctrl+Q quit. |
| The reveal label | "Show in Folder" in the menu and both toasts | The New Song toast said "Show Folder"; the plan names one label, as the export toast had it. |
| The sheet's width | 800 px, on every OS | At 720 px a PC's longer keys cut three names short in WebKitGTK ("Move notes by the grid / by …"). pc-keys now checks no row is cut. |
| Ctrl+Shift+E, not Shift+Ctrl+E | Our labels say Ctrl first | It is Windows' order and most apps'. GTK writes "Shift+Ctrl+E" in its own menu rows; that order is GTK's, not ours. |
| Small Mac changes | The sheet's "⌥click / ⌫" is "⌥-click / ⌫", as a knob's tooltip says; the graph's zoom tooltip "⌘ scroll" is "⌘ + scroll", as the roll's says | One spelling each. No suite reads them. |
| Words every OS shares | No key in them | The Empty template is the same file on a Mac and on Linux. |
| ⌥3 while typing in the code editor | Nothing happens, on every OS | Taste §5.3: the page's own keys never fire in a field. Not a Linux fault. |

### Checks

| Check | Result |
|---|---|
| `cargo test -p music2-desktop --bin music2-desktop` | 7 passed (4 new) |
| `cargo test -p m2-editor` | 183 passed |
| `cargo clippy -p music2-desktop -p m2-editor --all-targets -D warnings` | clean |
| The web build | passes |
| `pc-keys` | 36 of 36 |
| `taste`, `s8-tracks`, `newsong`, `code` (the sheet, the template, the toasts, Monaco) | 39, 75, 65 and 58: all pass |
| `renders.sha256`, `renders.json`, `std.wasm`, `Cargo.lock` | unchanged (no std or engine code changed) |

### Left

- Ctrl+Period (⌘. panic) comes with the WIP song bar (B1); check it on Linux then.
- B1 moves the WIP's own Mac strings to the helper (break 24: "(⇧ for fine)", the Play title's
  "(⇧Space)", the sheet's "⇧Space"); B2's pc-keys gains a song-bar tooltip.
- Windows (later): Redo is Ctrl+Y there (OS-54); the layout's "elsewhere" branch is GTK's today.
- The Mac run: the menu bar as before (the app menu, Window, Full Screen), and pc-keys passing on the
  Mac (its Chrome built for the Mac, with a PC's user agent).

## Step A's full check

**In short**
- `ci/check.sh` at `22c09cd` (A3's last commit), with the test plugins: **music2 check: PASS**. Every
  row passed the first time: nothing passed only on a retry, and "what failed" and the retry list are
  empty.
- Run in tmux (`~/m2-runs/tw check-tnA`), 12 minutes, on a quiet machine (load under 3 at the start).
  Logs: `~/m2-runs/check-tnA/` and `check-tnA.tgz`.

| Row | Result |
|---|---|
| toolchain, web build, build | PASS (rustc 1.99.0) |
| app (Xvfb) | PASS: the UI connected, first-song in 1 s, with-plugins in 1 s. `app-first-song.png` shows Inter and the Linux menu bar (File, Edit, Track, View, Transport, Help). |
| rust tests | PASS: 967 passed, 0 failed |
| clippy, std kernels, std.wasm | PASS; `std.wasm` rebuilt to the same bytes in another folder |
| mac lint | PASS: the macOS code (the Mac's menu bar included) type-checks and lints |
| browser suites | PASS: 33 of 33 runs, 1,252 of 1,252 checks, pc-keys 36 of 36, live3b with the plugins |
| renders | PASS: 10 of 10 match `e2e/renders.sha256` |
| repo unchanged | PASS |

Not run here: `--gates` (the timing gates). The task named the plain check for step A's end; the
gates run with the final phase.

## B1: the merge of `4804247`, the WIP song bar

**In short**
- Taste's unfinished song bar is merged: the tempo and time signature fields, the song's name to
  rename in place, the ruler's marks, Stop back where play started, Shift+Space to continue, loop
  the selection, Stop All Sound, and New Song's Tempo.
- 4 conflicts, each where step A changed the same lines. The lock file merged on its own.
- The plan's fixes, one commit each: the timeline says where tempo and meter live (`home`); Play has
  a test id; New Song checks the tempo first; Stop All Sound opens no audio device; the contract.
- The song bar's own words spell Shift per OS, and the name field keeps the Mac app's clearance.
- Checks: every crate touched passes, clippy is clean, all 33 browser suites pass (1,252 checks,
  live3b with the test plugins), and 10 of 10 renders match. No render record moved.

### What was built

| Commit | What |
|---|---|
| `13643f1` | The merge. Only the 4 resolutions (below). `Cargo.lock` merged on its own: one new edge, `m2-editor` to `toml` (break 20); `cargo check --locked` passes. |
| `c9b6f99` | Break 7. The actor published only `tempo`, `meter` and `end`, so `home` never reached the page. It now takes every field of the view by name, so a new field can't be left out again. The page's type has `home` always (no older servers). |
| `3745436` | Breaks 6, 15, 24. `data-testid="play"` on Play; graph, layout, live3a and live3b use it. Play's title and the tempo's tooltip say "Shift+Space" and "Shift for fine" off the Mac. |
| `9d03b71` | Break 11 (below). |
| `90c1b63` | Break 12. New Song checks the tempo before anything else, so a bad one makes no folder, `~/Music/music2` included. The test fails with the old order (tried). |
| `fcb22b1` | Break 22. Stop All Sound with no player does nothing; before, it started one, opening the audio device. Tested. |
| `5fe4987` | Break 21. Contract: the HOME table's `data/arrangement.json` row; HOME-1's list gains `arrangement`, with its *Today*; WRITE-18 lists `tempo`, `meter` and `title`. |
| `7ce07f4` | Attack C. The name's field, once double-clicked, now stops at the Mac app's clearance (548 px once the time shows), as the name does. |
| `7659b49` | layout read the playhead just after Stop, which now goes back where play started: it passed only by a race. It now judges the last beat it saw while playing, and checks that Stop goes back. 39 checks. |

### The conflicts

| File | Resolved |
|---|---|
| `apps/music2-desktop/src/menu.rs` | A3's menu per OS, with the WIP's items: Rename Song… in File; Continue, Loop Selection (Ctrl+L) and Stop All Sound (Ctrl+.) in Transport, on both OSes. |
| `web/src/components/TopBar.tsx` | A1's bar (the Mac app's 80 px, the time hidden sooner there, the name's 548 px) and A3's labels, with the WIP's fields and its `SongTitle`. |
| `web/src/components/Shortcuts.tsx` | A3's helper, with the WIP's rows: "Shift+Space" off the Mac. |
| `web/src/components/CodeView.tsx` | A3's word delete off the Mac, with the WIP's Monaco ⌘L (select the line) and ⌘. (quick fix). |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| The two test manifests (break 11) | Taken from the fixture's own `music2.toml`, with a comment or a literal name added, not written "0.2" | Here std is still 0.1 (the `sounds` merge's slice 3 isn't in), so "0.2" would fail now. From the fixture, they pass now and after the bump, with no edit. |
| The published timeline | Destructure `TimelineView` by name | The fault was a list of keys. Now a new field won't compile until it is published. |
| Stop All Sound with no player | Do nothing | There is no sound to stop. The engine's panic is tested in Rust; the route still answers `{}`. |
| WIP review (attack: "review it before B2 builds on it") | Read `view_file.rs`, the transport's `to`, the routes and `set_loop` against step 7. Nothing more to fix in B1. | `.music2/view.json` is written whole by rename, never a Tx; the watcher skips dot folders; a read-only window keeps the loop in memory; `/api/graph/panic` isn't a write, so a read-only window has it. Break 23 ("Loop turned off" during an export hold) is B2's. |

### The app on Linux (Xvfb)

A3 left Ctrl+Period for B1. The worktree's debug `music2-desktop` on first-song, on an invisible Xvfb
screen, a scratch HOME, `M2_AUDIO=none`, keys sent with `~/m2-runs/tools/xkey`:

| Check | Result |
|---|---|
| The Transport menu | Play / Stop, Continue, Go to Start, Loop, Loop Selection Ctrl+L, Stop All Sound Ctrl+. |
| Ctrl+L with a clip selected | loops it: bars 1 to 9, lit, and kept in `.music2/view.json` |
| Ctrl+L in the code editor | selects the line; the loop stays off |
| Ctrl+. in the code editor | Monaco's quick fix ("No code actions available") |
| Ctrl+. outside it | no error. What it does is not visible without sound. |
| Space, Space, Shift+Space | plays; stops back at 1.1.1; continues from where it stopped (3.2.2 after 1.2 s) |

Pictures: `~/m2-runs/tn-scratch/b1/` (`01-transport.png`, `02-loop-selection.png`,
`05-code-ctrl-l.png`, `06-code-ctrl-period.png`, `08-playing.png` to `11-stopped2.png`).

### Checks

| Check | Result |
|---|---|
| `cargo test -p m2-session`, `-p m2-editor`, `-p m2-server`, `-p m2`, `-p music2-desktop`, after the merge | all pass but the one the plan named: `the_published_timeline_is_the_sessions_and_shares_what_didnt_change` (no `home`) |
| The same at the slice's end | all pass: 151; 190 (the WIP's 7 song tests); 176 (the WIP's 4); m2's 13 (`apps/m2/tests/song.rs`); 7 |
| `cargo clippy --workspace --all-targets -D warnings` | clean |
| The web build | passes |
| Every browser suite (`run.mjs all`, with the test plugins) | 33 of 33, 1,252 of 1,252 checks, at `7ce07f4`. Then layout alone: 39 of 39. |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.sha256`, `renders.json`, `std.wasm`, `std/m2pkg.toml` | unchanged since `09b2dfd`. `Cargo.lock`: the one edge. |

### Numbers (Linux, information only)

fx-fixes measures the top bar with the WIP's fields, in a page and laid out as the Mac app's:

| Width | Left side ends (page) | Name starts | Left side ends (Mac app) | Name starts |
|---|---|---|---|---|
| 1100 | 391 | 480 | 459 | 480 |
| 1180 | 455 | 480 | 459 | 480 |
| 1280 | 455 | 480 | 523 | 548 |
| 1440 | 455 | 480 | 523 | 548 |

The attack's estimate for the Mac app was 523 px: that is what it measures. At least 21 px clear
everywhere. The fields have fixed widths, so the font changes little.

### Left

| What | Where |
|---|---|
| `s8-songbar`; newsong's Tempo check; pc-keys' song-bar tooltip | B2 |
| s8-songbar should also check the name's field in the Mac app's bar (1100, 1180, 1280 px, T8), and that a two-decimal tempo ("128.55 BPM") fits its 76 px field | B2 |
| Stop with `to` in one call: the route stops, then seeks. A status read in between could see it stopped where it was. B2's server test of Stop with `to` should say whether it matters. | B2 |
| Break 23: "Loop turned off" during an export hold | B2 |
| Empty lane space moves the start point (design §4.3) | B2 |
| WRITE-10's rows, `docs/gaps.md`, README (the song bar, the CLI's three commands) | B2 |
| When the `sounds` merge's slice 3 lands: the WIP added no manifest that names std's version | — |
| Scratch: `~/m2-runs/tn-scratch/b1/` (the app run), `renders-b1/`, `e2e-all-051142/`. Logs: `~/m2-runs/logs/tn-b1-*`. | — |

## B2: finish 8b.2, the song bar

**In short**
- The song bar is finished: a browser suite for it (`s8-songbar`, 68 checks), New Song's Tempo
  checked, the server tests the plan asked for, the contract, gaps and README.
- Space now stops back where play started at once. Before, the playhead ran on from the start
  point for up to 200 ms after a stop, then jumped back. The fix is in the player's status, not
  the engine.
- The playhead moves at the tempo the engine plays, so it no longer runs ahead and jumps back
  after a tempo edit, or while an export holds playback.
- "Loop turned off" is said during an export's hold too (attack break 23).
- Stop All Sound during an export stays silent after it: tested, and no engine change was needed.
- A click on empty lane space moves where play starts, while stopped.
- Checks: the crates touched pass, clippy is clean, the web builds, all 34 browser suites pass
  (1,328 checks, live3b with the test plugins), 10 of 10 renders match. No render record moved.

### What was built

| Commit | What |
|---|---|
| `355842a` | The player's status says a play or a stop just sent, as it says a seek, until the audio thread takes it (`core/play`). Stop with `to` is one server call, `Graph::stop_to`, which the route uses; it answers where it stopped. Tests: `a_play_or_stop_reports_its_state_until_taken`, `stop_with_to_goes_back_and_answers_where_it_stopped`. RENDER-29's *Today*. |
| `dd89cae` | `stop_all_sound_during_an_export_stays_silent_after_it`, with its control (an export with no panic gives the notes back). RENDER-10's *Today*. |
| `d7e2602` | The status (and the state) carry `bpm`, the tempo the engine plays at its beat; the page's clock moves the playhead at it. The tempo route judges the loop in the tempo map the edit made. Test: `during_an_export_a_tempo_edit_turns_the_loop_off_and_the_playhead_keeps_the_tempo_heard`. RENDER-9a's and RENDER-29's *Today*. |
| `1d87d7e` | A click on empty lane space, while stopped, moves the playhead there (below). taste §5.1 and §5.2, design-8b §4.3. |
| `149fdd1` | `e2e/s8-songbar.mjs` (68), in `run.mjs` and `e2e/README.md`. newsong's Tempo (65 to 71). pc-keys: the song bar's tooltips and the sheet's transport rows (36 to 37). |
| `c988572` | Contract: WRITE-10's rows and 8b.2's *Today*; RENDER-1's *Today* said the pause shows under the name (it is the pill since A1); `d7e2602` had doubled a sentence of RENDER-29's *Today*. `docs/gaps.md`: #6, #7, #8, #37, the loop lines, What works today, and the Space, Space loose end (T8). README: the song bar, the transport keys, `m2 tempo`, `meter`, `title`, the routes. |
| this commit | design-8b's "Loop turned off" words as built; these notes. |

### The new suite: `s8-songbar`

| Part | Checks |
|---|---|
| Tempo | the field and its tooltip (how, and "Saved in data/arrangement.json"); a drag shows its value and writes nothing until let go, then `arrangement.json` once, with no toast; Undo names it; Shift-drag is a tenth; a double-click types, 1000 is refused in words, Escape puts it back, "128.55 bpm" writes 128.55 and fits the 76 px field; no Alt-click reset; three undos give back the bytes |
| Time signature | the picker; 7 beats, each an eighth, is 7/8 and re-bars the ruler (bar 2 at 84 px); two undos |
| Ruler | right-click at bar 5 offers both changes; Add tempo change writes beat 16 and opens it to type; 140 typed; its mark; the field edits the change at the playhead ("Tempo from bar 5: 140 BPM"); the popover removes it; three undos |
| Title | a double-click opens it; an empty name is refused in words; Enter writes only `name` in `music2.toml`; the bar and the window's title follow, with no toast; undo; the field clear of the tempo and Undo in the Mac app's bar at 1100, 1180 and 1280 px |
| Transport | Space plays from where the ruler put the playhead; Space stops back there, the playhead sampled every frame for 300 ms never running on; Shift+Space continues; Space then stops back at the same start; a click on empty lane space moves the playhead while stopped, not while playing; ⌘. calls panic and playback goes on; C loops 4 bars and turns it off; ⌘L loops the selected clip; `.music2/view.json`; reopening keeps the loop |
| Loop off | a 0.012-beat loop, then a drag to 144 BPM: the toast says "Loop turned off: it's too short to play at 144 BPM", and the loop is off with its range kept |
| Read only | the tempo shows its value and why; a drag, a double-click, the picker, the name and the ruler's menu edit nothing |

Tried without the status fix: the run-on check fails (the playhead went 4.00, 4.04 … 4.35 in one
run, 4.02 then 4.00 in another), and so does the lane click right after a stop (the page still
thought it played). With it: 4.00 only, in every run.

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| The playhead ran on after Space (B1's "a status read in between") | Fix it in the player's status: a play or a stop sent reads as done until the audio thread takes it, as a seek already did | It mattered: a poll right after the stop saw "playing" at the start point, so the playhead ran on for up to 200 ms and the Play button stayed lit. Waiting in the route for the engine would have fixed only Stop with `to`. Play now lights at once too. This is the player's status (`core/play`), not engine work: the engine is unchanged. |
| Code that waited for "not playing" as a sign the engine stopped | One server test (`stopping_freezes_the_transport_and_the_engine_runs_on`) now waits for the beat to hold still | Nothing else read it that way: the other readers (a slow save's message, the toggle, a replaced engine) want what the user asked for, which is now what they get. |
| The tempo the playhead moves at | The status carries `bpm`, the tempo the engine plays at its beat (the server's timing map, updated when a send reaches the engine) | The attack's fix: a tempo edit plays from its switch, after the timeline says it, and only after an export's hold. One number per poll, as the clock used before. |
| "Loop turned off" during a hold | Judged in the tempo map the edit made | The engine plays that map from the switch. Setting a loop during a hold is still judged in the tempo the engine plays (written in RENDER-9a): rare, and the engine refuses it at the switch. |
| A click on empty lane space (plan 5.6.3) | Built while stopped: the playhead goes there, as a ruler click does. While playing, nothing. | Taste §5.1 says it moves the start point. While playing that would be a start point nobody sees (nothing marks one), or, as a jump, a click to deselect would make the song jump. Live does the same with its insert marker. A drag, a double-click's second click, or a press that began on a clip isn't it. |
| Stop All Sound during an export | No engine change | The test was green at once: `resume()` marks the lent plugins to play their held notes again, then the held panic clears those notes, so nothing restarts. |
| "Loop turned off" words | The WIP's, as the plan says; design-8b now says them too | The limit is two blocks or 5 ms, not a beat. |
| The Space, Space line in gaps.md (T8) | A loose end of its own | `sounds`' slice 4 adds its own line in What works today; a separate place keeps that merge free of conflicts. Join them when it lands. |

### Checks

| Check | Result |
|---|---|
| `cargo test -p m2-play` | all pass: 17 unit (1 new), and the integration tests that aren't recorded |
| `cargo test -p m2-server` | 179 passed (3 new; 1 changed to wait for the engine) |
| `cargo test -p m2-gates` | all pass (the gates read the player's status) |
| `cargo clippy --workspace --all-targets -D warnings` | clean |
| The web build | passes |
| Every browser suite (`run.mjs all`, with the test plugins) | 34 of 34, 1,328 of 1,328 checks, at `c988572` |
| `s8-songbar` alone | 68 of 68, five runs; newsong 71 of 71; pc-keys 37 of 37 |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.sha256`, `renders.json`, `std.wasm`, `std/m2pkg.toml` | unchanged since `09b2dfd`. `Cargo.lock`: B1's one edge. |

### Numbers (Linux, information only)

The name's field, open with a long name, in a page laid out as the Mac app's bar:

| Width | Left side ends | Field | Undo starts |
|---|---|---|---|
| 1100 | 459 | 480–640 | 944 |
| 1180 | 459 | 480–700 | 1024 |
| 1280 | 523 | 548–732 | 1124 |

### Left

| What | Where |
|---|---|
| The full `ci/check.sh` with `--gates`: the gates read the player's status, which now says a play or a stop at once. Their tests pass; the timed gates run in the final phase. | final phase |
| A review of 8b.2 (the WIP and this slice): the builders, the routes, read-only windows, the words | B-end |
| Screenshots of the top bar and the New Song dialog from the app on Xvfb | B-end |
| When `sounds`' slice 4 lands: join its "Going back to bar 1 … is a jump" line with B2's Space, Space loose end in `docs/gaps.md` | the merge |
| Setting a loop during an export's hold is judged in the tempo the engine plays, not the edit's | later, if it matters |
| Scratch: `~/m2-runs/tn-scratch/e2e-all-055337/`, `renders-b2/`. Logs: `~/m2-runs/logs/tn-b2-*`. | — |

## B3: 8b.3a, std's gains

**In short**
- Every mixer channel now has a gain, its fader: `db`, 0 dB when left out. The master has one
  too, before its limiter. At 0 dB a gain is `x × 1.0`, so no render moved: 10 of 10 match, and
  `renders.json` is unchanged.
- The master's input now goes through a new std kernel, `std:stereo`. It copies a mono signal
  into both channels, so the master is stereo from the start. When a mix that is mono all the
  way gets its first pan, the master's gain and limiter keep going instead of starting again
  (lead decision T5). No record moved, so T5's fallback (measure and accept) wasn't needed.
- A track's mixer channel is now found by following the graph, not by where its name is written
  (lead decision T7). So `kick: k`, with `const k = kick.out` above the mixer, is found too.
- Checks: the crates touched pass, clippy is clean, all 34 browser suites pass (1,333 checks,
  live3b with the test plugins), the recorded tests pass, and 10 of 10 renders match.

### What was built

| Commit | What |
|---|---|
| `9ae5521` | `std/src/mix.ts`: the mixer's `db` isn't optional, the master gets `db`, both doc comments say "−96 dB or lower is silence". `std:stereo` (kernel, `m2pkg.toml`, `std.wasm` rebuilt; `std/build.sh --check` gives the same bytes). Tests: `the_mixer_and_master_gains_change_no_bit`, `a_first_pan_in_a_mono_mix_keeps_the_masters_gain_and_limiter`; `std_defaults_are_todays` checks the three new faders; `std_calls_keep_their_forms`, the mixer's view and the gate-5 knob test follow; `forms.ts` has the master's two new forms (tsc passes). s5-knobs (101 to 106). Contract PKG-5. README's std line, gaps #4, design-8b §5.1. |
| `57050c3` | `mix::mixer` follows the graph (ID-18). `mix_tests.rs`, 6 tests. Duplicate refuses a channel that doesn't name its track. Contract ID-18 and PKG-13's *Today*; design-8b §2.3. |
| this commit | These notes. |

### How the tests know

| Test | What it shows |
|---|---|
| `the_mixer_and_master_gains_change_no_bit` | Each of the 5 pure fixtures, rendered whole through today's mixer and master, and through a copy of the old ones in the project (`old-mix.ts`). Every frame is the same, bit for bit. Today's graph has more gains and one `std:stereo`. Tried with the master's default at 0.0001 dB: it fails at frame 1. |
| `a_first_pan_in_a_mono_mix_keeps_the_masters_gain_and_limiter` | Two mono tracks, loud enough that the limiter always works. `pan: 0` written by hand, as 8b.3b's first pan will. Only the new panner, channel a's gain and the sum are new instances; `std:stereo`, the master's gain and the limiter are kept. Tried without `std:stereo`: the limiter and the master's gain are new. |
| `mix_tests.rs` | first-song as it is; a channel given a binding; two tracks in one channel; one track in two (no channel); an audio sidechain into a project's own component (tried without the sidechain rule: fails); a strip driven by signals; a bus track. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| How to make the master stereo from the start (T5) | A new kernel, `std:stereo` (`in: audio<2>`, a copy), first in `master` | Every existing way is off by a little: a centred panner on mono is `x × 0.99999994`; `widen` turns −0.0 into +0.0. The runtime already copies a mono signal into both channels of a stereo port, so the copy is exact. A kernel isn't engine work. |
| Where `std:stereo` sits | At the master's input, not the mixer's output | Only std's own master changes channels. A user's effect between the mixer and the master (a plugin, a compressor) gets what the mixer makes, as before: a mono-input plugin given stereo could sound different. |
| A channel's left-out `db` in B3 | A read-only value on the mixer's box: "Plays its default: it is a dictionary entry's, which is written with its entry." | Writing it into its entry is 8b.3b (`Insertion::plan`). The master's `db` is a knob now: it goes into the options object, as any left-out control does. |
| s5-knobs | +1 knob (the master's db), +1 check that the two read-only faders play 0 dB and say why: 106 | The attack expected 3 new knobs. Two of them are read only until B4. |
| How the graph names a channel (T7) | The first node inside the mixer reads its channel's `pan` or `db` constant (`channels.kick.db`). A panner whose `pan` is a signal passes the walk on to the gain after it. | Design §14's ID-18 row. It needs no new data: the flat graph has the edges and kinds, the song's IR the frames and sidechains. |
| A strip whose `db` is a signal, with no panner | The read's place in the property says the channel | It reads no channel constant. Rare (a fader driven by an LFO), and the old rule found it. |
| A bus track (`drums` reads kick and hats) | Only the song's own nodes are walked: the bus reaches the mixer as itself; kick and hats have no channel | Otherwise one strip would read "Drums + Kick + Snare", and deleting Kick would say it shares a channel. Same as the old rule. |
| Builders and a channel that doesn't name its track (`kick: k`) | Duplicate refuses: "Kick's mixer channel doesn't name it, so duplicate it in code." Delete lists `const k = kick.out` as code it can't take out. | The builders edit the channel's text. A copy of `kick: k` would play the kick, not the copy. Nothing is written in either case (tested). |
| A std version bump | None | No render changed, and 0.2 isn't released (plan §5.7). The `sounds` merge's slice 3 bumps std to 0.2. |

### Checks

| Check | Result |
|---|---|
| `cargo test` of `m2-session`, `-editor`, `-server`, `-script`, `m2`, `-play`, `-gates`, `-runtime`; std's kernels | all pass: session 152, editor 196 (6 new), server 180 (1 new), script std 7, runtime 130, play's unrecorded tests, gates 33 + harness 6, kernels 110 |
| Commit `9ae5521` alone (the graph rule stashed) | editor 190, server 180, m2, script, session's gate tests: all pass; clippy clean |
| The recorded tests (`m2-play --release`, `bit_identity`, `live`, `churn`, `--ignored`) | 5 of 5 pass, in tmux: `live_is_the_export_on_all_of_p16`, `…_on_p50`, `a_no_op_activation_on_p50`, `p16_churns_200_edits_within_20_mb`, `p16_loops_ten_minutes_without_drift` |
| `cargo clippy --workspace --all-targets -D warnings`; std's kernels | clean |
| Every browser suite (`run.mjs all`, with the test plugins) | 34 of 34, 1,333 of 1,333 checks |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.sha256`, `renders.json` | unchanged. `std.wasm` changed (the new kernel); `std/build.sh --check` rebuilds it to the same bytes. |

### Numbers (Linux, information only)

| What | Value |
|---|---|
| Kernels each fixture gains | first-song 4 (Kick's and Bass's gains, the master's, `std:stereo`); resample 3; Starter 3; Empty 2; `examples/sounds` 2 (its channels all write `db`) |
| p16 churn | 283 activations: 184.3 MB after 70 edits, then 178.1 MB |
| p16, ten minutes of a loop | 79 laps, 504 Ons each, rendered in 122.3 s (release) |
| `bit_identity`'s three | 417.7 s together, under other load |

### Left

| What | Where |
|---|---|
| A channel's left-out `db` becomes a knob: `Insertion::plan` writes into dictionary entries; the read-only words go | B4 |
| Key each strip by its channel (ID-14's std follow-up, T4) | B4 |
| The first pan in an all-mono mix still makes the sum's adds new: stateless, so a crossfade of equal signals. B4's live test should hear it. | B4 |
| A user's own stateful effect between the mixer and the master (`master(compress(mixer(…)))`) in a mix that is mono all the way still starts again on the first pan | later, if it matters |
| The timed gates with the new nodes (p16 and p50 gain the master's gain and `std:stereo`) | final phase |
| When the `sounds` merge's slice 3 lands: rebuild `std.wasm` (`std/build.sh`, then `--check`), never take a side | the merge |
| Scratch: `~/m2-runs/tn-scratch/e2e-all-062703/`, `renders-b3/`. Logs: `~/m2-runs/logs/tn-b3-*`. | — |

## B4: 8b.3b, the builders

**In short**
- A channel's fader that the channel leaves out is now a knob. Its first move writes `db` into the
  channel's entry: `kick: kick.out` becomes `kick: { in: kick.out, db: db(-3) }`. One undo step.
- The value follows the mixer's style: `db(-3)` where other channels write `db(…)`, else `-3`.
- A channel's first pan writes `pan: 0` into its entry (`mix::insert`, `POST /api/graph/mix/insert`).
  The answer names the new pan knob, which the Mix view's drag will lease (B5).
- The graph state carries `mix`: a strip per track, in track order, with knob ids, and the master.
- The first pan is heard as nothing: live, the song plays within about 1e-6 of the same song with
  no edit, on first-song and in a mix that is mono all the way.
- Checks: the crates touched pass, clippy is clean, 15 browser suites pass (live3b with the test
  plugins), and 10 of 10 renders match. No render record moved.

### What was built

| Commit | What |
|---|---|
| `c01b2c9` | `Insertion::plan` learns dictionary entries (`lens.rs`): a property in an object entry, or a wrap of a bare one that keeps its text, in the dictionary's style. Edits are a list now (`Control::planned`), so a cable and an automation lane go into an entry too. History shows the entry. `mix::view` and `mix::insert` with the by-channel assertion (`mix.rs`). A shorthand channel (`{ saws }`) reads as a property. Tests: 13 in `mix_tests.rs`; `forms.ts` gains 28 forms (a fader and a cable into each of 7 entries, a first pan into each, and the `db(…)` style), and tsc passes. s5-knobs: the two faders are knobs (117). Contract PKG-3, ID-14, WRITE-10's fader row; design-8b §5.2. |
| `8da57a9` | `mix` in the graph state; the route, in `WRITES`. Server tests: the state and the route; a read-only window writes nothing; the two live first-pan tests. s5-knobs: the route (120). Contract WRITE-10; design-8b §5.3, §5.4. |
| this commit | These notes. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| Key each strip by its channel (T4's first choice) | Not done; the first pan asserts by channel | Tried: with `key()` in std's mixer, two mixers in one module with a channel key in common are `key.duplicate`. `key()`'s keys are unique per key frame, not per component call. T4 says to fall back then. |
| The by-channel assertion | Every channel keeps its tracks and its controls' values and their readers; this one gains only the pan, read by its new panner; the graph gains that constant and that panner, net; no value changes | The attack's three points, plus a net node count by kind, so a filter slipped in beside the pan is refused (tested). |
| The style rule's reach | Dictionary entries only | Design §5.2's rule is about the mixer object. Options objects keep their plain numbers, so no existing insertion changes. |
| The value's spelling | Only unit calls in base units (`db`, not `ms`), imported under their own name | Then the number written is the one played, with no scale. |
| History's words | The entry, not the call: `kick: kick.out` → `kick: { in: kick.out, db: db(-3) }`; past 60 characters its old value is `…` (`bass: …` → `bass: { in: …, db: db(-3) }`) | The mixer call is long; the entry is what changed. Same 60-character rule as a call. |
| `mix::insert`'s revision | None: the channel is named by its key | The builder plans on the accepted revision, as the track builders do. The route ignores a `revision`. |
| The Mix view's JSON | `modules` (a list) for `module`; a strip for a channel no track reaches, named by its key; `mixer` can be `unwritten`; `message` gives the words; `feeds` and `muted_in_song` wait for 8b.4 | A shared channel ("Kick + Snare") has several tracks; 8b.4 is the slice that uses the other two. Design §5.4 says so. |
| The mono-mix live test | Two tracks written in code with std's first `bass` and `kick` | Every instrument + Track offers is stereo since T3, so + Track can't make a mix that is mono all the way any more. |
| The bound for "inaudible" | 1e-5 (−100 dBFS) against the same song with no edit, sample for sample | Stronger than "no step bigger than twice Kick's own": any cut or click is far above it. Measured: offline 1.1e-6 (first-song's limiter carries the panner's 1e-7 on), live 8.3e-7 and 1.0e-6. The design's 1e-6 was an estimate. |
| An entry the first pan can't take | Refused in words: "Can't add a pan to Hats: its entry names it already."; with no single mixer, the Mix view's own message | The Mix view offers the first pan only where `pan_insert` is true. |

### Checks

| Check | Result |
|---|---|
| `cargo test` of `m2-editor`, `-server`, `m2`, `-gates` | all pass: editor 209 (13 new), server 183 (3 new), m2's, gates 33 + harness 6. The harness's `the_live_engine_passes_every_gate` failed once while other test binaries ran beside it, then passed run alone (load, as in B3). |
| The first commit alone (server changes stashed) | editor 209, server, m2: pass; clippy clean; s5-knobs 117 of 117 |
| `cargo clippy --workspace --all-targets -D warnings` | clean |
| `tsc -p crates/editor/tests/forms` | passes (and fails on a planted `db("x")`) |
| Browser suites (`run.mjs`, four at a time, with the test plugins) | 15 of 15 ok: graph 19, views 30, start 24, start (empty) 14, fixes-graph 42, s6-builders 55, s6-fixes 19, taste 39, gestures 30, s8-tracks 75, s5-knobs 120, effects 12, live3b 60, layout 39, newsong 71 |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.sha256`, `renders.json` | unchanged |

### Numbers (Linux, information only)

| What | Value |
|---|---|
| `mix::view`, per state (dev build, under load) | first-song 89 µs, `examples/sounds` 386 µs; the graph view beside it 382 µs and 2.08 ms |
| The first pan, offline render against before (first-song, 4 s) | 1.1e-6 at most (−119 dBFS) |
| The first pan, live against no edit, through the activation (0.5 s after) | first-song 8.3e-7; the mono mix 1.0e-6 |

### Left

| What | Where |
|---|---|
| The Mix view itself: strips, faders, the drag that calls the first pan, then leases the new knob | B5a |
| `feeds` and `muted_in_song` in the Mix view's strips | 8b.4 |
| The style rule for options objects (`master(…, { ceiling: db(-2) })` then gets `db: -3`, not `db(-3)`) | later, if it reads badly |
| A strip whose fader is a signal shows no fader (`db` null); B5 decides its words | B5a |
| The timed gates with the new state field (`mix` adds a view's walk to each state) | final phase |
| When the `sounds` merge's slice 3 lands: rebuild `std.wasm` (`std/build.sh`, then `--check`), never take a side; `forms.ts` may need recording again | the merge |
| Scratch: `~/m2-runs/tn-scratch/b4/`. Logs: `~/m2-runs/logs/tn-b4-*`. | — |

## B5a: 8b.3c, the Mix view

**In short**
- The Mix view (⌥3, or X) has a strip for each track, in track order, and Master pinned on the right.
  Each strip has its colour, name, pan, an empty row for mute and solo (8b.4), and a fader with its
  level under it.
- A fader is heard while it moves and written once when let go, one undo step, with no toast. 0 dB is
  at 75% of the travel; the 4 px foot is silence (−96 dB, "−∞"). ⇧ for fine, ⌥-click for 0 dB, a
  double-click on the level to type, ↑ ↓, and the wheel: only up and down moves a fader, 0.1 dB a
  notch, one edit per scroll; a sideways scroll moves the strips (lead decision T6).
- A level or a pan the code leaves out reads in grey. A fader's first move writes `db` into the
  channel's entry; a pan's first drag writes `pan: 0` once it passes 4 px, then goes on as a lease, and
  two undos put the entry back.
- The knob's gesture (the lease) is now one hook, shared by the Device view's knobs and the Mix view.
- The Device view still shows the mixer's knobs; B5b makes it one compact card.
- Checks: the new suite `s8-mix` (63 checks) and the 16 suites the slice can touch pass; no render
  record moved.

### What was built

| Commit | What |
|---|---|
| `de85438` | `web/src/components/Knob.tsx`: KnobControl's gesture as a hook (`useKnobGesture`: preview, lease keepalive, commit, cancel, steps), the knob's drag, the small knob (now any size, and with `vertical`, deaf to a sideways scroll), its menu, the home marker, and one value bubble for the knobs and the song bar. No behaviour change. |
| this commit | `Mix.tsx` (strips, faders, pans, Master, the strip's menu, a strip with no fader); `mix` in the page's state and `graphApi.mixInsert`; the `MixSoon` placeholder and the shell's `reveal` go; the shell's `codeAt` (Edit in code at a line) and `renaming` that says where (a header or a strip). `e2e/s8-mix.mjs` in `run.mjs` and `e2e/README.md`; views and taste read the Mix view. README's Mix, gaps #4, design-8b §5.5 and §5.7. These notes. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| A fader's values | To a tenth of a dB | What the level shows; pro faders step so. ⇧ still gets there: 0.02 dB a pixel near 0 dB. |
| A press on a fader | Starts a drag from where the thumb is, anywhere on its column; never jumps | Taste principle 5: every drag is precise. The knob's row works the same way. |
| The level's text | "−6.0", no unit (taste's wireframe); the bubble and tooltip say "−6.0 dB" | The column is all dB; 76 px is narrow. |
| A strip's pan | 24 px, written to 3 significant figures as every knob is | One rule for knobs. "R 13" reads the same as 0.133. |
| The first pan's undo names | Both steps "Kick pan", not structural | The strip shows each undo; nothing off screen changes, so no toast. |
| Scrolling a strip's pan | Up and down only, as a fader (T6) | Otherwise a sideways scroll across the strips turns pans. |
| Pressing a strip | Selects its track; the dock stays on Mix | taste.md §3.7. The selected strip is `accent-wash`, its name 600. |
| The strip's menu | Rename (the strip's own name turns into a field), Colour ▸, Edit in code; Master has only Edit in code | taste.md §3.5. Mute in song waits for 8b.4. Rename in place, as TrackName does everywhere. |
| Edit in code | Selects the Song and opens `song.ts` at the channel's line | The channel lives in the mixer's call. The track header's own Edit in code still opens the track's file. |
| A strip whose level is a signal | "Its level is set in code", with Edit in code | B4 left its words to B5. |
| Where the fader sits | Its track at 40% of the strip's width | 8b.8's meter goes beside it, so nothing moves when meters come (taste §1.8). |
| A track whose strip a `why` explains, and a song whose mixer isn't `ok` | The strip (or the whole view) says the server's words, with Edit in code | The server already says it plainly (B4). |

### Checks

| Check | Result |
|---|---|
| Web build (`tsc` with the SDK and `forms.ts` projects, then Vite) | passes |
| After the refactor (`de85438`): s5-knobs, gestures, live3b (with the test plugins), taste, s8-songbar, cables, s6-builders, graph, undo | 9 of 9 ok: 120, 30, 60, 39, 68, 12, 55, 19, 10 |
| After the Mix view: s8-mix, views, taste, s6-builders, pc-keys, s8-tracks, code, layout, notes | 9 of 9 ok: 63, 30, 39, 55, 37, 75, 58, 39, 52. The taste lint passes on the Mix view. |
| Rust | not touched in B5a |
| `renders.sha256`, `renders.json` | unchanged (no engine, std or song file changed) |

### Numbers (Linux, information only)

| What | Value |
|---|---|
| `s8-mix` alone | about 20 s |
| first-song's fader travel at the dock's first height (300 px) | 173 px: 0 dB to −6 dB is 24 px |

### Left

| What | Where |
|---|---|
| The Song's Device view as one compact mixer card; the 9 suites that drag or read the mixer's knobs there move to the Mix view | B5b |
| A first options object in a call written over several lines lands on the last argument's line: the master's first fader move writes `}), { db: -4.8 },`. Right, but not how a person would lay it out. | B-end review (B4's `Insertion::plan`) |
| A meter beside each fader, M and S, mute in song, the sidechain row | 8b.8, 8b.4 |
| Scratch: `~/m2-runs/tn-scratch/b5/`. Logs: `~/m2-runs/logs/tn-b5-*`, `tn-b5a-*`. | — |

## B5b: 8b.3c, the compact mixer card and the suites that follow

**In short**
- Select the Song, and the Device view shows its mixer as one small card, "Mixer · 4 channels · Open
  Mix ⌥3", not a row for every channel (MX-6). Each channel's signal is one cable into it. The
  master's volume and ceiling stay knobs on its own card.
- The 8 suites that dragged or read the mixer's knobs in the Device view now do it on the Mix strips:
  gestures, graph, live3b, s5-knobs, s6-builders, s6-fixes, start and taste. Their counts are
  unchanged. `s8-mix` checks the card too: 67 checks.
- A fader and a strip's pan now behave exactly as a knob does when typing: Tab moves on, text that
  isn't a value gets its line, and a double-click anywhere on a fader types.
- A level reads "−6.0 dB" on its strip, as taste's words table says (B5a showed "−6.0").
- A control called just `db` (the master's, a gain's) is labelled **Volume**, not "Db", which reads
  like a note name.
- Checks: 22 browser suites (23 runs, live3b with the test plugins) pass; no render record moved.

### What was built

| Commit | What |
|---|---|
| this commit | `compactMixer` and the card's row in `NodeGraph.tsx`; `focusNext` in `Knob.tsx`, shared by every type-in; the Mix's type-ins (`knob-type-in`, Tab, the error line), a fader's double-click, levels with their unit; `controlLabel`'s Volume. The 8 suites; `s8-mix`'s card checks and its screenshot of the Song's Device view; `run.mjs` and `e2e/README.md`. README (the Song's mixer card), design-8b §5.5 and §5.7. These notes. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| Which mixer is a card | Only the song's own, when the Mix view has it (`mix.mixer` is `ok`) | The attack's note on claim 7: a track's own mixer (fixes-graph puts one in Keys) is an effect like any other, and keeps its box. |
| The card's inputs | One port, "channels", that each channel's signal is drawn into once; a control cable into the mixer (an LFO on a pan) keeps its own line style | One cable per track says what goes in, and the card stays one row. Nothing is hidden: a signal still shows where it goes. |
| The card's words | Title "Mixer"; its row "4 channels" and **Open Mix ⌥3**, a link in the accent | Design-8b §5.5's "Mixer · 6 channels · open Mix ⌥3", in sentence case. ⌥3 is spelled per OS (`opt`). |
| A level's text on its strip | "−6.0 dB", with the unit (B5a had "−6.0") | taste.md's words table writes dB so, and the owner is new to music: the unit says what the number is. It fits a 76 px strip. |
| The Mix's type-ins | The knob's: one test id (`knob-type-in`), Tab writes and moves to the next knob or fader, unreadable text gets a line ("Type a level, like −6 or −inf", "Type a pan, like L20, C or R35") | taste.md §2.14 has one type-in. gestures checks them all alike. |
| A double-click on a fader | Anywhere on it types, as on a knob's row | taste.md §5.2's gesture table: "double-click, or Return when focused". |
| "Db" | A control named `db` is Volume | std's gain says so itself ("Volume: louder or quieter, in dB"); "Db" reads as D flat. |
| gestures' "its name drags it too" | Its value drags it: a strip's pan has no name of its own | The check is that the whole control drags, not only the circle (taste.md §2.2). |
| gestures' Tab check | From Hats' fader to Bass's pan, the next control | A strip's pan comes before its fader (the attack's note on B5). |
| live3b's lead drag | Down into the fader's foot, kept inside the window | A fader's travel is shorter than a knob's 300 px; the check is still a drop of 10 dB heard while the save compiles. |
| s6-builders' "the knob is a lane now" | The strip's pan reads "—", "Its pan follows a lane or a signal in the code" | The lane node is the Device view's; the Mix view shows the knob is gone. |

### Checks

| Check | Result |
|---|---|
| Web build (`tsc` with the SDK and `forms.ts` projects, then Vite) | passes |
| s8-mix, gestures, graph, live3b (with the test plugins), s5-knobs, s6-builders, s6-fixes, start, start (empty), taste, views, fixes-graph, effects, ports, dsp, cables, undo, pc-keys, s8-tracks, code, layout, notes, s8-songbar | 23 of 23 ok: 67, 30, 19, 60, 120, 55, 19, 24, 14, 39, 30, 42, 12, 46, 33, 12, 10, 37, 75, 58, 39, 52, 68 |
| Rust | not touched in B5 |
| `renders.sha256`, `renders.json` | unchanged since `09b2dfd` (no engine, std, kernel or example file changed in B5) |

### Numbers (Linux, information only)

| What | Value |
|---|---|
| The 23 runs, four at a time | about 2 min 50 s after the build |
| first-song's Song graph in the Device view | the compact card is 80 px tall where the mixer was 16 rows; the whole graph fits the dock at 75% |

### Left

| What | Where |
|---|---|
| A first options object in a call written over several lines lands on the last argument's line (`}), { db: -4.8 },`) | B-end review (B4's `Insertion::plan`) |
| Off the Mac, a mouse scrolls sideways with Shift+wheel. Not checked: whether WebKitGTK and Chrome send that to a fader as a sideways scroll (the strips move) or as up and down (the fader moves) | B-end review, with T6 |
| M and S, mute in song, a meter per strip, the sidechain row | 8b.4, 8b.8 |
| The full `ci/check.sh` | the final phase |
| Scratch: `~/m2-runs/tn-scratch/b5/`. Logs: `~/m2-runs/logs/tn-b5b-*`. | — |

A last commit after these notes: a first pan refused in the middle of a drag asks no more until the
next press (one toast, not one a move). s8-mix, gestures, taste and live3b (with the test plugins)
pass again: 67, 30, 39, 60.

## Final phase: main merged, the review's fixes, the final checks

**In short**
- `main` had moved on past `cbac57c`: the `sounds` merge's last slices (std 0.2, its docs and review
  fixes) and 8b's back ends (mute and solo, clips by id, audition, meters, Remove automation). It is
  merged first, so each fix lands on what goes to `main`.
- The review confirmed 17 findings (16 problems: UI-6 and W5 are the same). Each is fixed below, with a
  test that failed before, or in the docs.

### The merge of `main` (`143f63d`)

| What | Result |
|---|---|
| `sounds-merge` | already in `main` (`e1003ba`): merging `main` brings it |
| Conflicts | 3 files: `core/play/src/lib.rs` (the player's message counts: both kept, by `git rerere`'s recorded resolution, read again), `docs/gaps.md` #4, #8, #20 (both said), `docs/architecture.md` RENDER-29, WRITE-10, WRITE-18 and PKG-13 (both said) |
| `std.wasm` | `std/build.sh`, then `std/build.sh --check`: the same bytes |
| Rust | the whole workspace builds; every test passes; clippy is clean |
| Browser suites | 35 of 35 runs pass (live3b with the test plugins), 1,343 checks |
| Render records | unchanged since `09b2dfd` |

### The review's fixes

| Finding | Commit | What changed | The test that failed before |
|---|---|---|---|
| meter-drift | this commit | A later time signature change keeps its bar, not its beat: an edit and its inverse give back the same file, and a meter before a change at bar 2 is never refused. The song.rs header, WRITE-10's *Today* and design-8b §4.1 say so. | `a_later_meter_change_keeps_its_bar` (the 3/4 went 32 → 20), `a_meter_before_a_change_at_bar_2_is_accepted`; `a_meter_change_lands_on_a_bar_line` updated |
