**FilmCraft is a command-driven editing engine with several frontends, built on top of its own media-processing stack.** The GUI is one client of the engine; the CLI, MCP server, and browser app reuse the same core. Most codecs, timeline operations, rendering, and audio processing live in reusable Rust crates. [^architecture][^engine]

The architecture below refers to the same revision I inspected earlier: `7b6c134`.

## 1. Overall structure

The repository explicitly assigns crates to **seven dependency layers**. Higher layers can depend on lower layers; same-layer dependencies require an explicit exception. A build tool checks these rules rather than leaving them as documentation. [^layers]

| Layer | Main components | Responsibility |
|---|---|---|
| **L6 — Applications** | `apps/filmcraft`, `filmcraft-cli`, `filmcraft-web` | Desktop executable, command-line application, browser application |
| **L5 — Frontends and adapters** | `ui-egui`, `automation`, `platform` | Editor UI, MCP integration, native hardware-codec integration |
| **L4 — Engine** | `engine` | Session, command dispatch, undo history, media management, jobs |
| **L3 — Processing and output** | `render`, `gpu`, `export`, `scopes`, `format` | CPU/GPU rendering, export, scopes, project-file support |
| **L2 — Editing and integration** | `edit`, `codecs`, `interchange`, `captions`, `speech` | Timeline operations, media-opening/decoding orchestration, interchange, captions and transcription |
| **L1 — Core models** | `project`, `media`, `frame`, `audio-dsp`, `text` | Project data, source/frame interfaces, audio DSP, text layout |
| **L0 — Foundations** | `time`, `geom`, `color`, `bitstream`, individual codecs and containers | Time mathematics, geometry, color, bitstreams, format implementations |

These are the implemented groups, not every name reserved in its layer checker. For example, separate `playback`, `effects`, and `audio` crates are planned names; much of that functionality currently lives in `ui-egui`, `project`, and `render`. [^architecture][^layers]

## 2. The engine: commands operating on a session

**The central abstraction is `filmcraft_engine::Session`.**

It holds the current project, editing state, history, media pool, preferences, jobs, and related runtime state. Project-changing operations enter through:

```rust
Session::execute(command_id, json_parameters)
```

Commands have stable names such as `sequence.addEdit`, `timeline.trim`, and `file.exportMedia`. Each command definition contains its label, menu location, optional shortcut, parameter documentation, availability check, and execution function. [^engine][^architecture]

Conceptually, the path is:

```text
Menu / shortcut / CLI / agent
             │
             ▼
    Resolve command definition
             │
             ▼
    Check whether it is enabled
             │
             ▼
       Run the operation
             │
             ▼
 Update project/history, record command, emit events
```

The engine emits events such as `ProjectChanged`, `OpenSequence`, and `Toast`, which frontends consume. Read-only inspection commands use the same registry. **UI-only operations are an exception:** workspace switching, panel layout, zoom, and playback controls also have a separate UI command table. The menu dispatcher routes to either that table or the engine. [^architecture]

### Undo is snapshot-based

The project is an `Arc<Project>`, with copy-on-write editing and structural sharing. An edit produces a new project state; the previous snapshot goes into history. Undo and redo swap snapshots rather than reconstructing the project by replaying inverse operations. The inspected implementation initializes a **200-entry history limit**. [^engine][^architecture]

This also lets rendering and export hold a stable project snapshot while editing continues. That does **not** mean the entire application is lock-free—media caches and other runtime systems still have their own synchronization. [^architecture]

## 3. The project model: media references plus timeline instructions

The project model separates a **source asset** from an **instance of that asset on a timeline**:

```text
Project
├── Bins
└── Project items
    ├── Media assets
    ├── Sequences
    ├── Subclips
    ├── Graphics
    └── Adjustment layers

Sequence
├── Sequence settings
├── Video tracks
├── Audio tracks and submixes
└── Markers / In / Out

Track
├── Clip instances
└── Transitions

Clip instance
├── Referenced project item
├── Timeline start
├── Source In point
├── Duration and speed
└── Effects, keyframes, masks
```

For example, two timeline clips can reference the same source video while using different source ranges, effects, and playback speeds. An ordinary trim changes timeline data rather than rewriting the source video. Sequence validation checks invariants such as unique IDs and non-overlapping items within a track. [^architecture]

**Time is integer-based.** Timeline positions use `Tick(i64)` at **254,016,000,000 ticks per second**, with rational frame rates. The chosen timebase exactly represents the supported common video-frame and audio-sample durations. Commands can accept seconds, frames, or timecode, but the engine converts them at the boundary. [^architecture]

Effects are also defined as data: parameter schemas drive the Effects panel, parameter controls, and documentation exposed to agents. Project files use JSON in `.fcproj` files; native saving uses an atomic write-and-replace operation. [^architecture][^engine]

