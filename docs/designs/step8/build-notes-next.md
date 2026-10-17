# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is left. The plan is
`~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground.

## Lane A

Hear it while you drag.

## Lane B

Play in.

## Lane C

Settings and safety. Branch `next-c`, worktree `wt/nx-c`.

Lane C started before P0 had landed: it made what it needs itself, in its own files and small blocks of the shared
ones. Its suites start servers of their own where they need an environment (`M2_AUDIO=fail`), so they don't need
P0's `env` rows in `run.mjs`.

### C1. A song opens with no sound device

**In short**

| What | Now |
|---|---|
| No sound device when a song opens | The song opens. It plays silently: the playhead moves, nothing sounds |
| The status pill | "No sound output", in the warning colour, with a crossed-out speaker |
| Its popover | "No sound device was found, so the song plays silently. Plug one in: it's used from your next action." |
| A device plugged in | Within about 2 s the song moves to it, where it was, stopped. A toast says "Sound output is back. Press Play to go on." |
| A device that is listed but won't open | Not tried at every action: again only when the default device changes, or after 30 s |

**What changed**

- `core/play/src/outputs.rs` (new): `Choice` (the output and buffer size a player opens; C2 uses it), `Output`,
  `outputs()`, `default_output()`, `output_ready()`. `M2_AUDIO` picks how the process meets the hardware: the
  devices, `none` (the silent player, which lists `M2_AUDIO_OUTPUTS`), or `fail` (no output starts: a test seam).
- `core/play/src/lib.rs`: `Player::start()` is `start_on(&Choice::default())`. `start_on` opens the chosen output
  (C2), or fails in today's words under `M2_AUDIO=fail`. `Player::frames()` and `Player::fell_back()` read two new
  `Shared` atomics. `silent()` is `silent_as("none", None)`.
- `crates/server/src/graph/output.rs` (new): `Outputs`, which starts players (the starter and the probe are
  injectable; unit tests keep `Player::silent()`). When a start fails, the silent player stands in, by its epoch, and
  the log says why once. `Graph::look_for_output()` marks an output to try, and `Graph::no_output()`.
- `crates/server/src/graph.rs`: `Inner::device()` asks `Outputs::start` for a player, and lets a real one take over
  from the stand-in (`Outputs::came_back`), where it was, stopped, with the toast. Not while an export holds the
  player. `Activity` gains `output` (the device's name) and `no_output`. `Inner.output`, `Graph::with_outputs`.
  `let_go` and `retire` are untouched (plan X2).
- `crates/server/src/lib.rs`: the `music2-tell` thread calls `look_for_output()` every 2 s.
- `web/src/lib/settings.ts` (new: the status's `output` and `no_output` types), `Status.tsx`: the `no-output` state,
  after the song's problems and before a dropout.
- `crates/server/src/graph/tests.rs`: one line. A device change's new player now says its name (`output: "none"`).

**Tests**

| Test | Result |
|---|---|
| `core/play` `outputs_put_the_default_first`, `a_chosen_device_that_is_gone_falls_back_to_the_default`, `a_fixed_buffer_is_used_when_in_range` (C2's, built here with `Choice`) | ok |
| `m2-server` `graph/output_tests.rs`: `a_song_opens_without_an_audio_device`, `the_silent_player_says_so_once`, `a_fallback_player_plays_on_its_clock`, `a_device_that_comes_back_is_used_from_the_next_action` | ok, three runs of the whole lib in a row |
| `cargo test -p m2-play -p m2-server`, clippy, the mac lint of both | ok |
| `nx-nodevice` (new, kind `none`, 9 checks) | ok |
| `taste` | 39, ok |

**Red first**

- `a_song_opens_without_an_audio_device` and the rest, with `Outputs::start` passing the start's error on (today's
  open): `panicked at output_tests.rs:46:28: the song opens: no audio output device`.
- With `came_back` never taking over: `panicked at output_tests.rs:119:5: the next action loads the song into the
  new player`; `the_silent_player_says_so_once`: `left: 1, right: 2` (never tried again).
- `core/play`, with `silent_pick` ignoring the choice: `outputs_tests.rs:45`: `left: ("Speakers", false), right:
  ("Headphones", false)`; `outputs_tests.rs:59`: `left: Some(480), right: Some(256)`.
- `nx-nodevice` on the new server with `DIST=` the page before (plan X19: no older server runs with
  `M2_AUDIO=fail`): 3 of 9 passed, then it stopped. `FAIL  the status pill says "No sound output": undefined
  (undefined)`; `FAIL  in warn, with the VolumeX mark: undefined`.

**Left**

- The "Settings…" link in the popover comes with C2.
- The look for an output only runs while a song is open; with none open, the next song's open tries again.

**For gaps.md:** #13 done: a song opens with no sound device, plays silently, the pill says so, and a device plugged
in is used from the next action.

**Contract:** RENDER-28 or the player's section: a start that fails starts the silent player instead, and the status
says `no_output`; the server looks for an output every 2 s while it stands in; a start that failed is tried again
when the default output changes or after 30 s.

**For 8a's merge** (`core/play/src/lib.rs`, each a small block):
- `pub mod outputs;` and its `pub use` line.
- `Shared`: `frames: AtomicU32` and `fell_back: AtomicBool`, after `last_callback_ns` (X6).
- `Callback::fill_in`: one line after `last_callback_ns`'s store: `self.shared.frames.store(…)`.
- `Player::start()` calls `start_on(&Choice::default())`; `start_on` and `start_in` replace `start`'s first lines and
  its device and config lines (`outputs::pick`, `outputs::buffer`, the `fell_back` store). `Choice` folds into
  8a's `Options` (an output name beside `frames`); `M2_AUDIO=fail` and `M2_AUDIO_OUTPUTS` go into
  `Options::from_env()` (plan X1). C never reads `M2_AUDIO_FRAMES` itself: `silent_as` keeps today's line for it.
- `Player::silent()` is `silent_as("none", None)`; `silent_as` is today's `silent` with a name and a buffer size.
- `Player::frames()` and `Player::fell_back()`, after `now_ns()`.
- The unused `HostTrait` import went (the device lookups moved to `outputs.rs`).

### C2. The Settings window

**In short**

| What | Now |
|---|---|
| Open it | ⌘, on a Mac (Ctrl+, elsewhere), music2 ▸ Settings… on a Mac, File ▸ Settings… elsewhere. From the start screen too. The ⌥K sheet lists it under File |
| Sound output | A list: "System default (Speakers)" first, then each device. A chosen device that is gone says "(not connected)" and plays on the default |
| Buffer size | System default, then 64 to 2048 samples, each with its time ("256 samples (5.3 ms)"), only the sizes the device takes. A hint: "Smaller is quicker to respond. Larger is safer if the sound drops out." |
| Now | What plays: "Headphones · 48 kHz · 256 samples (5.3 ms)". If play was on when you changed something: "Playback stopped. Press Play to go on." No toast |
| New songs go in | The folder. In the app: Change… (a folder dialog titled "Choose Where New Songs Go"). In a browser: a field. "Use the default" goes back to `~/Music/music2`. Exports follow the folder |
| Kept | In `~/.music2/settings.json`, for this computer, never in a song, with no undo |
| While an export runs | The output and buffer lists are greyed: "Wait for the export to finish." |
| The status pill | "No sound output" has "Settings…"; "Heavy load" has "Audio settings…" |

**What changed**

- `crates/server/src/settings.rs` (new, shaped like `recent.rs`): `Settings` over a given file, written whole through a
  temporary file; a file that doesn't read is the defaults.
- `crates/server/src/routes_settings.rs` (new, its own router; one `.merge` line in `routes.rs`): `GET /api/settings`
  and `POST /api/settings {output?, songs?}`. Not a project write: no session, works with no song and read-only. An
  output change while an export runs is refused: "Wait for the export to finish."
- `crates/server/src/graph/output.rs`: `Graph::set_output` (the player let go, a new one started at once with the song
  loaded where it was, stopped; whether it was playing), `output_choice`, `output_now` (`Now`).
- `crates/server/src/project.rs`: `songs_dir()` follows `set_songs_dir` (a process-wide `RwLock`), else
  `default_songs_dir()`. `songs_folder()` checks a typed folder: a full path, not inside a song (X38).
- `crates/server/src/lib.rs`: `AppState::keep_settings(file)` reads and applies them (a kept folder that is now inside a
  song is the default); `settings()`; the `mod` lines after `routes_view`.
- `crates/server/examples/headless.rs`, `apps/music2-desktop/src/main.rs`: `keep_settings` beside `keep_recent`.
- `apps/music2-desktop/src/menu.rs`: `settings` ("Settings…", `CmdOrCtrl+Comma`), in the Mac's app menu after About,
  and in File above Quit's line elsewhere. `actions.rs`: `song_folder` takes `{"start", "title"}` JSON (X38).
- Web: `lib/settings.ts` (the API, `useSettings`, `openSettings`, the words for buffers and rates),
  `components/SettingsDialog.tsx` (new), `App.tsx` (the ⌘, line last in `chord()`, `case "settings"` beside
  `shortcuts`, `<SettingsDialog />` after `<Shortcuts />`), `Shortcuts.tsx` (File: Settings), `Status.tsx` (the
  two links).
- The dialog's lists are buttons that open a menu (taste ST-4: no native select). Their menus sit above the dialog
  (`z-[80]!`). ↑ ↓ move in an open menu, Return and Space pick, Esc closes the menu only, Tab closes it.

**Tests**

| Test | Result |
|---|---|
| `m2-server` `settings_are_kept_in_their_file`, `a_bad_settings_file_is_the_defaults` | ok |
| `m2-server` `choosing_an_output_makes_a_new_player_where_it_was` (`graph/output_tests.rs`) | ok |
| `m2-server` `tests/songs_folder.rs`: `new_songs_go_where_settings_say` (its own process, X20) | ok |
| `music2-desktop` `settings_is_command_comma_in_the_app_menu_on_a_mac_and_in_file_elsewhere`, `the_folder_dialog_takes_its_title_from_the_page` | ok |
| `cargo test -p m2-server -p music2-desktop`, clippy and the mac lint of `m2-play`, `m2-server`, `music2-desktop` | ok |
| `nx-settings` (new, kind `none`, 38 checks) | ok |
| `nx-nodevice` (now 11: its Settings… link) | ok |
| `taste` 39, `start` 24, `newsong` 71, `pl-projects` 66, `export` 37, `pc-keys` 37 | ok |

**Red first**

- `settings_are_kept_in_their_file` with `change` never writing: `settings.rs:126`: `left: Choice { device: None,
  frames: None }, right: Choice { device: Some("Headphones"), frames: Some(256) }`. `a_bad_settings_file_is_the_defaults`:
  `settings.rs:151`, `frames: None` against `Some(128)`.
- `choosing_an_output_makes_a_new_player_where_it_was` with `set_output` only keeping the choice:
  `output_tests.rs:160: it was playing`.
- `new_songs_go_where_settings_say` with `keep_settings` not setting the folder: `songs_folder.rs:34`: `left:
  "/home/jerry/Music/music2", right: "/tmp/m2-songs-folder-48080/My Songs"` (it stops before anything is made).
- The desktop tests without the rows: `menu.rs:513`: `left: None, right: Some(("Settings…",
  Some("CmdOrCtrl+Comma")))`; with `folder_ask` ignoring JSON: `actions.rs:266`, the JSON as the folder and the old title.
- `nx-settings` on the new server with `DIST=` the page before: 3 of 38 passed, then it stopped. `FAIL  ⌘, opens the
  Settings window on the start screen`; `FAIL  ⌘, opens it with a song open`; `FAIL  Done has the focus: undefined`.

**Left**

- The Now row says the rate as the app says rates elsewhere ("48 kHz"), not the plan's "48,000 Hz".
- `exporting` greys the lists and the server refuses the change, but no suite runs an export beside Settings (an export of
  first-song is over too fast to catch).
- An input device, settings for every song's view, a theme (plan §10).

**For gaps.md:** #31 done: Settings (⌘,) chooses the sound output and its buffer size, shows what plays, and where new
songs go; kept in `~/.music2/settings.json`. Left: an input device, view settings for every song.

**Contract:** the homes table: `~/.music2/settings.json` (`output.device`, `output.frames`, `songs`), this machine's,
no undo, applied at once. RENDER-31 unchanged: the engine runs at the song's rate whatever the device's. A change of
output or buffer makes a new player where the song was, stopped.

## Lane D

Effects and small fixes.

## Merge

## End
