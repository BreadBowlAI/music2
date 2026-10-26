# Security

**In short**
- Found a security problem? Report it in private on GitHub: the repository's **Security** tab, then **Report a vulnerability**. Please don't open a public issue for it.
- Before 1.0, only `main` gets fixes, and they ship in the next release. 0.1.0 is the first release, an alpha.
- A song is code. Opening someone's song runs that code, in a sandbox that can only describe music.
- CLAP plugins are native programs. They run in a second process, but they are not sandboxed: trust them as you trust any app you install.

## How to report

1. Open the repository on GitHub, click **Security**, then **Report a vulnerability**. Only the maintainers see what you write.
2. Say what you found, how to make it happen, and what it lets someone do. A small song folder that shows it helps most.
3. The maintainer answers in the report and fixes it on `main`. When the fix is in, GitHub publishes an advisory that credits you, unless you would rather not be named.

## Which versions get fixes

| Version | Fixes |
|---|---|
| `main` | yes, and the next release carries them |
| a release (0.1.0) or anything older | no: update to the newest release, or build from `main` |

## The security model

What each part of music2 is allowed to do, true to the code at the time of writing.

| Part | How it runs | What holds it in |
|---|---|---|
| **A song's code** (its `.ts` files) | The types are stripped by oxc, then QuickJS runs it **inside the app's own process** (and inside `m2` and the headless server). | No files, no network, no clock, no randomness: `Date` and `performance` are deleted, `Math.random` throws, `eval` and the `Function` constructors refuse. It imports only the song's own `.ts` files, from a snapshot the app passes in, and the packages its `music2.toml` lists. 256 MB of memory, a 16 MB stack, 10 seconds a run. All it can do is describe a graph of sounds. (`core/script/src/lib.rs`) |
| **Sounds** (kernels) | WebAssembly, compiled by wasmtime with Cranelift. | A kernel may import only `m2.asset_read`, which hands it audio the app has already read; any other import is refused. One 32-bit memory with guard pages, Cranelift's Spectre guard on, at most 256 MB. **No time limit:** a kernel that loops forever stalls the sound. Today only std's kernels load (`std/kernels/std.wasm`, shipped with the app); sound packages from other people aren't built yet (gap #39). (`core/runtime/src/packages.rs`, `core/runtime/src/sandbox.rs`, [kernel-abi.md](docs/kernel-abi.md)) |
| **A song's files** | Its JSON data files and WAV audio (and a MIDI file you import) are read by music2's own readers and the `hound` crate. | A clip plays only an exact file in the song's own `assets/` folder. Code imports outside the song are refused ("is outside the project"). |
| **CLAP plugins** | Native code, in a second process: the app runs itself again with `--m2-plugin-host`. A crash there doesn't take the app down. | **Not sandboxed:** a plugin runs as you, with your rights. A song names plugins by id. Opening it loads only plugins already installed on your computer, and hands each the state saved in the song (`data/<track>/<key>.state`). (`core/plugins/src/remote/mod.rs`) |
| **The local API** | The app serves its UI and its API on `127.0.0.1` only, on a free port. | Every `/api/` call needs one of two 24-byte random keys, both made at each launch. It goes in the `x-m2-token` header, or as `?token=` in the address (the UI's WebSocket can't send a header). The window's page gets a key of its own, handed only to it. The other, the tools key, is what scripts and agents use: the app writes it to `~/.music2/session.json`, which only you can read: the file is written whole, to a new file made with mode 0600 and then renamed over the old one, and a `~/.music2` made at launch is 0700 (one that was there keeps its mode). A request with the tools key is an agent's: its changes are shown as an agent's, and are refused while you hold what they change, or when they would end it (AGENT-3, AGENT-10). It can't act on what you hold: your drags, your unsaved code, undo's skip, or your take's stop. Your own controls answer it 403: play and the loop, recording, listening mute and solo, the view, plugin windows, exports, settings, and opening songs (AGENT-11). Keys are compared in constant time. The UI's own files need no token. The desktop window lets pages from `http://127.0.0.1` and `http://localhost`, on any port, call into the app: Tauri's default set (`core:default`), dragging and maximizing the window, and music2's own command, `action` (the file dialogs, Open Recent, Show in Finder, the prompts about unsaved work). The window opens only the app's own server (or, while developing, the address in `MUSIC2_DEV_URL`). (`crates/server/src/lib.rs`, `crates/server/src/routes.rs`, `crates/server/src/api.rs`, `apps/music2-desktop/src/main.rs`, `apps/music2-desktop/capabilities/default.json`) |

So, what can go wrong:

- **Opening a song from someone else** runs their code, but that code can only describe music. The real risks are bugs: in QuickJS, in wasmtime or Cranelift, or in music2's own file readers. All of them run inside the app's process.
- **A plugin is as safe as the plugin.** Install plugins only from makers you trust. A song can hand an installed plugin a state file it brings; a bug in that plugin's own loader could be reached that way.
- **Programs on your computer** that run as you can read the tools key and drive the app. That is by design: it is how scripts and agents work with music2. They can't pass as you: every change they make is shown as an agent's, and you can take it back. The API is not meant for other machines.

## Known issues

| Issue | Status |
|---|---|
| Kernels have no time limit. | Open ([gaps.md](docs/gaps.md), Playback). It matters once kernels from other people can load. |
| The desktop window has no Content Security Policy (`"csp": null` in `apps/music2-desktop/tauri.conf.json`). | Open. |
| The window's native commands are allowed for any page on `127.0.0.1` or `localhost`, on any port, not only the app's own server: its port is chosen at launch, so the capability can't name it. | Open. It matters only if the window is ever sent to another local page. |
| Plugins aren't sandboxed. | As designed today: they are native code in their own process. |
