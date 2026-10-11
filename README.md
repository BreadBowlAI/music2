# music2

A full music studio (a DAW, like Ableton Live or Logic Pro) with a Rust audio engine and a React UI in a native Mac app. It's part of **suite2**, alongside film2 and dj2.

There's no AI inside the app, and no beginner mode: every tool is visible. Every edit is a named command with a JSON Schema, so a Claude Code session on your machine, or any script, can drive the open app (see [Scripting](#scripting)).

See [`docs/music2-design.md`](docs/music2-design.md) for the product and architecture, and [`docs/filmcraft-architecture.md`](docs/filmcraft-architecture.md) for the research it borrows from.

**Next direction: songs as programs.** A new, minimal core is being built alongside the app: songs are TypeScript graphs, and every instrument and effect is a package. See [`docs/song-as-program.md`](docs/song-as-program.md) and [Song as a program (new core)](#song-as-a-program-new-core).

## Run it

music2 is a native Mac app (Tauri): the React UI in a system web view, around the Rust engine, which makes the sound.

```bash
(cd web && npm install && npm run build)                 # once, and after UI changes
cargo run --release -p music2-desktop                    # run the app
(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build)   # → target/release/bundle/macos/music2.app
```

Drag `music2.app` into Applications. After that, double-clicking a `.m2song` file opens it in music2.

- **Menu bar:** File (New, Open…, Save, Save As…, Import Audio…, Export as WAV…, Show in Finder), Edit, View, Transport.
- **File handling:** real file dialogs, and an "unsaved changes" prompt before quitting or opening another song.
- **Right-click menus** on clips, tracks, empty lanes and piano-roll notes.
- **Drag and drop from Finder:** drop a WAV onto a bar to place it there, or drop a song to open it.

Songs save to `~/Music/music2/` by default.

To work on the UI with hot reload:

```bash
cd web && npm run dev                                                 # UI on :5173 (proxies /api to :4100)
MUSIC2_DEV_URL=http://localhost:5173 cargo run -p music2-desktop      # the app, engine on :4100, UI from Vite
```

## Using it

- **Timeline:** tracks on the left, clips on the lanes. Double-click a lane to add a clip, drag clips to move them, and drag a clip's right edge to resize it.
- **Piano roll:** click a MIDI clip to edit its notes. It shows all notes; "In-key only" hides the ones outside the song's key.
- **Track inspector** (right panel, ⌘1): volume, pan, output routing to a bus, sends, the instrument and its knobs, effects, sidechain sources, and the master chain.
- **Sounds** (⌘2): the recordings in the song. Place one on the timeline or turn it into a Sampler instrument.
- **Automation:** the pulse icon on a track opens a lane for volume, pan or any knob. Click to draw, drag to move, right-click to delete.

Shortcuts:

- **Space** plays or stops.
- **⌘Z** / **⇧⌘Z** undo and redo.
- **⌘S** saves.
- **⌘D** duplicates the selected clip.
- **Delete** removes the selected note or clip.
- **Right-click** almost anything for its options.
- **Enter** jumps back to the start.

## Scripting

When the desktop app is running, it writes `~/.music2/session.json` (readable only by you) with its engine URL and a per-launch token. Anything you run locally, such as a Claude Code session, can use them to read and edit the open song. Changes show up live and can be undone like any other edit.

```bash
S=~/.music2/session.json; URL=$(jq -r .url $S); H="x-m2-token: $(jq -r .token $S)"
curl -s -H "$H" $URL/api/commands                  # every command + JSON Schema
curl -s -H "$H" $URL/api/describe                  # compact song summary (?clip=ID for notes)
curl -s -H "$H" -H 'content-type: application/json' $URL/api/exec \
  -d '{"cmd":"track.add","params":{"name":"Claps","instrument":"m2/drums"}}'
curl -s -H "$H" -H 'content-type: application/json' $URL/api/steps \
  -d '{"label":"Add claps","steps":[["track.add",{"name":"Claps","instrument":"m2/drums"}],
       ["clip.add",{"track":"$new","start_bar":1,"length_bars":1,"notes":[{"pitch":39,"start":1,"length":0.5}]}]]}'
```

`/api/steps` runs a whole recipe as one undo step, and `/api/import-path` imports a WAV by path. The design doc (Part 4) lists the other endpoints.

With the app closed, the CLI runs the same commands on a song file:

```bash
music2-cli new song.m2song
music2-cli commands [--json]
music2-cli exec song.m2song track.add '{"name":"Claps","instrument":"m2/drums"}'
music2-cli exec song.m2song clip.add '{"track":"Claps","start_bar":1,"length_bars":1,"notes":[{"pitch":39,"start":1,"length":0.5},{"pitch":39,"start":3,"length":0.5}]}'
music2-cli import song.m2song vocal.wav --at 5
music2-cli describe song.m2song
music2-cli render song.m2song out.wav
```

## Plugins (`m2plug`)

A plugin is a single JSON file: a graph of built-in DSP nodes plus a few 0–1 macro knobs. It can't run code. See the examples in [`plugins/`](plugins/) and the spec in the design doc (Part 5).

```bash
music2-cli plugin check my-pad.m2plug.json      # validate before publishing
music2-cli plugin add github:you/m2-glitter-pad  # installs m2plug.json from the repo
```

## Song as a program (new core)

The new core is in `core/`, the standard library in `std/`, and the CLI in `apps/m2`. A project is a folder of TypeScript modules plus timeline JSON and assets. Examples:
- [`examples/first-song`](examples/first-song): std instruments, automation, a sidechain, a mask.
- [`examples/with-plugins`](examples/with-plugins): Surge XT and Airwindows Consolidated (CLAP) as graph nodes, with automation and saved state. Needs both plugins installed.
- [`examples/resample`](examples/resample): bounce a track to audio, then chop, reverse, pitch and stretch it on an audio lane.

```bash
cargo run --release -p m2 -- build examples/first-song            # run the TypeScript, check the graph
cargo run --release -p m2 -- render examples/first-song out.wav   # render to WAV
cargo run --release -p m2 -- kernels                              # every kernel and its typed ports
cargo run --release -p m2 -- plugins                              # installed CLAP plugins (and `plugins clap:<id>` for ports)
cargo run --release -p m2 -- controls examples/first-song         # every number the graph editor can change
cargo run --release -p m2 -- set examples/first-song 'song.ts#2/0.hats.pan' 0.45 --expect 0.3   # verified one-token edit
cargo run --release -p m2 -- bounce examples/resample keys-bounce --track Keys --force   # render a track to an asset
std/build.sh                                                      # rebuild std's WebAssembly kernels
cargo run --release -p m2-runtime --example bench                 # WebAssembly vs native speed
```

Errors point at the TypeScript line that caused them. Feeding a dB value into an oscillator's frequency gives:

```
tracks/bass.ts:12:24: [Bass/n10] input `freq` expects control<hz>, got control<db>
```

## Layout

| Layer | Crates |
|---|---|
| L6 apps | `apps/music2-desktop` (the app), `apps/music2-cli` |
| L5 frontends | `crates/server` (HTTP + WebSocket for the web UI in `web/`), `crates/audio-io` (cpal) |
| L4 engine | `crates/engine`: session, commands, snapshot undo, plugin install |
| L3 processing | `crates/render` |
| L2 plugin standard | `crates/graph` |
| L1 models | `crates/project`, `crates/dsp` |
| L0 foundations | `crates/time`, `crates/theory` |
| New core | `core/ir`, `core/song`, `core/runtime`, `core/script`, `core/session`, `core/plugins`, `core/kernel`; CLI `apps/m2`; package `std/` |

`cargo test --workspace` runs the test suite. For automated UI tests, the UI also opens in headless Chrome at the app's engine URL, given `window.__M2__ = {native: false, token}`; URL params like `?select=Hook&tab=sounds` set up a state.
