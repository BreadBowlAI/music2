# @music2/web: songs and feeds in a page (Parts 3 and 4 of the live note)

**In short**
- Parts 3 and 4 of [design.md](design.md), in one note, built on Part 2, the engine in a browser ([web-engine.md](web-engine.md), accepted 2026-11-28).
- **`@music2/web`** is an npm package for developers who put a music2 song in their own web page. It is the engine, not the app: open a song, play, stop, seek, loop, set a knob, render, listen to events, and feeds. It calls no model. It fetches only the song's URL and the files under it.
- **Songs reach a page as a web bundle.** A new command, `m2 export <song> --web <dir>`, writes a folder any static host can serve: the compiled graph (the IR), the song's data files as they are, the samples it plays, and the kernels of the packs it plays, with a manifest of sizes and hashes. So Part 3 runs no song code in the page and needs no library: it doesn't wait for the R2 step.
- **The same bits.** A bundle played or rendered in Chrome gives the app's render bit for bit ([RENDER-33](../../architecture.md#the-bit-identity-test)). A browser suite checks the 26 renders of `e2e/renders.sha256`, each from its song's bundle.
- **Feeds speak `feed/1`**: the bodies and answers of the app's feed routes ([FEED-4](../../architecture.md#feeds), [FEED-5](../../architecture.md#feeds)), written up in the guide as a protocol. The page owns its agent: `@music2/web` gives the page open, put, status, close and the events, and the page relays them from whatever agent it has. music2 runs no server for this.
- **Anyone's pack needs the time limit first.** A bundle may hold only std and music2's own packs until the Worker watchdog ([web-engine.md](web-engine.md), decision 6) is built.
- **Measured:** the starter template's bundle is 363 KB, 119 KB if the host compresses it, not counting the engine's own module, which isn't built yet (estimated 1–3 MB).
- **Where the code says otherwise** (see [Questions, and what was decided](#questions-and-what-was-decided)): the npm scope `@m2` belongs to an npm organisation that already exists; NET-2 and NET-4 can't hold word for word in a browser; the feed logic lives in the server crate, so it must move to be "the same"; Keep has no song to write on the web; and nothing records std's hashes, so a page can't yet tell std from a stranger's module.
- Status: **accepted, 2026-11-28.** The owner said to go ahead with Parts 3 and 4 and picked the npm name, `@music2/web`; the lead settled the other questions (see [Questions, and what was decided](#questions-and-what-was-decided)). Docs only. Nothing here changes the app.

## Words used here

| Word | Meaning |
|---|---|
| page | a web page someone else writes, which uses `@music2/web` |
| web bundle | the folder `m2 export --web` writes: what a page needs to play one song |
| manifest | `m2web.json`, the bundle's list of its files, sizes and hashes, and what the song is |
| IR | the song's compiled graph, as `m2 check --json` prints it |
| relay | the page's own code that passes messages between its agent and `@music2/web` |
| `feed/1` | the feed messages, as a protocol any player can speak |
| CORS | the rule that a page may read a file from another site only when that site says so, with a header |
| cross-origin isolated | a page served with two headers (COOP and COEP), which gets `SharedArrayBuffer` |
| Cache Storage | the browser's store of fetched files, per site |
| the autoplay rule | a browser starts sound only after a click or a key press on the page |
| watchdog | the page's thread, stopping the engine's Worker when a kernel runs too long |

## 1. What a page gets

### The API (a sketch)

```ts
export function open(url: string, options?: OpenOptions): Promise<Song>;

export interface OpenOptions {
  integrity?: string;          // "sha256-…" of m2web.json, as `m2 export --web` prints it
  maxBytes?: number;           // the whole bundle, as decoded; 256 MiB unless given
  cache?: boolean;             // keep files in Cache Storage, by hash; true unless given
  context?: AudioContext;      // the page's own, at the song's sample rate
  signal?: AbortSignal;        // stops the downloads
  progress?: (done: number, total: number) => void;
}

export interface Song {
  readonly name: string;
  readonly sampleRate: number;
  readonly seconds: number;    // to the last clip's end, as `m2 render` counts it
  readonly tracks: Track[];    // key, label, lanes; which lane a feed can open on
  readonly controls: Control[]; // id, label, unit, range, value: what `set` takes
  play(): Promise<void>;       // from a click or a key press (the autoplay rule)
  stop(): void;
  seek(beat: number): void;
  loop(range: { start: number; end: number } | null): void; // in beats
  set(control: string, value: number): void;
  render(range?: { from?: number; seconds?: number }): Promise<Rendered>;
  on(event: "state", f: (s: State) => void): () => void;   // playing, beat, bar, underruns, isolated
  on(event: "fault", f: (f: Fault) => void): () => void;   // a sound that stopped, in words
  feed(track: string, options?: { lowBars?: number; writer?: string }): Promise<Feed>;
  readonly feeds: Relay;
  close(): Promise<void>;
}

export interface Feed {
  readonly track: string;
  put(from: number, to: number, notes: Note[]): Promise<PutAnswer>; // FEED-4
  status(): FeedState;                                              // FEED-5
  on(why: "low" | "empty" | "jump" | "state" | "closed", f: (s: FeedState) => void): () => void;
  played(): Note[];            // what played, for the page to keep (section 6)
  close(): Promise<void>;
}

/** feed/1 for a relay: a message in, the answer the app would give out. */
export interface Relay {
  handle(act: "open" | "put" | "status" | "close", body: object): Promise<object>;
  on(f: (event: object) => void): () => void; // each {"t": "feed", …} event, as JSON
}

export interface Note { pitch: string; start: number; length: number; velocity?: number }
export interface Rendered { left: Float32Array; right: Float32Array; wav(): Blob }
```

```js
import { open } from "@music2/web";
const song = await open("https://example.com/songs/starter/", { integrity: "sha256-…" });
button.onclick = () => song.play();
song.set("tracks/bass.ts#1/@cutoff", 600);
```

### What it is not

- Not the app's GUI: no timeline, no cards, no code editor. The page draws what it wants.
- No recording, no plugins, no Playground, no undo: a bundle is a song to play, not to edit.
- `set` changes a **cell**: a number the graph plays, heard at the next block, with no rebuild ([RENDER-22](../../architecture.md#cells)). A **setting** decides what is built (a card's settings, a menu that picks another kernel), so a change needs the song's code. Part 3 refuses it in words ([section 4](#4-what-is-refused-in-which-words)).
- `render` is an offline render by the same engine in the same Worker: `wav()` is the file `m2 render` writes. The bit test uses it, and a page can offer a download with it.

### Where it lives

`@music2/web` is built in the repo, in `core/web` (the web's shell, [web-engine.md](web-engine.md) decision 2), with its own `package.json`: the engine's module, its Worker, the worklet that copies blocks to the speakers, and the TypeScript API above. CI builds it and runs its suites. Publishing it to npm is the owner's call, later ([Questions](#questions-and-what-was-decided)).

It loads its own files from where the page put them (`new URL("./engine.wasm", import.meta.url)`), and the bundle from the URL the page gives. Nothing else.

## 2. The web bundle

### What the engine needs, read from the code

The app renders a song with `Renderer::new(flat, data, packages, sample_rate)` (`crates/editor/src/render.rs`). Each of the four comes from files a bundle can carry:

| Input | Made by | From |
|---|---|---|
| `flat`, the checked graph | `m2_ir::check(&ir, kernels)` (`core/ir/src/check.rs`) | the IR, and each kernel's ports from its pack's `m2pkg.toml` |
| what the code declares (lanes, plugin states, assets) | `m2_runtime::decls(&ir)` | the IR alone |
| `data` (`DataSet`, `core/song/src/data.rs`) | `Project::data(&decls)`, read in `read_data` (`core/session/src/lib.rs`) | `data/arrangement.json`, the lane files, the assets and package files the kernels read |
| `packages` | `PackageSet` (`core/runtime/src/packages.rs`) | each pack's `m2pkg.toml` and `kernels/*.wasm`; its listing `m2pack.json` decides whether it is music2's own ([ABI-11](../../architecture.md#kernel-abi-reservations)) |

So the song's code is needed only to make the IR. `Ir` is already `Serialize` and `Deserialize` (`core/ir/src/lib.rs`, `IR_VERSION` 2), and the starter's IR names no machine path. The IR names kernels by id (`std:svf`, `@m2/effects:reverb`), and its constants keep their origin (a call site and a control), which is how `set` finds a cell.

The bundle carries the song's data files **as they are**, not a new format, so the web reads them with the same `m2-song` code and gets the same `DataSet`.

### The folder

```
starter-web/
  m2web.json                          the manifest
  ir.json                             the compiled graph
  controls.json                       every control `set` takes
  music2.toml  music2.lock            the song's manifest and lock, as they are
  data/arrangement.json               tempo and meter
  data/bass/notes.json …              each lane file the IR declares
  assets/kick.wav  assets/kick.wav.json   each asset a kernel or clip plays, with its note
  packs/std/0.10.0/m2pkg.toml
  packs/std/0.10.0/kernels/std.wasm
  packs/std/0.10.0/samples/kit/kick.wav   a package file the song reads (PACK-5), if any
  packs/@m2/drums/2.2.0/m2pkg.toml
  packs/@m2/drums/2.2.0/m2pack.json
  packs/@m2/drums/2.2.0/kernels/drums.wasm
```

- Only packs whose kernels the IR names. The starter depends on `@m2/synths`, but its synths are built from std's kernels: no `@m2/synths:` kernel is in its IR, so `synths.wasm` stays out.
- Only the assets and package files the song plays: `DataSet`'s kernel assets and clips, by hash.
- No TypeScript, no presets, no pictures, no `.state` files (plugins are refused).
- `controls.json` is what `m2 controls` lists (made by the same code, `crates/editor/src/controls.rs`), as JSON: each control's id, label, unit, range and value, and where it is in the IR (its site and slot). The IR keeps only a site's number; the id (`tracks/bass.ts#1/@cutoff`) needs the build's site table, so the bundle carries the answer.

### The manifest, `m2web.json`

| Field | What it says |
|---|---|
| `format` | `1`. `@music2/web` reads one format; an other is refused in words (no backwards compatibility before 1.0, [goals](../../goals.md)) |
| `music2` | the version of music2 that wrote it |
| `ir`, `abi` | the IR's version (2) and the kernel ABI's (1) |
| `song` | `name`, `sample_rate`, `seconds` (to the last clip's end) |
| `tracks` | each track's `key`, `label` and lanes (`key`, `kind`), and which notes lane a feed opens on ([FEED-1](../../architecture.md#feeds)) |
| `packs` | each pack's `name`, `version`, `hash` (the lock's) and `license`. Informative only: the engine decides trust from the bytes, never from this |
| `files` | every other file: `path`, `size`, `sha256` |
| `credits` | each sample: its path, and its note's `from` (pack, version, author, licence; or your own folder) ([LIB-7](../../architecture.md#library)) |

No date and nothing of the machine: the same song exports to the same bytes, on any computer. `m2 export --web` prints the manifest's own hash, for `open`'s `integrity`.

### How big a bundle is

Measured on 2026-11-28: each song's IR from `m2 check --json` (minified), its `data/` and `assets/` files, and the `.wasm` and `m2pkg.toml` of each package whose kernels its IR names. "Compressed" is gzip -9 of each file, what a host that compresses would send; many hosts don't compress `.wav`.

| Song | Packages it plays | IR | Data | Assets | Kernels | Manifests | Total | Compressed |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| `templates/starter` | std, drums, effects | 38 KB | 15 KB | — | 284 KB | 26 KB | **363 KB** | **119 KB** |
| `examples/first-song` | std, drums | 24 KB | 13 KB | — | 179 KB | 20 KB | 237 KB | 75 KB |
| `examples/sounds` (every std synth and drum) | std, drums, effects, synths | 107 KB | 62 KB | — | 340 KB | 31 KB | 541 KB | 153 KB |
| `examples/sampler` | std | 6 KB | 6 KB | 170 KB | 133 KB | 14 KB | 330 KB | 176 KB |
| `examples/resample` (an audio clip) | std, drums | 12 KB | 5 KB | 1.5 MB | 179 KB | 20 KB | 1.75 MB | 483 KB |

The kernels: `std.wasm` 132,924 bytes (45,918 compressed), `drums.wasm` 46,515, `effects.wasm` 104,355, `synths.wasm` 56,539. A pack's listing is about 0.5 KB. `controls.json` and the manifest weren't made (nothing writes them yet): estimated at a few KB each.

Not in the table, and **estimated**:
- The engine's own module (`m2-runtime`, `m2-song`, `m2-ir` built for `wasm32`) and its glue, fetched once per page, not per song: perhaps 1–3 MB, a third of that compressed. It doesn't build yet; track `web-node` of Part 2 measures it.
- Song code in the page (QuickJS and `oxc` in WebAssembly, [section 8](#8-later)): perhaps 3–6 MB more, only for a page that runs code.
- Big sample packs: the grand piano is 260 MB ([design.md](design.md)). It needs web sample sets, later.

## 3. How the page loads a bundle

1. **The manifest.** `GET <url>/m2web.json`. When the page gave `integrity`, its SHA-256 must match. Its `format`, `ir` and `abi` must be the ones this `@music2/web` reads.
2. **The paths.** Every path in `files` must be a plain relative path under the bundle's URL: no `..`, no leading `/`, no `//`, no other host, no `?` or `#`. One that isn't is refused before any fetch.
3. **The limit.** The sizes in `files`, added up, must fit `maxBytes`. Each file is then fetched with its own size as its limit, counted as its bytes come, and stopped as soon as it is over (as [NET-4](../../architecture.md#network) does).
4. **The hashes.** Each file's SHA-256 is checked after its last byte, by the engine's own `sha2` code (it builds for `wasm32`, [web-engine.md](web-engine.md)), before anything uses it. A kernel's bytes are compiled only then, with `WebAssembly.compile(bytes)`, never `compileStreaming`, which would compile before the hash is known.
5. **The cache.** A checked file is kept in Cache Storage under its hash. The next `open`, of this song or another on the same site, takes a file with the same hash from there, and fetches nothing for it. `std.wasm` is fetched once per site. With `cache: false`, nothing is kept.
6. **Then the engine.** The Worker gets the bytes: IR, data, assets, packs. It checks the IR against the kernels' ports (`m2_ir::check`), reads the data, makes the renderer, and stretches any warped clip (`std:stretch`, before Play, as Part 2's `runtime-web` does). Then `open` returns.

Each request is a `GET` with `credentials: "omit"` (no cookie), `referrerPolicy: "no-referrer"`, `cache: "no-cache"` for the manifest and the default for the rest, and nothing about the person. Every failure says why, in words, naming the host but not the whole address (as [NET-6](../../architecture.md#network) does).

## 4. What is refused, in which words

### At export

| What | Words (`m2 export --web`, exit 1) |
|---|---|
| A node hosts a CLAP plugin | "This song plays a plugin (Vital, on the track Lead). A web page can't load plugins. Bounce the track to audio in the app (Bounce to audio), then export again." ([RENDER-40](../../architecture.md#bounce-to-audio)) |
| A pack that isn't std or music2's own, before the watchdog | "This song plays @alice/felt-piano, which isn't one of music2's own packs. A web bundle can hold only std and music2's own packs until the web has a time limit for other packs' code." |
| The code doesn't build, or the graph has an error | the same words `m2 check` prints, file and line first |
| A lane file that doesn't read | the same words `m2 check` prints: the page would play other notes than the app |
| The folder isn't empty, or is inside the song | "Export to a new, empty folder outside the song." |

A warning, not a refusal: "3 samples go into the bundle, and anyone who opens the page can download them. Their licences: …", with each sample's credit, and "no licence on record" for your own recordings ([Questions](#questions-and-what-was-decided)).

### At open

| What | Words (`open` rejects with an `Error`) |
|---|---|
| Not `https://` (or `http://` to this computer) | "music2 opens songs over https only (or http on this computer, for testing)." |
| `integrity` differs | "m2web.json isn't the file this page asked for: its hash differs." |
| Another format | "This bundle is format 2 (music2 0.5 wrote it); this @music2/web reads format 1. Export the song again with the music2 that matches this @music2/web, or use the @music2/web that matches the bundle." |
| A path that leaves the bundle | "The bundle names a file outside its folder (../x.wav): refused." |
| Over `maxBytes`, or a file over its size | "The bundle is 410 MB; this page allows 256 MB." / "assets/pad.wav is bigger than the bundle says." |
| A hash differs | "assets/kick.wav isn't the file the bundle names: its hash differs. The host changed it, or a copy is half written. Export it again." |
| A fetch fails | "Couldn't fetch m2web.json from example.com: the server didn't answer, or it doesn't let this page read it (CORS)." The browser doesn't say which, so `@music2/web` can't either |
| A module that isn't std or music2's own | as at export, and decided again here, from the bytes |
| No `WebAssembly`, `Worker` or `AudioWorklet` | "This browser can't play music2 songs: it has no AudioWorklet." |
| `set` on a setting | "`shape` decides what is built, so changing it needs the song's code, which a web bundle doesn't carry." |
| `set` on an id the bundle doesn't list | "This song has no control tracks/bass.ts#9/@cutoff." |
| `play()` outside a click or key press | "The browser starts sound only after a click or a key press on the page: call play() from one." |

A page that isn't cross-origin isolated isn't refused: it plays through a `MessagePort` instead of a shared ring ([web-engine.md](web-engine.md), decision 1), and `state` says `isolated: false`.

## 5. The same bits

**The test** (track `web-bits`): for each of the 13 songs of `e2e/renders.sha256`, `m2 export --web` writes its bundle; a browser suite serves it from a local server and, in headless Chrome:
- `render()` (the song) and `render({ from: 4, seconds: 6 })` give WAV files whose SHA-256 is the line in `e2e/renders.sha256`. The page's own file, made by the engine in a Worker, against the app's.
- `play()` from beat 0, with the Worker's blocks captured as they go into the ring, gives the same `f32` bits as `render()`, for the first 10 s. That is [RENDER-33](../../architecture.md#the-bit-identity-test) on the web: live and export are one engine. The capture is before the worklet and before the browser's own resampling, as RENDER-33 is before the device's.

Part 2's `web-chrome` track proves the engine from the app's IR. This proves the bundle: that what `m2 export --web` writes, fetched, hashed and read by `@music2/web`, is the song the app plays.

## 6. Feeds in a page (Part 4)

### `feed/1`

Four acts and one event. Each body and answer is the app's, as FEED-4 and FEED-5 say them. The app carries them over HTTP; a page over whatever its relay uses.

| Act | Body | Answer | In the app |
|---|---|---|---|
| `open` | `{"track": "bass", "low_bars": 4, "as": "Claude"}` | the feed's state, `"why": "open"` | `POST /api/feed/open` |
| `put` | `{"feed": "bass", "from": 64, "to": 80, "notes": [{"pitch": "A1", "start": 64, "length": 0.5}], "as": "Claude"}` | `{"ok": true, "taken", "late", "lock", "written_to", "playhead"}` | `POST /api/feed/put` |
| `status` | `{}` | every open feed's state | `GET /api/feed` |
| `close` | `{"feed": "bass"}` | the state, `"why": "closed"` | `POST /api/feed/close` |
| the event | — | `{"t": "feed", "feed", "why"?, "playhead", "lock", "written_to", "buffer", "low_bars", "low_mark", "empties", "late", "writer", "playing"}` | `/api/ws`, `m2 watch` |

- A refusal is `{"error": "<words>"}`, as the app's routes answer one, with the same words.
- The limits are the same: 64 bars ahead, 4,096 notes, 128 sounding and 64 starting in 128 frames, 16 bars behind.
- The app's `session` (which open song) is the app's envelope, not part of `feed/1`: a page's `Song` is its session.
- No transport. `feed/1` has no play, stop or seek, in the app or a page, so a relayed agent can't press Play unless the page itself chooses to give it that ([AGENT-11](../../architecture.md#agents)).
- Keep isn't in `feed/1` ([below](#what-differs-on-the-web)).

The guide gets a page, `docs/guide/feed-protocol.md`: the acts, the event, the limits, the words of each refusal, and the loop of [Playing live](../../guide/live.md) written once for both.

**Unchanged, at the message level.** An agent's loop is "wait for `low`, then put the next bars from `written_to`". Against the app it sends those bodies to `/api/feed/put`, or runs `m2 feed`. Against a page it sends the same bodies to the page's relay. Only the transport differs, and the relay is one call:

```js
const song = await open(url);
const relay = song.feeds;
relay.on((event) => socket.send(JSON.stringify(event)));        // to the agent
socket.onmessage = async (m) => {
  const { act, body, id } = JSON.parse(m.data);                  // from the agent
  socket.send(JSON.stringify({ id, answer: await relay.handle(act, body) }));
};
```

The socket, the agent and its model are the page's. `@music2/web` opens no connection for them.

### Where the feed logic runs

Today it is in the server: `crates/server/src/graph/feeds.rs` holds the feed, the lock, `late`, the written stretches, low, empty and jump, what played, and the state's JSON. A page can't build the server (axum, the session, the editor). Writing it again in TypeScript would be a second implementation, which drifts.

So the logic moves to a core crate both use (track `feed-core`): `Feed`, `Now`, `History` and the state's JSON, with the clock given by its caller (`Instant::now()` traps on the web, [web-engine.md](web-engine.md)). The app's server keeps the routes, the control plane's lock, the `music2-feeds` thread and Keep, which writes a Tx through `m2-editor`. It already depends only on `m2_runtime` (`NotesData`, `LaneSwap::Feed`, `FEED`) and `m2_song` for the part that moves.

On the web, the feed logic runs on the **page's thread**, and the engine in its Worker, as the app splits them (the server holds feeds, the engine plays):
- a put builds the feed's `NotesData` off the engine's thread and sends it as `LaneSwap::Feed`, as FEED-2 says;
- the Worker tells the page its playhead as it fills the ring, and the page's look runs every 50 ms, as the app's thread does;
- when the watchdog stops a Worker ([section 7](#7-anyones-pack-and-the-time-limit)), no feed is lost: the page sends each feed to the new engine again, as the app sends a feed to each new plan.

### What differs on the web

| | In the app | In a page |
|---|---|---|
| The lock's margin | two of the device's buffers, at least 50 ms | two of the ring's lengths (the Worker's blocks ahead of the worklet), at least 50 ms |
| Who opens a feed | the musician's track menu, or an agent | the page (`song.feed`), or its agent through the relay |
| The writer's name | `as`, from the tools key's caller ([AGENT-10](../../architecture.md#agents)) | `as` in the body, as the page passes it on: a page has no keys |
| The Live mark | on the track's header | the page draws what it wants from the events |
| Keep what played | one undo step in the song ([FEED-7](../../architecture.md#feeds)) | `feed.played()` gives the notes, as a lane file writes them; the page keeps them where it likes |

## 7. Anyone's pack, and the time limit

In the app, a stranger's pack runs with a deadline on every call ([ABI-6](../../architecture.md#kernel-abi-reservations)), and only music2's own packs and std run without ([ABI-11](../../architecture.md#kernel-abi-reservations)). A browser has no epochs, and nothing can stop a call on the worklet's thread. A page with a kernel stuck in a loop would hang its engine for good.

So, until the watchdog is built:
- `m2 export --web` refuses a song with any pack but std and music2's own ([section 4](#at-export)).
- `@music2/web` decides again from the bytes, as the app does: a pack is music2's own when its listing names the very `m2pkg.toml` and module it read, and `m2_song::pack::own` names that listing's hash (`core/song/src/pack/bundled.rs`, from `packs/versions.toml`, built into the engine). A manifest can't say "trust me".

Then track `web-watchdog` builds [web-engine.md](web-engine.md)'s plan: the engine marks "in a call into node X" in a `SharedArrayBuffer`; the page's thread stops a Worker past its limit (`terminate()`), starts a new one without that sound, sends it the feeds, seeks it to where it was, and tells `fault` in the words of `sound.too-slow` ([ABI-7](../../architecture.md#kernel-abi-reservations)). Only a cross-origin isolated page has that shared mark; a page without the headers plays only std and music2's own packs. The cost: the whole song stops for a moment, where the app silences one sound.

## 8. Later

| Track | What | Needs |
|---|---|---|
| `web-code` | song code in the page: the same `m2-script`, QuickJS and `oxc` built to WebAssembly ([web-engine.md](web-engine.md), decision 4). A bundle may carry `code/` (the song's and its packs' TypeScript) beside `ir.json`; a page that loads `@music2/web/code` can change settings and code, and rebuild | its test: each bundle's code, built in Chrome, gives its `ir.json` byte for byte |
| `web-library` | a bundle names a pack by name, version and hash instead of carrying it; `@music2/web` fetches it from the library's URL, only when the page names that URL | the library live (R2) |
| `web-samples` | a pack's smaller web set of samples, and samples loaded as they are needed, not all at open ([design.md](design.md), piece 4) | a pack that needs it (the grand piano) |
| `app-export` | the app's Export menu gets "For a web page…", as `m2 export --web` | the owner's yes on the menu |
| publish | `@music2/web` on npm | the owner's call |

## Decisions (accepted, 2026-11-28)

1. **`@music2/web` is an npm package (an ES module with TypeScript types) for developers: the engine, not the app's GUI.** Open, play, stop, seek, loop, set a knob, render, events, and feeds. It calls no model, and fetches only the bundle's URL and the files under it (NET-2 on the web). It is built and tested in the repo, in `core/web`; publishing to npm is the owner's call, later.
   - *Why:* developers want a song in their own page, with their own look. The app is where songs are made.
   - *The cost:* a public API to keep stable, once published. Before 1.0 it changes as the song format does ([goals](../../goals.md)).
   - *The other choice:* a ready player (a `<music2-player>` element with buttons). It can be built on top later; the engine can't be got out of a player.
2. **Songs reach a page as a web bundle, written by a new `m2 export <song> --web <dir>`**: the IR, the song's data files as they are, the assets and package files it plays, and the kernels of the packs it plays, with a manifest of sizes and hashes. No song code and no library in Part 3. Song code in the page comes after, as its own track (`web-code`); packs from the library once it is live (`web-library`).
   - *Why:* the engine needs only the IR, the data and the kernels (`Renderer::new`, read from the code above). A static host serves a bundle. Part 3 doesn't wait for R2, nor for QuickJS in WebAssembly, which isn't linked yet ([web-engine.md](web-engine.md)).
   - *The cost:* a bundle can't change settings or code; only cells. A bundle carries std and packs again for each song (the cache shares them within a site).
   - *The other choice:* the page runs the song's code from its folder. That needs QuickJS in the page (3–6 MB, estimated) and every pack's TypeScript, for every page, even one that only plays.
3. **The same bits:** a bundle played or rendered in Chrome gives the app's render bit for bit (RENDER-33), checked by a browser suite against `e2e/renders.sha256`, each from its song's bundle.
   - *Why:* "professional, not a toy" ([design.md](design.md)) means the page sounds like the export. Part 2 measured that the kernels give the same bits in a Worker.
   - *The cost:* a browser suite for every pull request that touches the engine, the bundle or `@music2/web`.
   - *The other choice:* "sounds the same", by a loudness or spectrum bound. Weaker, and it would hide a real difference.
4. **Feeds in `@music2/web` speak `feed/1`: the app's feed bodies, answers and events (FEED-4, FEED-5), written up in the guide.** The page owns its agent: `@music2/web` gives the page open, put, status, close and the events (low, empty, jump), and a relay that answers a message as the app would. No server in music2 for this.
   - *Why:* an agent's loop works against the app or a page with the same messages. music2 has no AI in it ([AGENT-9](../../architecture.md#agents)), on the web too: the page chooses its agent, its model and its transport.
   - *The cost:* `feed/1` becomes a promise: a change to a feed body is a change to the protocol, said in the guide.
   - *The other choice:* a music2 relay server that pages and agents both connect to. It would be a service to run, and a place where the person's music passes through someone else's computer.
5. **A bundle may hold only std and music2's own packs until the Worker watchdog is built** ([web-engine.md](web-engine.md), decision 6). The engine decides from the bytes, as ABI-11 does in the app.
   - *Why:* a pack is someone else's code in the page. Without a time limit, one loop hangs the engine for good.
   - *The cost:* a song with a stranger's pack can't go on the web until track `web-watchdog`.
   - *The other choice:* let it play, unlimited, behind a flag the page sets. A page would turn it on to make a song work, and its listeners would bear the risk.
6. **The feed logic moves from `crates/server` into a core crate the app and `@music2/web` both use.** On the web it runs on the page's thread; the engine in its Worker. (Added here: it is what decision 4's "the same messages" needs in the code.)
   - *Why:* one implementation of the lock, `late`, low, empty and jump, so the answers can't drift. On the page's thread, a watchdog's restart loses no feed.
   - *The cost:* a refactor of a part just built, with its 13 server tests and the `live` suite as the proof that nothing moved.
   - *The other choice:* a TypeScript copy, tested against the app by the same fixtures. Faster to write, and two to keep.

## Questions, and what was decided

Where the code said something other than the plan, or the plan left a call open. Each was settled on 2026-11-28: the npm name by the owner, the rest by the lead under the owner's go-ahead ("go ahead and work on part 2, 3, and 4 if you think it's right").

1. **The npm name: `@music2/web`** (the owner's pick). The npm scope `@m2` belongs to an organisation music2 doesn't hold (the registry answered `GET /-/org/m2/package` with one package, 2026-11-28), and inside music2 `@m2/…` names music2's own packs (`@m2/drums`), which this isn't. `@music2` was free that day. Publishing stays the owner's call, later.
2. **NET-2 and NET-4 in a page: a web rule beside them, NET-7.** A browser's `fetch` can't set `User-Agent` or `Accept-Encoding`, and it decodes a compressed answer before the page sees it, so a page can't refuse one as NET-4 says, and the size it counts isn't `Content-Length`. NET-7: a GET with no cookie and no referrer; sizes counted on the decoded bytes; the hash checked on the decoded bytes, which is what matters. Edited in the contract in track `web-net`.
3. **The command is `m2 export <song> --web <dir>`.** There is no `m2 export` today (the app's Export is the page's, and `m2 render` writes the song's WAV); `export` is what the app's menu calls it, and it leaves room for the other exports. Its only form at first is `--web`.
4. **Keep stays the app's.** FEED-7 writes what played as an undo step, and a page has no song files. `feed/1` leaves Keep out, and a page gets `played()`, the notes, to keep as it likes.
5. **std's hashes are recorded.** ABI-11 trusts std because the host loads its own; in a page, std comes from the bundle. So `packs/versions.toml`'s record gains std's versions and hashes (as PACK-17's does for packs), and the engine trusts a bundle's std only when its hash is one of them. Track `bundle` adds the record, with a test that a changed std module is refused.
6. **Samples in a public bundle: export warns, it doesn't refuse.** A bundle gives every sample file to anyone who opens the page, and many sample licences allow using a sample in music but not handing out the file. Export says so, with each sample's credit, and the manifest carries the credits. The packs design checks no licence against a list (D6), and a refusal would block a musician's own recordings, so this note doesn't either.

## Risks

| Risk | What would show it | Measured? |
|---|---|---|
| Firefox and Safari: their Workers' floats, their AudioWorklets, `AudioContext({ sampleRate })` | the bits suite, run in each | no: Part 2 measured Chrome 133 and Node 24 only |
| File sizes: a song with many samples is tens of MB; a phone on a slow network waits | the bundle's size, printed at export; the `progress` callback | sizes for five songs (above); no network timing |
| CORS: a bundle on another site than the page needs `Access-Control-Allow-Origin`, and the failure has no detail | the loader's suite, with a second local origin | no |
| Cross-origin isolation: the shared ring and the watchdog need COOP and COEP; COEP blocks any cross-site file without CORS or CORP, which may break the rest of a page | the `MessagePort` path's latency and underruns; a page without the headers in the suite | no |
| A page's strict Content-Security-Policy needs `'wasm-unsafe-eval'` to compile WebAssembly, and `worker-src` for the Worker | a suite page with a strict policy | no |
| `@music2/web` loaded from a CDN on another site: a Worker can't start from a script on another site | the package's own files served cross-site in the suite | no |
| Memory: one WebAssembly memory per sandbox; the starter has up to 396 kernels in all voices ([web-engine.md](web-engine.md)) | the starter played on a phone | in Node only (Part 2) |
| Warped clips are stretched at open: a long audio clip may make `open` slow | `open`'s time for `examples/resample` | no |
| Samples' licences: a bundle hands out the files | the export's warning; [Questions](#questions-and-what-was-decided), 6 | — |
| Page jank delays the feed's 50 ms look, so `low` comes late | the web feeds suite, with a busy page thread | no |
| A browser changes how it fetches, caches or computes | the suites, on every pull request | — |

## The build, in small tracks

Each track is one pull request with its own test, written first. Each track's contract edit comes first, in its own commit: a new section of [architecture.md](../../architecture.md), "The web", with rules WEB-1 on, and NET-7 ([Questions](#questions-and-what-was-decided), 2).

| Track | What | Its test | Needs |
|---|---|---|---|
| `bundle` | `m2 export <song> --web <dir>`: the folder, the manifest, `controls.json`, the refusals at export, the licence warning; the guide's commands page made again | exporting the starter writes the layout above, each hash right; twice gives the same bytes; a plugin song and a stranger's pack are refused in those words | the owner's yes. Not Part 2: it is app code |
| `feed-protocol` | `docs/guide/feed-protocol.md`; a fixture of `feed/1` acts and the answers' fields; a Rust test that runs it against the app's routes | the fixture against the app: every answer has its fields and its refusal words | — |
| `feed-core` | the feed logic out of `crates/server/src/graph/feeds.rs` into a core crate, the clock from the caller | every feeds test and the `live` suite unchanged; the crate builds for `wasm32-unknown-unknown` | Part 2's `runtime-web` |
| `web-net` | NET-7; `@music2/web`'s loader: the manifest, paths, limits, hashes, the cache | a suite on a local server: a changed byte, a path with `..`, a file over its size and a wrong `integrity` are each refused in words; a compressed answer is hashed after decoding; no cookie or referrer is sent; a second open fetches nothing | `bundle`, Part 2's `web-node` |
| `web-player` | `@music2/web`'s API: open, play, stop, seek, loop, set, render, the events; the package in `core/web` | a browser suite: the starter's bundle opens, plays 30 s through the worklet, a `set` of a cell is heard at the next block, a setting is refused in words, `play()` outside a click is refused | `web-net`, Part 2's `web-chrome` |
| `web-bits` | the proof of [section 5](#5-the-same-bits) | the 26 renders from the 13 bundles match `e2e/renders.sha256` in Chrome; live from beat 0 matches the render | `web-player` |
| `web-feeds` | `song.feed`, the relay, the events, `played()`; the feed logic on the page's thread | the `feed/1` fixture through the relay gives the app's answers; a put lands on its beat in the Worker's output; `low`, `empty` and `jump` each come once | `feed-core`, `feed-protocol`, `web-player` |
| `web-watchdog` | the Worker watchdog; then a bundle may hold anyone's pack | a pack whose kernel loops for ever: the page's thread is never late, the song goes on without that sound, `fault` names it in ABI-7's words, the feeds play on | `web-player`, a cross-origin isolated page |
| later | `web-code`, `web-library`, `web-samples`, `app-export`, publish | [section 8](#8-later) | their own needs |

`bundle` and `feed-protocol` can start now, beside Part 2's tracks.
