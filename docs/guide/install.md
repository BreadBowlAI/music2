# Install and run music2

**In short**
- Download music2 from the [Releases page](https://github.com/BreadBowlAI/music2/releases): for a Mac with Apple silicon, or for Linux on an x86_64 PC. 0.1.0 is the first release, an early test version (an alpha).
- On a Mac, the first time you open it, macOS asks you to allow it once in System Settings (below).
- Or build music2 from source: you need [Rust](https://rustup.rs) and [Node.js](https://nodejs.org) 24, and on Linux some system libraries.

## Download

Each release has these files. Pick the one for your computer:

| Your computer | The file | What it is |
|---|---|---|
| A Mac with Apple silicon (M1 or newer), macOS 11 or newer | `music2-<version>-macos-arm64.zip` | the app, `music2.app` |
| Ubuntu 24.04 or newer, Debian 13 or newer | `music2_<version>_amd64.deb` | a package: installs the app and `m2` |
| Another Linux on an x86_64 PC, as new as those | `music2-<version>-linux-x86_64.AppImage` | the app in one file: nothing to install |
| Linux, the command line only | `m2-<version>-linux-x86_64.tar.gz` | `m2`, for the AppImage's users |

Intel Macs and Windows aren't supported yet.

### On a Mac

1. Download the `.zip` and double-click it, if your browser hasn't unpacked it already. You get `music2.app`.
2. Drag `music2.app` into your **Applications** folder.
3. Open it. The first time, macOS stops it: "Apple could not verify music2 is free of malware". Click **Done**.
4. Open **System Settings → Privacy & Security**. Scroll down to **Security**: it says "music2 was blocked". Click **Open Anyway**, then type your password.
5. Open music2 again, and click **Open Anyway** once more. From then on it opens like any other app.

macOS asks because this release isn't signed with an Apple developer certificate, nor checked (notarized) by Apple. A later release may be.

If you use the Terminal, this one line does steps 3 to 5 instead: `xattr -dr com.apple.quarantine /Applications/music2.app`.

`m2`, the command line, is inside the app: `/Applications/music2.app/Contents/MacOS/m2`.

### On Linux, the .deb

```bash
sudo apt install ./music2_0.1.0_amd64.deb      # use the name of the file you downloaded
```

apt also installs what music2 needs: WebKitGTK 4.1 (the window's web view), GTK 3, ALSA (sound) and D-Bus, if they aren't there yet. It needs glibc 2.39 or newer, so apt refuses it on an older system. Then open music2 from your apps, or type `music2`. `m2` is installed as `/usr/bin/m2`. To remove it: `sudo apt remove music2`.

### On Linux, the AppImage

```bash
chmod +x music2-0.1.0-linux-x86_64.AppImage     # once: let it run
./music2-0.1.0-linux-x86_64.AppImage
```

The AppImage needs glibc 2.39 or newer, ALSA, and FUSE 2. On Ubuntu 24.04 install it once with `sudo apt install libfuse2t64`. Without it, run the AppImage with `--appimage-extract-and-run` at the end.

For `m2` beside the AppImage, download `m2-<version>-linux-x86_64.tar.gz` too and unpack it anywhere: `tar xzf m2-0.1.0-linux-x86_64.tar.gz`. Its `bin/m2` finds what it needs in its `lib/` folder, so keep the two together.

### Check what you downloaded

The release's `SHA256SUMS` file holds each file's checksum. In the folder you downloaded to, `sha256sum -c SHA256SUMS --ignore-missing` (Linux) or `shasum -a 256 -c SHA256SUMS --ignore-missing` (Mac) says `OK` for each file that came through whole.

## Build from source

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

## The start screen

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

## The app and packages, built yourself

To build `music2.app` on a Mac, or the `.deb` and the AppImage on Linux:

```bash
(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build)   # → target/release/bundle/
```

`ci/release.sh build <folder>` does the same and gives the files their release names; `ci/release.sh check <folder>` checks them, as the release workflow does (`.github/workflows/release.yml`, which makes a release from a tag).

The app carries everything it needs in `music2.app/Contents/Resources`, or on Linux in `/usr/lib/music2` (inside the AppImage too): the UI (`web/`), the standard sounds (`std/`: `m2pkg.toml`, `kernels/std.wasm`, `src/` and `presets/`), the song templates (`templates/`) and the examples (`examples/`). So it runs from anywhere, without this source folder. `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` point it at other copies; when it runs outside a bundle (`cargo run`), it uses the ones in this source folder.

It also carries `m2`, the command line, beside the app's own program: `music2.app/Contents/MacOS/m2`, or `/usr/bin/m2` in the Linux packages. The same build makes it (`tauri build` runs `apps/music2-desktop/bundle-m2.sh` before it bundles the app). It finds std, the templates and the examples inside the app too. It is signed as the app is, before the app is sealed: the same identity (`APPLE_SIGNING_IDENTITY`, else `signingIdentity` in `tauri.conf.json`), and the hardened runtime with the app's entitlements when that is on.

## The m2 command

`m2` works on song folders from the terminal, with the app open or closed: agents use it, and so can you. From source, build it with `cargo build --locked --release -p m2`; it is then `target/release/m2` in this folder. In `music2.app` it is `music2.app/Contents/MacOS/m2`; from the `.deb`, `/usr/bin/m2`; in the `.tar.gz`, `bin/m2`. A new song's `AGENTS.md` names the exact `m2` it was made with, so an agent finds it without your PATH. Every command is in [m2 commands](commands.md).

The build signs the app ad hoc (with no developer certificate) and isn't notarized by Apple. On the Mac that built it, it just opens. A copy downloaded, AirDropped or sent in Messages to another Mac is held back the first time: macOS says it can't check the app. Open **System Settings → Privacy & Security** there and click **Open Anyway** (once). It runs on Apple silicon Macs only. The hardened runtime is off (`tauri.conf.json`): under it, macOS kills the app as soon as it compiles std's kernels. A release signed with a Developer ID turns it on, with `apps/music2-desktop/entitlements.plist`, which allows that (and loading other developers' plugins); that is set up but not yet tried.
