# Studio: the plan

For the `studio` track: branch `studio`, worktree `$SCRATCH/wt/st`, made from `main` at `85b069a` (steps 1
to 7, the sounds, taste's look, all of 8b, polish, 8a's offline half, and `next`). Written 2026-10-17 by a planning
agent, read only: files and `git`, nothing built or run. File:line cites are at `85b069a`. Sizes: **S** under 2 hours
of agent time, **M** half a day, **L** most of a day *(estimates)*.

**The Decisions section overrides the rest of this file. So does the Attack section, once one is added.**

---

## In short

- This track turns music2 into a studio: you can bring sounds in, record what you play, swap files with other music
  apps, and get help in plain words. Four parts are built side by side.
- Nothing an existing song exports changes. Every render stays the same.

| Part | What you get | Size |
|---|---|---|
| **A. Audio** | Bring in a WAV file: File ▸ Import Audio…, or drag it onto a track or below the tracks. See its waveform. Move it, trim it, fade it in and out, make it louder or quieter, play it backwards. Add an audio track from + Track. | M + M + M |
| **B. Record** | A Record button (R). One bar of clicks counts you in, then what you play on a MIDI keyboard or the keyboard piano becomes a clip. You see it while you play. One Undo takes it back. Settings chooses which MIDI keyboard plays. | M + M + M + S |
| **C. MIDI files and stems** | Open a MIDI file from another app: each part becomes a track with a fitting sound. Save notes as a MIDI file. Export each track as its own WAV file (stems). | M + M + M |
| **D. Help** | Help ▸ Make Your First Song: seven steps that tick themselves as you do them. Every button says what it does when you hold the mouse over it. Code mistakes say what went wrong in plain words, on your own line. | M + M + M + S |

- **Left for later:** recording audio (a microphone), stretching audio to the tempo, MP3 and AAC, stems of a song
  that plays plugins.
- **About 2 days.** Day 1: the lead's groundwork and the first half of each part. Day 2 morning: the rest. Day 2
  afternoon: join them, a review, the full check, pictures for you.

---

## Decisions

The lead and the lanes take each decision as written. Nobody waits for an answer, and nobody asks the owner.

### Lane A: audio

