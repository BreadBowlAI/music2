# Install and run music2

**In short**
- You build music2 from source: you need [Rust](https://rustup.rs) and [Node.js](https://nodejs.org) 24, and on Linux some system libraries.
- Build the window's UI once, then `cargo run` starts the app on its start screen.
- On a Mac you can also build `music2.app`, which carries everything it needs, `m2` included.

You need a Mac, or Linux (Ubuntu 24.04), with [Rust](https://rustup.rs) and [Node.js](https://nodejs.org) 24. On Linux, install the system libraries first:

```bash
sudo apt install pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev
```

Then, on either:

```bash
(cd web && npm ci && npm run build)                                      # the window's UI: once, and after UI changes
cargo run --locked --release -p music2-desktop                           # the app, on its start screen
cargo run --locked --release -p music2-desktop -- ~/Music/music2/Demo    # or opened on a song folder
```

The start screen has three ways in:

- **New Song…** makes a new song from a template and opens it (see [Your first song](first-song.md)).
- **Open Project Folder…** opens a song you already have. A song folder is any folder with a `music2.toml` file in it (see [The folder](code.md#the-folder)). File → Open Project Folder… (⌘O) does the same at any time.
- **Examples** lists the examples. Picking one opens a copy of it in your songs folder, so the example itself stays as it is.

Everything you change is written into the song's folder right away.

## The UI with hot reload

To work on the UI with hot reload:

```bash
cd web && npm run dev                                                 # the UI on :5173 (it passes /api on to :4100)
MUSIC2_DEV_URL=http://localhost:5173 cargo run --locked -p music2-desktop   # the app, its server on :4100, the UI from Vite
```

## music2.app on a Mac

To build `music2.app`:

```bash
(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --bundles app)   # → target/release/bundle/macos/music2.app
```

The app carries everything it needs in `music2.app/Contents/Resources`: the UI (`web/`), the standard sounds (`std/`: `m2pkg.toml`, `kernels/std.wasm`, `src/` and `presets/`), the song templates (`templates/`) and the examples (`examples/`). So it runs from anywhere, without this source folder. `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` point it at other copies; when it runs outside a bundle (`cargo run`), it uses the ones in this source folder.

`music2.app` also carries `m2`, the command line, beside the app's own program: `music2.app/Contents/MacOS/m2`. The same build makes it (`tauri build` runs `apps/music2-desktop/bundle-m2.sh` before it bundles the app). It finds std, the templates and the examples inside the app too.

## The m2 command

`m2` works on song folders from the terminal, with the app open or closed: agents use it, and so can you. From source, build it with `cargo build --locked --release -p m2`; it is then `target/release/m2` in this folder. In `music2.app` it is `music2.app/Contents/MacOS/m2`. A new song's `AGENTS.md` names the exact `m2` it was made with, so an agent finds it without your PATH. Every command is in [m2 commands](commands.md).

The build signs the app ad hoc (with no developer certificate) and isn't notarized by Apple. On the Mac that built it, it just opens. A copy downloaded, AirDropped or sent in Messages to another Mac is held back the first time: macOS says it can't check the app. Open **System Settings → Privacy & Security** there and click **Open Anyway** (once). It runs on Apple silicon Macs only. The hardened runtime is off (`tauri.conf.json`): under it, macOS kills the app as soon as it compiles std's kernels. Notarizing will need it on, with entitlements that allow that (and loading other developers' plugins).
