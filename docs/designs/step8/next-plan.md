# Next: the plan

For the `next` track: branch `next`, worktree `/home/jerry/m2-runs/wt/nx`, made from `main` at `70f9b61` (steps 1 to 7,
the sounds, taste's look, all of 8b but 8b.7 and 8b.10, and polish). Written 2026-10-17 by a planning agent, read
only: files and `git`, nothing built or run. File:line cites are at `70f9b61`. Sizes: **S** under 2 hours of agent
time, **M** half a day, **L** most of a day *(estimates)*.

**The Decisions section overrides the rest of this file. So does the Attack section, once one is added.**

---

## In short

- This track makes the app nicer to play and safer to use. Four parts are built side by side.
- Nothing you export changes. The click, the keys you play and the drags you hear never reach an export.

| Part | What you get | Size |
|---|---|---|
| **A. Hear it while you drag** | While the song plays, a note, a clip or an automation point you drag is heard where it is, before you let go. Dragging the tempo is heard as you drag. | L |
| **B. Play in** | A metronome: a click on each beat (K, or its button). Your computer keys as a piano (⌘K): A S D F… play notes. A MIDI keyboard plays the selected track; plug it in or out at any time. | M + S + M |
| **C. Settings and safety** | A song opens with no sound device, and says so. A Settings window (⌘,): the sound output, the buffer size, where new songs go. Undo says what it undoes ("Undo Hats pan"). | S + M + M |
| **D. Effects and small fixes** | Turn an effect off and on. Move an effect before or after another. Pick which track Duck listens to. Seven small fixes: the ruler, drum rows, the mixer. | M + S + S + M |

- **Left for later:** recording what you play, a count-in, undo that survives closing the app, effect presets.
- **About 2 days.** Day 1: the four parts. Day 2 morning: the rest of them. Day 2 afternoon: join them, a review,
  the full check, pictures for you.

---

## Decisions

The lead and the lanes take each decision as written. Nobody waits for an answer, and nobody asks the owner.

| # | Question | Decision |
|---|---|---|
| Q1 | Where does `midir` live? Platform design §2.4 and §8 put it outside `m2-os`, like cpal; the HANDOFF and this track's brief say in. | **In `m2-os`**: one file, `core/os/src/midi.rs`, no OS folder (midir hides CoreMIDI and ALSA). The server calls `m2_os::midi::listen`. The End changes platform design §2.4 and §8's 8b row to say so |
| Q2 | Which `midir`? | `=0.11.0` (2026-04-18). It takes `alsa` 0.11, already in `Cargo.lock`. 0.11.1 is from 2026-10-10, under two weeks old: not taken. Root `[workspace.dependencies] midir = "=0.11.0"`, `midir.workspace = true` in `core/os/Cargo.toml` |
| Q3 | Which MIDI inputs play? | Every input port, all channels. ALSA's "Midi Through" ports are skipped. No input picker (left) |
| Q4 | Which track does MIDI play? | The selected track's notes lane: the lane of the clip Notes shows, if it is that track's; else the track's first notes lane. The window says which over its WebSocket (`{"t":"midi","module","lane"}`) whenever the selection changes; the last window to say wins. Song row, nothing selected, or a track with no notes lane: nothing plays, and the MIDI icon's tooltip says "Select a track with notes to play it" |
| Q5 | Which MIDI messages? | Note On (velocity 0 is an Off), Note Off, the sustain pedal (CC 64), All Notes Off and All Sound Off (CC 123, 120). Pitch bend, mod wheel, aftertouch, program change, clock: ignored (left) |
| Q6 | A MIDI key held over 10 s, and audition's watchdog (`QUIET`, `graph/audition.rs:27`) | A device source is never let go by the watchdog. It is let go when the keyboard is unplugged, the project closes, or ⌘. |
| Q7 | The computer-keys piano: which keys? | Logic's Musical Typing, by key place (`e.code`), so other keyboard layouts work: A W S E D F T G Y H U J K O L P ; ' play C to F a bit over an octave up (A is C4 = MIDI 60 at first); Z and X move an octave down and up (A from C1 to C7). Velocity always 0.8 (`KEY_VELOCITY`). No velocity keys, no sustain key |
| Q8 | Its name and switch | "Keyboard piano". A 28 px toggle in the top bar after the metronome (`Keyboard` icon), ⌘K on a Mac and Ctrl+K elsewhere, and View ▸ Keyboard Piano. Off at every launch, never kept. While on, a small card at the timeline's lower right shows the keys, the octave and "Plays Bass" |
| Q9 | The clash rule | While on, the piano owns exactly its 20 keys (the 18 notes and Z X), with no ⌘ ⌃ ⌥, outside text fields and the code editor. Those keys never also run their shortcut: A (Show Automation), S (Solo), E (Dock), X (Mix), Z (Zoom to fit), K (Metronome) wait until it is off. Every other key keeps its shortcut: Space, Return, C (Loop), M (Mute), B (Draw), Q, ⌫, the arrows, Esc. The ⌥K sheet lists the keys it takes |
| Q10 | The metronome: in the player, or a kernel outside the song? | **In the live engine, in Rust**: `core/runtime/src/click.rs`, mixed by `Engine::render` into each block after the plan renders it, before an export hold's fade. Only the live `Engine` mixes it, and only `core/play`'s `Live::new` makes one (`core/play/src/lib.rs:72`); offline renders and exports never make an `Engine`, so the click can't reach them. No kernel, no plan, no prepare, nothing in the song |
| Q11 | What it plays | A 30 ms decaying sine blip on every beat of the meter's unit (quarters in 4/4, eighths in 6/8), 1,500 Hz at −12 dBFS on a bar's first beat, 1,000 Hz at −16 dBFS on the others, the same in both channels. Only while the transport moves: never while stopped or held for an export. No count-in (left, with recording) |
| Q12 | Where it is switched and kept | The 28 px `Metronome` toggle after Loop (taste §3.3, x 176), K (taste §3.3), Transport ▸ Metronome. Kept per song in `.music2/view.json` as `metronome`, like the loop; off for a new song |
| Q13 | No sound device when a song opens (gaps #13) | The silent player plays instead: the playhead moves, nothing sounds. The status pill says "No sound output" (`warn`, `VolumeX`); its popover: "No sound device was found, so the song plays silently. Plug one in: it's used from your next action." While silent, the server looks every 2 s; when a default output exists, the next action makes a real player where it was, stopped, and a toast says "Sound output is back. Press Play to go on." |
| Q14 | Settings: where kept, how applied | `~/.music2/settings.json` (`{"output": {"device", "frames"}, "songs"}`), this machine's, never a song's, no undo. A page dialog, not a native window. Each change applies at once; a Done button closes it |
| Q15 | The output list | "System default (<its name>)" first, then cpal's output devices. A chosen device that is gone plays on the default, and the dialog says "(not connected)" beside it |
| Q16 | Buffer sizes | System default, 64, 128, 256, 512, 1024, 2048 samples, each only where the device's range allows, each with its time in ms. A change of device or size makes a new player where it was, stopped, with "Press Play to go on" (as a device change does today). The sample rate stays the song's (RENDER-31) |
| Q17 | Where new songs go | A folder picker (the desktop's `song_folder` action; a text field in a browser) and "Use the default" (`~/Music/music2`). `project::songs_dir()` follows it, and with it the Exports folder |
| Q18 | Settings' menu row and key | Mac: music2 ▸ Settings… ⌘, after About. Linux: File ▸ Settings… Ctrl+, above Quit. The ⌥K sheet lists it. The status pill's "Heavy load" popover gets "Audio settings…" (taste §3.3's DSP click) |
| Q19 | Undo names | Kept on the server for each undo step, by the step's Tx id (`Session::tops`, new and read only). The window that made an edit names it (`POST /api/graph/step`, sent by `history.ts`'s `recorded`); a step made elsewhere (an agent's Tx, `m2`) gets a plain name from what it changed: "Bass notes", "the tempo or time signature", "the code in tracks/bass.ts". Shown in Undo's and Redo's tooltips, in the Edit menu ("Undo Hats pan"), and in the toast after undoing a structural step ("Undid adding Reverb"). Lost on close (gaps #29's second half, left) |
| Q20 | Lane leases: when | Only while playing. Stopped, a drag commits on release as today. Notes and automation clips (move, resize), automation points, and the roll's note drags and paint. Audio lanes answer `previewable: false` and commit on release (design-8b §9.1). Esc during a drag cancels: the accepted data plays again |
| Q21 | Clip ids from preview to commit | The lease picks its Tx id at begin and the commit uses it (`clips::edit_as`), so a copied clip has the same id in the preview and the commit, and a note sounding in it isn't cut at release (RENDER-26a) |
| Q22 | The tempo lease's prepares | On the actor, as one coalescing job (the newest value; one in flight), not on a control-plane thread (design-8b §9.2). A tempo dragged during a long compile sounds after the compile; the field never waits. The design's thread is left |
| Q23 | Audio during a tempo drag (owner's Q2: it drifts) | Each audio player reads its unchanged renders at a lease offset, so it never skips and lags behind the grid until release; the commit or cancel crossfades it back (design-8b §9.2). **Fallback** if A4 runs past its half day: a song whose plan has an audio lane hears the tempo on release, as today (the field still drags); all else hears it live. Listed as left then |
| Q24 | Bypass in the song's code | `bypass: true` in the effect call's options object: `reverb(x, { size: 0.8, bypass: true })`. The SDK runs the effect's body as always (its frame, knobs and descriptor stay), records `bypass` on its frame, and returns the call's audio input instead of its output. Nothing reads the effect's output, so it isn't in `Flat.order` (`core/ir/src/check.rs:541`) and the engine never runs or makes it. **The engine doesn't change.** No song today writes `bypass`, so no IR byte and no render moves. Turning it on again is a code edit: the effect starts fresh, the reader crossfades over 5 ms (RENDER-25a) |
| Q25 | Which calls can be bypassed | A component whose first input is audio and whose output is audio (every std effect, Duck included). Anything else refuses `bypass` with words, as an unknown option is refused today (`core/script/sdk/core.ts:1310-1316`) |
| Q26 | Reorder | Only two effects written one directly inside the other, `delay(reverb(x, R), D)` ⇄ `reverb(delay(x, D), R)`, each keeping its own options. The effect card's menu: "Move before Reverb", "Move after Delay". Anything else is refused: "Reverb and Delay aren't next to each other in the code, so they can't swap here. Change their order in code." Both effects may start fresh (their `(` moves; a 5 ms crossfade) |
| Q27 | Duck's sidechain picker | On the Duck card, the side input row: "Ducks to: Kick ▾", listing the song's other tracks. A track that exports a control signal (Kick's `env`) is written as `kick.env`; another as `follow(hats.out)`, importing `follow` from `std/dsp`. Only where the duck's file imports that track (song.ts); else refused: "Bass's Duck is in tracks/bass.ts, which can't hear other tracks. Change it in code." |
| Q28 | The ruler: markers over bar numbers, and change marks touching them | `RULER_H` 28 → 36 (`web/src/lib/layout.ts:7`): markers keep their 14 px band at the top; bar numbers go below it (top 17 px); a tempo or meter change sits on the numbers' line, after the bar's number when it is at a bar line (6 px gap), else 3 px after its beat; beat ticks and the loop brace stay at the foot. taste.md §3.2 follows at the End |
| Q29 | Follow and zoom cover the last bar number | The timeline's width gains the tools' width (`TOOLS_W`, `SongTimeline.tsx:1335`) past the song's end, so the last number scrolls clear of them |
| Q30 | A drum lane's note menu and keys | On a lane with note names: "Up a row" ↑, "Down a row" ↓, and no octave rows (`NoteMenu.tsx:88-89`). Its key column draws every row alike: no black keys behind the names |
| Q31 | A first options object in a call written over several lines | On its own line at the arguments' indent, with a trailing comma when the call's last argument had one: `master(\n    compress(…),\n    { db: -2.5 },\n  )` |
| Q32 | Shift+wheel over a fader off the Mac | Shift+wheel never moves a fader, on any OS: it scrolls the strips sideways. When the browser sends it as up and down (deltaY with Shift), the page scrolls the strips itself |
| Q33 | `s8-mute`'s own lint | Replaced by `e2e/lintlib.mjs`'s `lint`, as polish did for four suites (PL-T4). Its count in `run.mjs` and the README follows |
| Q34 | Gates for 8b.7 and MIDI (`m2-gates lane-drag`, `tempo-drag`, G13 for a device) | **Cut**: `crates/gates` is 8a's this round. Rust tests time nothing; Linux timings are information only |
| Q35 | `m2` commands for bypass, reorder, the sidechain, the metronome | Left (gaps #37): `apps/m2` stays out of this track |
| Q36 | A lane needs a change in another lane's file | Only through the rules in §7.2. If a lane needs more, the lead makes it in `next` and the lanes merge `next` |

---

## Attack

See "## Attack" and the "## Decisions" after it, at the end of this file: they override everything above them, the first Decisions table included.

---

## 0. Where things stand

| Item | What exists | What's missing |
|---|---|---|
| Lane leases (8b.7, WRITE-22a) | A clip or note edit is one Tx, heard as its commit's preview before it is durable (PLANE-5, `crates/server/src/graph/preview.rs`). `Player::swap_lane` (`core/play/src/lib.rs:930`). `clips::apply` is pure (`crates/editor/src/clips.rs:158`); `clips::edit` makes the Tx (`:921`). `reach_to` builds swaps (`graph/actor.rs:264`). `NotesData::build` is public (`core/runtime/src/sched.rs:103`). Knob leases (`graph/lease.rs`) show the shape | Begin, update, commit, cancel for lanes; the page's drags using them |
| Tempo lease (RENDER-27a) | A tempo edit is an identity `Activate` that keeps the beat (RENDER-27). The tempo field drags and writes on release (`web/src/components/SongBar.tsx:57`, `TempoField`) | The lease; audio drift (`AudioPlayer`, `core/runtime/src/audio.rs:411`, has a `shift` for crossfades already) |
| MIDI (8b.10) | Live notes by source (MON-7): `Graph::audition(client, …)` (`graph/audition.rs`); a watchdog lets a quiet source go after 10 s | midir, a device source, the target, the status |
| Keyboard piano | `strike` and `release` in `web/src/lib/audition.ts:101`, `:129` | All of it |
| Metronome (gaps #19) | taste §3.3 places its button; nothing else | All of it |
| No device (gaps #13) | `Inner::device()` (`crates/server/src/graph.rs:567`) calls `Player::start()`, which fails with "no audio output device" (`core/play/src/lib.rs:782`), so the open fails | The fallback |
| Settings (gaps #31) | `Player::start()` takes the default device and config; songs go in `m2_os::dirs::songs()` (`crates/server/src/project.rs:28`) | A choice, a file, a dialog, a menu row |
| Undo names (gaps #29) | The page names the steps it made (`web/src/lib/history.ts`) and forgets them on reload; the server's steps carry `label` (before, after) values only; the Edit menu says "Undo" | Names on the server |
| Effects (gaps #21) | Add and remove (`crates/editor/src/effect.rs:203`, `:264`); the card's header buttons (`web/src/components/NodeGraph.tsx:766-800`) | Bypass, reorder, the duck picker |
| Ruler | Markers at the top 14 px over the bar numbers (`Markers.tsx:86`), numbers at top 4 px, change marks at bottom 6 px (`SongTimeline.tsx:1528-1560`); tools sticky over the ruler's end (`:1341`) | Room for both, room at the end |
| Drum lanes | Names in the key column; the menu says "semitone"; black keys drawn behind names | The words and the plain rows |
| Options layout | `Insertion::plan` writes `, { db: … }` after the last argument (`crates/editor/src/lens.rs:211-224`), so `}), { db: -2.5 },` (`mix_tests.rs:368-369`) | A line of its own |
| Fader wheel | Sideways deltas are ignored (`Mix.tsx:654`); Shift with deltaY moves the fader | The rule |

---

## 1. How the track runs

### 1.1 Branches and worktrees

- `next` in `/home/jerry/m2-runs/wt/nx` is the lead's: P0 (§2), the merges and the End (§9).
- After P0 the lead makes four lanes from `next`:
  ```
  cd /home/jerry/m2-runs/wt/nx && git config rerere.enabled true
  for l in a b c d; do git worktree add /home/jerry/m2-runs/wt/nx-$l -b next-$l next; done
  ```
  Each lane runs `git config rerere.enabled true` in its worktree too.
- When `main` moves (8a lands), the lead merges `main` into `next`; lanes merge `next` into their branch when it
  moves. Merges only: never rebase, never rewrite history, never push.
- Never touch `wt/8a`, nor tmux sessions `m2`, `1`, `2`, `ssh_tmux`. Never delete anything under `~/m2-runs/plugins`.

### 1.2 Builds

Every cargo, npm or music2 run has `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Builds are
`--locked`, never `cargo update`. The one exception: lane B's first build after adding `midir` (§4, B3).

| Who | Target | Builds |
|---|---|---|
| Lead (`wt/nx`) | `/home/jerry/m2-runs/s8n/target-nx` | the merge checks, the desktop app for pictures, the End's full check |
| Lane X (`wt/nx-x`) | `/home/jerry/m2-runs/s8n/target-x` | `cargo build --locked -p m2-server --example headless -p m2`, and the tests and clippy of the crates it touches |
| Lanes B and C, also | their own target | `cargo test --locked -p music2-desktop` and its clippy (menu rows); the mac lint of the crates they touch: `CC_aarch64_apple_darwin=$PWD/ci/stub-cc CXX_aarch64_apple_darwin=$PWD/ci/stub-cc AR_aarch64_apple_darwin=$PWD/ci/stub-ar cargo clippy --locked --target aarch64-apple-darwin -p <crates> --all-targets -- -D warnings` |

- After each build, copy the binaries per commit, so a rebuild never swaps one under a running suite:
  `B=/home/jerry/m2-runs/s8n/bin/<lane>-<sha>; mkdir -p $B/examples; cp $T/debug/examples/headless $B/examples/; cp $T/debug/m2 $B/`
  (`run.mjs` finds `m2` one folder above the server's).
- Web: `cd web && npm ci && npm run build` (it type-checks). Once per worktree also `cd e2e && npm ci`.
- Long jobs (cargo builds and tests over a few minutes, suite batches, the full check) go through
  `TW_WAIT=<seconds> ~/m2-runs/tw <name> '<cmd>'` (no double quotes or parentheses in the command). Logs in
  `~/m2-runs/logs/`. Name them `nx-<lane>-<what>`.
- **`Cargo.lock`: only lane B changes it**, only by adding `midir` and what it needs (§4, B3). Every other lane's
  `git diff Cargo.lock` is empty at each commit.
- At its end each lane deletes `target-<lane>` and `bin/<lane>-*`. The lead deletes `target-nx` and `bin/` after
  the pictures. (Scratch targets once filled 390 GB.)

### 1.3 Suites

- **Tools** (`/home/jerry/m2-runs/s8n/tools/`; change only these copies). P0 edits:
  - `lane-suites.sh <lane> <out> <suite>…`: `repo=/home/jerry/m2-runs/wt/nx` for `nx`, else
    `/home/jerry/m2-runs/wt/nx-<lane>`, and its header comment. `DIST=<folder>` serves an older page (red first);
    `OLD=<folder>` an old tree's std, templates and examples; `M2_HEADLESS` (required) a copy under
    `/home/jerry/m2-runs/s8n/bin/`.
  - `app-shot.sh`: runs under `flock /home/jerry/m2-runs/s8n/scratch/x97.lock`, so two lanes never share `:97`.
    Display `:97` only, scratch under `/home/jerry/m2-runs/s8n/scratch/`. It passes `M2_MIDI=inject` to the app, so
    the app never opens a MIDI device here and a `HOOK` can post one (§9 step 6).
  - `lane-a-suites.sh`, `ui-suites.sh`: unused; left as they are.
- **Running:**
  ```
  M2_HEADLESS=/home/jerry/m2-runs/s8n/bin/a-<sha>/examples/headless \
    /home/jerry/m2-runs/s8n/tools/lane-suites.sh a /home/jerry/m2-runs/s8n/scratch/e2e/a1 s8-leases taste s8-clips
  ```
  `--repo` is the lane's worktree, which built the server, so std, the templates and the examples are the lane's.
  Each suite gets a scratch HOME under `<out>` and `M2_AUDIO=none` (`e2e/run.mjs`, `envFor`): never the real
  `~/.music2` or `~/Music`.
- **Server env per suite** (P0): a `SUITES` row may carry `env: {…}`, merged into `envFor`'s. Every suite's server
  gets `M2_MIDI=inject`: no suite ever opens a MIDI device, and lane B's injection route answers (§4, B3).
- **MIDI in tests:** `/dev/snd/seq` gives `EACCES` here. Rust tests call `Graph::midi(…)` with events, as
  `m2_os::midi::listen` would. Suites post to `POST /api/graph/midi/inject` (only with `M2_MIDI=inject`).
- New suites run as a Mac page and open one PC page each (`open(…, { pc: true })`, `e2e/fixlib.mjs`) for the Linux
  words ("Ctrl+K", "Ctrl+,").
- Each new suite lints its new states itself with `e2e/lintlib.mjs` (a dialog, the piano card, a menu). Nobody edits
  `e2e/taste.mjs`.

### 1.4 Checks per slice

1. The Rust tests and `cargo clippy --locked -p <crates> --all-targets -- -D warnings` of the crates the slice
   touches (and the mac lint for lanes B and C, §1.2).
2. The web build.
3. `taste`.
4. The slice's own suite.
5. The suites it moves (listed per item).
6. Lanes A, B and D, whose changes reach what plays: the renders, as `ci/check.sh` runs them:
   `M2=$T/debug/m2 e2e/baseline.sh --tree . <out>` then `(cd <out> && shasum -a 256 -c <repo>/e2e/renders.sha256)`:
   10 of 10.

The full `ci/check.sh` runs once, at the End.

### 1.5 Red first

Every new Rust test and suite check that tests behaviour is shown failing first:
- a suite: run it on the page and server from before the change (`DIST=` the old `web/dist` copy, `M2_HEADLESS=` the
  old `bin/` copy, `OLD=` when std changed), and copy two or three `FAIL` lines into the build notes;
- a Rust test: run it with the change taken out (a stub that does nothing), and copy the failing assert.

### 1.6 Commits and build notes

- Small and green. Subject in plain words, a blank line, a short body, then exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01R1SbPDdWrZoQ7z8dxU28hJ
  ```
- Each slice adds its section to `docs/designs/step8/build-notes-next.md` in the same commit: what changed, tests,
  red first, what's left, and two lines for the End: **For gaps.md** and **Contract**. A slice that touches
  `core/runtime` or `core/play` adds a third: **For 8a's merge**: each block it added there, file and function, in
  a sentence each (§1.9).
- P0 makes that file with `## P0`, `## Lane A`, `## Lane B`, `## Lane C`, `## Lane D`, `## Merge`, `## End`, a
  line under each. A lane writes only under its own heading.

### 1.7 The desktop app

Only on the invisible Xvfb screen `:97`, through `tools/app-shot.sh` (it takes the lock), a scratch HOME under
`/home/jerry/m2-runs/s8n/scratch/` and `M2_AUDIO=none`. Never on a real screen. Lanes B and C may open it once each to
check their menu rows (B: View ▸ Keyboard Piano, Transport ▸ Metronome; C: File ▸ Settings… with Ctrl+,, and Edit ▸
Undo with a name). The End takes the pictures (§9).

### 1.8 Writing for the owner

The In short sections, the build notes' In short, captions, HANDOFF and every word in the UI: short sentences, plain
words, tables. Keys as `chord()`, `keyName()` and `shifted()` spell them for each OS (`web/src/lib/native.ts`). The
track's ID is "ID", never "key"; "key" in the UI means a key you press or the musical key.

### 1.9 `core/runtime` and `core/play`: 8a's rule

8a changes `core/runtime`, `core/play` and `core/os` (cores, thread priority, then `rt.rs`). Here, each change there is
a small block of its own, named in the build notes' **For 8a's merge**. All of them:

| Lane | File | Block |
|---|---|---|
| A (A4) | `core/runtime/src/audio.rs` | `AudioPlayer.lease: i64`; `cur` read at `runs.shifted(self.lease)` in `fill`; `anchor(&mut self, …)` |
| A (A4) | `core/runtime/src/plan.rs`, `lib.rs` | `Keep.audio_as_is` (keep each audio lane's renders in a prepare); `Renderer.lease_map: Option<Arc<TempoMap>>` and one line where an activation enters (`take_over`) to anchor each audio player; jumps re-anchor |
| A (A4) | `core/runtime/src/tests/live/lease.rs` (new), one `mod lease;` line in `tests/live.rs` | its tests |
| A (A4) | `core/play/tests/no_alloc.rs` | `the_lease_offset_never_allocates`, at the file's end |
| B (B1) | `core/runtime/src/click.rs` (new), `lib.rs` | `pub mod click;` |
| B (B1) | `core/runtime/src/engine.rs` | `Engine.click: Click`; `pub fn set_click(&mut self, on: bool)`; in `render`, one line after `let runs = *cur.last_runs();`: `self.click.mix(&mut frames, &runs, cur)` |
| B (B1) | `core/runtime/src/tests/click.rs` (new), `tests.rs` | `mod click;` |
| B (B1) | `core/play/src/lib.rs` | `Shared.click: AtomicBool`; `Player::set_click`; in `Callback::render`, `shared` in the destructure and one line before `l.fill(…)`: `l.engine.set_click(…)` |
| B (B1) | `core/play/tests/no_alloc.rs` | `the_click_never_allocates` |
| C (C1, C2) | `core/play/src/outputs.rs` (new), `lib.rs` | `pub mod outputs;`; `Player::start` becomes `start_with(&Choice::default())` (the device and config lines only); `M2_AUDIO=fail` and `M2_AUDIO_OUTPUTS` seams; `Shared.frames: AtomicU32` and one line in `fill_in` after `last_callback_ns`; `Player::frames()`; `pub fn output_ready() -> bool` |

Nothing here edits `core/os` but lane B's new `midi.rs`, its `pub mod midi;` line and `midir` in
`core/os/Cargo.toml`.

---

## 2. P0: the lead's ground — S

About an hour, then the lanes start. No behaviour a user can reach.

1. `git config rerere.enabled true` in `wt/nx`.
2. `docs/designs/step8/build-notes-next.md` with its headings (§1.6). Commit.
3. `e2e/run.mjs`: a `SUITES` row may carry `env`, merged into `envFor(home, o)`; every suite's server gets
   `M2_MIDI=inject`. One line in `e2e/README.md` says so. The suites all pass as before (`env` is empty on every row;
   `M2_MIDI` is unknown to today's server). Commit.
4. The tools (§1.3): `lane-suites.sh`'s repo mapping, `app-shot.sh`'s lock.
5. The four worktrees (§1.1); `npm ci` in each `web/` and `e2e/`.
6. A first `cargo build --locked -p m2-server --example headless -p m2` into `target-nx`, copied to `bin/nx-<sha>`:
   the "before" server every lane's red-first runs use, with `web/dist` copied to
   `/home/jerry/m2-runs/s8n/scratch/dist-before`.

---

## 3. Lane A: hear it while you drag — `wt/nx-a`, branch `next-a`

8b.7, design-8b §9, WRITE-22a and RENDER-27a. On the existing `Player::swap_lane` and the knob leases' path. One Tx at
release, as today.

### A1. Lane leases on the server — M

**What the user sees.** Nothing yet (A2 is the page).

**The back end.**
- **Routes** (new `crates/server/src/routes_leases.rs`, its own router, merged in `routes.rs`):
  - `POST /api/graph/lane/lease/begin {session, revision, gesture, client?, ttl_ms?}` → `{lease, previewable, lanes:
    [{module, lane}], heard}`. `gesture` is a `clips::Gesture` (`Move`, `Resize`, `Notes`, `Points`, `Create`). A
    gesture on an audio lane, or any other op, answers `previewable: false` and makes no lease. A read-only window
    gets 423 with WRITE-18's words (the lease would end in a write).
  - `POST …/update {lease, seq, gesture}` → `{seq}` (the engine message's, null when nothing was sent or the update
    was older than one taken).
  - `POST …/keepalive {lease}`; `POST …/cancel {lease}`.
  - `POST …/commit {lease, seq, gesture}` → the same answer as `POST /api/graph/clips` (`{file, before, after,
    revision, made}`), 409 with `changed: true` when a clip it names changed since (WRITE-12). In `WRITES`.
  - `GET /api/graph/leases` lists lane leases too, with `kind: "lane"`.
- **The control plane** (new `crates/server/src/graph/lanes.rs`, an `impl Graph` block as `audition.rs` is):
  - `EngineView` gains `accepted: Arc<Revision>`, published by the actor's first phase (`session.accepted_arc()`), so
    begin and update never wait for the session (RENDER-29).
  - `ControlPlane.lanes: LaneLeases`: each `{id, token, owner, tx_id, gesture, seen (the shas it named), seq, sent:
    Vec<(LaneAddr, slot, generation)>, touched}`.
  - **begin:** check the gesture on `accepted` with `clips::apply`, minting ids by `ClipId::derive(&tx_id, lane, n)`
    with a `tx_id` picked now (`m2_song::tx::random_id()`). Then as update.
  - **update:** outside the lock, `clips::apply(&accepted, &gesture, mint)` → for each lane edited: its slot in
    `plays.info.lanes`; notes: `NotesData::build(&clips, accepted.compiled.data.tempo(), rate)`, sent only when
    `caps.fits(needs)` (else heard at the commit, which grows the lane); control: `ControlLane::new(clips)`. Under
    the control lock, only if `plays.generation` is the one it built for: `player.swap_lane(generation, slot, swap)`.
    A lane the last update touched and this one doesn't (a clip dragged onto another track and back) gets its
    accepted data back.
  - **commit:** an actor job: the lease ends and `clips::edit_as(s, &gesture, &tx_id)` runs (new, in
    `crates/editor/src/clips.rs`: `edit` with a given Tx id; `edit` calls it). Its PLANE-5 preview swaps the
    candidate, which is the preview: nothing heard changes. A refused commit swaps the accepted data back and says
    why.
  - **cancel, idle TTL (10 s; the page's keepalive every 2 s), the window closing (`client_gone`):** under the
    control lock, each lane in `sent` gets its accepted data back; the lease ends.
- **The actor** (`graph/actor.rs`, lane A's blocks in `settle`'s first phase, under the control lock after its own
  sends):
  - a settle whose swaps touched a leased lane: if every clip in `seen` still has its sha, re-apply the gesture on
    the new accepted revision and send it; else end the lease, the accepted data playing, and tell that window:
    "Bass changed elsewhere: your drag was dropped."
  - a settle that sent an activation (a new generation): re-send each live lease's preview to the new generation.
    The queue is FIFO, so the new plan plays the preview from its first block or the one after.
- **Exports** render the accepted revision offline: a lease never reaches one.

**Tests** (`crates/server/src/graph/lanes_tests.rs`, driven player, `M2_AUDIO` none). Red first: each against a stub
`begin` that answers `previewable: false`.
- `a_lane_lease_previews_with_no_revision`: playing, a Bass note moved a beat later: the engine plays its On at the
  new beat before the commit; `data_rev` doesn't move.
- `the_commit_changes_nothing_heard`: after the commit, no message changes the lane's data; one revision, one undo
  step.
- `cancel_puts_the_accepted_notes_back`; `a_closed_window_cancels_its_lane_lease`; `an_idle_lane_lease_expires`.
- `an_outside_write_to_another_clip_keeps_the_preview`; `an_outside_write_to_the_dragged_clip_ends_the_lease`.
- `a_new_plan_carries_the_preview` (a code edit elsewhere during the drag).
- `a_preview_too_big_for_its_room_waits_for_the_commit`.
- `lane_previews_never_wait_for_a_compile` (a 1.5 s compile, as `previews_never_wait_for_a_compile`).
- `a_note_dragged_away_from_the_playhead_is_ended` (RENDER-26a).
- `a_copied_clip_keeps_its_id_from_preview_to_commit`; `an_audio_lane_gesture_is_not_previewable`.
- `an_export_during_a_lane_lease_has_the_accepted_notes`.
- `crates/editor`: `edit_as_uses_the_ids_it_is_given`.

**Files.** `crates/server/src/routes_leases.rs`, `graph/lanes.rs`, `graph/lanes_tests.rs` (new); `graph.rs`
(`EngineView.accepted`, `ControlPlane.lanes`, `mod lanes;`, the calls in `expire` and `client_gone`); `graph/actor.rs`
(the publish of `accepted`, the two settle blocks); `routes.rs` (one `.merge`); `lib.rs` (`mod routes_leases;`);
`crates/editor/src/clips.rs` (`edit_as`).

### A2. Lane leases on the page — M

**What the user sees.** While the song plays, dragging a notes or automation clip, a clip's edge, an automation point,
or a note in Notes (move, resize, pitch, paint) is heard where it is at once. Letting go writes it, one Undo step, as
now. Esc during the drag puts it back. Stopped, nothing changes. Nothing on screen is new: the sound is the feedback
(design-8b §9.3).

**The back end.** New `web/src/lib/laneLease.ts`:
- `begin(graph, gesture)`: only while `status.playing`; null when not playing, refused or not previewable (the drag
  then works as today).
- `update(gesture)`: the latest gesture, at most one per animation frame, `seq` counting up; a keepalive every 2 s.
- `finish(gesture, step)`: the commit; resolves as `clips.ts`'s `send` does (`Done`), and records the step
  (`history.ts`).
- `abort()`: cancel (Esc, `pointercancel`, the view unmounting).
- `SongTimeline.tsx` (lane A's regions only, §7.2): the clip drag (`Drag`, move and resize) begins at the drag's
  threshold, updates on each move, and `drop` calls `finish` instead of `send` when a lease lives; the point drag
  and Draw's paint the same through `sendPoints`.
- `NoteView.tsx` (lane A's regions only): a note drag or paint sends `{op: "notes", clip, notes, length}` from the
  draft on each frame; release calls `finish` (one Clips Tx: `Gesture::Notes`) instead of `writeLane` for that drag.
  Menu tools, keys and typed edits stay as they are.

**Tests.** Suite `e2e/s8-leases.mjs` (kind `fix`, about 35 checks *estimate*), the clip and roll half (A3 adds the
tempo half). Red first on `dist-before`: no `lane/lease` request is made.
- Playing: drag Bass's clip one bar: one `begin`, three or more `update`s, one `commit` (the page's requests,
  intercepted); `GET /api/graph/leases` lists a `lane` lease mid-drag; `view.revision` the same mid-drag, one more
  after; `data/bass/notes.json` written once.
- Esc mid-drag: a `cancel`, no revision, the lease gone, the clip back.
- A second page closes mid-drag: its lease is gone within 1 s.
- Stopped: no `begin`; one `/api/graph/clips` request, as today.
- An automation point dragged while playing: begin, updates, commit.
- Notes: a note dragged while playing: begin, updates, commit with `op: "notes"`; the undo step is "Bass notes".
- Lint of the timeline mid-drag.
- Moves: `s8-clips`, `s8-automation`, `pl-notes`, `notes`, `gestures`, `g11-play`, `taste`.

**Files.** `web/src/lib/laneLease.ts` (new), `SongTimeline.tsx` and `NoteView.tsx` (drag code only),
`e2e/s8-leases.mjs` (new), `e2e/run.mjs` and `e2e/README.md` (its row after `s8-automation`).

### A3. The tempo lease — M

**What the user sees.** Dragging the tempo in the top bar (or in a tempo change's popover): the number turns `accent`
(taste §3.3) and the song plays at the dragged tempo as you drag, playing or not (the time readout follows). Let go:
written once, one Undo step. Esc: back to what it was.

**The back end** (new `crates/server/src/graph/tempo_lease.rs`; routes in `routes_leases.rs`):
- `POST /api/graph/tempo/lease/begin {session, at}` (the tempo entry in effect at beat `at`) → `{lease, bpm}`;
  `update {lease, seq, bpm}`; `commit {lease, seq, bpm}` → `/api/graph/tempo`'s answer; `cancel {lease}`; TTL and
  the window closing as A1.
- An update stores the newest bpm in the lease and queues one actor job, `TempoPreview` (Q22): if a value waits, the
  actor builds `DataSet::with_arrangement(…)` (new in `core/song/src/data.rs`: the same lanes, a new arrangement and
  tempo map), prepares an identity activation against the plan it sent last (`Basis::Keep`,
  `Continues::identity`), and sends it under the control lock only if that plan is still the last one sent; else it
  prepares again. One in flight; the newest value next.
- `Plays.tempo: Option<f64>`: what plays isn't the accepted arrangement. While it is set, `way()` takes an
  activation, never a reach (`actor.rs:525`), so a cancel, an expiry or the commit's settle always activates the
  accepted arrangement.
- `Timing` (seek, the loop check, the status's beat and `bpm`) uses the lease's map while it lives.
- Commit: 8b.2's builder (`song::tempo`, as `Graph::tempo` runs it), one Tx writing `data/arrangement.json`; the
  lease ends with it.
- The prepare's cost at p50 is measured once in a test and written in the build notes (design-8b §9.5),
  information only.

**Tests** (`graph/tempo_lease_tests.rs`). Red first against a stub update that does nothing.
- `the_tempo_is_heard_at_the_first_block_after_its_prepare` (the status's `bpm`, and a note's frame).
- `one_tempo_prepare_in_flight` (50 updates: at most 50 activations, the last value heard, none coalesced).
- `a_stale_tempo_prepare_is_done_again` (an edit's activation lands between).
- `a_tempo_commit_is_one_tx`; `a_tempo_cancel_goes_back`; `a_closed_window_ends_its_tempo_lease`.
- `a_seek_during_a_tempo_lease_lands_on_its_beat`.
- `an_export_during_a_tempo_lease_has_the_accepted_tempo`.
- `core/song`: `with_arrangement_keeps_every_lane`.
- Suite `s8-leases`, the tempo half: the number is `accent` while held (computed colour); the status's `bpm` takes
  three or more values mid-drag; `arrangement.json` is written once, on release; Esc puts the tempo back; lint.
  Moves: `s8-songbar`, `fx-fixes`.

**Files.** `graph/tempo_lease.rs`, `graph/tempo_lease_tests.rs` (new); `graph.rs` (`ControlPlane.tempo`, `Timing`);
`graph/actor.rs` (`Plays.tempo`, `way()`, the job); `routes_leases.rs`; `core/song/src/data.rs`
(`with_arrangement`); `web/src/lib/tempoLease.ts` (new); `SongBar.tsx` (`TempoField`'s drag only).

### A4. Audio drifts during a tempo drag — M (fallback: Q23)

**What the user hears.** In a song with audio clips (`examples/resample`), dragging the tempo: the audio plays on at
its own speed, drifting off the grid, never skipping. On release it crossfades to the new timing in 5 ms.

**The back end** (`core/runtime`, the blocks of §1.9):
- The leased prepare keeps each audio lane's renders as they are (`Keep.audio_as_is`): nothing is re-rendered during
  the drag (RENDER-27a).
- `AudioPlayer.lease` (`audio.rs:411`): an offset added to the transport's frame where `cur` is read. At the block
  line where a leased activation enters, it becomes the read position just before the line minus the transport's new
  frame, so the read position doesn't move. A seek or a wrap during the lease re-anchors it on the jump's beat in the
  accepted map (`Renderer.lease_map`), declicked as any jump is.
- Commit and cancel bring it back to 0 through the existing `swap(lane, shift)` with `shift` = the offset (design-8b
  §9.2): `prev` read at its offset, `cur` the new renders (commit) or the same ones (cancel), crossfaded over 5 ms.

**Tests** (`core/runtime/src/tests/live/lease.rs`, new):
- `a_tempo_drag_never_skips_an_audio_lane`: `examples/resample` from 1:00, 50 updates from 124 to 128 BPM: the
  output's largest sample-to-sample step is no larger than in the same span with no drag, and a wrap during the drag
  lands on its beat. Red first: without the offset the step is about the jump of 0.5 s of audio.
- `a_tempo_commit_crossfades_the_audio_back`; `a_cancel_crossfades_it_back`.
- `the_lease_offset_never_allocates`, appended to `core/play/tests/no_alloc.rs` (a tempo-leased activation and a
  wrap under the trap; §7.2's rule for that file).
- `core/play/tests/bit_identity.rs` unchanged; the renders 10 of 10.

**If it runs past its half day:** Q23's fallback (the actor skips the preview while `plays.info.lanes` has an audio
lane), a test `a_song_with_audio_hears_the_tempo_on_release`, and the left line in the build notes.

---

## 4. Lane B: play in — `wt/nx-b`, branch `next-b`

### B1. The metronome — M

**What the user sees.** A `Metronome` toggle after Loop in the top bar (taste §3.3; everything right of it moves 32
px). Click it, or press K, or Transport ▸ Metronome: while the song plays, a click on each beat, a higher one on the
bar's first. Kept per song. Exports never have it.

**The back end.**
- `core/runtime/src/click.rs` (new): `Click { on, ringing, phase, step, gain }` and
  `mix(&mut self, frames: &mut [[f32; 2]; BLOCK], runs: &Runs, r: &Renderer)`: for each moving run, the beats it
  covers, `[b0, b1)` (`r.tempo().beat_at`), the grid points in it (every `4 / unit` quarter notes from each meter's
  start, `Arrangement::meter_at`), each one's frame (`frame_of`), a blip started there; an accent where
  `bar_at(g).1` is 0. Half-open ranges, so a run that starts on a beat clicks it and a run that ends on one doesn't.
  Sine, 30 ms exponential decay (Q11). Plain fields, no allocation, nothing while off and not ringing (so with it off,
  not a bit changes).
- `engine.rs`, `lib.rs`, `core/play/src/lib.rs`: the blocks of §1.9. The click mixes before the hold's fade, so an
  export's hold silences it too; meters read taps inside the plan, so they never see it.
- Server: `POST /api/graph/metronome {session, on}` (in `routes_8b.rs`): `player.set_click(on)` and
  `view_file::write(dir, "metronome", on)`; `view_file::KEYS` gains `"metronome"`, so the state's `local` has it.
  When a project loads into a player (`Open::load`, `actor.rs:500`, one block), the player gets the song's
  `metronome`. A read-only window keeps it in memory only, as the view route does.
- Web: `web/src/lib/metronome.ts` (new): state from `local.metronome`, the toggle; the button in `TopBar.tsx`;
  `App.tsx` `key()` gains `k: "metronome"` and `appCommand` `case "metronome"`; `menu.rs` Transport ▸ Metronome
  (no accelerator: K is the page's); the ⌥K sheet's Transport row "Metronome on / off: K".

**Tests.**
- `core/runtime/src/tests/click.rs` (new): `the_click_sounds_on_each_beat_louder_on_the_bar` (120 BPM 4/4, two bars,
  a silent plan: energy only at the beats' frames, the accent at beats 0 and 4); `the_click_follows_the_meter_and_a_
  tempo_change` (3/4, then 6/8); `the_click_lands_on_a_loop_wrap`; `no_click_while_stopped_or_held`;
  `the_click_off_changes_no_bit` (the engine's output equals the offline render's bits). Red first: `set_click` a
  stub: the beats' frames are silent.
- `core/play/tests/no_alloc.rs`: `the_click_never_allocates` (on while playing, under the trap).
- Server: `the_metronome_is_kept_per_song` (view.json, reopen); `an_export_never_has_the_click` (metronome on: the
  export's bytes equal `m2 render`'s).
- Suite `e2e/nx-click.mjs` (kind `fix`, about 14 checks): the button after Loop, off at first (`aria-pressed`
  false); a click and K toggle it; `view.json` has `metronome: true`; a reopen keeps it; the state's `local`; the PC
  page's K; the song's name keeps its room at 1100 px; lint. Red first on `dist-before`: no button. Moves:
  `s8-songbar`, `fx-fixes`, `pc-keys`, `taste`.
- The renders: 10 of 10.

**Files.** `core/runtime/src/click.rs`, `tests/click.rs` (new); `core/runtime/src/{engine.rs, lib.rs, tests.rs}`;
`core/play/src/lib.rs`; `core/play/tests/no_alloc.rs`; `crates/server/src/routes_8b.rs`; `graph/view_file.rs`;
`graph/actor.rs` (`Open::load`'s block); `web/src/lib/metronome.ts` (new); `TopBar.tsx`, `App.tsx`, `Shortcuts.tsx`,
`menu.rs` (lane B's blocks); `e2e/nx-click.mjs` (new).

### B2. The keyboard piano — S

**What the user sees.** A `Keyboard` toggle after the metronome, ⌘K (Ctrl+K off the Mac), or View ▸ Keyboard
Piano. Off at every launch. While on, a card at the timeline's lower right: two rows of key caps (A S D F G H J K L ; '
and W E T Y U O P) with the notes they play, "Z  C4–F5  X", and "Plays Bass"; a held key lights. Letters then play
the selected track (Q4, Q7, Q9). Typing in a field or the code editor never plays. Switching windows lets every note
go.

**The back end.** Web only.
- `web/src/lib/playTarget.ts` (new): the selected track's notes lane (Q4); shared with B3.
- `web/src/lib/piano.ts` (new): `usePiano` `{on: false, octave: 4}`; the key map by `e.code`; a capture-phase
  `keydown` and `keyup` listener on `window` while on: no ⌘ ⌃ ⌥, not in a field, not in `.monaco-editor`, no
  `e.repeat`; a piano key is `preventDefault` + `stopImmediatePropagation` (so `App.tsx`'s shortcut never runs) and
  `strike(module, lane, pitch, KEY_VELOCITY, {always: true})`; its `keyup` releases that key's handle. `blur` and
  `visibilitychange` release all. Z and X change the octave; notes held keep sounding until their key is up.
- `audition.ts`: `strike` takes `{always}`: the piano plays while Notes' audition toggle is off.
- `components/KeyboardPiano.tsx` (new): the card; `TopBar.tsx` the toggle; `App.tsx`: `chord()` maps ⌘K to `piano`;
  `case "piano"` asks `editorHooks.chord?.("piano")` first (the code editor's own ⌘K chords), else toggles;
  `menu.rs`: View ▸ Keyboard Piano, `CmdOrCtrl+K`; the ⌥K sheet: View "Keyboard piano on / off: ⌘K" and a group
  "Keyboard piano" with the keys.

**Tests.** Suite `e2e/nx-piano.mjs` (kind `fix`, about 30 checks; notes counted from the page's WebSocket frames as
`s8-audition` does). Red first on `dist-before`: ⌘K does nothing.
- Off at first: the toggle's `aria-pressed` false, no card; A shows automation as before.
- ⌘K (Mac page) and Ctrl+K (PC page) turn it on; with Bass selected the card says "Plays Bass".
- A sends an On for pitch 60 on `bass.notes`; its key up an Off; W is 61; K is 72; X then A is 72; Z Z then A is 48.
- While on: S solos nothing (the state's `overlays` unchanged), E keeps the dock, Z doesn't zoom, A doesn't show
  automation; C still loops, M still mutes, Space still plays.
- "asdf" typed into the rename field types and sends nothing; the code editor gets its letters.
- A key held with repeats sends one On; the window's `blur` lets go (the status's `live` back to 0).
- Nothing selected: the card says "Select a track with notes to play it", and A sends nothing.
- Off again: A shows automation again. Lint of the card.
- Moves: `s8-audition`, `pc-keys`, `taste`.

**Files.** `web/src/lib/{playTarget.ts, piano.ts}`, `components/KeyboardPiano.tsx`, `e2e/nx-piano.mjs` (new);
`audition.ts`; `TopBar.tsx`, `App.tsx`, `Shortcuts.tsx`, `menu.rs` (lane B's blocks).

### B3. The MIDI keyboard — M

**What the user sees.** Plug in a MIDI keyboard, before or while the app runs: a toast says "Keystation 49 is
connected. It plays the selected track." A small `KeyboardMusic` icon shows in the top bar, before the status pill,
while one is connected; its tooltip: "MIDI keyboard: Keystation 49 · plays Bass". Keys play the selected track at
their velocity; the sustain pedal holds. Unplug it: its notes stop, the icon goes, and a toast says it was unplugged.
Nothing is recorded.

**The back end.**
- **`Cargo.lock`:** add `midir = "=0.11.0"` (Q2), then one plain `cargo build -p m2-os` (midir isn't in
  `~/.cargo/registry`, so cargo fetches it; `--offline` can't). Write in the build notes exactly what `Cargo.lock`
  gained (`git diff Cargo.lock`): expected new entries `midir 0.11.0`, `coremidi 0.9.x`, `coremidi-sys`, `block`,
  `jni-min-helper` and its build helper; existing `alsa 0.11.0`, `libc`, `log`, `bitflags 2`, `windows 0.62.2`,
  `parking_lot`, `jni 0.21.1`, `ndk-context` and the wasm crates reused. No other entry may change; if one would,
  stop and pin the new crate lower. From then on `--locked`. Run the mac lint for `m2-os` (§1.2): it fetches the
  Mac-only crates.
- **`core/os/src/midi.rs`** (new; `pub mod midi;`): `pub fn listen(on: Arc<dyn Fn(Event) + Send + Sync>) ->
  Listening`. A thread `m2-midi` lists input ports every second (one `MidiInput` kept for listing, made again at most
  every 5 s when it won't open), connects each new port with its own `MidiInput` ("music2"), drops gone ones, and
  calls `on(Event::Plugged(name))`, `on(Event::Unplugged(name))`, and, from midir's callback,
  `on(Event::Bytes(name, &bytes))`. Ports named "Midi Through" are skipped. One that won't open (`EACCES` here) is
  "no devices", never an error and never a message. Dropping `Listening` stops the thread and closes every
  connection.
- **`crates/server/src/graph/midi.rs`** (new): `ControlPlane.midi`: the devices, the target `(client, module, lane)`,
  each device's sustain and its held-back Offs. `Graph::midi(Event)`:
  - Plugged: the device joins the list, the news says so, a toast.
  - Unplugged: `audition_gone("midi:<name>")` (one `Release`), the device leaves, a toast.
  - Bytes: `0x9n` with velocity > 0 → an On, handle `note + 128 × channel`, pitch the note, velocity `v / 127`;
    `0x8n`, or `0x9n` at 0 → an Off (held back while the pedal is down); `0xBn 64` ≥ 64 down, < 64 up (the held
    Offs go); `0xBn 120` or `123` → let go of all. All through `Graph::audition("midi:<name>", …)` on the target's
    lane. No target: nothing. No project open: nothing.
  - `Graph::midi_target(client, module, lane)` from the WebSocket's `{"t":"midi","module","lane"}` (an arm in
    `routes_8b.rs`'s `ws_text`); `module: null` clears it.
  - The state and news: `midi: {devices, target}`.
- **`audition.rs`:** `Source.device` (a `midi:` client); `expire_auditions` skips device sources (Q6);
  `expire_auditions_at(now)` for tests.
- **`crates/server/src/lib.rs`:** `start()` calls `m2_os::midi::listen` unless `M2_MIDI` is `none` or `inject`, and
  keeps its `Listening` in `AppState`. With `inject`, `POST /api/graph/midi/inject {device, plugged?, bytes?}`
  (`routes_8b.rs`) feeds `Graph::midi` as the listener would; otherwise it answers 404. The desktop app and the
  headless server get it through `start()`: no change in `apps/`.
- **Web:** `web/src/lib/midi.ts` (new): sends the target from `playTarget.ts` on every selection or clip change and
  on each (re)connect; `components/MidiStatus.tsx` (new): the icon and its tooltip, from `graph.midi`.

**Tests.**
- `core/os`: `listening_never_errors` (here: no devices; dropping stops the thread within 2 s);
  `through_ports_are_skipped` (the name filter).
- `crates/server/src/graph/midi_tests.rs` (new, driven player): `a_midi_key_plays_the_selected_track` (the
  player's `live_notes` 1); `note_off_and_velocity_zero_end_it`; `velocity_is_the_keys`;
  `channels_dont_share_a_key`; `the_sustain_pedal_holds_offs_until_it_lifts`; `all_notes_off_lets_go`;
  `unplugging_a_keyboard_lets_go_of_its_notes`; `a_held_midi_key_outlives_the_watchdog` (red first: today's watchdog
  lets it go at 10 s); `no_track_selected_plays_nothing`; `a_new_target_leaves_held_notes_where_they_sound`;
  `midi_never_waits_for_a_compile`; `an_export_has_no_midi_note`.
- Suite `e2e/nx-midi.mjs` (kind `fix`, about 20 checks; notes injected through `/api/graph/midi/inject`): no icon at
  first; plug "Test Keys": a toast, the icon, its name in the tooltip; Bass selected: note on 60 → the status's
  `live` 1, note off → 0; Keys selected, then a note: `GET /api/graph`'s `midi.target` is `keys.notes`; the pedal
  holds an Off until it lifts; unplug while a key is held: `live` 0, the icon gone, a toast; the PC page the same;
  lint. Red first on the `before` server: the inject route is 404.
- Moves: `s8-audition`, `taste`.

**Files.** `Cargo.toml`, `Cargo.lock`, `core/os/Cargo.toml`, `core/os/src/{lib.rs, midi.rs}`;
`crates/server/src/graph/{midi.rs, midi_tests.rs}` (new), `graph/audition.rs`, `graph.rs` (`ControlPlane.midi`,
`mod midi;`, the state's `midi`), `routes_8b.rs`, `lib.rs` (lane B's block); `web/src/lib/midi.ts`,
`components/MidiStatus.tsx`, `e2e/nx-midi.mjs` (new); `TopBar.tsx` (lane B's block).

---

## 5. Lane C: settings and safety — `wt/nx-c`, branch `next-c`

### C1. A song opens with no sound device — S

**What the user sees.** Q13: the song opens, plays silently, and the status pill says "No sound output"; plug a
device in and the next action uses it.

**The back end.**
- `core/play`: `M2_AUDIO=fail` makes `start_with` fail with today's words (a test seam); `pub fn output_ready() ->
  bool` (a default output exists now: false with `fail`, true with `none`).
- `crates/server/src/graph.rs`, `Inner::device()` (lane C owns it): a start that fails starts `Player::silent()`
  instead, with `Device.fallback: true` and a notice once. The starter and the probe are injectable
  (`Inner.start: Box<dyn Fn(&Choice) -> Result<Player> + Send + Sync>`, `Inner.ready`), real by default; unit tests
  keep `Player::silent()`. `Activity` gains `output: Option<String>` (the device's name) and `no_output: bool`.
- `Graph::look_for_output()`, called every 2 s by the server's `music2-tell` thread: while the player is a fallback
  and `output_ready()`, the device is marked to go; `let_go` (`graph.rs:448`) then says "the sound output is back",
  and the next action loads the song into a real player where it was, stopped, with the toast.
- Web: `Status.tsx` gains the `no-output` state and its popover line, with "Settings…" (C2).

**Tests.** `crates/server/src/graph/output_tests.rs` (new): `a_song_opens_without_an_audio_device` (red first:
today's open fails with "no audio output device"); `the_silent_player_says_so_once`;
`a_fallback_player_plays_on_its_clock`; `a_device_that_comes_back_is_used_from_the_next_action`. Suite
`e2e/nx-nodevice.mjs` (kind `fix`, `env: {M2_AUDIO: "fail"}`, about 8 checks): the song opened; the pill says "No
sound output"; Play moves the playhead; the popover's words; lint. Red first on the `before` server: the open fails.

**Files.** `core/play/src/lib.rs` (C's block), `crates/server/src/graph.rs` (`Inner::device`, `Device`, `Activity`),
`crates/server/src/lib.rs` (the tell thread's call), `graph/output_tests.rs`, `web/src/components/Status.tsx`,
`e2e/nx-nodevice.mjs` (new).

### C2. The Settings window — M

**What the user sees.** ⌘, (Ctrl+, off the Mac), the menu's Settings… (Q18), or "Audio settings…" in the status
pill's Heavy load popover, with a song open or on the start screen. A dialog, "Settings":

| Row | What it does |
|---|---|
| **Sound output** | A list: "System default (MacBook Pro Speakers)" first, then each device; a gone one "(not connected)" |
| **Buffer size** | System default, 64 … 2048 samples, each with its ms; a hint: "Smaller is quicker to respond. Larger is safer if the sound drops out." |
| **Now** | "MacBook Pro Speakers · 48,000 Hz · 512 samples (10.7 ms)" |
| **New songs go in** | The folder, "Change…" (the folder picker), "Use the default" |

Each change applies at once (Q14, Q16); Done or Esc closes; Tab goes round (`useModalKeys`).

**The back end.**
- `core/play/src/outputs.rs` (new): `Output { name, default, rate, frames: Option<(u32, u32)> }`, `outputs()` (the
  default host's output devices, the default first); `Choice { device: Option<String>, frames: Option<u32> }`;
  `Player::start_with(&Choice)`: the device by name, else the default (`Player.fell_back`); `BufferSize::Fixed(n)`
  when given and in range, else the default. `start()` is `start_with(&Choice::default())`. Under `M2_AUDIO=none`:
  `outputs()` lists `M2_AUDIO_OUTPUTS` (a comma list; "none" when unset) and `start_with` names the silent player
  after the choice, at `frames` (else `M2_AUDIO_FRAMES`): the suites' seam. `Player::frames()` from `Shared.frames`.
- `crates/server/src/settings.rs` (new, shaped like `recent.rs`: the file is given, tests pass a temp one, written
  whole through a temp file, a file that doesn't read is the defaults); `AppState::keep_settings(file)` reads and
  applies it; `apps/music2-desktop/src/main.rs` and `crates/server/examples/headless.rs` call it beside
  `keep_recent`.
- `Graph::set_output(choice)`: kept in `Inner.output`, used by `Inner::device()`; the player is let go (as after a
  device change: `retire`, resume at its beat), and the next action starts the new one; a notice "Sound now plays
  through Headphones. Press Play to go on."
- `project.rs`: `songs_dir()` returns the settings' folder when set (a process-wide `RwLock<Option<PathBuf>>` set by
  `settings.rs`); otherwise `m2_os::dirs::songs()` as now.
- Routes (new `crates/server/src/routes_settings.rs`): `GET /api/settings` → `{output: {device, frames}, outputs,
  now: {device, rate, frames}, songs, songs_default}`; `POST /api/settings {output?, songs?}` → the same. Not a
  project write: no session, no undo, allowed with no song and in a read-only window.
- Desktop: `menu.rs`'s `settings` row (`CmdOrCtrl+Comma`; Q18 places it); its id goes to the page.
- Web: `components/SettingsDialog.tsx`, `lib/settings.ts` (new); `App.tsx`: `chord()` maps ⌘, to `settings` (always);
  `case "settings"` sits beside `shortcuts`, before the `graph` check, so it works on the start screen; the ⌥K sheet
  gets "Settings: ⌘," in its File group; `Status.tsx`'s link.

**Tests.**
- `core/play`: `outputs_put_the_default_first`; `a_chosen_device_that_is_gone_falls_back_to_the_default`;
  `a_fixed_buffer_is_used_when_in_range` (the silent seam).
- Server: `settings_are_kept_in_their_file`; `a_bad_settings_file_is_the_defaults`;
  `choosing_an_output_makes_a_new_player_where_it_was`; `new_songs_go_where_settings_say`.
- Desktop: `settings_is_command_comma_in_the_app_menu_on_a_mac_and_in_file_elsewhere`.
- Suite `e2e/nx-settings.mjs` (kind `fix`, `env: {M2_AUDIO_OUTPUTS: "Speakers,Headphones"}`, about 30 checks): ⌘, on
  the Mac page and Ctrl+, on the PC page open it; it opens from the start screen too; the list has the default first;
  Headphones chosen: `GET /api/settings`'s `now.device` and a toast; 256 chosen: `now.frames` 256 and "256 samples
  (5.3 ms)"; the songs folder typed: the New Song dialog's folder shows it; the scratch HOME's
  `.music2/settings.json` has all three; a reload keeps them; Esc closes; Tab order; lint. Red first on
  `dist-before`: ⌘, opens nothing.
- Moves: `start`, `newsong`, `pl-projects`, `export`, `pc-keys`, `taste`.

**Files.** `core/play/src/{outputs.rs, lib.rs}`; `crates/server/src/{settings.rs, routes_settings.rs}` (new),
`project.rs`, `lib.rs`, `routes.rs` (one `.merge`), `graph.rs` (`Inner.output`, `set_output`),
`examples/headless.rs`; `apps/music2-desktop/src/{main.rs, menu.rs}`; `web/src/components/SettingsDialog.tsx`,
`lib/settings.ts` (new), `App.tsx`, `Shortcuts.tsx`, `Status.tsx`; `e2e/nx-settings.mjs` (new).

### C3. Undo says what it undoes — M

**What the user sees.** Undo's tooltip says "Undo Hats pan (⌘Z)" for any step, made in this window or elsewhere,
and after a reload; the Edit menu says "Undo Hats pan" and "Redo …"; undoing a part added or removed says so in a
toast, "Undid adding Reverb" (as now). A step made by an agent reads plainly: "Undo Bass notes".

**The back end.**
- `core/session/src/session.rs`: `pub fn tops(&self) -> (Option<&str>, Option<&str>)`: the ids of the Tx on top of
  undo and redo (each `TxStep.inverse.id`). Read only; Tx, its ops and expectations unchanged
  (`the_write_api_is_step_6s` untouched).
- `crates/server/src/graph/names.rs` (new): `Names { by_id: HashMap<String, Named>, made: VecDeque<(u64, String)> }`
  (600 at most), `Named { name, structural }`.
  - The actor (lane C's block in its job loop): after any job, a new id on top of undo with no name gets a plain one
    from its step (Q19's words, from `Stepped.file` and the revision's change), and `made` records `(revision, id)`.
  - `Graph::undo` and `redo` (lane C's actor block): the top's name is read first and given to the new top of the
    other stack after; the answers carry `name` and `structural`.
  - `POST /api/graph/step {session, revision, name, structural}` names the step the edit at `revision` made (only one
    `made` has).
  - The state: `undo_name`, `redo_name`.
- Web: `history.ts`: `recorded()` also posts the name; `next()` reads `graph.undo_name` and `redo_name`;
  `graphUndo` toasts structural steps by the answer. `TopBar.tsx`'s Undo and Redo tooltips (lane C's block only).
  The page sends `nativeAction("undo_names", JSON.stringify({undo, redo}))` when they change.
- Desktop: `actions.rs` `undo_names` → `menu::set_undo_names(app, undo, redo)`: `set_text` on the `undo` and `redo`
  items ("Undo" alone when none).

**Tests.** `core/session`: `tops_are_the_steps_undo_and_redo_take`. Server `graph/names_tests.rs` (new):
`undo_names_follow_undo_and_redo`; `a_named_step_keeps_its_name_through_undo_redo_undo`;
`a_step_made_elsewhere_gets_a_plain_name` (a Tx through `/api/tx/commit` on Bass's notes → "Bass notes");
`a_refused_undo_keeps_the_names`. Desktop: `the_undo_items_take_a_name`. Suite `e2e/nx-undo.mjs` (kind `fix`, about
16 checks): Hats' fader moved → "Undo Hats level" (the page's own word for it); a Tx posted from outside → "Undo Bass
notes" (red first: "Undo"); ⌘Z → Redo's tooltip names it; Reverb added then undone → the toast "Undid adding
Reverb"; a reload keeps the names (red first: they were lost); lint. Moves: `undo`, `s8-tracks`, `s8-mix`, `taste`.

**Files.** `core/session/src/session.rs`; `crates/server/src/graph/{names.rs, names_tests.rs}` (new), `graph.rs`
(`mod names;`, the state's names), `graph/actor.rs` (lane C's blocks), `routes_settings.rs` (the step route, in
C's own router, so `routes.rs` keeps its one line); `web/src/lib/history.ts`, `TopBar.tsx` (Undo and Redo only),
`lib/native.ts` if a helper is needed; `apps/music2-desktop/src/{actions.rs, menu.rs}`; `e2e/nx-undo.mjs` (new).

---

## 6. Lane D: effects and small fixes — `wt/nx-d`, branch `next-d`

Every edit here is one Tx of today's ops: a `TextEdit` with `Confined`, as add and remove effect are.

### D1. Turn an effect off and on — M

**What the user sees.** Each effect card in the Device view has a `Power` button by its remove button. Off: the card
dims, its title says "Reverb · Off", the sound goes on without the effect at once (a 5 ms crossfade), and the knobs
stay to set. On again: back. One Undo step each: "turning Reverb off". Tooltip: "Turn Reverb off (keeps its
settings)".

**The back end.**
- `core/script/sdk/core.ts` (`parse`, `instance`, `:1246-1316`): `bypass` is an option of any component whose first
  input is audio and whose output is audio (Q25); a `true` there runs the body as always, records
  `record.bypass = true` on its frame, keeps `produced(site, …)` on the effect's own output (so the editor still finds
  the call's audio, and remove and add after it work), and returns the first input. `core/ir`: the frame's
  `bypass: Option<bool>` (written only when true: ID-11, every other IR keeps its bytes).
- `crates/editor/src/view.rs`: an effect node carries `bypassed`.
- `crates/editor/src/effect.rs`: `bypass(s, at, call, on)`: on: `, bypass: true` after the options object's last
  property, or `{ bypass: true }` into empty braces, or a new options object (written by `lens.rs`'s rule, D4e); off:
  the property out, with its comma, and the braces when it was alone. Confined to the call. The planner asserts:
  bypassed, the candidate's graph is exactly `remove_effect`'s (compared as `wraps()` compares); on again, exactly
  the graph with it.
- Server: `POST /api/graph/effect/bypass {session, revision, node, on}` (new `crates/server/src/routes_fx.rs`,
  `graph/fx.rs`).
- Web: `NodeGraph.tsx`'s card header (lane D's); `web/src/lib/effects.ts` (new) for the calls.
- **The engine doesn't change** (Q24): the effect's nodes aren't in `Flat.order`, so nothing runs or is made.

**Tests.**
- `core/script`: `bypass_returns_the_audio_input`; `bypass_keeps_the_frame_and_its_knobs`;
  `bypass_is_refused_on_an_instrument`; every fixture's IR bytes unchanged.
- `crates/editor`: `bypass_writes_bypass_true_into_the_options`; `bypass_into_a_call_with_no_options`;
  `bypass_off_takes_it_out_and_its_braces`; `a_bypassed_effect_runs_nothing` (no node of it in `flat.order`);
  `bypass_then_unbypass_is_the_same_code`; `bypass_refuses_a_call_that_spreads_its_arguments`.
- The renders: 10 of 10.
- Suite `e2e/nx-effects.mjs` (kind `fix`, about 30 checks with D2 and D3): Power on Reverb (added first): the file
  has `bypass: true`, the card dims and says Off, ⌘Z takes it back; on again; lint. Red first on `dist-before`: no
  button. Moves: `effects`, `s6-builders`, `s5-knobs`, `graph`.

**Files.** `core/script/sdk/core.ts`, `core/script` tests, `core/ir/src/lib.rs` (the frame field);
`crates/editor/src/{effect.rs, view.rs}`, `effect_tests.rs`; `crates/server/src/{routes_fx.rs, graph/fx.rs}` (new),
`lib.rs`, `routes.rs` (one `.merge`), `graph.rs` (`mod fx;`); `web/src/components/NodeGraph.tsx`,
`web/src/lib/effects.ts` (new); `e2e/nx-effects.mjs` (new).

### D2. Move an effect before or after another — S

**What the user sees.** The effect card's ⋯ menu: "Move before Reverb", "Move after Delay", greyed with the reason in
the tooltip when it can't (Q26). One Undo step: "moving Delay before Reverb".

**The back end.** `effect::reorder(s, at, call, earlier)`: with the effect the call wraps (earlier) or the one
wrapping it (later), `outer(inner(x, I), O)` → `inner(outer(x, O), I)`: two `TextEdit`s, the callee heads and the
tails swapped, each effect's own text kept. Confined to the outer call. The planner asserts: with both effects taken
out, the candidate's graph and the baseline's are the same, and the two are in the other order on that path. Route
`POST /api/graph/effect/move {session, revision, node, earlier}`. The card's menu in `NodeGraph.tsx`.

**Tests.** `crates/editor`: `two_nested_effects_swap`; `the_options_go_with_their_effect`;
`an_effect_not_next_to_another_is_refused_in_words`; `moving_twice_is_the_same_code`. `nx-effects`: Delay added
after Reverb, "Move before Reverb": the code reads `reverb(delay(…), …)` wrapped the other way; ⌘Z; the greyed row's
words.

### D3. Pick what Duck listens to — S

**What the user sees.** On a Duck card, the side input's row: "Ducks to: Kick ▾"; the list has the song's other
tracks; one picked is written, one Undo step: "Bass's duck listens to Hats" (Q27).

**The back end.** `effect::sidechain(s, at, call, track)`: the duck call's `side` argument's text replaced with
`<handle>.env` (the track's first control export, from the IR module's exports) or `follow(<handle>.out)` (importing
`follow` from `std/dsp` when the file doesn't, `sites::import_insertion`); `<handle>` is the name the file imports the
track by (`sites::imported_names`); refused in words when it doesn't import it. Confined to the argument and the
import line. The planner asserts: the candidate's graph is the baseline's with the duck's side input reading the new
source. Route `POST /api/graph/effect/sidechain {session, revision, node, track}`. The row in `NodeGraph.tsx`.

**Tests.** `crates/editor`: `the_picker_writes_the_tracks_env`; `a_track_with_no_env_gets_follow_of_its_out_and_the_
import`; `a_duck_in_a_track_file_is_refused_in_words`. `nx-effects` (on the starter template, whose bass ducks to
`kick.env`): pick Clap: `follow(clap.out)` and the import written; ⌘Z puts both back.

### D4. Small fixes — M

| # | Fix (Q) | Where | Test (red first) |
|---|---|---|---|
| a | Markers and bar numbers both show; a change mark never touches its bar's number (Q28) | `lib/layout.ts` (`RULER_H`), `SongTimeline.tsx` (`Ruler`: numbers, marks), `Markers.tsx` (the band) | `nx-nits`: a marker "Chorus" at bar 5 and bar 5's number: their boxes don't overlap, and `elementFromPoint` at the number's centre is the number (red: the marker's flag); a tempo change at bar 9: "9" and "♩140" at least 4 px apart (red: 0) |
| b | The last bar number scrolls clear of Follow and zoom (Q29) | `SongTimeline.tsx` (the scroll width, `ViewTools`) | `nx-nits`: scrolled to the end, the last number's box ends left of the tools' box (red: under them) |
| c | A drum lane's menu and keys (Q30) | `NoteMenu.tsx`, `NoteView.tsx` (`Keys`, the lane's row fill only) | `nx-nits`: a Drum kit's note menu has "Up a row" and no octave rows (red: "Up a semitone"); row 61's key fill equals row 60's (red: a black key) |
| d | (in a) | | |
| e | A first options object on its own line (Q31) | `crates/editor/src/lens.rs` (`Insertion::plan`, the new options object arm, and the empty call when written over lines) | `crates/editor`: `a_first_options_object_goes_on_its_own_line` (red: `}), { db: -2.5 },`); `mix_tests.rs:368-369` follows; `s8-mix` reads the code it writes |
| f | Shift+wheel over a fader scrolls the strips (Q32) | `Mix.tsx` (the fader's `turn`, `:651`) | `nx-nits` at 1100 px: the PC page's Shift+wheel (deltaY) over Hats' fader leaves its value and moves the strips' `scrollLeft` (red: the fader moves); the Mac page's (deltaX) the same |
| g | `s8-mute` uses `lintlib.mjs` (Q33) | `e2e/s8-mute.mjs`, its row in `run.mjs` and the README | `s8-mute` passes; red first: a state its old copy passes and lintlib flags, as PL-T4 did |

Suite `e2e/nx-nits.mjs` (kind `fix`, about 25 checks) with a, b, c and f, and lint of each state. Moves: `pl-zoom`,
`pl-arrange`, `s8-clips`, `s8-automation`, `s8-songbar`, `pl-notes`, `pl-mix`, `s8-mix`, `s8-mute`, `taste`,
`fx-fixes` (the dock's room follows `RULER_H`).

**Files (D4).** `web/src/lib/layout.ts`, `SongTimeline.tsx` (`Ruler`, `ViewTools`, the width: lane D's regions),
`Markers.tsx`, `NoteMenu.tsx`, `NoteView.tsx` (`Keys` only), `Mix.tsx`; `crates/editor/src/{lens.rs, mix_tests.rs}`;
`e2e/s8-mute.mjs`, `e2e/nx-nits.mjs` (new).

---

## 7. File ownership

### 7.1 Who edits what

| File | Owner | Others |
|---|---|---|
| `crates/server/src/{routes_leases.rs, graph/lanes.rs, graph/tempo_lease.rs}` and their tests, `crates/editor/src/clips.rs`, `core/song/src/data.rs`, `web/src/lib/{laneLease.ts, tempoLease.ts}`, `SongBar.tsx`, `e2e/s8-leases.mjs` | A | — |
| `core/runtime/src/{audio.rs, plan.rs}`, `core/runtime/src/tests/live/lease.rs` | A | — (8a: §1.9) |
| `core/os/**`, `Cargo.toml`, `Cargo.lock`, `core/runtime/src/{click.rs, engine.rs, tests/click.rs, tests.rs}` | B | 8a: §1.9 |
| `core/play/tests/no_alloc.rs` | B (`the_click_never_allocates`), A (`the_lease_offset_never_allocates`) | each appends one test at the file's end; the integrator joins |
| `crates/server/src/{graph/midi.rs, graph/audition.rs, graph/view_file.rs, routes_8b.rs}` and their tests | B | — |
| `web/src/lib/{playTarget.ts, piano.ts, midi.ts, metronome.ts, audition.ts}`, `components/{KeyboardPiano.tsx, MidiStatus.tsx}`, `e2e/nx-{piano,midi,click}.mjs` | B | — |
| `core/play/src/outputs.rs`, `core/session/**` | C | — |
| `crates/server/src/{settings.rs, routes_settings.rs, project.rs, graph/names.rs, examples/headless.rs}` and their tests, `graph/output_tests.rs` | C | — |
| `apps/music2-desktop/src/{main.rs, actions.rs}` | C | — |
| `web/src/components/{SettingsDialog.tsx, Status.tsx}`, `web/src/lib/{settings.ts, history.ts}`, `e2e/nx-{settings,nodevice,undo}.mjs` | C | — |
| `core/script/**`, `core/ir/**`, `crates/editor/src/{effect.rs, view.rs, lens.rs, mix_tests.rs, effect_tests.rs}`, `crates/server/src/{routes_fx.rs, graph/fx.rs}` | D | — |
| `web/src/components/{NodeGraph.tsx, Markers.tsx, NoteMenu.tsx, Mix.tsx}`, `web/src/lib/{effects.ts, layout.ts}`, `e2e/{s8-mute,nx-effects,nx-nits}.mjs` | D | — |
| `core/runtime/src/lib.rs` | A (A4's lines), B (`pub mod click;`) | separate lines |
| `core/play/src/lib.rs` | B (the click), C (`start_with`, the seams, `frames`) | separate hunks: B in `Callback::render`, C in `fill_in`, `start` and `silent` |
| `crates/gates/**`, `apps/m2/**`, `core/plugins/**` | nobody | — |

### 7.2 The shared files, and the rule for each

| File | Rule |
|---|---|
| `web/src/App.tsx` | B: `k: "metronome"` in `key()`'s map, the ⌘K line in `chord()`, `case "metronome"` and `case "piano"` at the switch's end. C: the ⌘, line in `chord()` (after B's place: the last line before `return null;`), `case "settings"` beside `shortcuts`, before the `graph` check. A and D: none. The integrator joins neighbouring lines |
| `web/src/components/TopBar.tsx` | B: the metronome and piano toggles after Loop, `<MidiStatus />` before `<Status />`. C: the Undo and Redo tooltips only. Nobody else |
| `web/src/components/Shortcuts.tsx` (the ⌥K sheet) | Rows land with their features, at the end of their group: B in Transport (Metronome) and View (Keyboard piano), plus a new last group "Keyboard piano"; C in File (Settings). A and D have no keys |
| `web/src/components/SongTimeline.tsx` | A: the drag code only (`edits`, `drop`, `sendPoints`, the clip and point drag handlers). D: `Ruler`, `ViewTools`, `TOOLS_W` and the scroll width only. Neither edits the other's functions |
| `web/src/components/NoteView.tsx` | A: the drag, paint and `send`/`commit` path. D: `Keys` and the named lane's row fill. Nothing else |
| `web/src/lib/graph.ts` | Nobody. A new type goes in the lane's own `lib/` file by `declare module "./graph"`, as `graph8b.ts` does |
| `apps/music2-desktop/src/menu.rs` | B: `ITEMS` rows `metronome` and `piano` after `panic`; Transport ▸ Metronome after Follow Playhead; View ▸ Keyboard Piano after Show Browser; one test. C: `ITEMS` `settings` after `shortcuts`; the Mac app menu's row after About and Linux File's above Quit's line; `set_undo_names`; two tests. Both append; the integrator joins |
| `crates/server/src/lib.rs` | Each lane's `mod` lines after `mod routes_view;`; B's MIDI block in `start()` and its `AppState` field; C's `keep_settings`, its field and the tell thread's call. Fields at the struct's end |
| `crates/server/src/routes.rs` | One `.merge(…)` line each after `routes_view`'s (A, C, D). Nothing else |
| `crates/server/src/graph.rs` | New fields at the end of their struct (`EngineView`, `ControlPlane`, `Device`, `Activity`, `Inner`), `mod` lines at the end of the list, new functions in the lane's own `graph/<x>.rs` as `impl Graph` blocks. C owns `Inner::device()`. 8a changes one function here: before editing near a function, read `git diff main...8a -- crates/server/src/graph.rs` and stay out of it |
| `crates/server/src/graph/actor.rs` | A: the publish of `accepted`, the two settle blocks, `Plays.tempo`, `way()`, the tempo job. B: one block in `Open::load`. C: the names block in the job loop, and undo and redo. Each its own hunk, none inside another's |
| `e2e/run.mjs`, `e2e/README.md`'s table | P0: `env` and `M2_MIDI`. Then rows after anchors: A `s8-leases` after `s8-automation`; B `nx-piano`, `nx-midi` after `s8-audition`, `nx-click` after `s8-songbar`; C `nx-undo` after `undo`, `nx-settings`, `nx-nodevice` after `pl-projects`; D `nx-effects` after `effects`, `nx-nits` after `pl-arrange`. A lane that changes another suite's count edits only that row |
| `e2e/lintlib.mjs`, `e2e/taste.mjs` | Nobody |
| `docs/designs/step8/build-notes-next.md` | Each part under its own heading |
| `docs/gaps.md`, `README.md`, `docs/architecture.md`, `taste.md`, `docs/designs/platform/design.md`, `HANDOFF.md`, `e2e/README.md`'s prose | **The End only**, from each slice's For gaps.md and Contract lines |
| `Cargo.lock` | Lane B only (B3). At a merge of `main`: take `main`'s, then a plain `cargo build` adds `midir`'s entries back; never `cargo update` (HANDOFF's merge notes) |

---

## 8. Order, by day *(estimates)*

| When | Lead | Lane A | Lane B | Lane C | Lane D |
|---|---|---|---|---|---|
| Day 1, first hour | P0 (§2) | reads; `lanes_tests.rs` red | reads; `tests/click.rs` red | reads; `output_tests.rs` red | reads; the bypass tests red |
| Day 1, rest | merges `main` into `next` if it moved | A1 | B1 | C1, then C2's `core/play` and server | D1 |
| Day 1, evening | | A2 | B2 | C2's page, menu and suite | D2, D3 |
| Day 2, morning | | A3, then A4 (or Q23's fallback) | B3 | C3 | D4 |
| Day 2, midday | merges each lane into `next` as it finishes, smallest first, `rerere` on; the merge checks (§9 step 1) | done | done | done | done |
| Day 2, afternoon | The End (§9) | | | | |

If a lane is late at Day 2 midday, its unfinished slice waits and the End lists it in `docs/gaps.md`. Each lane
deletes its `target-<lane>` and `bin/<lane>-*` when it is done.

---

## 9. The End (lead)

1. **Merge** each lane into `next` (merge commits, `rerere`), then `main` if it moved. After all four: the web build;
   every browser suite (`lane-suites.sh nx <out> all`, with `CLAP_PATH` for `live3b`); `cargo test --locked
   --no-fail-fast` and clippy on every crate a lane touched (`m2-os`, `m2-runtime`, `m2-play`, `m2-song`,
   `m2-session`, `m2-script`, `m2-ir`, `m2-editor`, `m2-server`, `music2-desktop`); the mac lint; the renders.
2. **Review** with a skeptic, two lenses (how it behaves; tests, docs and taste), as polish's End did. Each finding: a
   fix with a check that fails before it, its own commit.
3. **Docs** (one commit):
   - `docs/gaps.md`: #13 done; #19 done (count-in left); #21 bypass, reorder and the picker done (presets, Pump
     left); #24 playing live done (recording, count-in, quantize left); #29 names done (kept after close left); #31
     done (input, view settings for every song left); the loose ends "A dragged or nudged clip is heard only once it
     lands" (now heard while playing; a nudge still on landing) and "Undo steps have no names on the server" (gone);
     What works today gains the new rows.
   - `README.md`: what works.
   - `docs/architecture.md`: WRITE-22a and RENDER-27a *Today*, with design-8b §9.7's edits (and Q22, Q23 as built);
     the planes table's control row (lane and tempo leases); RENDER-29 *Today*; MON-7 (a device source, never let go
     by the watchdog); RENDER-28 (the click: plain fields, no allocation); RENDER-1 and RENDER-3 (nothing of the
     click or a live note in an export); FMT-20 (`view.json` `metronome`); the homes table (`~/.music2/settings.json`);
     WRITE-21 (names by step, `Session::tops`); PLAT-2's *Today* (`m2_os::midi`); PKG-3 or the component section
     (`bypass`).
   - `taste.md`: §3.3 (the metronome exists; the keyboard piano toggle; the MIDI icon), §3.2 (the ruler's 36 px and its
     rows), the piano card, the Settings dialog.
   - `docs/designs/platform/design.md` §2.4 and §8 (Q1).
   - `e2e/README.md`'s rows; `HANDOFF.md` (below).
4. **The full check once:** `TW_WAIT=3600 ~/m2-runs/tw check-next 'cd /home/jerry/m2-runs/wt/nx &&
   CARGO_TARGET_DIR=/home/jerry/m2-runs/s8n/target-nx M2_CHECK_OUT=$HOME/m2-runs/check-next ci/check.sh'`. Renders
   must match `e2e/renders.sha256`.
5. **The Linux app on `:97`** (`app-shot.sh`; never a real screen): Ctrl+, opens Settings; File ▸ Settings… is there;
   Ctrl+K shows the piano card; Transport ▸ Metronome; Edit ▸ Undo with a name after an edit.
6. **Pictures**, in `~/m2-runs/app-shots/next-<sha>/` with `captions.txt` (each one plain line, and "This is the
   Linux version. On your Mac it uses the Mac's own font and ⌘ keys."): the top bar with the metronome on and the
   MIDI icon (`app-shot.sh` passes `M2_MIDI=inject`, and a `HOOK` posts a keyboard to the app's
   `/api/graph/midi/inject` with the URL and token in its `session.json`); the keyboard piano card; the Settings
   dialog; an effect turned off in the Device view; the Duck picker; the ruler with a marker and a tempo change at
   bar lines; a drum lane's menu.
7. **HANDOFF**: a "next" row; the Mac by-ear steps: plug a MIDI keyboard in while the app plays, play, use the pedal,
   unplug it; the metronome while looping; drag a note and an automation point while playing; drag the tempo while
   `examples/resample` plays (the audio drifts, then snaps back); Settings with real devices (headphones, a buffer of
   128); a song opened with the output unplugged (an audio interface).
8. **Clean up:** each lane's target and bins (the lanes do it), then `target-nx` and `bin/`; the lane worktrees
   (`git worktree remove`), keeping their branches.

---

## 10. Cut, and left for later

| What | Why |
|---|---|
| `m2-gates lane-drag`, `tempo-drag`, and G13 for a device (Q34) | `crates/gates` is 8a's this round |
| Tempo prepares on a control-plane thread (Q22) | The actor's coalescing job fits the time; a tempo dragged during a long compile sounds after it |
| Audio drift during a tempo drag, if A4 runs late (Q23) | `core/runtime` work beside 8a; the fallback hears audio songs' tempo on release |
| Audio clip drags heard while dragging | They need renders (design-8b §9.1) |
| Recording MIDI, a count-in, quantize on input (gaps #24's rest) | Recording is its own step |
| A MIDI input picker, pitch bend, mod wheel, aftertouch, program change, MIDI clock, notes placed inside a block (`midi_instant`) | Not in the brief; each needs a live control path or the platform's §8 `time` module |
| The piano's velocity and sustain keys | Kept small; a fixed 0.8 as the roll's keys |
| The metronome's level, sound and a cue output | Later, in Settings |
| Undo kept after the app closes (gaps #29's second half) | The brief leaves it |
| Effect presets, the old Pump (gaps #21's rest) | Not in the brief |
| Reordering effects not written one inside the other, or across files | A structural rewrite; refused in words |
| `m2` commands for bypass, reorder, the sidechain, the metronome (Q35) | `apps/m2` stays out (gaps #37) |
| Settings: input device, view settings for every song, theme | Not in the brief; the sample rate stays the song's (RENDER-31) |

---

## 11. Not checked

- midir 0.11.0's exact API (the `connect` callback's arguments, `ports()`, the error types): B3 reads its source once
  cargo has fetched it.
- Whether CoreMIDI's port list refreshes for a polling client with AppKit's run loop running (hot-plug): the Mac run.
- What `Cargo.lock` gains for `midir` exactly (Mac and Android entries come with it): B3 writes it down.
- Whether the mac lint builds `coremidi-sys` with the stub C compiler: B3 runs it before committing.
- cpal 0.18's names for a device's buffer range (`SupportedBufferSize::Range`) and what it lists on this server (no
  sound card): C2 reads them; the suites use the silent seam.
- `CmdOrCtrl+Comma`: muda 0.20.0's parser takes "COMMA" and "," (`src/accelerator/mod.rs:286`); the menu test
  settles it. Whether Tauri's `set_text` on GTK works while a menu is open: C3 checks on `:97`.
- Whether WebKitGTK sends Shift+wheel as deltaY with Shift or as deltaX (D4f handles both).
- Whether a call whose output nothing reads draws a check warning today (D1 reads `core/ir/src/check.rs` first).
- Where 8a's live half moves `Engine::render` and `Callback::render` (B1's two lines, C's `fill_in` line, A4's
  blocks): the merge of `main` says.
- The tempo prepare's cost at p50 (design-8b §9.5): measured in A3, information only.

---

## Attack

Read against the code at `70f9b61` (`wt/nx`) and against 8a's branch at `33885a6` (through `git show` and `git diff
main...8a` only; its worktree untouched). Each finding has an id; **its decision has the same id under Decisions**.

### Clashes with 8a

| # | What is wrong | Evidence |
|---|---|---|
| X1 | C2's `Player::start_with(&Choice)` has the name, and half the job, of 8a slice 2's `Player::start_with(Options)`. 8a's `Options::from_env()` also redefines `M2_AUDIO_FRAMES` (device and silent player) and sets `BufferSize::Fixed`. Two functions of one name with two signatures: a merge conflict, then a compile error | `git show 8a:docs/designs/step8/design-8a.md` lines 596-606 and 1012 |
| X2 | 8a slice 2 changes `retire()` (`graph.rs:462`) to call `Player::close()`. C1 plans to edit `let_go` (`graph.rs:448`), the function just above it | design-8a.md line 1018; `graph.rs:448`, `:462` |
| X3 | A4 edits the Renderer's struct end, its constructor's end and the lane pre-pass. 8a already moved all three: `whole: Option<Box<Schedule>>` at the struct's end, `render_block_on` and `pre()`, `Plan`'s fields in `plan.rs`, `mod keep;` in `tests/live.rs`. And lane A is four M slices (2 days) in a lane that has 1.5 | `git diff main...8a -- core/runtime/src/lib.rs` (hunks at old 1663, 1922, 2011-2036); plan §8 |
| X4 | `mod click;` in `core/runtime/src/tests.rs` goes on the exact line where 8a inserted `mod fpenv;` (after `mod cell;`): a conflict | `git diff main...8a -- core/runtime/src/tests.rs` |
| X5 | `core/play/tests/no_alloc.rs` is a third lane's file too: 8a slice 2 adds "`no_alloc.rs` on workers". Three appends at one end | design-8a.md line 1022 |
| X6 | `Shared` (`core/play/src/lib.rs:220`): B's `click` and C's `frames` both go "at the end": a conflict between B and C | §1.9 |

### Wrong or missing against the code

| # | What is wrong | Evidence |
|---|---|---|
| X7 | Undo names keyed by Tx id can't work. A save's inverse always has the id `"undo-save"`, so two saves on the stack share one id; and an undo's inverse gets a new id each time | `core/session/src/take.rs:229`; `core/session/src/tx/mod.rs:683` (`inverse_id`) |
| X8 | The desktop menu is built again whenever `recent.json` changes, which drops any `set_text` name. And the page's `undo_names` native action is a second copy of what the server knows | `apps/music2-desktop/src/main.rs:208` |
| X9 | B1 adds `"metronome"` to `view_file::KEYS` **and** a new route. Then `POST /api/graph/view {metronome}` would write `view.json` and the state's `local` without telling the player | `crates/server/src/graph/view_file.rs:24`, `:94` (`set_view`) |
| X10 | D1's bypass, three gaps. (a) The code editor type-checks songs with the SDK's types: `{ size: 0.8, bypass: true }` is an "unknown property" error in Code, and a call type with no options form (`Call<D>`) rejects any options object. (b) `parse()` reads an options object only when the component has a control or a setting. (c) The Device view lays cards out by `port_edges`: with the effect's output unread, its card leaves the chain and drops below it as a branch, so the card jumps when turned off. (d) A CLAP plugin may have its own `bypass` parameter | `core/script/sdk/core.ts:817` (`CallOptions`), `:855` (`Call`), `:1325` (`hasOptions`); `web/src/lib/codeMonaco.ts:120`; `web/src/components/NodeGraph.tsx:343` (`layout`) |
| X11 | Taste puts Bypass and "Move left and right" in the effect card's ⋯ menu. That menu doesn't exist: the header has only Remove and Add. The plan adds a Power button instead, and D2 assumes the ⋯ menu is there | `taste.md:815`; `NodeGraph.tsx:768-800` |
| X12 | `RULER_H` is the roll's ruler too, and the dock's limit. 36 would grow the roll's ruler, which has no markers | `web/src/components/NoteView.tsx:200`, `:774`, `:775`; `web/src/lib/layout.ts:25` |
| X13 | The keyboard piano's top-bar toggle isn't in taste §3.3, which lists every element of the bar. The metronome already takes 32 px. Taste keeps ⌘K "for musical typing, as in Logic", which has no toolbar button for it | `taste.md:718-741`, `:1216` |
| X14 | Taste's Transport menu order is "Loop, Loop Selection, Metronome and Follow". The plan puts Metronome after Follow, which dodges an existing assertion | `taste.md:1266`; `apps/music2-desktop/src/menu.rs:501` |
| X15 | ⌘K is a native accelerator, which fires while typing. In the app it never reaches Monaco, so Monaco's ⌘K chords stop working in Code. `editorHooks.chord?.("piano")` can't help: Monaco has no single action for ⌘K. In a browser the same key would go to Monaco instead: two behaviours | `menu.rs:6-10`; `web/src/components/CodeView.tsx:27` (`MONACO_CHORDS`), `:287` |
| X16 | Stuck piano notes. A Mac sends no keyup for a letter let go while ⌘ is held. And nothing stops the piano while a dialog (⌥K sheet, New Song, Settings) is open | — |
| X17 | The roll's writes go through `graph.ts`'s `lanes` queue (`writeLane`). A2's lease commit from NoteView would skip it, race a roll write still on its way, and come back 409. `graph.ts` belongs to nobody | `web/src/lib/graph.ts:774-801` |
| X18 | A, C and D each add a `mod` line in `lib.rs` and a `.merge` line in `routes.rs` at the same spot; A, B, C and D each add `mod` lines at the end of `graph.rs`'s list. Four lanes, one place | `crates/server/src/routes.rs:92-96`; `crates/server/src/lib.rs:13-17` |
| X19 | nx-nodevice's red-first run would start the **before** server with `M2_AUDIO=fail`. That server knows only `none`, so it opens a real ALSA device on this machine | `core/play/src/lib.rs:776-782`; the track's rule: every music2 run has `M2_AUDIO=none` |
| X20 | A process-wide songs folder set by a unit test leaks into `project.rs`'s tests, which read `songs_dir()` in the same test binary, in parallel | `crates/server/src/project.rs:878`, `:1109` |
| X21 | Settings toasts "Sound now plays through Headphones". A direct edit is not a toast (ST-5), and the toast would sit under the modal | `taste.md:1081` |
| X22 | Choosing an output during an export retires the player the export holds | `crates/server/src/export.rs:248` (`Lent`) |
| X23 | C1: a device that is listed but won't open makes `output_ready()` true for ever. Every action tries it, fails and says so | — |
| X24 | A3's `one_tempo_prepare_in_flight` ("50 updates: at most 50 activations, none coalesced") can't fail. `Plays.tempo: Option<f64>` can't hold a song with several tempo entries. And nothing refuses a tempo commit after another window changed the tempo | plan A3 |
| X25 | Several "red first" tests can't be red: exports never had the click or a live note, and the stopped drag never leased. And nx-click never listens for the click | plan B1, B3, A1, A2 |
| X26 | Nearly every crate depends on `m2-os` (runtime, song, script, session, editor, plugins, play, gates, both apps). With midir a plain dependency there, all of them build midir, alsa-sys and coremidi | `grep -l m2-os */*/Cargo.toml` |
| X27 | midir isn't in `~/.cargo/registry`. If crates.io can't be reached, B3 stalls on Day 2 morning, the worst time | `ls ~/.cargo/registry/cache/*/` |
| X28 | "Ducks to: Kick" is jargon for a beginner. And "the track's first control export" may be an LFO, not a level | `templates/starter/tracks/kick.ts:8` |
| X29 | D1 writes a new options object by D4e's new rule, but D4e is scheduled after it. With X10 and X11, lane D grows past 1.5 days | plan §8 |
| X30 | The plan doesn't say where the piano card and the Settings dialog mount. The timeline (`SongTimeline.tsx`) already has two lanes in it | — |
| X31 | "A held key lights": in what? Taste has one accent | `taste.md` §2.5; `NoteView.tsx:1008` (the roll's keys light in the track's colour) |
| X32 | The piano's 20 keys as 20 rows would flood the ⌥K sheet | `web/src/components/Shortcuts.tsx` |
| X33 | "In `WRITES`": that list is in `routes.rs`, not in A's or D's file | `routes_view.rs:23` (`WRITES_VIEW`) shows the pattern |
| X34 | Esc is the page's `deselect`, so Esc during a lane drag would also deselect | `web/src/App.tsx:52` |
| X35 | Suites that start their own server don't get `envFor`'s `M2_MIDI`. Harmless here (EACCES); on the Mac a plugged keyboard would play into them | `e2e/s8-audition.mjs:292`, `e2e/live3b.mjs:267`, `e2e/pl-projects.mjs` |
| X36 | "Metronome (K)" doesn't tell a beginner what it does | — |
| X37 | The MIDI target when the window that set it closes: not said | Q4 |
| X38 | `song_folder` is titled "Choose Where to Save the Song", wrong for Settings. And a typed songs folder isn't checked | `apps/music2-desktop/src/actions.rs:67-72` |

Checked and fine: exports and `m2 render` never make an `Engine` (`crates/editor/src/render.rs`, `export.rs`), so the
click can't reach one. A node outside `Flat.order` is never instantiated (`core/runtime/src/lib.rs:907-916`), so a
bypassed effect costs nothing. `follow`'s input is a sidechain input (`std/src/dsp.ts:81`), so a picked track's mute
never stops the ducking. No std component, template or example uses a `bypass` option today. The piano's letters
clash only with A, E, S, X, Z (and the new K): the roll's and the timeline's own keys are Q, arrows, ⌫, Esc and ⌘
chords. 8a doesn't touch `core/os/src/lib.rs` or `core/ir`.

---

## Decisions

These override everything above them, the first Decisions table (Q1 to Q36) included, where they differ.

### By finding

| # | Build with |
|---|---|
| X1 | C names it **`Player::start_on(&Choice)`**; `start()` calls `start_on(&Choice::default())`. C never reads or redefines `M2_AUDIO_FRAMES`; the silent seam takes `frames` from `Choice.frames` only. **For 8a's merge**: "`Choice` folds into 8a's `Options` (an output name beside `frames`); `M2_AUDIO=fail` and `M2_AUDIO_OUTPUTS` go into `Options::from_env()`" |
| X2 | C never edits `let_go` or `retire`. The fallback's checks live in `Inner::device()` only |
| X3 | **A4 isn't built.** Q23's fallback is the build, inside A3: while `plays.info.lanes` has an audio lane, the tempo lease sends no previews, and the release commits as today. Test `a_song_with_audio_hears_the_tempo_on_release`. Lane A then touches nothing in `core/runtime` or `core/play`. Left (gaps): "audio drifts during a tempo drag", to build on top of 8a's live slice |
| X4 | `core/runtime/src/click.rs` ends with `#[cfg(test)] #[path = "tests/click.rs"] mod tests;`. `tests.rs` isn't touched |
| X5 | B's test goes in a new `core/play/tests/no_alloc_click.rs`: `mod common;`, `#[global_allocator] static TRAP: alloc::Counting`, its own `trapped()`. `no_alloc.rs` isn't touched |
| X6 | B's `click: AtomicBool` goes after `running`; C's `frames: AtomicU32` after `last_callback_ns` |
| X7 | **Names live on the undo step, in `core/session`.** `TxStep` gains `made: u64` (the revision it made) and `name: Option<(String, bool)>` (the words, structural). `Session::name_step(made, name, structural) -> bool` names the step that revision made, at any depth. `Session::step_names()` gives undo's and redo's tops as `{name, structural, file}`. `step()` carries the name to the step it pushes on the other stack. No `graph/names.rs` map, no `made` list, no actor job-loop block. The server's plain names come from `file` when `name` is none. Tx, ops, expectations and `the_write_api_is_step_6s` don't change. Extra test: `two_saves_keep_their_own_names` |
| X8 | The desktop's title loop (`keep_title_in_sync`, every 300 ms) reads the names from `server.graph` and sets the Undo and Redo items. `menu::build` titles them from the last names (kept in a static), so a rebuild keeps them. No `undo_names` action. Left: while the code editor has focus, Edit ▸ Undo undoes typing though its title names the song's step |
| X9 | **No metronome route.** `KEYS` gains `"metronome"` (`[&str; 7]`). `set_view` gives the player `set_click` when `metronome` is among the changes. The page sets it with `setLocal("metronome", …)`, as the audition toggle does. A new player (a device change) gets it from the control plane's `local`, not the file, so a read-only window keeps it |
| X10 | SDK: an audio-to-audio component's `CallOptions` gains `bypass?: boolean`, and its call type always has the options form. `parse()` takes an options object for such a component even with no controls. `core/script/sdk/check/calls.ts` gains the cases. A component that declares its own `bypass` control or setting keeps it, and Turn off is refused: "It has its own Bypass knob." `view.rs`: for a bypassed call, the edge from its input into the call that reads it starts at the bypassed card instead (`bypassed: true`; drawn dashed in `line-strong`), so the card keeps its place. Tests: `a_bypassed_effect_keeps_its_place_in_the_chain` (`crates/editor`). nx-effects: after Turn off, the Code view has no marker, and the card's box doesn't move |
| X11 | **The effect card gets taste's ⋯ menu** (`MoreHorizontal`, shown on hover and focus; `Popover` and `MenuRow`): "Turn off" or "Turn on", "Move left", "Move right". A row that can't run is greyed, with Q26's words in its tooltip. **No Power button.** An off card: its body at 50% opacity, its title "Reverb · Off" in `fg-muted`. Undo names: "turning Reverb off", "moving Delay before Reverb" |
| X12 | `RULER_H = 36` stays the timeline's. A new `ROLL_RULER_H = 28` serves NoteView's three uses (D's lines; A adds its own imports after NoteView's last import). `--spacing-ruler` becomes 36. `dockMax` follows (fx-fixes moves) |
| X13 | **No piano toggle in the top bar.** A 28 px `Keyboard` toggle sits in the Notes view's tools, in `AuditionTools.tsx` (B's), beside "Hear notes". Also ⌘K, View ▸ Keyboard Piano and the ⌥K sheet. The card has a × ("Keyboard piano off (⌘K)") |
| X14 | Taste's order: Loop, Loop Selection, Metronome, Follow Playhead. `menu.rs:501`'s assertion becomes Metronome then Follow, in B's commit |
| X15 | ⌘K **always** toggles the piano, from the code editor and from fields too, in the app and in a browser: `chord()` returns `{ id: "piano", always: true }`. No `editorHooks.chord` step. Monaco's ⌘K chords are given up (taste keeps ⌘K for this). The piano never plays while typing anyway |
| X16 | A Meta keydown or keyup lets every piano note go. The piano doesn't play while a modal (`[aria-modal]`) is open. Shift doesn't matter (Shift+letter has no shortcut today, `App.tsx:53`). nx-piano adds: A down, Meta down, A up, Meta up → `live` 0 |
| X17 | P0 (the lead) adds to `graph.ts`: `export function afterLaneWrites<T>(graph, f: () => Promise<T>): Promise<T>`, chained on `lanes.queue` and counted in `pending`. A's `finish` in NoteView runs through it |
| X18 | **P0 makes every new module once**, as a stub, with its `mod` and `.merge` lines: `routes_leases.rs`, `routes_fx.rs`, `routes_settings.rs` (an empty `router()` each), `settings.rs`, and `graph/{lanes,tempo_lease,midi,fx}.rs`. Each lane then owns its stubs and fills them. (X7 removes `graph/names.rs`) |
| X19 | No server older than C1 ever runs with `M2_AUDIO=fail`. nx-nodevice's red first is the new server with `DIST=dist-before` (the old page shows no "No sound output"), plus `a_song_opens_without_an_audio_device`'s red with a stub |
| X20 | `new_songs_go_where_settings_say` is an integration test, `crates/server/tests/songs_folder.rs`, in its own process. No unit test sets the songs folder |
| X21 | No toast after a Settings change. The dialog's Now row changes, and when play stopped, a line under it says "Playback stopped. Press Play to go on." nx-settings checks the Now row. Toasts stay for an output that comes back by itself (C1) and for a MIDI keyboard plugged in or out |
| X22 | While an export runs, the output and buffer rows are greyed: "Wait for the export to finish." |
| X23 | After a real start fails, C1 waits until the list of devices changes, or 30 s, before it tries again. "Sound output is back" only after a real start worked |
| X24 | The test becomes: with a prepare hook that sleeps 50 ms, 50 updates over 100 ms make at most 4 activations, and the last carries the 50th value. `Plays.tempo` is `Option<Arc<Arrangement>>` (the leased one). The commit answers 409 when `arrangement.json` changed since begin: "The tempo changed elsewhere: your drag was dropped." |
| X25 | Those tests stay, named **guards** in the build notes, not red first. nx-click adds a real one: on a capture server of its own (`M2_AUDIO_CAPTURE`, as `s8-audition` does; with `M2_MIDI: "none"`), metronome on and playing: energy at the beats' frames; off: none (red first: no click) |
| X26 | `m2-os` gets a feature: `midi = ["dep:midir"]`, `midir = { workspace = true, optional = true }`. Only `m2-server` turns it on. The root line `midir = "=0.11.0"` goes after `cpal = "0.18"`, away from 8a's `m2-pool` line. At a merge of `main`: take `main`'s `Cargo.lock`, then `cargo build --offline` adds midir back from the local cache |
| X27 | **B adds midir first thing on Day 1, before B1.** If crates.io can't be reached, B3 ships the server side, the inject route, the page and its suite on a no-device `m2_os::midi`, and the real listener is left |
| X28 | The row reads "**Listens to:** Kick ▾". It writes `<handle>.env` only for an export named `env`; any other track gets `follow(<handle>.out)` |
| X29 | Lane D's order: D4e, D1, D2, the rest of D4, then D3. D3 is the first thing cut if the lane runs late |
| X30 | `<KeyboardPiano />` mounts in `App.tsx` after `<Toasts />` (B). It portals to the body and places itself from `[data-testid=timeline]`'s box (a `ResizeObserver`). `<SettingsDialog />` mounts after `<Shortcuts />` (C). Nobody edits `SongTimeline.tsx` for them |
| X31 | A held key lights in the track's colour, as the roll's keys do. Caps 20 px, `text-xs`; the card `raised`, 1 px `line`, `rounded-md`. Note names by `pitchName` (60 is C4) |
| X32 | The sheet: View gets "Keyboard piano on / off: ⌘K". A last group "Keyboard piano" has two rows: "Play notes: A to ' and W to P", "Octave down / up: Z / X" |
| X33 | `routes_leases.rs` and `routes_fx.rs` keep their own list of write routes and their own 423 middleware, as `routes_view.rs` does |
| X34 | While a drag lives it adds a capture-phase Esc listener on the window that cancels it and stops the event |
| X35 | P0 adds `M2_MIDI: "none"` to the env of the three suites that start their own server |
| X36 | Tooltip: "Metronome: a click on each beat (K)" |
| X37 | The target stays until another window says otherwise or the song closes. A lane that has gone plays nothing, and the MIDI icon's tooltip says "Select a track with notes to play it" |
| X38 | `song_folder` takes a title from the page. Settings uses "Choose Where New Songs Go". A typed folder must be absolute and not inside a song (no `song.ts` above it), else words under the field |

### By item

| Item | Build with |
|---|---|
| P0 | As planned, plus X17 (`afterLaneWrites`), X18 (the stubs), X35 (three env lines) |
| A1 | As planned, plus X33 |
| A2 | As planned, plus X17 and X34 |
| A3 | As planned, plus X3 (the fallback) and X24 |
| A4 | Not built (X3) |
| B1 | Plus X4, X5, X6, X9 (no new route), X14, X25, X36. §1.9's B rows: `click.rs` (with its own tests path), `engine.rs`'s three lines, `lib.rs`'s `pub mod click;`, `core/play/src/lib.rs` (`Shared.click` after `running`, `Player::set_click`, the line in `Callback::render`), `core/play/tests/no_alloc_click.rs` (new) |
| B2 | Plus X13, X15, X16, X30, X31, X32. Moves: `s8-audition`, `pc-keys`, `taste`, `pl-notes` (AuditionTools) |
| B3 | Plus X26, X27, X37 |
| C1 | Plus X2, X19, X23 |
| C2 | Plus X1, X20, X21, X22, X38 |
| C3 | Built as X7 and X8. Files: `core/session/src/{session.rs, tx/mod.rs, take.rs}` (the step's fields); `graph.rs` (`undo`, `redo`, the step route's call); `actor.rs` (two lines in the state, after `can_redo`); `routes_settings.rs` (the step route); `history.ts`, `TopBar.tsx`; `apps/music2-desktop/src/{main.rs, menu.rs}` |
| D1 | Plus X10, X11, after D4e (X29) |
| D2 | X11's rows and words |
| D3 | X28; last in the lane (X29) |
| D4 | X12 for a |
| End | taste.md also: §3.5 the ⋯ menu as built, §3.3 the MIDI icon (and no piano toggle), the Notes tools' piano toggle |

### The questions

Every question stands as written except these:

| Q | Now |
|---|---|
| Q1, Q2 | Plus X26 (a feature) and X27 (fetch first) |
| Q8 | X13 (the toggle is in the Notes tools) and X15 (⌘K always) |
| Q9 | Plus X16 |
| Q12 | X9 (through the view route) and X14 (the menu's order) |
| Q13 | Plus X23 |
| Q14, Q16 | X1 (`start_on`), X21, X22 |
| Q17 | Plus X38 |
| Q19 | X7 and X8 |
| Q20 | Plus X34 |
| Q22 | Plus X24 |
| Q23 | The fallback is the build (X3) |
| Q24, Q25 | Plus X10 |
| Q26 | X11: rows "Move left" and "Move right" in the ⋯ menu |
| Q27 | X28 |
| Q28 | X12 |

### The order, by day

| When | Lane A | Lane B | Lane C | Lane D |
|---|---|---|---|---|
| Day 1 | A1, A2 | midir first (X27), then B1, B2 | C1, C2 | D4e, D1, D2 |
| Day 2 morning | A3 with the fallback | B3 | C3 | the rest of D4, then D3 |