## 4. Media ingestion and decoding

The input path separates **containers**, **codecs**, and the **common media-source interface**:

```text
File / random-access byte reader
              │
              ▼
Container reader: MP4, MOV, Matroska, MXF, etc.
              │
              ▼
      Compressed video/audio samples
              │
              ▼
       Selected codec implementation
       ├── Registered hardware decoder
       └── Built-in software decoder
              │
              ▼
        MediaSource interface
              │
              ▼
       Decoded-frame caches
```

The container crates understand file layout and sample indexing. Individual codec crates understand compressed bitstreams. The higher-level `codecs` crate connects them and handles opening files, selecting decoders, seeking, and audio decoding. [^architecture]

### The important boundary: `MediaSource`

A media source exposes operations resembling:

```text
video_frame(request)
audio(start, frame_count, sample_rate)
```

The engine’s `MediaPool` lazily opens and shares sources between monitors, thumbnails, playback, and export. Consumers therefore work with a common interface rather than knowing whether a frame came from H.264, a still image, a proxy, or another source type. [^architecture]

For inter-frame video, seeking goes to an earlier synchronization frame and decodes forward. The GOP-aware cache retains decoded frames, while sequential playback reuses the decoder. Frame caches have byte budgets rather than growing without bounds. [^architecture]

Proxies are handled at the source-provider level. Playback can substitute a lower-resolution source, while export explicitly requests the full-resolution provider. Missing media can produce an offline placeholder instead of breaking the whole render. [^architecture]

## 5. Video rendering: evaluate the timeline, then execute it

FilmCraft separates **what should appear at a particular time** from **how to calculate those pixels**.

At timeline time `t`, the renderer determines the active clips, maps timeline time into each clip’s source time, retrieves frames, evaluates effects and keyframes, and applies transforms, opacity, transitions, and compositing. [^architecture]

There are two execution paths:

| Path | Implementation | Role |
|---|---|---|
| **CPU** | `render::render_sequence` | Reference rendering implementation; also used by export |
| **GPU** | `render::plan::plan_frame` → `gpu::GpuCompositor` | Accelerated preview rendering using a frame plan |

The GPU plan contains decoded image layers and their transforms, opacity, blend modes, and supported effect parameters. Features that the GPU path cannot handle are pre-rendered on the CPU and supplied as images. [^architecture][^export-pipeline]

### GPU execution

The `gpu` crate uses **wgpu and WGSL shaders**. Decoded YUV planes can be uploaded separately, with conversion performed in shaders. Compositing uses linear-light, premultiplied-alpha representations; supported effects run as compute passes. CPU/GPU comparison tests are part of the project’s correctness strategy. [^gpu][^architecture]

### Playback scheduling

On desktop, `ui-egui/src/frames.rs` manages worker jobs with priorities:

```text
Current monitor frame → playback prefetch → thumbnails
```

The UI does not decode frames synchronously. It displays a cached frame while waiting, cancels obsolete work during scrubbing, and prefetches upcoming frames during playback. [^architecture]

**A significant architectural detail:** playback scheduling currently belongs to the UI crate, not a standalone headless playback engine. Rendering is reusable below it, but the interactive playback coordinator is still frontend-side. [^architecture]

## 6. Audio: a mixer graph, not just attached soundtracks

Audio has its own processing graph:

```text
Source audio
    │
    ▼
Clip gain, effects, volume/pan automation, transitions
    │
    ▼
Track strips: inserts, fader, sends, pan
    │
    ▼
Submix buses
    │
    ▼
Master / Mix bus
    │
    ├── Playback output
    └── Export audio
```

`audio-dsp` implements the signal-processing primitives. `render::audio` and `render::mixer` assemble them into clip processing and a routing graph. The graph supports automation, effect state, latency compensation, multichannel routing, and pre-/post-fader processing. [^architecture]

The implementation evaluates mixer lanes per sample and effect parameters on a fixed sample grid, aiming to make output independent of how playback callbacks or export batches divide the audio into requests. Sequential readers retain DSP state; non-sequential reads use pre-roll to establish it. [^architecture]

**Audio is the playback master clock.** On desktop, the sound output’s played-sample count drives the playhead, and video follows. Without an audio device, playback falls back to a wall clock. Native output uses `cpal`; browser output uses WebAudio. [^architecture][^web]

## 7. Export: a separate render–encode–mux pipeline

Export snapshots the project and resolved settings, then runs a job:

```text
Project snapshot + export settings
                │
                ▼
      Render at output frame times
                │
                ▼
 Resize / fit / overlays / output-color conversion
                │
                ▼
       Video encoder + audio encoder
                │
                ▼
          Container writer
                │
                ▼
            Output file
```

