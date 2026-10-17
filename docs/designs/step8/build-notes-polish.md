# Polish: build notes

What each part of the polish track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8c/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix and sounds.

## Lane B

Timeline.

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

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