| # | Question | Decision |
|---|---|---|
| Q1 | Which files import? | **WAV only**: what the asset decoder reads (`load_wav`, `core/runtime/src/audio.rs:54`, `hound`): 8, 16, 24 or 32-bit integer, or 32-bit float, at any rate, mono or stereo. A file with more channels keeps its first two, and the toast says so ("vox.wav has 6 channels: music2 plays the first two."). Anything else is refused in words: "music2 reads WAV files. Save it as a WAV first: most audio apps can (Export, then WAV)." A file is taken as WAV by its `RIFF`/`WAVE` header, not its name |
| Q2 | Where does an imported file go? | `assets/<name>`, a copy (FMT-14): the file's own name made safe by `AssetName::for_import` in `core/song/src/path.rs` (FMT-5: core/song spells paths): NFC, control characters dropped, `/` `\` `:` made `-`, leading dots dropped, empty becomes `audio.wav`, the extension lowercased, at most 120 bytes. A taken name gets a number: `vox 2.wav` (FMT-14). The same bytes already in `assets/` (same size, then same hash) are used again, with no copy |
| Q3 | Durable before named (WRITE-1) | The copy goes to a hidden part file in `assets/` (`.vox.wav.<pid>.part`, never an asset name), is pushed (`m2_os::fs::push`), decoded once with `load_wav` (so what imports is what plays), then named create-only (`Rename::Excl`, as `render.rs`' `name_new`), its folder synced. Only then is the Tx that names it planned |
| Q4 | Undo of an import | Takes the clip (and a track it added) back, as one step. The file stays in `assets/`: music2 never deletes an asset. Cleaning up unused audio is left |
| Q5 | What a dropped file's tempo means | **No warp.** The file plays at its own speed from where it lands. Its clip is as long as the file at the song's tempo there (`tempo.beat_at(tempo.seconds_at(b) + seconds) − b`). A tempo change later keeps the audio's speed, so the clip's end may cut it or leave silence: drag the edge. Tempo found in the file (an `acid` chunk) is ignored. Stretching to the tempo (warp) is left |
| Q6 | Drop targets | On a track that has an audio lane, at a bar: onto that lane. On any other track: a new audio track right after it. Below the tracks: a new audio track for each file, at the end. Elsewhere (ruler, headers, dock, browser): nothing, and the hint says where to drop. The beat is the timeline's grid line at or before the pointer (`snapSong`, `web/src/lib/grid.ts:86`). Several files on one lane land one after another |
| Q7 | Import from the menu | File ▸ Import Audio… (⇧⌘I on a Mac, Ctrl+Shift+I elsewhere; Logic's key). Several files may be picked. They land at the bar line at or before the playhead, on the selected track when it has an audio lane, else on a new audio track after the selected one (at the end with none) |
| Q8 | An audio track in + Track | A last group, "Audio", with one row: "Audio track", "Plays recordings and loops: drop a WAV file on it". It writes `tracks/audio.ts`: `track("audio", { name: "Audio" }, ({ lane }) => lane.audio("clips"))`, the mixer's last channel and an empty lane file, as one undo step (`tracks::add_tracks`, P0). Not offered in Change Instrument |
| Q9 | Waveform peaks | Min and max per 256 frames per channel, as `i8`, made once from the decoded file and kept by content in `<cache>/peaks/<sha256>.m2pk` (`m2_os::dirs::cache`, beside `hashes.json`; FMT-16): a magic `M2PK`, a version, channels, rate, frames, the bucket size, then the pairs. Made at import, or at the first ask, on a blocking task of the route: never on the actor, never on the audio thread. A file that doesn't read is made again |
| Q10 | Fades | Two 7 px handles at a clip's top corners, shown on hover or when it is selected. Drag one in: a fade in or out, in seconds, as the renderer makes it (a half cosine). The bubble says "Fade in 0.40 s". Double-click a handle: no fade. A fade in and a fade out together are at most the clip. One Tx on release |
| Q11 | Gain | The clip's menu, **Gain…**: a small popover with one field, "−3.0 dB", which has the focus; ↑ ↓ step 0.5 dB, ⇧↑ ⇧↓ 3 dB, Return sets it, Esc closes. From −48 to +12 dB. The waveform scales with it. A clip whose gain isn't 0 says it in its label: "vox.wav · −6 dB" |
| Q12 | Reverse | The clip's menu, **Reverse**, with a check while on: one Tx. A reversed clip's edges and splits now work: the offset math is mirrored (A3). A pitched clip's edges still refuse, as today (pitch isn't in the app) |
| Q13 | The right edge past the audio's end | It stops where the file ends; the bubble says "End of vox.wav". The editor refuses past it too, from the file's header (`hound`, no decode) |
| Q14 | Audio drags while playing | Heard on release, as today (they need renders; design-8b §9.1) |
| Q15 | Join audio clips | Refused as today (it needs a render of them as one sound): left |
| Q16 | Undo names | As notes clips' ("moving the Vox clip"). New: "importing vox.wav", "importing 3 files", "Vox fade in", "Vox fade out", "Vox clip gain", "reversing the Vox clip", "removing the Vox clip's fades", "adding Audio" |

### Lane B: recording

| # | Question | Decision |
|---|---|---|
| Q17 | The Record button | In the top bar right after Play / Stop (x 144; everything to its right moves 32 px). 28 px, `Circle` filled (taste §2.12). Stopped: a ghost with the circle in `danger`. Counting in: `danger-soft` fill, the beats left (4, 3, 2, 1) in `danger-strong` `text-sm` 600 in place of the circle. Recording: `danger` fill, a white circle. No animation. Tooltip: "Record (R) · right-click for options" |
| Q18 | Its menu | Right-click (or Ctrl-click on a Mac): "Count in one bar" (a check, on by default), "Quantize the take" (a check, off by default), a line, "MIDI keyboards…" (opens Settings). Both checks are this machine's, kept in `~/.music2/settings.json` as `record: {count_in, quantize}` |
| Q19 | R and the other keys | R stopped: count in, then play and record from where play starts. R playing: record from now, no count-in. R counting: cancel (nothing plays). R recording: stop recording, play goes on, the take is written. Space while recording: stop both, the take is written. Esc while counting: cancel. Transport ▸ Record, no accelerator (R is the page's). R is never a keyboard piano key |
| Q20 | The count-in | One bar of clicks at the tempo and time signature where play starts, a click on each beat of the meter's unit, the first one higher (the metronome's sound), even with the metronome off; the song is silent meanwhile; then it plays from the start point, so the downbeat lands exactly a bar after the first click. Built in the live engine (`Click`, `Msg::CountIn`): the client can't time it. Unticked: recording starts at once. During the take the metronome follows its own setting (K) |
| Q21 | What records | Notes from a MIDI keyboard that plays (Q28) and from the keyboard piano, played on the take's lane. Notes heard from Notes' own clicks (audition) never record. The take's lane is the play target when Record is pressed (`playTarget.ts`: the clip Notes shows if it is the selected track's, else the track's first notes lane). No such lane: "Select a track with notes to record on it." |
| Q22 | Timing | A note goes where it was heard: the engine's published beat when the server takes the note, less one device buffer (what is in the speakers then), clamped to the take's start. A note lands on a block line (2.7 ms at 48 kHz); quantize does the rest. Velocity as played. The sustain pedal: a note held by it ends where the pedal let it go |
| Q23 | The take | A new notes clip on the take's lane, from the bar line at or before where recording started to the bar line at or after where it stopped (at least a bar). It lies over any clip there (both play); it is selected afterwards, so ⌫ takes it out. Notes still held at the stop end there. Nothing played: nothing is written, and a toast says "Nothing was played, so nothing was recorded." |
| Q24 | While recording | The clip grows in its lane up to the playhead: a 1.5 px `danger` outline, `danger-soft` body, the notes as the timeline draws notes, the label "Recording". The server sends `{"t":"take"}` every 100 ms on a latest-only channel of its own, as the meters' |
| Q25 | The write | One `Clips` Tx (`Gesture::Set`), one undo step named on the server "recording on Bass" ("Undo recording on Bass") |
| Q26 | Quantize the take | When ticked, each note's start goes to the nearest line of Notes' grid (Auto counts as 1/16); its end is kept, as Quantize does (`ends: false`) |
| Q27 | Recording while looping | **The first pass only.** At the loop's wrap the take is written (one Tx) and recording ends; the loop plays on and the next pass plays the take. One clip per pass would pile clips on one lane, all sounding together: left, with overdub. Any jump while recording (a click on the ruler) ends the take the same way |
| Q28 | Which MIDI keyboard plays | Settings gains **MIDI keyboards**: a row for each one connected or turned off before, its name and a "Plays" switch, on by default; one gone says "(not connected)". With none: "No MIDI keyboard is plugged in." Kept in `settings.json` as `midi: {off: [names]}` by the names `m2_os::midi` gives. One turned off neither plays nor records; the top bar mark's tooltip says so |
| Q29 | Recording audio (gaps #25) | **Cut**: it needs an input stream in `core/play`, 8a's area this round |
| Q30 | Export while recording | Refused: "Stop recording first." Joined at the End (it touches lane C's Export) |

### Lane C: MIDI files and stems

| # | Question | Decision |
|---|---|---|
| Q31 | A MIDI-file crate? | **None**: `Cargo.lock` has none (no `midly`). `crates/editor/src/smf.rs` reads and writes Standard MIDI Files: formats 0 and 1, ticks per quarter (an SMPTE division is refused in words), running status, sysex and other meta skipped; kept: notes, program changes, tempo (FF 51), time signature (FF 58), track name (FF 03), end of track. Format 2: refused. `Cargo.lock` doesn't change |
| Q32 | What becomes a track | Every pair of (MIDI track, channel) with notes, in file order (format 0 splits by channel). At most 32; more is refused: "groove.mid has 40 parts: music2 brings in 32 at most." The name: the track's name event, else the instrument's label |
| Q33 | Which sound | Channel 10: **Drum kit** (General MIDI notes, as `std/drums` names them). Else by its program, General MIDI's families: 0–7 FM keys, 8–15 FM bell, 16–23 Synth pad, 24–31 Synth pluck, 32–39 Synth bass, 40–55 Strings, 56–63 Supersaw, 64–87 Lead, 88–103 Synth pad, 104–111 Synth pluck, 112–119 FM metal, 120–127 Synth pad. No program: Synth bass when its notes average below C3, else FM keys. One table in `midifile.rs` serves import and export |
| Q34 | Notes | One clip per track, from bar 1 (or the bar dropped on) to the bar line after its last note. Ticks to beats by the file's division; velocity `v / 127`; a note with no Off ends at its pitch's next On or its track's end |
| Q35 | Tempo | The song is empty when no lane has a clip: then the file's tempo changes and its time signatures (those on a bar line) are written into `data/arrangement.json` in the same Tx. Else a question: "groove.mid is at 128 BPM. The song is at 120 BPM." [Keep 120] (the default, Return) [Use 128] ("Use" writes the file's tempo map). A file with no tempo is 120 (SMF's default) |
| Q36 | Import as one step | All tracks and clips are one Tx, one undo step: "importing groove.mid". File ▸ Import MIDI… (no key) starts at bar 1. A one-track file dropped on a notes track's row goes into that lane at that bar (no new track); any other drop makes new tracks at the end, from that bar |
| Q37 | MIDI export | The clip's menu, **Export as MIDI…** (notes clips), and the Export dialog's **MIDI** (the song). Format 1, 960 ticks a beat; track 0 holds the song's name, its tempo map and time signatures; one MIDI track per notes lane with notes, named "Bass" (or "Bass · lead" for a second lane), with a program change from Q33's table; a drum kit's lane on channel 10, the rest on channels 1 to 9 and 11 to 16 in order (the 17th on shares 16). Notes cut at their clip's edges as they play; velocity `round(v × 127)` in 1 to 127; a tuned pitch rounded. Tracks muted in the song are left out, as the audio export leaves them out |
| Q38 | Where exported files go | The app: a save panel (P0's `save_file`) starting at `Exports/<song>.mid` (a clip: `<song> <Track>.mid`). A browser: straight to Exports, a free name |
| Q39 | Stems | The Export dialog's **Stems**: one WAV per track, each the track's own output (before its mixer channel, so no fader, pan or master), rendered exactly as `m2 bounce <song> x --track <key>` renders it: 32-bit float, from 0 to 2 s past the song's last clip, **the same bits**. Into `Exports/<song> stems/` (then `… stems 2`), named "1 Kick.wav", "2 Hats.wav" in the tracks' order. Tracks muted in the song are left out. On 8a's executor, as an export (`export_threads`). One export at a time |
| Q40 | Stems of a song that plays plugins | **Cut**: in the app a render of a song with plugins borrows the live instances (RENDER-1), keyed by the song's own graph; a track's graph would need its own lending. The Stems row is greyed: "Not yet for a song that plays plugins (Surge XT)." |
| Q41 | MP3 or AAC | **Cut**: no encoder in `Cargo.lock` |
| Q42 | The Export dialog | ⇧⌘E and the Export button open "Export", a page dialog: three rows (Song, Stems, MIDI), each a line in plain words; Song is chosen and **Export…** has the focus; Return or a click goes on: for Song, the mute question first (ExportAsk, as today), then the save panel; for Stems and MIDI, as above. Esc closes, Tab goes round (`useModalKeys`). File ▸ Export Audio… becomes File ▸ Export… |

### Lane D: help

| # | Question | Decision |
|---|---|---|
| Q43 | Which controls need a tooltip | Every visible `button`, `[role=button]`, toggle (`aria-pressed`), `[role=tab]`, `[role=switch]`, `[role=slider]`, `[data-knob]`, fader and `select`: a non-empty `title` in plain words, with its key when it has one ("Loop (C)", keys by `chord()` and `keyName()`). Not: rows of a menu or list (`[role=menu] *`, `[role=listbox] *`, `[role=option]`), whose words are the action, and the code editor's insides |
| Q44 | The lint | `e2e/tiplib.mjs` `tips(sel)` (P0's first commit) lists each control under `sel` without one. Every new suite runs it on what it adds. `st-tips` walks every screen, dialog and popover and fails on any. Nobody edits `e2e/lintlib.mjs` or `taste.mjs` |
| Q45 | Errors in plain words | Made in `crates/editor/src/explain.rs` (P10: no words in core) from the code and the message, for the common cases (D2's table): a sentence, a fix, and the place in your own code. The original message, its code and the place inside std go under **Details** (`ui.tsx`' `Details`). An error no rule knows keeps its own words |
| Q46 | Your own line | `core/session`'s `build_code` points a graph-check error at the innermost call in the project whose frames made the node (`Node.frame`, the module's frames, the sites' `in_project`). The place inside a package moves to a new `m2_ir::Diagnostic.inner`. The code editor marks your line |
| Q47 | Make Your First Song | Help ▸ Make Your First Song (no key), and "New to making music? Make your first song" on the start screen (it opens New Song with Empty chosen, then the guide). A card at the top of the side browser (it opens the browser if hidden), so it never covers the timeline: seven steps (D3), each ticking itself; a fold chevron makes it one line, "Your first song · 3 of 7"; × closes it. Kept per song in `.music2/view.json` as `guide: {open, done}`. A step done stays done |

### Shared

| # | Question | Decision |
|---|---|---|
| Q48 | New crates | None. Every lane's `git diff Cargo.lock` is empty at every commit |
| Q49 | New suites | Named `st-…`: `st-audio`, `st-record`, `st-midi`, `st-stems`, `st-tips`, `st-guide`, `st-errors` |
| Q50 | `m2` commands | `m2 import` (A4) only if lane A has time. MIDI files, stems and recording from `m2`: left (gaps #37) |
| Q51 | Gates | No new gate: `crates/gates` is 8a's. Rust tests time nothing; Linux timings are information only |
| Q52 | A file dropped in a browser, and by the app | One drop layer (P0) takes both: Tauri's drag-drop events (paths and a position) in the app, and HTML5 `drop` with its `File`s in a browser. Paths go to the server as paths (it copies them); `File`s as bytes |

---

## Attack

None yet. When one is added, it goes at the end of this file, with its own Decisions after it, and both override
everything above.

---

## 0. Where things stand

| Item | What exists | What's missing |
|---|---|---|
| Audio lanes | `lane.audio(key)` (`core/script/sdk/core.ts:1611`); clips with `offset` (seconds), `gain_db`, `fade_in`, `fade_out` (seconds), `reverse`, `pitch`, `warp` (`core/song/src/lane.rs:171`); rendered into memory and cached by content (`core/runtime/src/audio.rs`, `render_clip` `:266`). `examples/resample` has one | Any way in from the app |
| Writing an asset | `write_new_wav` (create-only, pushed, synced: `audio.rs:80`); `place_clip` puts one clip as a Tx (`crates/editor/src/data.rs:334`), as `m2 bounce --place` does | Import (FMT-14 Today: "Importing isn't built") |
| Audio clip gestures | Move, copy, resize either edge with `offset` (`clips.rs:530`), split (`:559`), duplicate, delete, paste. A reversed or pitched clip's edge refuses (`audio_edge`, `:635`). Create refuses on an audio lane (`:173`) | Fades, gain, reverse; reversed edges; a right edge that stops at the file's end |
| The timeline's audio clip | An `AudioWaveform` icon and the file's name (`SongTimeline.tsx:2259`) | A waveform |
| + Track | `tracks::add` (`crates/editor/src/tracks.rs:360`): one track playing an instrument; `adds_one` asserts one module (`:333`) | Audio tracks; several tracks in one Tx |
| The metronome | `core/runtime/src/click.rs`, mixed after the plan in `Engine::render` (`engine.rs:634`) | A count-in |
| Live notes | `Graph::audition` (`crates/server/src/graph/audition.rs:110`): windows and MIDI keyboards (`midi:<name>`, `graph/midi.rs`) play the target lane | Recording; a keyboard picker |
| MIDI files | Nothing; no crate in `Cargo.lock` | All of it |
| Stems | `m2 bounce --track` re-roots the IR at a track's export (`apps/m2/src/main.rs:481-496`) and writes 32-bit float | In the app |
| Export | One click: the native save panel, then a render (`web/src/lib/exporting.ts:39`, `crates/server/src/export.rs:131`) | A dialog with choices |
| Tooltips | `title` on most controls (157 in `components/`) | A rule and a lint |
| Errors | `files::problem` places each draft error (`crates/server/src/files.rs:219`); the compile banner lists three (`Status.tsx:229`). A graph-check error's `src` is inside std (`core/ir/src/check.rs:293`, `:1055`) | Plain words, your own line |
| Help | `docs/music-basics.md` (partly out of date), the ⌥K sheet | A guide in the app |
| Files from the OS | The desktop has `tauri-plugin-dialog` (`apps/music2-desktop/src/actions.rs`: `export_path`, `song_folder`); the page listens to Tauri's `menu` event (`web/src/lib/native.ts:53`) | An open-file and a save-file action; drops |

---

## 1. How the track runs

### 1.1 Branches and worktrees

- `studio` in `$SCRATCH/wt/st` is the lead's: P0 (§2), the merges and the End (§9).
- The lead's first commit makes the build notes and the module stubs (§2.1); then four lanes from `studio`:
  ```
  cd $SCRATCH/wt/st && git config rerere.enabled true
  for l in a b c d; do git worktree add $SCRATCH/wt/st-$l -b studio-$l studio; done
  ```
  Each lane runs `git config rerere.enabled true` in its worktree too.
- The lanes start at once, on work that needs nothing from P0 (§8). When P0 lands on `studio`, each lane merges
  `studio`. When `main` moves (8a lands), the lead merges `main` into `studio`, and the lanes merge `studio`. Merges
  only: never rebase, never rewrite history, never push.
- Never touch `wt/8a`, `wt/8a-g`, `wt/8a-g-base`, nor tmux sessions `m2`, `1`, `2`, `ssh_tmux`. Never delete anything
  under `$SCRATCH/plugins`.

### 1.2 Builds

Every cargo, npm or music2 run has `CARGO_HOME=/home/<user>/.cargo RUSTUP_HOME=/home/<user>/.rustup`. Builds are
`--locked`, never `cargo update`. No lane adds a crate (Q48).

| Who | Target | Builds |
|---|---|---|
| Lead (`wt/st`) | `$SCRATCH/sst/target-st` | the "before" server, P0, the merge checks, the desktop app for pictures, the End's full check |
| Lane X (`wt/st-x`) | `$SCRATCH/sst/target-x` | `cargo build --locked -p m2-server --example headless -p m2`, and the tests and clippy of the crates it touches |
| Every lane, for its menu row | its own target | `cargo test --locked -p music2-desktop` and its clippy; the mac lint of the crates it touches: `CC_aarch64_apple_darwin=$PWD/ci/stub-cc CXX_aarch64_apple_darwin=$PWD/ci/stub-cc AR_aarch64_apple_darwin=$PWD/ci/stub-ar cargo clippy --locked --target aarch64-apple-darwin -p <crates> --all-targets -- -D warnings` |

- After each build, copy the binaries per commit, so a rebuild never swaps one under a running suite:
  `B=$SCRATCH/sst/bin/<lane>-<sha>; mkdir -p $B/examples; cp $T/debug/examples/headless $B/examples/; cp $T/debug/m2 $B/`
  (`run.mjs` finds `m2` one folder above the server's; suites that run `m2` need it).
- Web: `cd web && npm ci && npm run build` (it type-checks). Once per worktree also `cd e2e && npm ci`.
- Long jobs (cargo builds and tests over a few minutes, suite batches, the full check) go through
  `TW_WAIT=<seconds> $SCRATCH/tw <name> '<cmd>'` (no double quotes or parentheses in the command). Logs in
  `$SCRATCH/logs/`. Name them `st-<lane>-<what>`.
- Never move a `target/` between worktrees. At its end each lane deletes `target-<lane>` and `bin/<lane>-*`. The lead
  deletes `target-st`, `bin/` and `scratch/` after the pictures (scratch targets once filled 390 GB).

### 1.3 Suites

- **Tools** (`$SCRATCH/sst/tools/`; change only these copies): `lane-suites.sh <lane> <out> <suite>…`
  already maps `st` to `wt/st` and `a`…`d` to `wt/st-a`…`wt/st-d`. `DIST=<folder>` serves an older page (red first);
  `OLD=<folder>` an old tree's std, templates and examples; `M2_HEADLESS` (required) a copy under
  `$SCRATCH/sst/bin/`. `app-shot.sh` takes `scratch/x97.lock` itself (never wrap it in `flock`), uses `:97`
  only, a scratch HOME, `M2_AUDIO=none`, `M2_MIDI=inject`.
- **Running:**
  ```
  M2_HEADLESS=$SCRATCH/sst/bin/a-<sha>/examples/headless \
    $SCRATCH/sst/tools/lane-suites.sh a $SCRATCH/sst/scratch/e2e/a1 st-audio taste s8-clips
  ```
  Each suite gets a scratch HOME under `<out>`, `M2_AUDIO=none` and `M2_MIDI=inject` (`e2e/run.mjs`, `envFor`): never
  the real `~/.music2` or `~/Music`.
- **The "before" server and page** (the lead, first thing): `cargo build --locked -p m2-server --example headless -p m2`
  at `85b069a` into `target-st`, copied to `bin/st-85b069a/`; `web/dist` copied to
  `$SCRATCH/sst/scratch/dist-before`. Every red-first suite run uses them.
- **How a suite drops files** (P0's `e2e/droplib.mjs`):
  - *As the app does:* an app page (the Tauri shim `pl-projects.mjs:111-142` uses, made shared in P0's
    `e2e/applib.mjs`) answers `plugin:event|listen` for Tauri's drag-drop events too. `drop(page, paths, x, y)` fires
    enter, over and drop with real paths in the suite's scratch folder, as WKWebView and WebKitGTK do; the server
    copies them. `menu(page, id)` sends a menu command; `answers` sets what `open_file` and `save_file` return.
  - *As a browser does:* `dropFiles(page, x, y, [{name, bytes}])` builds a `DataTransfer` with `File`s in the page and
    dispatches `dragenter`, `dragover` and `drop` at the point; the page uploads the bytes.
  - `wav({seconds, rate, channels, shape})` writes a WAV (16-bit or float; a sine, a ramp, silence) and `smf({...})`
    (lane C adds it) a MIDI file, both in node, into the suite's scratch folder.
  - The menu's import in a browser page clicks a hidden file input: suites use puppeteer's `waitForFileChooser`.
- New suites run as a Mac page and open one PC page each (`open(…, { pc: true })`, `e2e/fixlib.mjs`) for the Linux
  words ("Ctrl+Shift+I"). Each lints its new states with `lintlib.mjs`' `lint` and `tiplib.mjs`' `tips`.

### 1.4 Checks per slice

1. The Rust tests and `cargo clippy --locked -p <crates> --all-targets -- -D warnings` of the crates the slice
   touches, and the mac lint of them where §1.2 says.
2. The web build.
3. `taste`.
4. The slice's own suite.
5. The suites it moves (listed per item).
6. Lanes B and C, whose changes reach the engine or the render path: the renders, as `ci/check.sh` runs them:
   `M2=$T/debug/m2 e2e/baseline.sh --tree . <out>` then `(cd <out> && shasum -a 256 -c <repo>/e2e/renders.sha256)`:
   10 of 10.

The full `ci/check.sh` runs once, at the End.

### 1.5 Red first

Every new Rust test and suite check that tests behaviour is shown failing first:
- a suite: run it on the page and server from before the change (`DIST=` the `dist-before` copy, `M2_HEADLESS=` the
  `bin/st-85b069a` copy, `OLD=` when std changed), and copy two or three `FAIL` lines into the build notes;
- a Rust test: run it with the change taken out (a stub that does nothing, or the old rule), and copy the failing
  assert.

A test that can't be red (it guards something that never happened) is named a **guard** in the build notes.

### 1.6 Commits and build notes

- Small and green. Subject in plain words, a blank line, a short body, then exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01R1SbPDdWrZoQ7z8dxU28hJ
  ```
- `docs/designs/step8/build-notes-studio.md` is made by the lead's first commit with `## P0`, `## Lane A`,
  `## Lane B`, `## Lane C`, `## Lane D`, `## Merge`, `## End`, a line under each. A lane writes only under its own
  heading. Each slice adds its section in the same commit: **In short** (a table for the owner), what changed, tests,
  red first, guards, what's left, and two lines for the End: **For gaps.md** and **Contract**. A slice that touches
  `core/runtime` or `core/play` adds **For 8a's merge**: each block it added there, file and function, a sentence
  each (§1.9).

### 1.7 The desktop app

Only on the invisible Xvfb screen `:97`, through `tools/app-shot.sh`, a scratch HOME under
`$SCRATCH/sst/scratch/` and `M2_AUDIO=none`. Never on a real screen. Each lane may open it once to check
its menu rows (A: File ▸ Import Audio… with Ctrl+Shift+I; B: Transport ▸ Record; C: File ▸ Import MIDI…, File ▸
Export…; D: Help ▸ Make Your First Song). The End takes the pictures (§9).

### 1.8 Writing for the owner

The In short sections, captions, HANDOFF and every word in the UI: short sentences, plain words, tables. Keys as
`chord()`, `keyName()` and `shifted()` spell them for each OS (`web/src/lib/native.ts`). A track's ID is "ID", never
"key"; "key" in the UI means a key you press or the musical key. "Clip", "track", "lane" and "notes" as the README's
Words you'll see.

### 1.9 `core/runtime` and `core/play`: 8a's rule

8a changes `core/runtime`, `core/play`, `core/os`, `crates/gates` and the crash and error paths (8a-gaps already
changed `Engine::render`'s error arm, `graph.rs`' `out_of_memory`, and added `graph/stopped.rs`). Only lane B edits
`core/runtime` and `core/play`, each change a small block of its own, named in its build notes' **For 8a's merge**:

| File | Block |
|---|---|
| `core/runtime/src/click.rs` | The count's fields at `Click`'s end; `count_in`, `counting`, `count_ended` after `is_on`; one branch at the top of `mix` for the count's blips |
| `core/runtime/src/click/tests.rs` | New tests at the file's end |
| `core/runtime/src/engine.rs` | `Msg::CountIn { beats, frames_per_beat }` at the enum's end; `structural()`'s `matches!` gains it; one arm in `apply_now` (`:409`); one line in its `Transport` arm (a Stop cancels a count); in `render` (`:594`): the idle check skips while counting, the idle-after-silence check too, and one line after `self.click.mix(…)` (`:634`): `if self.click.count_ended() { cur.transport_cmd(Cmd::Play) }`; one line where a hold starts (`:515`): a hold cancels a count. Nothing in the `Err(e)` arm 8a-gaps changed |
| `core/play/src/lib.rs` | `Player::count_in` after `Player::click`; an arm for `EngineMsg::CountIn` wherever a `match` over `EngineMsg` has no `_` |
| `core/play/tests/no_alloc_count.rs` | New file (as next's `no_alloc_click.rs`) |

Nothing here edits `core/os`. Lane D's `core/session/src/lib.rs` and `core/ir/src/lib.rs` blocks are outside 8a's
area. Before editing near a function, read `git diff main...8a -- <file>` and `git diff main...8a-gaps -- <file>`,
and stay out of what they changed.

---

## 2. P0: the lead's ground — M

Step 1 comes before the lanes branch (under an hour). The rest runs beside the lanes' first slices, which need nothing
from it (§8); each lane merges `studio` once it lands (about midday of Day 1). No behaviour a user can reach.

1. **The first commit, then the lanes** (the lanes branch from it):
   - `git config rerere.enabled true`; `build-notes-studio.md` with its headings (§1.6).
   - **The module stubs** (X18 of `next`): each new module made once, empty, with its `mod` and `.merge` lines, so no
     two lanes add lines at one place or make one file twice:
     - `crates/server/src/{routes_audio.rs, routes_record.rs, routes_exchange.rs}`: an empty `router(state)` each,
       with its own list of write routes and its own read-only (423) middleware, as `routes_view.rs` has; `mod`
       lines after `mod routes_leases;` in `lib.rs`, `.merge` lines after `routes_leases`' in `routes.rs`.
     - `crates/server/src/{peaks.rs, stems.rs}` and `crates/server/src/graph/{import.rs, record.rs, exchange.rs}`:
       empty, with their `mod` lines (`graph.rs`' after `mod tempo_lease;`).
     - `crates/editor/src/{import.rs, smf.rs, midifile.rs, explain.rs}`: empty, `pub mod` lines after
       `pub mod rekey;`.
   - **The render's part files, shared:** `part_path`, `sweep` and `name_new` in `crates/editor/src/render.rs`
     become `pub(crate)` (three words), for lane A's import.
   - **The tooltip lint:** `e2e/tiplib.mjs`, `tips(sel)` (Q43), so lane D's first slice and every new suite can use
     it from the start.
   - `cargo build --locked -p m2-server --example headless` and clippy pass; commit. Then the four worktrees (§1.1)
     and `npm ci` in each `web/` and `e2e/`.
2. The "before" server and page (§1.3), from `85b069a`.
3. **Several tracks in one Tx** (`crates/editor/src/tracks.rs`, the lead's only; after P0, a change there is the
   lead's on `studio`):
   - `pub enum Play { Instrument(String /* ComponentId */), Audio }`; `pub struct NewTrack { play, label, key,
     clips: Vec<serde_json::Value> }` (whole clips in the lane format, ids minted as a gesture mints them);
     `pub fn add_tracks(s, tracks: &[NewTrack], after: Option<&str>, arrangement: Option<String>) -> Result<Vec<Made>,
     EditError>`: one Tx with each new track's file, all imports in one insertion, all channels in one edit of the
     mixer's object, each lane file (with its clips, canonical), `layout.json` with each colour, and, when given,
     `data/arrangement.json` written whole (WRITE-10's tempo rule: `Confined` admits the arrangement it writes).
     `Expect::Confined { ranges: as add's, lanes: every new lane, modules: Added(each) }`.
   - An audio track's file: `track("audio", { name: "Audio" }, ({ lane }) => lane.audio("clips"))`, no instrument
     import (`track_call_text` gains the audio form; `planned_call_forms_are_current` covers it).
   - `adds_one` becomes `adds(checked, &[(key, channel)])`: exactly those modules added, in any order, no value
     changed, each in its own channel. `add` is `add_tracks` with one instrument track.
   - Tests (`tracks_tests.rs`): `two_tracks_in_one_tx_are_one_undo_step`, `an_audio_track_plays_its_lane`,
     `a_new_track_carries_its_clips_with_fresh_ids`, `an_arrangement_written_with_new_tracks_is_admitted`; every
     existing `tracks_tests` test passes unchanged. Red first: the new tests against `add` called once per track.
4. **The drop layer** (`web/src/lib/drop.ts`, new; `components/DropLayer.tsx`, new, mounted in `App.tsx` after
   `<Toasts />`):
   - Listens to `getCurrentWebview().onDragDropEvent` in the app (`native`) and to `dragenter`/`dragover`/`drop`
     with `dataTransfer.files` on `window` in a browser.
   - `target(x, y)`: `{kind: "lane", module, lane, beat}` over a row with `data-lane`, `{kind: "track", module, beat}`
     over a track row with no audio lane (`data-track`), `{kind: "below", beat}` below the last row of
     `[data-testid=timeline]`, else `{kind: "none"}`. The beat: from `[data-beat-origin]`'s left edge (one attribute
     P0 adds on the timeline's lanes box in `SongTimeline.tsx`) and `zoomNow()`, snapped with `snapSong`.
   - `onFiles(extensions, {hint(files, target), drop(files, target)})`: lanes register their kinds in their own
     files; a file nobody takes makes the hint "music2 opens WAV and MIDI files". `files` are
     `{name, path?}` (app) or `{name, file}` (browser).
   - The hint: a 2 px `accent` outline where it lands (the lane's row from the beat to the beat plus one bar, or a
     dashed box below the tracks) and a bubble (taste §2.14) with the hint's words. Gone on leave or drop.
5. **The desktop's file dialogs** (`apps/music2-desktop/src/actions.rs`): `open_file` (arg
   `{"title", "filters": [{"name", "extensions"}], "multiple"}`, answers a JSON list of paths, or null) and
   `save_file` (arg `{"title", "name", "start", "filters"}`, answers a path or null), with `BUSY` as the others; a
   test each for reading the arg (as `the_folder_dialog_takes_its_title_from_the_page`).
6. **Suite helpers:** `e2e/applib.mjs` (the app page shim from `pl-projects.mjs`, plus drag-drop events and
   `open_file`/`save_file` answers) and `e2e/droplib.mjs` (`drop`, `dropFiles`, `wav`). `pl-projects` and
   `nx-settings` keep their own shims (nobody edits them for this).
7. The checks of §1.4 on what P0 touched (`m2-editor`, `m2-server`, `music2-desktop`; the web build; `taste`,
   `s8-tracks`, `pl-tracks`, `pc-keys`), commit, and write P0's section.

---

## 3. Lane A: audio in the app — `wt/st-a`, branch `studio-a`

Gaps #18. On the audio lanes the engine already plays (RENDER-27, RENDER-30's "rendered into memory").

### A1. Waveforms — M (needs nothing from P0)

**What the user sees.** Every audio clip shows its waveform, drawn in the track's `ink` over the clip's body: the
louder of its two channels, mirrored about the middle. A reversed clip's runs backwards; gain scales it (clipped at
the body's edge); fades taper it, with each fade as a thin `fg-muted` curve. It shows within a second of the clip
appearing. A clip whose file is missing keeps today's words. The clip's tooltip: "vox.wav, bars 3–5 · drag to move ·
drag the top corners to fade".

**The back end.**
- `crates/server/src/peaks.rs`: `Peaks { channels, rate, frames, bucket, data: Vec<i8> }` (Q9);
  `peaks::of(path) -> Result<Arc<Peaks>>`: the hash by `HashCache::global().hash(path)`, then an in-memory map (the
  last 32), then `<cache_dir()>/peaks/<hex>.m2pk` (`resources::cache_dir`), else `m2_runtime::audio::load_wav`,
  computed, written whole through a temp file and a rename. `bytes()` for the route.
- `routes_audio.rs`: `GET /api/graph/audio/peaks?session=&asset=` → `application/octet-stream`: the asset name
  checked as an `AssetName`, the file under the open song's `assets/`, `peaks::of` on `spawn_blocking`. A read:
  allowed in a read-only window.
- Web: `lib/peaks.ts` (fetch once per asset and pin, kept in a map, `usePeaks(asset, hash)`), `components/Wave.tsx`
  (canvas tiles of 1,024 px, drawn only while visible, from the clip's span in source seconds: the tempo map gives
  each pixel's song seconds, then `offset` plus the seconds into the clip, read backwards when reversed),
  `SongTimeline.tsx`: the audio branch of `inside()` (`:2257-2262`) draws `<Wave …/>` in place of the icon row, and
  `words()`' audio line.

**Tests.**
- `peaks.rs`: `peaks_are_min_and_max_per_256_frames` (a ramp and a sine), `a_mono_file_has_one_channel`,
  `peaks_are_made_once_and_kept_by_content` (a second ask decodes nothing; a copy of the file under another name
  reads the same entry), `a_cache_file_that_doesnt_read_is_made_again`, `a_file_that_doesnt_decode_says_why`,
  `the_cache_is_under_home` (PLAT-7).
- `routes_audio` test: `peaks_never_wait_for_the_actor` (the actor held by a slow job; the peaks answer). Guard.
- Suite `st-audio`, part 1 (kind `fix`; a WAV written by `wav()` into the song's `assets/` and a lane file naming
  it): the clip has a canvas whose middle rows have ink; a silent file draws a flat line; a ramp's reversed clip is
  its mirror; one peaks request per asset however often it redraws; lint; tips. Red first on `dist-before`: no
  canvas.

**Files.** `crates/server/src/{peaks.rs, routes_audio.rs}`; `web/src/lib/peaks.ts`, `components/Wave.tsx` (new);
`SongTimeline.tsx` (A's block); `e2e/st-audio.mjs` (new).

### A2. Bring audio in — M (after P0)

**What the user sees.**
- **File ▸ Import Audio…** (⇧⌘I, Ctrl+Shift+I): a file dialog, "Import Audio", WAV files, several allowed. They land
  as Q7 says, and the new clip is selected.
- **Drop** WAV files from the Finder or the Files app (Q6). While dragging, the hint: "Vox · bar 5", "New audio track ·
  bar 5", "3 new audio tracks · bar 5", or, for other files, "music2 opens WAV and MIDI files".
- **+ Track** (⌘T): the Audio group (Q8). Typing "audio" finds it. Its foot line: `tracks/audio.ts ·
  lane.audio("clips")`.
- Afterwards: one Undo step ("Undo importing vox.wav"); a toast only when a track was added ("Added Vox, playing
  vox.wav", with Undo) or the file was changed on the way in (Q1's channels).
- Refused, in a toast, with nothing written: not a WAV (Q1); "vox.wav doesn't read as a WAV file (<why>)"; "vox.wav is
  longer than 15 minutes: music2 keeps audio in memory for now, so it takes files up to 15 minutes."; a read-only
  window (the window's own words); a new track while the code doesn't compile: "The code doesn't compile, so no track
  can be added now. Drop it on an audio track, or fix the code first."

**The back end.**
- `core/song/src/path.rs`: `AssetName::for_import(file_name) -> AssetName` (Q2; A's block).
- `crates/editor/src/import.rs`:
  - `pub struct Imported { pub asset: AssetName, pub hash: Sha, pub seconds: f64, pub dropped_channels: u16 }`.
  - `pub fn audio(dir: &Path, from: From) -> Result<Imported, EditError>` with `From::Path(PathBuf)` or
    `From::Bytes { name, bytes }`: the `RIFF`/`WAVE` check; `m2_os::fs::make_dirs(assets)`; the same bytes already
    there (size first, then hash) → that asset; else Q3's part file (`clone_file`, else a copy), `push`, `load_wav`
    (refused over 15 minutes), `name_new` to the name, then `name 2`…, `sync_dir`. A failure leaves nothing;
    `sweep` clears old part files.
  - `pub fn place(s, files: &[Imported], at: Target) -> Result<Placed, EditError>`; `Target::Lane { lane, beat }`
    is one `clips::edit(Gesture::Set { lane, put })`, the clips one after another; `Target::After { module, beat }`
    and `Target::End { beat }` are one `tracks::add_tracks` with `Play::Audio`, a track per file named after it
    ("Lead vox" for `lead vox.wav`), each with its clip. A clip: `{start, length, asset, hash: pin, offset: 0,
    gain_db: 0, fade_in: 0, fade_out: 0, reverse: false, pitch: 0, warp: []}`, its length by Q5.
- `crates/server/src/graph/import.rs`: `Graph::import_audio(token, sources, target)`: the copies and their peaks on
  the calling thread (a blocking task), then one actor job for `place`, then the step's name ("importing vox.wav",
  `name_step`, `graph/steps.rs:83`).
- `routes_audio.rs`: `POST /api/graph/audio/import`: JSON `{session, paths, to}`, or the file's bytes as the body with
  `?session=&name=&to=` (`DefaultBodyLimit::max` 1 GiB on this route only); `POST /api/graph/audio/track {session,
  label?, after?}`. Both write routes: 423 read-only.
- Desktop: `menu.rs` `import_audio` ("Import Audio…", `CmdOrCtrl+Shift+I`) in File after Save As, under a line;
  a test `import_audio_is_shift_command_i_in_file`.
- Web: `lib/audioImport.ts` (new): registers `.wav` and `.wave` with the drop layer (its hint words, its drop);
  `importAudio(target)` for the menu: the app's `nativeAction("open_file", …)`, or a hidden
  `<input type=file multiple accept=".wav,.wave,audio/wav">` in a browser; the selection after. `Tracks.tsx`: the
  Audio group in `InstrumentPicker` when not changing an instrument (A's block). `App.tsx`: `case "import_audio"`.
  `Shortcuts.tsx`: File "Import audio: ⇧⌘I".

**Tests.**
- `crates/editor/src/import_tests.rs`: `an_import_copies_the_file_create_only`, `a_taken_name_gets_a_number`,
  `the_same_bytes_use_the_asset_there`, `a_name_is_made_safe_and_nfc`, `a_file_that_isnt_wav_is_refused_in_words`,
  `a_wav_that_doesnt_decode_leaves_nothing`, `a_file_over_15_minutes_is_refused`,
  `the_asset_is_whole_before_the_tx_names_it` (the Tx's planner reads the asset's full bytes and its folder lists it
  before `place` runs), `a_drop_on_an_audio_lane_is_one_clips_tx`,
  `a_drop_below_the_tracks_adds_a_track_and_its_clip_in_one_step` (one undo takes both),
  `a_drop_on_a_notes_track_adds_an_audio_track_after_it`, `the_clip_is_the_files_length_at_the_tempo_there` (120 BPM, 2
  s: 4 beats; over a tempo change too).
- `core/song`: `for_import_names_are_asset_names` (a table of awkward names).
- `crates/server/src/graph/import_tests.rs`: `an_imported_clip_plays_its_file` (an offline render after the import
  has the file's samples at the drop beat, bit for bit: a 48 kHz float WAV at unity gain),
  `a_path_and_an_upload_import_alike`, `a_read_only_window_refuses_an_import`, `the_peaks_are_ready_when_it_answers`.
- Suite `st-audio`, part 2: + Track ▸ Audio track (its file and lane); ⇧⌘I through the file chooser lands at the
  playhead's bar on the selected audio track; a drop below the tracks (app page, a real path) makes "Tone" at bar 3
  with its waveform; a drop on Kick (notes) makes an audio track after Kick; a browser-page drop on the audio lane at
  bar 5 uploads and lands there; the hints' words; an `.mp3` drop says Q1's words and writes nothing; `assets/` holds
  exactly the files; the lane file is canonical; one Undo takes each import back; the PC page's Ctrl+Shift+I; lint;
  tips. Red first on `dist-before` and the before server: no Audio group, ⇧⌘I does nothing, a drop does nothing.
- Moves: `s8-tracks` (the list's groups), `pl-tracks`, `pc-keys` (the sheet), `taste`.

**Files.** `core/song/src/path.rs` (A's block); `crates/editor/src/{import.rs, import_tests.rs}`;
`crates/server/src/{routes_audio.rs, graph/import.rs, graph/import_tests.rs}`; `apps/music2-desktop/src/menu.rs`
(A's rows); `web/src/lib/audioImport.ts` (new), `Tracks.tsx`, `App.tsx`, `Shortcuts.tsx` (A's blocks);
`e2e/st-audio.mjs`.

### A3. Edit audio clips — M

**What the user sees.**
- Move, ⌥-drag to copy, ⌘D, ⌘E, ⌘C ⌘V, ⌫ and Repeat… as with notes clips; heard on release (Q14).
- Trim either edge: the left edge slides where the audio starts in the file, so what is left plays where it did; the
  right edge stops at the file's end (Q13). Reversed clips trim and split too (Q12).
- Fades (Q10), Gain… (Q11), Reverse (Q12), and **Remove fades** (greyed with none) in the audio clip's menu, after
  Split. Undo names (Q16).

**The back end.**
- `crates/editor/src/clips.rs` (A owns it this track):
  - `Gesture::Audio { clip: ClipRef, gain_db: Option<f64>, fade_in: Option<f64>, fade_out: Option<f64>,
    reverse: Option<bool> }` (tag `"audio"`): only an audio clip; gain in −48 to +12; fades ≥ 0 and together at most
    the clip's seconds under the tempo map; one Tx, `Confined { lanes }`, as every gesture.
  - `move_left_edge` (`:530`) and `split` (`:559`) for a reversed clip: the left edge keeps `offset` and the right
    edge moves it; a split's first part gets `offset + (seconds − cut)` and the second keeps `offset`. `audio_edge`
    refuses only a pitched clip.
  - `Resize`: an unwarped, unpitched audio clip's right edge refused past the file's end (`asset_seconds`: `hound`'s
    header, no decode).
- Web: `Wave.tsx`: the two handles (pointer down stops the clip's own drag), the bubble, `edits.audio(…)` on release;
  `lib/clips.ts` (A owns it): the `audio` gesture; `SongTimeline.tsx`' `TimelineMenu`: the audio rows (A's block),
  `RepeatPopover`'s style for the Gain popover (`GainPopover`, in `Wave.tsx`' file or its own).

**Tests.**
- `clips_tests.rs`: `fades_gain_and_reverse_are_one_tx_each`, `fades_never_pass_the_clip`,
  `gain_stays_in_its_range`, `a_reversed_clip_trims_from_either_edge_and_plays_the_same_samples` (the lane renders
  the same bits where both play, before and after; red first: today's refusal),
  `a_reversed_clip_splits_into_the_same_samples`, `the_right_edge_stops_at_the_files_end`,
  `only_an_audio_clip_takes_audio_settings`.
- Suite `st-audio`, part 3: move, copy, left trim (`offset` grows in the file), right trim stops at the end, ⌘E,
  ⌘D, ⌫, each one Tx with its Undo name; a fade handle dragged writes `fade_in` and the Undo tooltip says "Undo Vox
  fade in"; Gain… −6 writes `gain_db: -6` and the waveform halves; Reverse writes `reverse: true` and mirrors it;
  Remove fades; lint of the handles, the menu and the popover; tips. Red first on `dist-before`: no handles, no rows.
- Moves: `s8-clips`, `s8-leases` (audio clip drags), `taste`.

**Files.** `crates/editor/src/{clips.rs, clips_tests.rs}`; `web/src/components/Wave.tsx`, `lib/clips.ts`,
`SongTimeline.tsx` (A's blocks); `e2e/st-audio.mjs`.

### A4. `m2 import` — S (optional; the first cut)

`m2 import <song> <file.wav>… [--track KEY] [--at BAR]`: `import::audio` and `place`; it takes the lock (WRITE-18's
list gains it). `apps/m2/tests/import.rs`: `import_places_a_clip_and_takes_the_lock`. Gaps #37's first item.

**Lane A's order:** A1 (Day 1 morning), A2 (Day 1 afternoon, after merging P0), A3 (Day 2 morning), A4 if time.
**If late:** A4 goes, then Remove fades, then the Gain popover's ⇧ steps.

---

## 4. Lane B: record what you play — `wt/st-b`, branch `studio-b`

Gaps #24's first half, #19's count-in, and the MIDI keyboard picker.

### B1. The count-in, in the engine — M (needs nothing from P0)

**What the user sees.** Nothing alone. With B3: one bar of clicks, then the song plays and recording starts.

**The back end** (8a's rule, §1.9):
- `core/runtime/src/click.rs`: `Click` gains a count: beats left, frames per beat, frames to the next click and to
  the bar's end. `count_in(beats, frames_per_beat)` lays the bar so it ends on a block line: the first click comes
  `ceil(bar / BLOCK) × BLOCK − bar` frames into the first block. While counting, `mix` makes the clicks itself from
  its own frames (the transport is stopped), the first accented, plain fields, no allocation. `count_ended()` is true
  once, for the block the bar ends with. `counting()`.
- `core/runtime/src/engine.rs`: `Msg::CountIn`, applied at once (it wakes an idle engine, as any message does); a
  Stop or a hold cancels it; an engine never idles while counting; after the click's mix, a count that ended starts
  the transport (`Cmd::Play`), so the song's first block is the one right after the bar.
- `core/play/src/lib.rs`: `Player::count_in(beats, frames_per_beat) -> Result<u64>`.

**Tests.**
- `core/runtime` (`click/tests.rs`): `a_count_in_clicks_one_bar_then_plays` (120 BPM 4/4: clicks at its frames 0,
  24,000, 48,000, 72,000 from the first; the transport plays from frame 96,000 on, at a block line, at the start
  point), `a_count_in_follows_the_meter` (3/4; 6/8 counts six eighths), `the_count_accents_its_first_click`,
  `stop_cancels_a_count_in`, `a_count_in_wakes_an_idle_engine`, `no_count_in_changes_no_bit` (guard).
- `core/play/tests/no_alloc_count.rs`: `the_count_in_never_allocates`.
- Red first: `count_in` a stub: no clicks and nothing plays.
- The renders: 10 of 10.

**Files.** `core/runtime/src/{click.rs, click/tests.rs, engine.rs}`, `core/play/src/lib.rs`,
`core/play/tests/no_alloc_count.rs`.

### B2. The recorder, on the server — M (needs nothing from P0)

**The back end.**
- `crates/server/src/graph/record.rs`: `ControlPlane.record: Option<Take>` (the open song's token, the lane, the
  start beat, the options, the notes so far, the notes held by `(source, handle)`, the last beat seen).
  - `Graph::record_start(token, lane, count_in, quantize) -> Result<Value>`: refuses with no notes lane (Q21), a
    read-only window, or a take already running. Stopped: the start beat is where play starts; with a count-in, one
    `Player::count_in` from the tempo and meter there (Q20), answering `{count_in: {beats, ms}}`; without, a Play.
    Playing: the start is the bar line at or before the beat now.
  - `Graph::recorded(source, ev)`: called by `audition()` for each On and Off it sends on the take's lane from a
    recording source (a `midi:` source, or a window's message with `record: true`): stamped by Q22
    (`Player::status().beats`, `Player::frames()`, the tempo map that plays).
  - `Graph::record_end(why)`: Stop, R, a wrap or jump (Q27), or the song closing (dropped, nothing written). Held
    notes end there; with quantize, starts go to the grid sent at start (B's own rounding, tested equal to the
    Quantize gesture's); the clip by Q23; then one actor job: `clips_as(token, Gesture::Set { lane, put: [clip] },
    None)` (`graph/clips.rs:31`) and `name_step(…, "recording on Bass", false)`. Answers `{revision, made}` or
    `{empty: true}`.
  - A thread (B's, in `lib.rs`): every 100 ms while a take runs, its `{"t": "take", module, lane, start, end, notes}`
    on a latest-only channel of its own (as `spawn_meters`, `lib.rs:301`), and the wrap or jump check.
- `graph/audition.rs`: `Audition.record: bool` (serde default false); one call to `recorded` per On and Off sent.
- `graph/midi.rs`: a keyboard in `settings.midi.off` plays and records nothing; `midi_state` says `off: true` for it.
- `graph.rs`: `ControlPlane.record` at the struct's end; `state["record"]` (`{phase: "count" | "take", module, lane,
  start}`); one line at the top of `transport` (`:1631`) and of `stop_to` (`:1650`): a stop ends the take first.
- `settings.rs` and `routes_settings.rs` (B owns both this track): `record: {count_in: true, quantize: false}` and
  `midi: {off: []}`, kept and answered by `GET`/`POST /api/settings`.
- `routes_record.rs`: `POST /api/graph/record/start {session, module, lane, count_in?, quantize?}`, `/stop
  {session}`, `/cancel {session}`. Write routes: 423 read-only.

**Tests** (`crates/server/src/graph/record_tests.rs`, a driven player):
`a_take_records_midi_notes_where_they_were_heard` (notes at known positions; the buffer taken off),
`piano_notes_marked_record_are_recorded_and_audition_is_not`, `stop_writes_one_tx_named_recording_on_bass` (the
Undo name), `held_notes_end_at_the_stop`, `the_take_spans_whole_bars`, `an_empty_take_writes_nothing`,
`quantize_puts_starts_on_the_grid` (the same notes as `Gesture::Quantize`), `a_loop_wrap_writes_the_first_pass`
(play goes on), `a_jump_ends_the_take`, `a_count_in_then_record_starts_at_the_start_point`,
`a_keyboard_turned_off_neither_plays_nor_records`, `record_needs_a_notes_lane`, `closing_the_song_drops_the_take`,
`a_read_only_window_cant_record` (423), `recording_never_waits_for_a_compile` (guard). Settings:
`record_options_and_keyboards_are_kept`. Red first: the start route 404 on the before server; `recorded` a no-op:
every take empty.

**Files.** `crates/server/src/graph/{record.rs, record_tests.rs, audition.rs, midi.rs}`, `graph.rs` (B's blocks),
`settings.rs`, `routes_settings.rs`, `routes_record.rs`, `lib.rs` (B's block).

### B3. The Record button and the take on screen — M (after P0)

**What the user sees.** Q17 to Q27. The keyboard piano's card says "Plays Bass · recording" while a take runs.

**The back end.** Web only, and one menu row.
- `web/src/lib/record.ts` (new): the state from `graph.record` and the `take` events; `startRecording()`
  (`playTarget.ts`' lane, the settings' options, Notes' grid in beats for quantize), `stopRecording()`,
  `cancelRecording()`; the count-in's beats left from the answer's `{beats, ms}` and the page's clock; a
  capture-phase Esc listener while counting (as `next`'s X34); after a take is written, its clip selected.
- `components/RecordButton.tsx` (new): Q17's button and Q18's menu (`PointMenu` rows). `TopBar.tsx`: after Play /
  Stop (B's block).
- `components/Take.tsx` (new): the growing clip, in its lane, from the take's start to the playhead
  (`clock.ts`). `SongTimeline.tsx`: one line in `LaneClips` (`:2211`) renders `<Take module lane />`, and `Notes`
  (`:2439`) is exported for it.
- `lib/piano.ts`, `lib/audition.ts` (B owns both this track): the piano's strikes carry `record: true`.
  `KeyboardPiano.tsx`: "· recording".
- `App.tsx`: `r: "record"` in `key()` (`:62`), `case "record"`. `Shortcuts.tsx`: Transport "Record: R".
- Desktop `menu.rs`: `ITEMS` `record` ("Record", no accelerator) after `metronome`; Transport: after Go to Start; a
  test `record_is_in_transport`.

**Tests.** Suite `st-record` (kind `fix`, about 45 checks; notes posted to `/api/graph/midi/inject`; a second part
on a capture server of its own, as `nx-click` does, for the count-in's clicks):
- the button after Play, 28 px, its tooltip; its right-click menu's rows and checks; the options kept in
  `settings.json`;
- R stopped: the server's `record.phase` is `count`, the button shows 4 then 1; then playing; injected notes at known
  moments make a growing `Take` with those notes; Space: one Tx, the lane file has one new clip with them, whole
  bars, selected; "Undo recording on Bass"; ⌘Z takes it out;
- the keyboard piano (⌘K, then A) records; a click on Notes' keys doesn't;
- quantize on: starts on 1/16; looping: the take is written at the wrap and play goes on;
- R while playing records from now with no count-in; Esc while counting cancels; nothing played: the toast;
- Settings' MIDI keyboards (B4); the PC page's R; lint; tips;
- capture part: four clicks a beat apart, the first louder, then the song's first beat a bar after the first click.
Red first on `dist-before` and the before server: no button; R does nothing; the start route is 404.
Moves: `s8-songbar` and `fx-fixes` (the song's name at 1100 and 1180 px: §3.8's order), `taste`, `pc-keys`,
`nx-piano`, `nx-midi`, `s8-audition`, `nx-click`.

**Files.** `web/src/lib/record.ts`, `components/{RecordButton.tsx, Take.tsx}` (new); `TopBar.tsx`,
`SongTimeline.tsx`, `App.tsx`, `Shortcuts.tsx` (B's blocks); `lib/{piano.ts, audition.ts}`, `KeyboardPiano.tsx`;
`apps/music2-desktop/src/menu.rs` (B's rows); `e2e/st-record.mjs`.

### B4. Which MIDI keyboard plays — S

**What the user sees.** Q28, in Settings (⌘,) after Buffer size.

**The back end.** B2's settings and `midi.rs`. Web: `SettingsDialog.tsx`, `lib/settings.ts` (B owns both): the
rows, from `graph.midi.devices` and the settings' `off` list; a switch writes `POST /api/settings {midi}` at once.
`MidiStatus.tsx`' tooltip names only the keyboards that play.

**Tests.** `st-record`: "Test Keys" off: an injected note plays nothing (`live` 0) and records nothing; on again:
it plays; the row says "(not connected)" after unplugging. Moves: `nx-settings`, `nx-midi`.

**Files.** `SettingsDialog.tsx`, `lib/settings.ts`, `MidiStatus.tsx`; `e2e/st-record.mjs`.

**Lane B's order:** B1 and B2 (Day 1), B3 (Day 2 morning, after merging P0), B4. **If late:** B4 goes, then the
capture part of `st-record`. Q27 stays: without it a loop would record over itself.

---

## 5. Lane C: MIDI files and stems — `wt/st-c`, branch `studio-c`

Gaps #36.

### C1. MIDI files, read and written; MIDI export — M (needs nothing from P0 but its stubs)

**What the user sees.** The clip's menu on a notes clip: **Export as MIDI…** (after Duplicate). The Export dialog's
MIDI (C3). A toast: "Saved Bass as a MIDI file: first-song Bass.mid", with Show in Finder (Show in Folder off the
Mac).

**The back end.**
- `crates/editor/src/smf.rs`: `Smf { format, ppq, tracks: Vec<SmfTrack> }`, `SmfTrack { name, events: Vec<(u64,
  Event)> }`, `Event::{NoteOn, NoteOff, Program, Tempo, Meter, Name, Other}`; `read(&[u8]) -> Result<Smf, SmfError>`
  (words with the byte where it failed); `write(&Smf) -> Vec<u8>` (Q31).
- `crates/editor/src/midifile.rs`: the sound table (Q33) and `program_for(component)`;
  `export_song(rev) -> Vec<u8>` and `export_clip(rev, &ClipRef) -> Vec<u8>` (Q37).
- `routes_exchange.rs`: `POST /api/graph/midifile/export {session, path?, clip?}` → `{path}`: a new file in Exports
  (`export::exports_dir`, a free name), or the path picked (replaced, as Export replaces a picked file).
- Web: `lib/midiFiles.ts` (new): `exportMidi(clip?)` (the app's `save_file`, else straight to Exports); the clip menu
  row in `SongTimeline.tsx`' `TimelineMenu` (C's block).

**Tests.**
- `smf_tests.rs`: `reads_format_0_and_1`, `running_status_and_sysex`, `a_note_without_an_off_ends_at_the_next_on`,
  `tempo_and_meter_events`, `an_smpte_division_is_refused_in_words`, `format_2_is_refused`,
  `a_cut_short_file_says_where`, `writes_what_it_reads`.
- `midifile_tests.rs` (export): `a_clip_round_trips`, `the_song_has_a_tempo_track_and_a_track_per_lane`,
  `a_drum_kit_lane_is_channel_10`, `notes_are_cut_at_their_clips_edges`, `muted_in_song_tracks_are_left_out`,
  `velocity_and_pitch_are_rounded_into_midi`.
- Red first: the export route 404 on the before server; the tests themselves are new code (guards where nothing
  existed: listed so).

**Files.** `crates/editor/src/{smf.rs, smf_tests.rs, midifile.rs, midifile_tests.rs}`,
`crates/server/src/routes_exchange.rs`, `web/src/lib/midiFiles.ts` (new), `SongTimeline.tsx` (C's block).

### C2. Import a MIDI file — M (after P0)

**What the user sees.** File ▸ Import MIDI… (no key): a file dialog for `.mid` and `.midi`; or a drop (Q36). Then
new tracks with fitting sounds (Q32, Q33), their notes in clips (Q34), the tempo by Q35 (its question as a page
dialog, `MidiTempoAsk`). A toast: "Imported groove.mid: Drums, Bass and Keys", with Undo. Undo: "Undo importing
groove.mid". Refused in words, nothing written: not a MIDI file ("groove.mid isn't a MIDI file: it doesn't start with
MThd."), an SMPTE file, no notes ("groove.mid has no notes."), Q32's limit, the code doesn't compile.

**The back end.**
- `midifile.rs`: `pub fn import(s, file: &Smf, name: &str, at: Target, tempo: Tempo) -> Result<Imported | Asks>`:
  the parts (Q32), each a `NewTrack` with `Play::Instrument(component)` and its clip, then one
  `tracks::add_tracks(…, arrangement)` (P0); a one-track file onto a notes lane is one `clips::edit(Gesture::Set)`.
  `Asks { file_bpm, song_bpm }` when Q35 asks and no answer came.
- `crates/server/src/graph/exchange.rs` (P0's stub, C's): an `impl Graph` block, `Graph::import_midi(token, file,
  name, at, tempo)`, one actor job as `graph/clips.rs` runs a gesture, then the step's name.
- `routes_exchange.rs`: `POST /api/graph/midifile/import`: JSON `{session, path, at, tempo?}` or the bytes with
  `?session=&name=&at=&tempo=`; answers `{asks}` or `{revision, tracks}`; names the step "importing groove.mid".
- Desktop `menu.rs`: `import_midi` ("Import MIDI…", no accelerator) after A's `import_audio`; a test.
- Web: `lib/midiFiles.ts` registers `.mid` and `.midi` with the drop layer (hint: "groove.mid: 3 tracks · bar 1", or
  "Into Bass · bar 3"); `importMidi()` for the menu; `components/MidiTempoAsk.tsx` (new, mounted in `App.tsx` after
  `<ExportAsk />`); `App.tsx` `case "import_midi"`.

**Tests.**
- `midifile_tests.rs` (import): `each_track_and_channel_becomes_a_track`, `channel_10_plays_the_drum_kit`,
  `programs_pick_std_sounds`, `no_program_picks_by_pitch`, `an_empty_song_takes_the_files_tempo` (the arrangement in
  the same Tx), `a_song_with_clips_asks_then_keeps_or_takes_the_tempo`, `one_undo_step_takes_the_import_back`,
  `a_one_track_file_dropped_on_a_notes_lane_goes_into_it`, `more_than_32_parts_is_refused`,
  `an_exported_song_imports_as_the_same_notes` (C1's export, then import: every note back, as beats).
- Suite `st-midi` (kind `empty`, about 40 checks; `smf()` writes the files): Import MIDI… through the chooser (app
  page: `open_file` answers): "Drums" plays the Drum kit and "Bass" the Synth bass, with their notes; the empty
  song's tempo is the file's 128; one Undo takes all back; on a song with clips the question says both tempos, Return
  keeps 120 and "Use 128" writes it; a one-track file dropped on Bass at bar 3 goes into Bass; a text file named
  `.mid` says its words; the clip menu's Export as MIDI… writes a file that reads back to the clip's notes (the suite
  reads SMF); lint; tips. Red first on `dist-before`: no menu command, the drop does nothing.

**Files.** `crates/editor/src/midifile.rs`; `crates/server/src/{routes_exchange.rs, graph/exchange.rs}`;
`apps/music2-desktop/src/menu.rs` (C's rows); `web/src/lib/midiFiles.ts`, `components/MidiTempoAsk.tsx`,
`App.tsx` (C's block); `e2e/st-midi.mjs`, `e2e/droplib.mjs` (`smf()`, at its end).

### C3. Stems, and the Export dialog — M

**What the user sees.** Q42's dialog; Q39's stems (Q40 greyed with plugins). Progress as today, the pill and the
ring, "Exporting stems: Hats (2 of 4)". Done: "Exported 4 stems to first-song stems", with Show in Finder. The PC
words: Ctrl+Shift+E.

**The back end.**
- `crates/editor/src/render.rs` (C owns it this track, but P0's three words): `pub fn rooted(ir, track, export) ->
  Result<Ir>` (moved from `apps/m2/src/main.rs:481-496`), `pub fn bounce_seconds(data) -> f64`,
  `Render::write_float(path)` (32-bit float, as `write_new_wav` writes it, through a part file and `name_new`).
  `m2 bounce` calls `rooted` (its arm in `apps/m2/src/main.rs`: C's block); its output keeps every byte (checked
  before and after on first-song's four tracks; the hashes in the build notes).
- `crates/server/src/stems.rs`: `stems::start(state) -> Result<PathBuf>`: refused with plugins (Q40) or while an
  export runs; the folder; for each track in the layout's order not muted in the song: `rooted`, `m2_ir::check`,
  `Render::new(…, 0.0, Length::Seconds(bounce_seconds), export_threads(…))`, `write_float("<n> <Label>.wav")`;
  progress and done as `export` events with `stem`, `of`, `stems`. It takes `Exports`' slot (`export.rs:41`: one
  `pub(crate)` method there, C's block).
- `routes_exchange.rs`: `POST /api/graph/stems {session}`.
- Web: `components/ExportDialog.tsx` (new, mounted after `<ExportAsk />`); `lib/exporting.ts` (C owns it):
  `exportSong()` opens the dialog; the song path as today after it; stems; MIDI through C1. `menu.rs`: `export`'s
  title "Export…" (its test follows). `Shortcuts.tsx`: File "Export: ⇧⌘E".

**Tests.**
- `crates/editor` (`render.rs`' tests): `a_stem_renders_what_m2_bounce_renders` (for each of first-song's four
  tracks, `Render` with `rooted` and `write_float` gives the same samples, bit for bit, as `m2 bounce`'s own loop:
  `Renderer::new`, `seek_on`, `render_on`, `write_new_wav`). `apps/m2/tests/render.rs` gains
  `bounce_of_a_track_still_writes_its_file` (the arm after the refactor). `m2 bounce`'s files before and after the
  refactor: the same bytes, by hand, the hashes in the build notes.
- `crates/server/src/stems.rs` tests: `stems_write_one_file_per_track_in_a_folder`,
  `stems_leave_out_tracks_muted_in_the_song`, `stems_of_a_song_with_plugins_are_refused_in_words`,
  `one_export_at_a_time` (stems while exporting, and the reverse).
- Suite `st-stems` (kind `fix`, about 20 checks): ⇧⌘E opens the dialog, Song chosen; Return exports the song as
  before; Stems writes `first-song stems/1 Kick.wav`… and each equals `m2 bounce` of its track (the suite runs `m2`
  and compares the data chunks); MIDI writes `first-song.mid`; a song with a `plugin("clap:…")` call greys Stems with
  Q40's words; Esc closes; Tab order; lint; tips. Red first on `dist-before`: no dialog.
- Moves: `export` (Return after Export), `s8-mute` (the question after the dialog), `live3b` (its export click),
  `pc-keys` (the sheet's Export row), `taste`.
- The renders: 10 of 10.

**Files.** `crates/editor/src/render.rs`, `apps/m2/src/main.rs` (the bounce arm), `apps/m2/tests/render.rs`;
`crates/server/src/{stems.rs, export.rs (one method), routes_exchange.rs}`; `web/src/components/ExportDialog.tsx`,
`lib/exporting.ts`, `Shortcuts.tsx` (C's row); `apps/music2-desktop/src/menu.rs`; `e2e/st-stems.mjs`.

**Lane C's order:** C1 (Day 1), C2 (Day 1 afternoon, after merging P0), C3 (Day 2 morning). **If late:** the
clip menu's Export as MIDI… goes (the dialog's MIDI stays), then Q36's drop into a lane.

---

## 6. Lane D: plain-language help — `wt/st-d`, branch `studio-d`

Gaps #11.

### D1. A tooltip on every control — M (first thing, Day 1)

**What the user sees.** Hold the mouse over any button, knob, fader or switch: a short line in plain words says what
it does, and its key. "Loop: play the same bars round and round (C)". "Mute Kick while you listen (M)". "Bass
volume: drag up or down". Done first, before the other lanes' screens grow.

**The back end.** Web only.
- `e2e/tiplib.mjs` (P0's; D owns it after).
- Titles added where missing, across `web/src/components/*.tsx`, as attribute-only hunks (§7.2). A title that is
  jargon may be reworded; then every suite that reads it moves with it. No other change to those files.

**Tests.** Suite `st-tips` (kind `fix`, about 30 checks, one per place): the start screen; the top bar; a track
header; the timeline's tools and grid chip; the Device view and a card; Notes and its tools; Mix and a strip; Code
and its file list; New Song, Save As, Rename Song, Delete, Change track ID, Settings and the ⌥K sheet; + Track's list;
the status, overlay and key popovers; the keyboard piano card; each `tips()` empty. Lanes A, B and C's places are
added at the End, once joined (the Export dialog, the Record button, the guide…). Red first on `dist-before`: the
list of controls without one (its count and the first ten in the build notes).
Moves: `pc-keys`, `taste`, any suite reading a reworded title.

**Files.** `web/src/components/*.tsx` (titles only); `e2e/st-tips.mjs` (new), `e2e/tiplib.mjs`.

### D2. Errors in plain words — M

**What the user sees.** The compile banner, the status popover's problems and the Code view's problem list lead
with a plain sentence and a fix, and point at your own line. "**Cutoff wants a frequency (Hz), but gets a level
(dB).** tracks/bass.ts, line 5. Write it in Hz, such as hz(800)." Under **Details**: ``input `cutoff` expects
control<hz>, got control<db>`` · `code.check` · `std/src/filter.ts:…`.

**The back end.**
- `core/ir/src/lib.rs`: `Diagnostic.inner: Option<String>` (serialized only when set).
- `core/session/src/lib.rs`, `build_code` (`:173`): a `code.check` error with a `node` gets `src` = the innermost
  project call among the node's frames (Q46), and its old `src` as `inner` (D's block).
- `crates/editor/src/explain.rs`: `pub struct Plain { pub words: String, pub fix: Option<String> }`;
  `pub fn explain(code, message, catalog) -> Option<Plain>`, the table below; the units' words in one place
  (Hz "a frequency (Hz), such as hz(800)", dB "a level (dB), such as db(-6)", seconds "a time, such as ms(250) or
  secs(1)", semitones "semitones, such as semis(7)", beats "beats, such as beats(1)", 0–1 "a number from 0 to 1",
  −1–1 "a number from −1 to 1").

  | Code and message | Words | Fix |
  |---|---|---|
  | `code.check`, a unit mismatch (`expects control<A>, got control<B>`) | "{Label} wants {A's words}, but gets {B's}." (the control's label from the component's descriptor, else the port's name) | "Write it in {A}, such as …" |
  | `code.check`, an input with nothing in it (`is not connected`) | "{Component} needs something to work on: its {X} input is empty." | "Give it a sound first, such as `reverb(bass.out)`." |
  | `code.check`, mono and stereo (`is mono … but a stereo signal is connected`) | "{X} takes one channel, but gets two." | "Make it mono first, or use the stereo version." |
  | `code.build`, a name not known (QuickJS's `ReferenceError`) | "`x` isn't known in {file}." | When std exports `x`: "Import it: `import { x } from "std/…"`." Else: "Check its spelling." |
  | `code.build`, a parse error (oxc's `Expected …`) | "There is a typo on line N of {file}." | "Check the brackets, commas and quotes on that line and the one before." |
  | `code.build`, an import that resolves to nothing | "{file} brings in `./tracks/bas`, which isn't there." | "The track files are: kick, bass, keys." |
  | `code.build`, `is not a function` | "`x` is used like an instrument or effect on line N, but it isn't one." | "Check the name." |
  | `pkg.undeclared` | "{file} uses {pkg}, but music2.toml doesn't list it." | "Add `{pkg} = "…"` under `[dependencies]` in music2.toml." |
  | `key.duplicate` | "Two tracks have the ID kick." | "Give one another ID: in Code, right-click its file, Change track ID…." |
  | `plugin.missing` | "Surge XT isn't installed on this computer, so it plays nothing." | "Install it, or pick another instrument: Change Instrument…" |
  | `asset.missing` | "vox.wav isn't in the song's assets folder, so its clip plays nothing." | "Put the file back, or delete the clip." |
  | `lane.invalid` | "data/bass/notes.json can't be read, so Bass plays what it had." | "Undo the last change to it, or fix the file." |

  The crash and out-of-memory words are 8a's: D leaves them as they are.
- `crates/server/src/files.rs`: `Problem` gains `plain`, `fix`, `inner`; `problem()` (`:219`) fills them.
  `graph/actor.rs`: one hunk where the state's and the news' `problems` are made (`:663`, `:673`): each gains
  `plain` and `fix` (D's block).
- Web: `lib/plain.ts` (new; the `Problem` fields by `declare module "./graph"`); `Status.tsx`' `Compile`
  (`:229`), `Attention.tsx` and `CodeView.tsx`' list: the plain words and the fix first, the original under
  `Details`.

**Tests.**
- `explain_tests.rs`, one per row, each on a real broken song built through the editor's test helpers (so a change
  in core's wording fails it): `a_unit_mistake_says_what_it_wants`, `an_empty_input`, `mono_and_stereo`,
  `a_name_not_known_proposes_its_import`, `a_typo_names_its_line`, `a_missing_file_lists_the_tracks`,
  `not_a_function`, `an_undeclared_package`, `two_tracks_with_one_id`, `a_missing_plugin`, `a_missing_asset`,
  `a_lane_file_that_doesnt_read`, `an_unknown_message_keeps_its_words`.
- `core/session`: `a_check_error_points_at_the_call_in_the_project` (in `tracks/bass.ts`, and through a component
  of the song's own in `lib/`: the innermost project call). Red first: today's `src` is `std/src/filter.ts`.
- `files.rs`: `diagnostics_carry_plain_words_and_the_technical_text`.
- Suite `st-errors` (kind `fix`, about 25 checks): `db(-3)` written as a lowpass cutoff in `tracks/bass.ts`: the
  banner says Q45's words and "tracks/bass.ts, line 5"; Details shows the original; Show in code opens bass.ts at line
  5, marked; `pluk(…)`: "isn't known… `import { pluck } …`"; a missing plugin in the popover; a deleted asset; lint;
  tips. Red first on `dist-before` and the before server: the old words, `std/src/filter.ts`.
- Moves: `s4-problems`, `s4-attn`, `attention`, `code`, `taste`.

**Files.** `core/ir/src/lib.rs`, `core/session/src/lib.rs` (D's blocks); `crates/editor/src/{explain.rs,
explain_tests.rs}`; `crates/server/src/files.rs`, `graph/actor.rs` (D's block); `web/src/lib/plain.ts` (new),
`Status.tsx`, `Attention.tsx`, `CodeView.tsx`; `e2e/st-errors.mjs`.

### D3. Make your first song — M

**What the user sees.** Q47. The steps (keys by `chord()`):

| # | Step | Ticks when |
|---|---|---|
| 1 | **Add drums.** Press ⌘T and pick Drum kit. | a track plays an instrument of the Drums group |
| 2 | **Make a beat.** Double-click its row for a clip, then click Kick and Snare in Notes. | that track's lanes hold 4 notes or more |
| 3 | **Add a bass.** Press ⌘T and pick Synth bass or 808 sub. | a track plays `std:synth-bass` or `std:synth-sub` |
| 4 | **Play some bass notes.** Make a clip on it and click a few notes in Notes. | that track holds 3 notes or more |
| 5 | **Listen.** Press Space. Press it again to stop. | the song played while the guide was open |
| 6 | **Change a sound.** Turn a knob in Device (⌥1), or right-click a track, Change instrument. | a knob's value was written, or an instrument changed |
| 7 | **Export it.** Press ⇧⌘E. Your song becomes a file you can share. | an export finished |

Done: "You made a song. It is in your Exports folder." with Show in Finder. The current step is `fg` 600 with its hint
under it; done steps `fg-muted` with a `CircleCheck` in `accent`; the rest `fg-faint`. 240 px wide (the browser's),
`raised`, 1 px `line`, `rounded-md`. The foot: "Start from an empty song…" (New Song with Empty chosen).

**The back end.** Web, and one view key.
- `crates/server/src/graph/view_file.rs`: `KEYS` gains `"guide"` (D's line).
- `web/src/lib/guide.ts` (new): the steps' checks from the state; `guideDid("sound")`; the `export` and transport
  events through `lib/events.ts`' listeners (no edit to `exporting.ts`); kept with `setLocal("guide", …)`.
- `components/Guide.tsx` (new); `Browser.tsx`: the card at its top, the browser opened for it; `StartScreen.tsx`:
  the link; `App.tsx`: `case "guide"`; one-line hooks `guideDid("sound")` in the knob's commit (`Knob.tsx`' gesture
  hook) and `changeInstrument` (`Tracks.tsx`).
- Desktop `menu.rs`: `guide` ("Make Your First Song", no accelerator), Help's first row; a test.

**Tests.** Suite `st-guide` (kind `none`; a new Empty song through the API, about 35 checks): `__menu("guide")`
opens the card in the browser column, nothing ticked; each step ticks as an app page does it (+ Track Drum kit; four
notes; Synth bass; three notes; Space; a knob's drag; ⇧⌘E then Return); the done line; × closes; a reload keeps the
ticks (`view.json`'s `guide`); the fold; the start screen's link opens New Song with Empty chosen, then the guide;
the card never overlaps `[data-testid=timeline]`; lint; tips. Red first on `dist-before`: the command does nothing.
Moves: `start`, `newsong`, `taste`.

**Files.** `graph/view_file.rs` (D's line); `web/src/lib/guide.ts`, `components/Guide.tsx` (new); `Browser.tsx`,
`StartScreen.tsx`, `App.tsx`, `Knob.tsx`, `Tracks.tsx` (D's lines); `apps/music2-desktop/src/menu.rs` (D's row);
`e2e/st-guide.mjs`.

### D4. Music basics, up to date — S

`docs/music-basics.md`: every "In music2" line says what the app does now (the key, mute and solo, drawing
automation, triplets, quantize and humanize, the metronome, playing live, recording, audio import, stems), in its
own plain style, and a line on the guide. D owns the file this track.

**Lane D's order:** D1 (Day 1 morning), D2 (Day 1 afternoon), D3 (Day 2 morning), D4. **If late:** D4 goes, then
the start screen's link, then the `not a function` and mono rows of D2's table.

---

## 7. File ownership

### 7.1 Who edits what

| File | Owner | Others |
|---|---|---|
| `crates/editor/src/tracks.rs`, `actions.rs` (desktop), `web/src/lib/drop.ts`, `components/DropLayer.tsx`, `e2e/{applib,droplib}.mjs` | P0 (lead) | after P0, the lead only; A and C may append helpers at `droplib.mjs`' end |
| `crates/editor/src/{import.rs, clips.rs, clips_tests.rs}`, `core/song/src/path.rs` (A's block), `crates/server/src/{peaks.rs, routes_audio.rs, graph/import.rs}` and their tests, `web/src/lib/{peaks.ts, audioImport.ts, clips.ts}`, `components/Wave.tsx`, `e2e/st-audio.mjs` | A | — |
| `core/runtime/src/{click.rs, click/tests.rs, engine.rs}`, `core/play/src/lib.rs`, `core/play/tests/no_alloc_count.rs` | B | 8a: §1.9 |
| `crates/server/src/{graph/record.rs, graph/audition.rs, graph/midi.rs, settings.rs, routes_settings.rs, routes_record.rs}` and their tests | B | — |
| `web/src/lib/{record.ts, piano.ts, audition.ts, settings.ts}`, `components/{RecordButton.tsx, Take.tsx, KeyboardPiano.tsx, SettingsDialog.tsx, MidiStatus.tsx}`, `e2e/st-record.mjs` | B | D: titles only |
| `crates/editor/src/{smf.rs, midifile.rs, render.rs}` and their tests, `crates/server/src/{stems.rs, routes_exchange.rs, graph/exchange.rs}`, `apps/m2/tests/render.rs` | C | P0's three `pub(crate)` in `render.rs` |
| `crates/server/src/export.rs` | C (one method) | — (8a: stay out of what `git diff main...8a` changes) |
| `web/src/lib/{midiFiles.ts, exporting.ts}`, `components/{ExportDialog.tsx, MidiTempoAsk.tsx, ExportAsk.tsx}`, `e2e/{st-midi,st-stems}.mjs` | C | D: titles only |
| `core/ir/src/lib.rs`, `core/session/src/lib.rs` (D's blocks), `crates/editor/src/explain.rs`, `crates/server/src/files.rs`, `web/src/lib/{plain.ts, guide.ts}`, `components/{Guide.tsx, Status.tsx, Attention.tsx, CodeView.tsx, Browser.tsx, StartScreen.tsx, Knob.tsx}`, `e2e/{st-tips,st-errors,st-guide,tiplib}.mjs`, `docs/music-basics.md` | D | — |
| `crates/gates/**`, `core/os/**`, `core/plugins/**`, `std/**` | nobody | — |

### 7.2 The shared files, and the rule for each

| File | Rule |
|---|---|
| `web/src/App.tsx` | P0: `<DropLayer />` after `<Toasts />`. A: `case "import_audio"`. B: `r: "record"` in `key()`'s map (`:62`), `case "record"`. C: `case "import_midi"`, `<ExportDialog />` and `<MidiTempoAsk />` after `<ExportAsk />`. D: `case "guide"`. New cases at the switch's end in the order A, B, C, D; imports at the import list's end. The integrator joins neighbouring lines |
| `web/src/components/TopBar.tsx` (the transport) | B: the Record button after Play / Stop, and its import. D: titles only. Nobody else |
| `web/src/components/SongTimeline.tsx` | P0: `data-beat-origin` on the lanes' box. A: the audio branch of `inside()` and `words()`' audio line; `TimelineMenu`'s audio rows. B: one line in `LaneClips`, and `export` on `Notes`. C: `TimelineMenu`'s "Export as MIDI…" row. D: titles only. Each a hunk of its own; in `TimelineMenu`, A's rows after Split's, C's after Duplicate's |
| `web/src/components/Tracks.tsx` (+ Track's list) | A: the Audio group in `InstrumentPicker`. D: titles and `guideDid("sound")` in `changeInstrument`. Nobody else |
| `web/src/components/Shortcuts.tsx` (the ⌥K sheet) | A: File "Import audio: ⇧⌘I" at File's end. B: Transport "Record: R" at Transport's end. C: File's Export row reworded "Export: ⇧⌘E" (in place). D: nothing |
| The Export dialog (`ExportDialog.tsx`, `ExportAsk.tsx`, `exporting.ts`) | C owns them. D: titles only. The End joins Q30 (no export while recording) |
| `apps/music2-desktop/src/menu.rs` | `ITEMS`: A's `import_audio` and C's `import_midi` after `save_as`, in that order; C's `export` title; B's `record` after `metronome`; D's `guide` after `shortcuts`. Rows: File: after Save As, a line, Import Audio… (A), Import MIDI… (C); Transport: Record after Go to Start (B); Help: Make Your First Song first (D). Each lane's tests at the test module's end. The integrator joins |
| `crates/server/src/lib.rs`, `routes.rs`, `graph.rs`' `mod` list, `crates/editor/src/lib.rs` | P0 only (§2.1). After P0: B's take channel thread and its `AppState` field (at the struct's end, and in `new`). Nothing else |
| `crates/server/src/graph.rs` | B: `ControlPlane.record` at the struct's end, `state["record"]` after `state["midi"]`, the two lines in `transport` and `stop_to`. Nobody else. 8a-gaps changed `out_of_memory`: stay out of it, `let_go` and `retire` |
| `crates/server/src/graph/actor.rs` | D: the problems hunk in the publish. Nobody else (8a-gaps added a line at `:95`) |
| `crates/server/src/graph/view_file.rs` | D: `"guide"` in `KEYS`. Nobody else |
| `apps/m2/src/main.rs` | C: the `bounce` arm calls `render::rooted`. A: an `import` arm (A4) and its usage line. Separate arms |
| `e2e/run.mjs`, `e2e/README.md`'s table | Rows after anchors: A `st-audio` after `s8-leases`; B `st-record` after `nx-midi`; C `st-midi` and `st-stems` after `export`; D `st-errors` after `attention`, `st-guide` after `start` (the `empty` row), `st-tips` after `taste`. A lane that moves another suite's count edits only that row. 8a-gaps adds `sound-stops` after `pl-projects` |
| `e2e/lintlib.mjs`, `e2e/taste.mjs` | Nobody |
| `web/src/lib/graph.ts` | Nobody. New fields by `declare module "./graph"` in the lane's own file, as `history.ts` does |
| `docs/designs/step8/build-notes-studio.md` | Each under its own heading |
| `docs/gaps.md`, `README.md`, `docs/architecture.md`, `taste.md`, `HANDOFF.md`, `e2e/README.md`'s prose | **The End only**, from each slice's For gaps.md and Contract lines |
| `Cargo.toml`, `Cargo.lock` | Nobody (Q48). At a merge of `main`: take `main`'s `Cargo.lock`; `cargo build --locked` must take it as it is |
| Any component, for D's titles | D's hunks add or reword a `title` only. When a lane rewrote the element meanwhile, the integrator keeps the lane's element and puts D's title on it |

---

## 8. Order, by day *(estimates)*

| When | Lead | Lane A | Lane B | Lane C | Lane D |
|---|---|---|---|---|---|
| Day 1, first hour | the first commit, the worktrees, the before server (§2.1, 2.2) | reads; `peaks.rs` tests red | reads; `click/tests.rs` red | reads; `smf_tests.rs` | reads; `st-tips` red on `dist-before` |
| Day 1, morning | P0 (§2.3–2.7) | A1 | B1 | C1's `smf.rs` | D1 |
| Day 1, afternoon | merges `main` into `studio` if it moved | merges `studio` (P0); A2 | B2 | merges `studio`; C1's export, C2 | D2 |
| Day 2, morning | | A3 (A4 if time) | merges `studio`; B3, B4 | C3 | merges `studio`; D3, D4 |
| Day 2, midday | merges each lane into `studio` as it finishes, B, C, A, then D; the merge checks (§9.1) | done | done | done | done |
| Day 2, afternoon | the End (§9) | | | | |

If a lane is late at Day 2 midday, its unfinished slice waits and the End lists it in `docs/gaps.md` (each lane's
"If late" says what goes first). Each lane deletes its `target-<lane>` and `bin/<lane>-*` when it is done.

---

## 9. The End (lead)

1. **Merge** each lane into `studio` (merge commits, `rerere`), then `main` if it moved. Join fixes, each its own
   commit with a red-first check: Q30 (Export refused while recording: `export::start` and `stems::start` ask
   `Graph::recording()`; the Export dialog greys Export with "Stop recording first."); `st-tips` gains the joined
   places (the Record button and its menu, the take, the Export dialog, the tempo question, the guide, the Gain
   popover, the drop hint) and every control it finds without a title gets one. Then: the web build; every browser
   suite (`lane-suites.sh st <out> all`, with `CLAP_PATH` for `live3b`); `cargo test --locked --no-fail-fast` and
   clippy on every crate a lane touched (`m2-song`, `m2-runtime`, `m2-play`, `m2-ir`, `m2-session`, `m2-editor`,
   `m2-server`, `music2-desktop`, `m2`); the mac lint; the renders.
2. **Review** with a skeptic, two lenses (how it behaves: the engine's count-in, the import's durability, the take's
   timing, stems' bits; tests, docs, words and taste), as `next`'s End did. Each finding: a fix with a check that
   fails before it, its own commit.
3. **Docs** (one commit):
   - `docs/gaps.md`: #18 done (warp of imported audio, joining audio clips, cleaning unused audio, streaming left);
     #24 recording done (pitch bend, the mod wheel, overdub and one clip per pass, the piano's velocity left); #19
     done (the click's level and sound left); #31 MIDI keyboards done (an input device left, with #25); #36 done
     (MP3 and AAC, stems with plugins left); #11 done; #37 `m2 import` if built; What works today gains the rows; the
     loose ends lose "Audio clips can't be edited in the app yet" and its kin.
   - `README.md`: what works, and the words "stems", "count-in", "take".
   - `docs/architecture.md`: FMT-14 *Today* (importing: create-only, a numbered name, the same bytes used again,
     durable before named); WRITE-1 *Today* (an import's file is pushed and its folder synced before the Tx that
     names it); FMT-16 and RENDER-30 (the peaks cache); WRITE-10 rows (import audio, audio clip settings, a take, a
     MIDI import, several tracks in one Tx with an arrangement); RENDER-28 (the count-in: plain fields, no
     allocation); RENDER-1 and RENDER-3 (stems are offline renders, `m2 bounce`'s bits; the count-in never reaches an
     export); MON-7 (a take reads live notes); DIAG-1 *Today* (plain words made in `crates/editor`,
     `m2_ir::Diagnostic.inner`, a check error at the project's call); FMT-20's homes (`view.json` `guide`;
     `settings.json` `record` and `midi`).
   - `taste.md`: §3.3 (Record after Play; everything right of it 32 px further), §2.8 (an audio clip's waveform and
     fade handles), §3.6 (+ Track's Audio group), the drop hint, the Export dialog, the tempo question, the guide
     card, §5.1 (R is Record, ⇧⌘I imports audio), §5.3 (the Transport and Help menus).
   - `e2e/README.md`'s rows; `HANDOFF.md` (below).
4. **The full check once:** `TW_WAIT=5400 $SCRATCH/tw check-studio 'cd $SCRATCH/wt/st &&
   CARGO_TARGET_DIR=$SCRATCH/sst/target-st M2_CHECK_OUT=$SCRATCH/check-studio ci/check.sh --gates'`
   (`--gates`: lane B changed the engine's block). Renders must match `e2e/renders.sha256`; nothing may pass only on
   a retry.
5. **The Linux app on `:97`** (`app-shot.sh`; never a real screen): File ▸ Import Audio… (Ctrl+Shift+I) and Import
   MIDI…, File ▸ Export…, Transport ▸ Record, Help ▸ Make Your First Song.
6. **Pictures**, in `$SCRATCH/app-shots/studio-<sha>/` with `captions.txt` (each one plain line, and "This is the
   Linux version. On your Mac it uses the Mac's own font and ⌘ keys."): audio clips with waveforms and a fade; the
   drop hint below the tracks; the Record button counting in; a take growing; the Export dialog; the tempo question;
   the guide card half done; an error in plain words with Details open; Settings' MIDI keyboards (a `HOOK` posts a
   keyboard to `/api/graph/midi/inject`).
7. **HANDOFF**: a `studio` row and an In short table; the Mac by-ear steps (61 on): drag a WAV from the Finder onto a
   track and below the tracks, play it, trim and fade it; import a MIDI file from GarageBand; record with a real MIDI
   keyboard, with the count-in, then while looping; turn one keyboard off in Settings; export stems and open them in
   another app; follow the guide from an empty song.
8. **Clean up:** each lane's target and bins (the lanes do it), then `target-st`, `bin/` and `scratch/`; the lane
   worktrees (`git worktree remove`), keeping their branches.

---

## 10. Cut, and left for later

| What | Why |
|---|---|
| Recording audio (#25) | It needs an input stream in `core/play`, 8a's area this round (Q29) |
| Stretching imported audio to the tempo (warp in the app) | No warp by default (Q5); the engine has warp, the app has no way to place markers |
| Stems of a song that plays plugins | Each track's graph would need the live instances lent to it (RENDER-1) (Q40) |
| MP3 and AAC export | No encoder in `Cargo.lock` (Q41) |
| Importing MP3, AIFF, FLAC | The decoder reads WAV only (Q1) |
| One clip per loop pass, overdub | Clips piled on one lane all sound; the first pass is written (Q27) |
| Pitch bend, mod wheel and controllers in a take or a MIDI file | They don't play live either (`next`'s Q5) |
| Joining audio clips | It needs a render of them as one sound (Q15) |
| Cleaning up unused audio files | An asset is never deleted by music2 (Q4) |
| Streaming long audio (RENDER-30's trigger) | Files are kept in memory; imports stop at 15 minutes |
| Audio clip drags heard while dragging | They need renders (Q14) |
| `m2` commands for MIDI files, stems and recording | `apps/m2` gets only `bounce`'s refactor and, if time, `import` (Q50) |
| New gates (import time, record timing) | `crates/gates` is 8a's (Q51) |

---

## 11. Not checked

- Tauri 2.12's drag-drop events: their names in `@tauri-apps/api` 2.12.3 (`tauri://drag-enter`, `-over`, `-drop`,
  `-leave` expected), whether `position` is in physical pixels on WKWebView and WebKitGTK, and whether `core:default`
  allows listening to them (P0 reads `node_modules/@tauri-apps/api` and the capability schema).
- Whether Chrome lets a page build a `DataTransfer` with `File`s and dispatch `drop` with it (P0's `dropFiles`; the
  app-page path is the fallback).
- `hound` 3.5 on `WAVE_FORMAT_EXTENSIBLE` 24-bit files and on files with chunks after `data` (A2 tests both).
- axum's body limit on one route (`DefaultBodyLimit` layered on the route only).
- QuickJS's and oxc's exact error words (D2's tests build real broken songs, so they read the real ones).
- Whether a `Node.frame` chain always reaches a project frame for every check error (D2 falls back to the old `src`).
- Where 8a's live half moves `Engine::render` (B1's lines): the merge of `main` says.
- The count-in's first click inside a block: whether `Click::mix` can start a blip mid-block while the transport is
  stopped (B1 reads `click.rs`).
- Whether the WAV header length (`hound`'s `duration()`) always equals `load_wav`'s frame count (A3 tests it on 8,
  16, 24 and float files).
- The stems' bits against `m2 bounce` on a song whose kernels make a NaN (`Render` isn't strict; `m2 bounce` is):
  no fixture does.

---

## Attack

Written 2026-10-17 by an attack agent, read only. It checks this plan against `85b069a` (`wt/st`), against
`8a` at `739cfc6` and `8a-gaps` at `8a3d70a`, and against 8a's own plan (`$SCRATCH/s8a/plan-live.md`).
File:line cites are at `85b069a` unless named. Nothing was built or run. The Decisions section right after this one
takes each item. Both override everything above, the first Decisions table included.

### In short

| Part | What was wrong | Fixed by |
|---|---|---|
| Audio | The song wouldn't see a new file in time: the clip would play silence, with an error, until the folder watcher caught up. Two imports at once could delete each other's half-copied file. A reversed clip's edge limits were the wrong way round. | D1 to D3 |
| Record | Note times were only as fine as the sound card's buffer (10 to 20 ms), and a quick tap could make a note with no length. A note just after a loop wrap would land at the take's start. Space during the count-in started the song early. Stopping would have waited for the song's code to build, which the transport must never do. Clicking another track mid-take sent the keys there, unrecorded. | D4 to D9 |
| MIDI files | An odd file could fail the whole import: notes with velocity 0, a division of 0, tempos and time signatures music2 can't store, a file over 2 MB from a browser. | D10, D11 |
| Help | The guide said "click" in Notes, which doesn't add a note (double-click does). A tooltip on every text button would only repeat its word. | D14, D19 |
| Keys | R and ⇧⌘I are free on a Mac and on Linux, while typing, and with the keyboard piano on. | none needed |
| 8a | No file fight, if lane B keeps its blocks small. One more small block in `core/play` is needed, and listed. | D29 |

### A.1 Wrong as written

| # | The plan says | The code says | What would happen |
|---|---|---|---|
| 1 | Q3, A2: copy, push, name the file, then plan the Tx that names it. | A Tx's baseline is the accepted revision plus the files the Tx touches, as they are on disk (`core/session/src/tx/mod.rs:1296`). A base on a file no op writes is refused (`core/song/src/tx.rs:397`). A clip naming an asset the snapshot doesn't list is `asset.missing` (`core/song/src/data.rs:473`), an error that doesn't stop a commit. | The import commits, but its revision says "`vox.wav` isn't in assets/", and the clip plays silence until the watcher's next batch (150 ms of quiet, then a tick). `an_imported_clip_plays_its_file` fails, or passes only after a sleep. `m2 bounce --place` has the same gap: its test only greps the lane file (`apps/m2/tests/relink.rs:205`). |
| 2 | A2: the part file by `clone_file`, else a copy; `sweep` clears old part files. | `sweep` deletes every `.<file>.<pid>.part` whose lock it can take (`crates/editor/src/render.rs:552`). A render locks its part at once (`render.rs:392`). A part's name is the file's name and the pid only (`render.rs:538`). | A part made by `clone_file` is unlocked until opened, so another import's or render's sweep of `assets/` can delete it halfway. Two imports of `vox.wav` at once in one server share one part name. |
| 3 | Q13, A3: "the right edge stops at the file's end"; a reversed clip's "left edge keeps offset and the right edge moves it". | A reversed clip plays its placed span `[offset, offset + seconds]` backwards (`core/song/src/lane.rs:195`, `core/runtime/src/audio.rs:305`). | The offset math is right. The limits are not: for a reversed clip the file's end limits the **left** edge, and `offset ≥ 0` limits the **right** edge. As written, the left edge could run past the file's end into silence. |
| 4 | Q22: "a note lands on a block line (2.7 ms)"; the published beat "less one device buffer". | The status is published once per callback (`core/play/src/lib.rs:1123`): 480 frames on the silent clock (`:870`), 256 to 1,024 on a Mac. `frames()` counts the device's frames (`:1227`), not the engine's, and the rates can differ. The last callback's time is kept but private (`:235`, `:411`). | Notes land on callback lines, 10 to 21 ms apart. A key struck and let go within one callback gets length 0. The buffer is taken off at the wrong rate when the device and the song differ. What the speakers play is about two buffers behind the published frame, not one. |
| 5 | Q22: clamp to the take's start. Q27: the 100 ms thread checks for a wrap or a jump. | The published beat jumps back at a loop wrap. A jump is `Graph::seek` (`crates/server/src/graph.rs:927`), which the plan doesn't touch. | A note struck just after a wrap is stamped near the loop's start, before the take's start, so the clamp puts it at the take's start: a wrong note. A jump forward isn't "the beat went back", so the thread can't see it. |
| 6 | Q19, B1: a Stop or a hold cancels a count. | While counting, the engine is stopped, so the status says stopped, and Space's `toggle` sends Play (`graph.rs:1631-1640`). | Space during the count-in starts the song at once; the count clicks on, and its end sends Play again. A hold (an export borrowing plugins) cancels the count, but the server never hears it: `record.phase` stays `count`. |
| 7 | B2: "one line at the top of `transport` and `stop_to`: a stop ends the take first", then "one actor job" writes it. | RENDER-29: the transport never takes the session lock (`docs/architecture.md:299`; the comments at `graph.rs:1630` and `:1649`). `audition()` holds the control plane's lock and never goes through the session (`graph/audition.rs:7-9`). | Space while recording would wait for a compile. A wrap found inside `recorded` (called from `audition`) would call the actor under the control lock. |
| 8 | Q21: the take's lane is the play target when Record is pressed. | MIDI keyboards play the lane a window last named (`graph/midi.rs:6-9`); the page names it again at every selection change (`web/src/lib/midi.ts`, `tell`). | Click another track mid-take: the keys sound there and nothing is recorded. |
| 9 | Q24 and §7.2: B adds a thread and an `AppState` field in `lib.rs`; `routes.rs` is P0's only. | A window gets messages only through `ws_loop`'s `select!` (`crates/server/src/routes.rs:193-226`). | A new channel needs an arm in `ws_loop`, in a file no lane may edit. |
| 10 | Q31, C1: smf.rs keeps notes, programs, tempo, meter, names. | Live MIDI reads a Note On with velocity 0 as an Off (`graph/midi.rs:79`); files do it all the time. The arrangement refuses tempos outside 20 to 999 BPM, and meters outside 1 to 64 beats or units 1 to 32 (`core/song/src/arrangement.rs:81`, `:98`, `:101`). | Not in the plan: velocity-0 Ons (notes that never end), a division of 0 (a divide by zero), `1 << dd` overflow on a bad meter, a tempo event of 6,000,000 µs a beat (10 BPM: refused by the arrangement, so the whole import fails), unknown chunks, notes of length 0, a huge file. The bytes route keeps axum's 2 MB body limit: only the audio route raises it. |
| 11 | Q35, Q36: the tempo question; a one-part file dropped into a lane is one `Gesture::Set`. | `Gesture::Set` writes one lane (`crates/editor/src/clips.rs:125-132`). | "Use 128" can't go in that Tx. |
| 12 | Q52, P0.4: the drop layer listens on `window`, and to Tauri's drag events. | The browser's instruments and effects are HTML5 drags (`web/src/components/Browser.tsx:207-262`) onto the timeline and the dock (`SongTimeline.tsx:1762`, `Dock.tsx:114`). Tauri's own drop handler is on: the window builder doesn't turn it off (`apps/music2-desktop/src/main.rs:90-103`). | Dragging an instrument onto the timeline would show "music2 opens WAV and MIDI files", or lose its drop. |
| 13 | P0.4: `getCurrentWebview().onDragDropEvent`; suites use the app shim from `pl-projects.mjs`. | `getCurrentWebview()` reads `__TAURI_INTERNALS__.metadata`, which the shim doesn't have (`e2e/pl-projects.mjs:121-133`). | Every app page in every suite throws as it loads. |
| 14 | D3, steps 2 and 4: "click Kick and Snare in Notes", "click a few notes". | In Notes a click moves the insert point; a double-click adds a note, or a click in Draw mode, B (`docs/designs/step8/taste.md:860-863`). | The guide's first real step does nothing. |
| 15 | C3: `Render::write_float`, "as `write_new_wav` writes it". | `Render`'s writer clamps to ±1 and writes 24-bit numbers (`render.rs:469-481`). | Built from that path, stems would clip above 0 dBFS: not `m2 bounce`'s bits. |
| 16 | A2: refusals "with nothing written". | The copy runs on the route's thread before the actor job (A2's own back end). | A track refused after the copy (the code stopped compiling meanwhile) leaves a new file in `assets/`. |
| 17 | A1 on Day 1 morning: st-audio part 1 writes its WAV with `wav()`. | `wav()` is in P0's `droplib.mjs`, which lands after the lanes start. | A1's suite can't run before midday. |
| 18 | Q17: the Record button at rest is a ghost with its circle in `danger`. | `danger` is for problems only (`docs/designs/step8/taste.md:261-266`). | Against taste.md as it stands. |
| 19 | Q43: every visible button needs a `title`. | Dialog buttons say their action ("Done", "Cancel", "Export…"); taste's principle 4 is "Controls, not captions". | Tooltips that only repeat a button's word: noise, and a bigger D1. |
| 20 | Q11: **Gain…** | taste's word for level is volume ("Bass volume"). | "Gain" is a word a beginner doesn't know. |
| 21 | Nothing. | A double-click on an audio lane says "bounce a track with `m2 bounce --place`" (`clips.rs:173-176`). | Wrong advice once the app imports audio. |
| 22 | Q28: a "Plays" switch per keyboard. | There is no switch in `ui.tsx`; Rename Song uses a check box (`RenameSongDialog.tsx:114`). | A new component nobody has drawn. |
| 23 | Q42: Stems and MIDI go through the same Export flow. | ExportAsk asks because the file holds every track, whatever you listen to (`web/src/lib/exporting.ts:23-27`). | Stems leave muted tracks out anyway, and MIDI isn't sound: the question would be wrong there. |
| 24 | D3's st-guide, step 7: "⇧⌘E then Return". | Without C3, ⇧⌘E exports at once (a save panel in the app, straight to Exports in a browser). | st-guide isn't green on `studio-d` alone, or it breaks when C3 lands. |
| 25 | §7.2: "A lane that moves another suite's count edits only that row." | Each row of `e2e/run.mjs` carries its suite's `expect` count (`run.mjs:59-113`). | `pc-keys` is moved by A, B, C and D, `taste` by all four: four lanes on one line. |
| 26 | Nothing. | An export of a song with plugins holds the engine, and a hold cancels a count (B1). | R during such an export clicks nothing and says nothing. |

### A.2 Tests that can't fail

| Test | Why it can't fail | Instead |
|---|---|---|
| `the_asset_is_whole_before_the_tx_names_it` | In one process the bytes read back whether or not they were pushed or synced. | `an_import_commits_a_revision_that_plays_the_asset`: right after the import's answer, the accepted revision has no `asset.missing` and lists the asset by its hash. Red without D1's take-in. That the bytes are pushed before the name is a code-read item for the End's review. |
| `writes_what_it_reads`, `an_exported_song_imports_as_the_same_notes` | A wrong tick scale in both the reader and the writer passes both. | Add `reads_the_spec_example`: the SMF 1.0 spec's format 0 and format 1 example files, as bytes in the test, read to their known notes and tempo. |
| C1's export tests, "guards where nothing existed" | A test of a new function is red against a stub. | Show each one red against stubs that return empty bytes; none is a guard. |
| `a_stem_renders_what_m2_bounce_renders` | It rebuilds bounce's loop in the test, with the same `rooted`. | Keep it; st-stems runs the real `m2`, and the before and after hashes of `m2 bounce` stay. |
| `peaks_never_wait_for_the_actor`, `no_count_in_changes_no_bit`, `recording_never_waits_for_a_compile` | They guard against what never happened. | Keep them as named guards, as the plan says. |

### A.3 Keys

- **R** is free on both systems. `key()` has no `r` (`web/src/App.tsx:62`) and no menu row has R. Page keys are
  ignored while typing (`App.tsx:146`) and while a dialog is open (`web/src/lib/modal.ts:27`).
- **R with the keyboard piano on:** the piano takes only its 20 keys, by `e.code` (`web/src/lib/piano.ts:20-23`,
  `:93`), and R isn't one. Page keys go by the letter (`e.key`), so on a Dvorak layout the letter R sits on a piano
  key (`KeyO`) and plays a note there; the button and Transport ▸ Record still work. Fine.
- **⇧⌘I** is free on a Mac (the Web Inspector is ⌥⌘I) and is Logic's Import Audio key. On Linux, Ctrl+Shift+I is free
  in a release build; a debug build's WebKitGTK inspector and Chrome's DevTools also use it. Only the browser page
  needs a `chord()` row for it.
- **Space, Return and Esc while counting:** Q19 names only R and Esc (A.1 #6).

### A.4 8a's live half

- `core/runtime/src/engine.rs`: 8a's live half doesn't touch it (plan-live §1). 8a-gaps changes only the render's
  `Err` arm (`engine.rs:622-631`). B's lines sit at `:634` (after `self.click.mix`) and `:651` (the idle check),
  more than 3 lines away: git merges them.
- `core/play/src/lib.rs`: 8a adds `Msg::Threads` and its arm in `take_held`'s outer match, a runner in `Callback`,
  the `fill` wrapper, `start`, `silent`, `driven_on` and `engine_threads()` (plan-live §3.4). B's arm goes in the
  inner `EngineMsg` match (`:491`), which 8a leaves alone; `Player::count_in` after `click()` (`:1065`); and D4's
  accessor after `frames()` (`:1227`). Separate blocks.
- After a crash, the server makes a new player (plan-live §3.5), whose engine clock starts again. A take that kept
  a `Player` or a clock across that would stamp nonsense.
- `Cargo.lock`: 8a adds 3 lines; a merge of `main` takes main's, as the plan says.
- `crates/server/src/graph.rs`: 8a-gaps adds `mod stopped;` after `mod songkey;` and changes `out_of_memory`
  (`:428`); P0's mod lines go after `mod tempo_lease;`. `graph/actor.rs`: 8a-gaps' line at `:95`, D's hunk at
  `:663`. No overlap.

### A.5 Too big, and the order

- P0 is M, and A2, C1's save panel, C2 and B3's menu row wait on it. Its first commit must carry everything a lane
  needs on Day 1 morning (A.1 #9, #17).
- C3 is nearer L than M: a render refactor, `m2 bounce`'s arm, stems, a new dialog and five moved suites.
- D2 is nearer L: a core change, 12 rules each tested on a real broken song, three views, a 25-check suite.
- D1 puts titles across every component on Day 1 while A, B and C start on the same files. Merged late, it makes
  the most conflicts.

### A.6 Checked, and fine

- Several tracks in one Tx fit core as it is: `Confined.modules` is a list (`core/song/src/tx.rs:162-167`), and
  an arrangement the Tx writes is admitted (`core/session/src/tx/expect.rs:259`). No core change.
- Q33's sounds exist with those labels (`std/src/synth.ts`, `fm.ts`, `drums.ts`); the Drum kit plays General MIDI
  notes (`std/src/drums.ts:51-70`).
- A3's offset math for a reversed clip matches the renderer; only the limits were wrong (A.1 #3).
- Fades are a half cosine (`audio.rs:302-317`). Dot files in `assets/` are never assets (`core/song/src/path.rs:221`),
  so part files never show as problems. `AssetName::untaken` already numbers names (`path.rs:242`). `m2_os::fs` has
  `push`, `sync_dir`, `make_dirs` and `rename(Excl)`.
- Nothing decodes on the audio thread: clips are decoded and rendered when a renderer is built (`audio.rs:1-7`).
- `m2_ir::Diagnostic` is built by a literal only in `core/ir/src/lib.rs:363`, so a new field breaks nothing else.
- A sustain pedal already holds Offs back live (`graph/midi.rs`), so a pedalled note ends where the pedal lifts, as
  Q22 says, with nothing more to build.

---

## Decisions

Each Attack item, and the questions it raised, with the decision to build with. These override the plan above,
its first Decisions table included. Nobody waits, and nobody asks the owner.

| # | Item | Decision |
|---|---|---|
| D1 | A.1 #1: the file before the Tx | The import's actor job takes the new file in before it plans `place`: `o.session.take_in_only(Snapshot::read_paths(dir, &{assets/<name>}, &\|_\| true, BTreeSet::new())?, &{assets/<name>})` (both public: `core/session/src/take.rs:129`, `core/song/src/lib.rs:181`). No core change. The watcher's later batch for that path then changes nothing. Test: A.2's first row, red first without the take-in. `m2 bounce --place` keeps its gap: one line for gaps.md (A4, if built, shares the helper and closes it). |
| D2 | A.1 #2: the part file | Made with `create_new` and locked at once (`try_lock`), filled with `std::io::copy` (no `clone_file`), pushed, checked by one decode, then named create-only, as `Render::write_as` does (`render.rs:362`); the lock goes after naming. One import at a time per server (a mutex in `Graph::import_audio`). Taken names are compared case-folded (`AssetName::folded`), so a song copied to a Mac never holds two names that differ only in case. Tests: `a_part_being_copied_is_never_swept`, `two_imports_of_one_name_both_land`. |
| D3 | A.1 #3: edge limits | With 1 ms of slack. Not reversed: left edge `offset ≥ 0`, right edge `offset + seconds ≤ the file`. Reversed: left edge `offset + seconds ≤ the file`, right edge `offset ≥ 0`. The bubble says "End of vox.wav" or "Start of vox.wav". `the_edges_stop_at_the_file_both_ways` covers four cases. |
| D4 | A.1 #4: note timing | A note's time is kept in engine frames that never wrap. One accessor in `core/play/src/lib.rs`, after `frames()` (B's block, listed for 8a's merge): `Player::heard_frame() -> Option<(u64, u64)>`, the engine clock and the transport frame the speakers play now: the last callback's, plus the time since it, less two device buffers turned into engine frames. The take keeps its anchor (the clock and transport frame where it started playing) and places each note from it through the tempo map that plays. Two buffers is one constant; HANDOFF's Mac step checks it by ear ("record along with the metronome: the notes sit on the lines"). A note is at least 1/64 beat long. "Block line" goes from Q22. Tests: `a_quick_tap_still_records_a_note`, `notes_land_between_callbacks` (red first: callback steps). |
| D5 | A.1 #5: wrap and jump | First pass only, as Q27: a note heard past the loop's end isn't recorded; the take ends at the loop's end, and notes held over it end there. `recorded` checks this on every note; the take thread checks when no note comes. A jump ends the take: one line at the top of `Graph::seek` (`graph.rs:927`, B's block), at the heard frame. Tests: `a_note_after_the_wrap_is_not_put_at_the_start` (red first: today's clamp), `a_note_held_over_the_wrap_ends_at_the_loop_end`, `a_click_on_the_ruler_ends_the_take`. |
| D6 | A.1 #6: the count-in and Play | In the engine, a Play, Stop or Seek read while counting cancels the count (one line in `apply_now`'s `Transport` arm, `engine.rs:409`), as a hold does. In the page, Space, Return, Esc and R while counting cancel it, and nothing plays. A count that hasn't turned into playing 300 ms after its end was cancelled: the take thread drops it (phase none, no toast). Test: `play_while_counting_cancels_the_count`. |
| D7 | A.1 #7: never wait | Ending a take never waits for the session. `record_end` (from Stop, R, a wrap, a jump) takes what it needs at once, under the control lock: held notes end, the end bar is fixed, and the take moves to a queue. The take thread writes it (the actor job, then the step's name) and sends `{"t": "take", "written": {revision, made}}` or `{"t": "take", "empty": true}`; the page selects the clip, or shows the toast, then. `transport` and `stop_to` stay as RENDER-29 says. `record_start` reads only the control plane (the plan that plays, its tempo and meter), never the session: R is transport. Guard: `stopping_never_waits_for_a_compile` (the actor held by a slow job). |
| D8 | A.1 #8: the take's lane | While a take runs, keys play the take's lane. The server ignores `midi` target changes (B's block in `graph/midi.rs`); `currentTarget()` returns the take's lane (`web/src/lib/playTarget.ts`, B's this track). After the take, the selection's target comes back. Test: `the_keys_stay_on_the_takes_lane`. |
| D9 | A.1 #9: the take's channel | P0's first commit adds `AppState.takes` (a `watch::Sender<String>`, beside `meters`) and its arm in `ws_loop`, empty until B uses it. B sends a take message only when its notes or its phase change; the page draws the take's end at the playhead itself. |
| D10 | A.1 #10: reading MIDI files | In `smf.rs`: a Note On with velocity 0 is an Off; a division of 0 or SMPTE is refused in words; unknown chunks are skipped; running status carries across meta and sysex events, as players do; a VLQ over 4 bytes, a chunk past the file's end or a missing status byte is refused in words that name the byte. An Off with no On is ignored. Two notes of one pitch that overlap: the first ends where the second starts. A note shorter than 1/64 beat gets 1/64 beat. Tempos are clamped into 20 to 999 BPM. A meter with beats outside 1 to 64, a unit outside 1 to 32, or not on a bar line is skipped; with none at the start, 4/4. Refused in words: a file over 16 MiB (the bytes route's body limit is 16 MiB), over 200,000 notes, or a note past bar 9,999. Tests: `reads_the_spec_example` (A.2), `a_mangled_file_never_panics` (every prefix of a fixture, then 2,000 seeded byte flips with a 10-line xorshift: `read` answers Ok or words, never panics), `velocity_0_ends_a_note`, `odd_tempos_and_meters_stay_in_range`. |
| D11 | A.1 #11: a drop into a lane | A one-part file dropped into a lane keeps the song's tempo, with no question: its notes go by beats. The question is only asked when new tracks are made (Q35). |
| D12 | A.1 #12: the app's own drags | The drop layer reacts only to drags that carry files. In a browser: `dataTransfer.types` includes `"Files"`, else it does nothing (no hint, no `preventDefault`). In the app: a Tauri drag event with no paths is ignored. st-audio checks that an instrument dragged from the browser still adds a track, with no file hint. |
| D13 | A.1 #13: Tauri's events | `drop.ts` uses `listen` from `@tauri-apps/api/event` for `tauri://drag-enter`, `-over`, `-drop` and `-leave`, as `onMenu` does (`web/src/lib/native.ts:76`), never `getCurrentWebview()`. Positions are divided by `devicePixelRatio` (P0 reads the package's source to confirm the units). `applib.mjs`' shim answers those four listens. |
| D14 | A.1 #14: the guide's words | Step 2: "**Make a beat.** Double-click its row for a clip. In Notes, double-click in the Kick and Snare rows (or press B, then click)." Step 4: "**Play some bass notes.** Make a clip on it. In Notes, double-click a few notes." |
| D15 | A.1 #15: stems' bits | `Render::write_float` writes the frames as they are (no clamp): 32-bit float, two channels, `round(seconds × rate)` frames, as `m2 bounce` does (`apps/m2/src/main.rs:523`). A song whose render makes a NaN: stems carry the silenced samples and `m2 bounce` fails; the build notes say so. |
| D16 | A.1 #16: late refusals | A2 checks all it can before copying: read only, the code compiles when a track is to be added, the lane is there, the file's header and length. A refusal after the copy (rare) keeps the new file: music2 never deletes an asset (Q4), and the same file imported again uses it (Q2). The words say "Nothing was added to the song", never "nothing written". |
| D17 | A.1 #17: `wav()` | P0's first commit carries `droplib.mjs`' `wav()` (node only), with the stubs. |
| D18 | A.1 #18: the red circle | Keep the red circle at rest: it is the record sign everyone knows. At the End, taste.md §2.6 gains: "`danger` also marks recording: the Record button's circle, and the take while it records. Nothing else that isn't a problem." |
| D19 | A.1 #19: which controls need a tooltip | `tips()` asks for a title on icon-only buttons, toggles, tabs, switches, sliders, knobs, faders, and any control that has a key. A button whose own words say its action and that has no key ("Done", "Cancel", "Change…") needs none. A title never just repeats the button's word: "Export (⇧⌘E)", not "Export". |
| D20 | A.1 #20: "Gain" | The clip menu says **Volume…**; the popover is "Clip volume"; the label "vox.wav · −6 dB"; Undo "Vox clip volume". |
| D21 | A.1 #21: an audio lane's double-click | A2 changes the words: "An audio clip plays a sound file. Drop a WAV file here, or use File ▸ Import Audio…". |
| D22 | A.1 #22: Settings' keyboards | Each keyboard's row has a check box, "Plays", as Rename Song's check box. No new component. |
| D23 | A.1 #23: ExportAsk | Stems and MIDI never ask ExportAsk's question. Only Song does, as today. |
| D24 | A.1 #24: st-guide | Step 7 runs `appCommand("export")`, then presses Export… if a dialog shows. It ticks on the `export` done event. Green on `studio-d` alone and after C3. |
| D25 | A.1 #25: suite counts | A lane never changes another suite's `expect` count. A moved suite keeps its number of checks; only their values change. New checks go in the lane's own `st-` suite. If that can't be, the lane's build notes give the new count and the integrator writes it. |
| D26 | A.1 #26: R while exporting | Refused: "Wait for the export to finish." (Settings' words). Export while recording stays Q30. |
| D27 | The count-in's frames | `count_in(beats, frames_per_beat: f64)`: click k at `round(k × frames_per_beat)`; the bar is `round(beats × frames_per_beat)` frames. A note struck in the count's last beat starts on the take's first beat; earlier ones are heard, not recorded. Test: `a_note_in_the_counts_last_beat_lands_on_the_first_beat`. |
| D28 | A take over other clips | Q23 stays: the take lies over what is there, and both play. That is how a beginner layers drums (kick first, then hats on a second take). ⌘Z takes a take back to try again ("Undo recording on Bass"). Replacing what lies under is left, with overdub. |
| D29 | A.4: 8a | B's blocks in `core/play/src/lib.rs` are three: the `CountIn` arm (`:491`), `Player::count_in` after `click()`, and D4's `Player::heard_frame` after `frames()`. Each is listed under For 8a's merge. A take never keeps a `Player`: it reads `inner.device()` each time, and a new player (after a crash, or a new device) ends the take as a Stop does. |
| D30 | A.5: order and size | P0's first commit grows to: the stubs, `tiplib.mjs`, `droplib.mjs`' `wav()`, the three `pub(crate)` words, and `AppState.takes` with its `ws_loop` arm. D1 merges into `studio` as soon as it is green (Day 1, before midday), and each lane merges `studio` then, before its UI slices. C3 is two commits: stems first (the render refactor, `stems.rs`, the route, st-stems through the API), then the Export dialog. If C3 is late, the dialog ships with Song and Stems, and MIDI stays on the clip menu. If D2 is late, its rows go in this order: mono, not a function, an undeclared package, two tracks with one ID. |
| D31 | Decoding once | The import decodes the file once, and the peaks come from that decode. The 15-minute limit is read from the header (`hound`) before decoding. Also refused in words: a file with no frames ("vox.wav has no sound in it."), a rate outside 8 to 384 kHz. |
| D32 | Where a drop lands | On the bar line at or before the pointer, as the hint says ("bar 5"). Q6's grid line goes. |
| D33 | Fade handles | Shown only on clips 24 px wide or more. The top 8 px of each corner is the fade handle; the edge below it resizes. |
| D34 | Words | The Record menu: "Count in first (one bar of clicks)", "Snap notes to the grid". The Export dialog's rows: "Song: the whole song as one WAV file", "Stems: each track as its own WAV file, before its volume and pan", "MIDI: the notes, for other music apps". The tempo question's second line: "Keep 120: the notes play at your song's speed." |
| D35 | Close Song while recording | The page stops the take first (App.tsx's `close_song` case, B's line), so it is written. Quitting the app drops it: one line for gaps.md. |
| D36 | ⇧⌘I in a browser | `chord()` gains `i` with ⇧ → `import_audio`, so the PC page works; in the app it is the menu's. pc-keys checks Ctrl+Shift+I on the PC page. |
| D37 | Writing MIDI files | No program change for an instrument Q33's table doesn't know. At one tick, Offs go before Ons. Two notes of one pitch that overlap: the first ends where the second starts. |
| D38 | Drops with no song, or read only | On the start screen the hint says "Open a song first", and nothing happens. In a read-only window the hint gives the window's own words. |
| D39 | The top bar at 1280 px | If fx-fixes fails at 1280 px once the Record button is in, the time readout shows from 1312 px; taste.md §3.8 says so at the End. |
| D40 | A lane gone mid-take | If the take's track is deleted while recording (another window), the write fails and the toast says "The recording couldn't be kept: Bass is gone." Nothing else. |