The shared per-frame export pipeline explicitly calls `filmcraft_render::render_sequence`. **That is the CPU renderer—not the GPU preview compositor.** Hardware video decoding and hardware encoding can still participate before and after this stage. [^export-pipeline][^architecture]

Native export overlaps rendering the next batch with encoding and muxing the current batch. Rendering uses Rayon parallelism, while encoder/container ownership remains on the calling thread. Memory limits constrain the number of in-flight frames. [^export-job]

The `Exporter::step()` interface is particularly useful: it advances a bounded batch of work. Desktop code can loop over it on a worker thread, while the browser can advance it cooperatively between UI frames. Jobs expose progress and cancellation, and the export queue stores project/settings snapshots for queued items. [^export-job][^architecture]

## 8. Platform integration and the browser build

The core receives host capabilities through interfaces such as `Services`, which provides file reads, writes, random-access readers, and directory access. Native code supplies filesystem services; the browser supplies a virtual file table and Blob-backed readers. [^engine][^web]

Hardware codecs are another adapter boundary. `platform` registers implementations behind the existing decoder and encoder interfaces:

```text
macOS   → VideoToolbox
Windows → Media Foundation / DXVA, NVENC
Linux   → VA-API, NVENC
```

The lower-level editing and rendering code does not need to directly call those native APIs. Also, `platform` is specifically the OS **media** integration crate; native audio, menus, and file dialogs are still wired up in the desktop application. [^platform][^architecture]

### The web app is the same Rust UI, compiled to WASM

It is not a separately implemented React editor:

| Desktop | Browser equivalent |
|---|---|
| egui/eframe native window | egui/eframe web runner |
| Native GPU backend | WebGPU, with WebGL2 fallback |
| Filesystem media readers | Blob-backed virtual files |
| Native audio output | WebAudio `AudioWorklet` |
| Native codec integration | WebCodecs |
| Native recovery storage | OPFS browser storage |
| TCP control channel | `window.filmcraft` JavaScript API |

The documented browser application is a static deployment and processes imported media locally. Its current WASM application build is single-threaded: rendering, software decoding, mixing, and encoding are scheduled cooperatively. Browser audio output uses its worklet separately. On the WebGL2 fallback, FilmCraft’s video compositor uses the CPU. [^web]

## 9. Agent integration

The agent architecture follows directly from the command-oriented engine.

**Headless MCP mode** creates an in-process `Session`. An agent can inspect a project, execute editing commands, and render frames without opening a window.

**Bridge MCP mode** forwards to a running desktop application over a JSON-lines control connection. This adds live UI inspection, screenshots, clicks, drags, and keyboard input. Interactive widgets expose stable automation IDs rather than requiring only coordinate-based interaction. [^automation][^architecture]

The browser exposes the corresponding control surface through functions such as:

```javascript
await filmcraft.execute(command, params);
await filmcraft.request(method, params);
await filmcraft.commands();
await filmcraft.inspect();
```

So an external agent can operate the editor at the semantic level—“execute this editing command”—while retaining a UI-control route for frontend-specific interactions. [^web][^automation]

---

### My architectural assessment

The strongest design choice is **separating editing intent from the UI**: commands, serializable project data, snapshots, and reusable render interfaces make the application scriptable and portable.

The main boundaries to watch are that **interactive playback is still UI-owned, export compositing is CPU-based, and hardware decoding is not a zero-copy path into the GPU compositor**. Those are meaningful constraints despite the broad hardware support. The browser also shares the codebase without yet sharing desktop-style parallel execution. [^architecture][^export-pipeline][^web]

This is a source-level architectural review; I have not built the application or run its tests.

---

## Sources

[^architecture]: [Architecture documentation — `docs/architecture.md`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/docs/architecture.md). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^engine]: [Engine façade and Session implementation — `crates/engine/src/lib.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/engine/src/lib.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^layers]: [Workspace automation and dependency-layer enforcement — `xtask/src/main.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/xtask/src/main.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^export-pipeline]: [Per-frame export pipeline — `crates/export/src/pipeline.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/export/src/pipeline.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^gpu]: [GPU compositor implementation — `crates/gpu/src/lib.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/gpu/src/lib.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^web]: [Browser architecture and integration documentation — `docs/web.md`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/docs/web.md). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^export-job]: [Stepped export, batching, and render/encode overlap — `crates/export/src/job.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/export/src/job.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^platform]: [Native hardware-codec registration — `crates/platform/src/lib.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/platform/src/lib.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.

[^automation]: [MCP automation modes and interfaces — `crates/automation/src/lib.rs`](https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/automation/src/lib.rs). Revision `7b6c134472287db4367fca80bcb498240ba36567`.
