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
