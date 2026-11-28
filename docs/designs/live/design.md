# Live feeds, and music2 on the web

**In short**
- A **feed** is a track's notes written by someone outside the app **ahead of the playhead**, the way a video player buffers ahead. An agent keeps the feed full; music2 plays it and says how full it is.
- music2 has no AI in it ([AGENT-9](../../architecture.md#agents)). It only plays, reports the buffer, and refuses a write that comes too late. The agent decides what to play.
- The engine change is small: a feed is a second set of notes on a notes lane, swapped in as an edit's notes are ([RENDER-26](../../architecture.md#plans-data-and-arrangement-swaps)), and never written to the song.
- Later, the same engine runs **in a web browser**: the same Rust, built to WebAssembly, playing the same kernels, so a song sounds the same, bit for bit, in the app and on a web page. Feeds work there with the same messages.
- Status: accepted by the owner, 2026-11-28, with all three decisions as proposed ("Yes, yes, yes"). Part 1 (feeds in the app) is designed here in full. Parts 2 to 4 (the web) are sketched; each gets its own note before it is built. The owner first asked for both: "agents should play live music… a buffer… like video playback", and "the core should serve on the web… professional, not a toy… open source".

## Why

Agents are slow and uneven. A model takes 5 to 20 seconds to answer, sometimes more. Audio can't wait: a block is due every few milliseconds. So an agent can't play notes "now". It must write them **ahead**, and the player must tell it when to write more.

Video streaming solved the same problem: the player keeps a buffer, plays from it, and asks for more before it runs out. Musicians know the idea from loopers and from Ableton Link's "start on the next bar". A feed is that idea for notes.

## Part 1: feeds

### The words

```
     playhead     lock             written to
        |--------|--------------------|
 played    final   buffered: can still   empty
                   be rewritten
```

| Word | Meaning |
|---|---|
| **Feed** | One track's notes lane, opened for live writing. It has an id (its track's key, as `bass`) and belongs to the open song's session. |
| **Playhead** | Where playback is, in beats of the song (the transport), as `m2 focus` says it. |
| **Lock** | The first beat that can still change. Before it, notes are on their way to the engine and are final. It is the playhead plus a small margin: two of the device's blocks, and at least 50 ms. |
| **Written to** | The end of what the feed holds. |
| **Buffer** | Written to minus the playhead, said in beats, in bars and in seconds at the tempo there. Beats are the unit: a tempo change doesn't spoil what was written. |
| **Low mark** | A buffer size, in beats, set when the feed opens (default: 4 bars). When the buffer drops below it, music2 says `low`, once, until it is above it again. |
| **Empty** | The playhead reached written to: the feed rests (plays nothing) until more comes, and music2 says `empty`, counting it. |

### What a feed plays

- Its notes play **with** the track's clips, through the track's sound, effects, fader, mute and solo, as the clips' notes do. To have the feed alone, use an empty track.
- Its notes are in song beats, sample-accurate: a note at beat 17 sounds on beat 17, not "when it arrived".
- Its notes never reach the song's files, an undo step, an export, a bounce or `m2 listen` (as an overlay doesn't, [MON-4](../../architecture.md#monitoring-overlays-and-mute-in-song)), unless the musician keeps them ([Keeping a jam](#keeping-a-jam)).

### Writing

One act, **put**: the notes from beat `from` to beat `to` are these. It replaces whatever the feed held in that range, so writing ahead and rewriting the future are the same act.

```json
{"feed": "bass", "from": 64, "to": 80,
 "notes": [{"pitch": "A1", "start": 64, "length": 0.5, "velocity": 0.9}]}
```

| Case | What happens |
|---|---|
| `from` is at or after the lock | Taken whole |
| `from` is before the lock | The part from the lock on is taken. Notes that start before the lock are left out, and the answer counts them (`late`). A note already sounding is never cut by a put |
| `to` is more than 64 bars past the playhead | Refused, in words: a feed is a buffer, not a song |
| The song isn't playing | Taken: the feed waits for Play |

A note is the lane format's note ([lane files](../../architecture.md#lane-files-and-clip-ids)), with `start` in song beats (not from a clip's start).

The answer says where things stand:

```json
{"ok": true, "taken": 31, "late": 0, "lock": 52.1, "written_to": 80, "playhead": 51.9}
```

### Hearing how full it is

Each feed's state is part of the events the app already sends (`GET /api/ws`, `m2 watch`):

```json
{"t": "feed", "feed": "bass", "playhead": 51.9, "written_to": 80,
 "buffer": {"beats": 28.1, "bars": 7.0, "seconds": 14.1}, "empties": 0}
```

- Sent each time the buffer crosses the low mark (`"why": "low"`), when it runs out (`"why": "empty"`), at a jump (`"why": "jump"`: Stop, a seek, a loop's wrap), and at most every quarter second while playing, as `transport` is today.
- `m2 feed wait` waits for one of these and prints it, so an agent in a terminal can loop on it without polling. Every wait has a time limit.

### A jump

A seek, a Stop or a loop's wrap moves the playhead. The feed keeps its notes: they are at song beats. So:

- Jump back, and what was written there plays again: a feed over a loop is a live loop the agent can rewrite ahead.
- Jump forward past written to, and the feed is empty until more comes.
- Either way the feed says `jump`, so the agent can re-plan.

Notes more than 16 bars behind the playhead are dropped, so a long jam stays small. Each dropped note is still in [what played](#keeping-a-jam).

### Opening and closing

| Act | Who | What |
|---|---|---|
| **Open** a feed on a track | the musician (the track's menu: **Let an agent play this track**) or an agent (`m2 feed open`) | The track shows a **Live** mark with the writer's name (as "Claude"), while the feed is open |
| **Close** | either | The feed's notes end at once (each sounding note gets its Off); what played is kept for Keep until the song closes |

Play, Stop, seek and the loop stay the musician's ([AGENT-11](../../architecture.md#agents)). An agent can fill a feed, but only the musician starts the song. Mute, solo and the fader work on a feed as on any track, so the musician can always silence it.

### Keeping a jam

The feed remembers what **played**: each note from the moment it passed the lock, up to the last 30 minutes. **Keep what played** (on the Live mark, or `m2 feed keep`) writes it into the song as one undo step, as a recording would ([MON-14](../../architecture.md#monitoring-overlays-and-mute-in-song)):

- into a new clip over the bars it covers, or
- when a clip with versions lies under them, as a new version of that clip ([VER-3](../../architecture.md#a-clips-versions)), for the Playground.

### How the engine plays it

No new engine machinery. A feed is a second data source on the lane's scheduler ([RENDER-12](../../architecture.md#events)):

1. The server holds each feed's notes, in beats, off the audio thread.
2. A put builds the lane's feed data again (at most 64 bars ahead and 16 behind, so small) and sends it as a `SwapLane` of that lane's feed half ([RENDER-26](../../architecture.md#plans-data-and-arrangement-swaps)): prepared off the audio thread, swapped at a block line, with nothing allocated in the audio thread.
3. The scheduler plays the clips' notes and the feed's notes together, from the transport. Its reconcile on a swap ([RENDER-26a](../../architecture.md#plans-data-and-arrangement-swaps)) already ends a note that a rewrite removed.
4. The lock is what makes this safe: a swap lands within about 10 ms of the request (the p95 the [write path](../../architecture.md#writes) measures), so a note past the lock is never "already due" when its swap lands.

The feed's notes are not live notes ([RENDER-14](../../architecture.md#events)'s live bit): they are timed by the transport, as a clip's are.

### The commands and routes

Routes (each declared once, [AGENT-8](../../architecture.md#agents)), open to agents:

| Route | Does |
|---|---|
| `POST /api/feed/open` | `{"track", "low_bars"?}` |
| `POST /api/feed/put` | the put above |
| `GET /api/feed` | every open feed's state, as the event says it |
| `POST /api/feed/keep` | `{"feed", "as": "clip" \| "version"}` |
| `POST /api/feed/close` | `{"feed"}` |

And `m2 feed`, for agents in a terminal:

```
m2 feed <song> open <track> [--low <bars>]
m2 feed <song> put <track> <from> <to> <notes.json | ->
m2 feed <song> status [--json]
m2 feed <song> wait [<track>] [--for low|empty|jump] [--timeout <seconds>]
m2 feed <song> keep <track> [--as clip|version]
m2 feed <song> close <track>
```

Feeds need the app open (or, later, a web page): they are playback, and with the app closed nothing plays.

### An agent's loop

```
m2 feed . open bass --low 4
(the musician presses Play)
loop:
  m2 feed . wait bass --for low --timeout 60      # returns with the state
  write the next 4 to 8 bars, from written_to on, to fit what plays
  m2 feed . put bass <written_to> <written_to+16> -
```

The song's `AGENTS.md` and the guide get a short "Playing live" section with this loop, the latency to plan for (a bar at 120 BPM is 2 seconds), and the rule: write ahead, rewrite only past the lock.

### Tests (each first, and seen to fail)

- A put lands on its beats, sample-accurate: a rendered capture of the live engine has each note's onset on its frame.
- A put before the lock takes only what is past it, and counts `late`; a sounding note is never cut by a put.
- A rewrite past the lock replaces what was there; a removed note that sounded gets its Off.
- Low, empty and jump are each sent once, at the right moment; `m2 feed wait` returns on them and times out.
- A feed never reaches an export, a bounce, `m2 listen` or a file; Keep makes one undo step with exactly what played.
- Mute and solo silence a feed. Close ends its notes.
- A browser suite: the Live mark shows the writer, Keep and Close work, nothing freezes while a feed plays.

## Parts 2 to 4: music2 on the web (sketch)

### The aim

A developer adds music2 to a web page and plays a real song in it: the same sounds as the app, the same mix, the same bits. Not a toy synth. And an agent can play live into it, with the same feed messages.

### The plan: the same engine, not a copy

| Part | In the app | In a browser |
|---|---|---|
| Engine (`core/runtime`, `core/play`'s engine) | Rust | the same Rust, built for `wasm32`, run in an AudioWorklet (the browser's audio thread) |
| Sound kernels | WebAssembly, run by `wasmtime` | the same `.wasm` files, run by the browser's own WebAssembly |
| The song's code | TypeScript in QuickJS, giving the IR | the page's JavaScript, with the same `@m2/core`, giving the same IR; the same Rust checker reads it |
| Packs and samples | from disk | from the library over HTTPS (needs the library live: the R2 step) |
| CLAP plugins | yes | no: a browser can't load them, and the song says so in words |
| Several cores | the pool | one core, unless the page is cross-origin isolated (two HTTP headers), then workers |

**The proof:** the 26 renders of `e2e/renders.sha256`, rendered in headless Chrome, must match the app's bits. WebAssembly's floats are exact except for NaN's bits and the relaxed SIMD ops; the kernels already avoid both, or this test will say where they don't.

### The pieces to build

1. **One kernel host interface** in `core/runtime`, with two backends: `wasmtime`, and the browser's WebAssembly. Today `wasmtime` is named in seven of the runtime's files; they move behind it. The time limit on a kernel ([the limit](../../architecture.md#kernel-abi-reservations)) needs a browser form (the browser has no epoch interrupt: a watchdog on the worklet's block time instead).
2. **The web as a platform** in `core/os` ([PLAT-2](../../architecture.md#platforms)): no files, no threads by default, the clock and the audio device from the AudioWorklet.
3. **`@m2/web`**, an npm package: open a song folder (or its IR and lane files) by URL, play, stop, seek, set a knob, and feeds:

   ```js
   import { open } from "@m2/web";
   const song = await open("https://example.com/my-song/");
   await song.play();
   const bass = song.feed("bass", { lowBars: 4 });
   bass.on("low", async (s) => bass.put(s.writtenTo, s.writtenTo + 16, await nextBars()));
   ```

   The page owns its agent: `@m2/web` calls no model and sends nothing anywhere but the song's and the library's URLs ([NET-2](../../architecture.md#network) holds on the web too).
4. **Big samples on the web:** a pack can name a smaller "web" set of its samples, and the Sampler loads samples as they are needed, not all at open. The grand piano (260 MB) needs this.

### What the web leaves out at first

Recording, the code editor, plugins, the Playground: the web player plays songs and feeds. The app stays where songs are made.

## Open source

Everything here is MIT OR Apache-2.0, as music2 is: the feed messages, written up in the guide as a protocol of their own (`feed/1`) so any player can speak it, and `@m2/web` on npm.

## Decisions (the owner said yes to each, 2026-11-28)

1. **May an agent open a feed itself, or only the musician?** Either, because the Live mark shows it at once, mute silences it, and nothing sounds until the musician presses Play. This widens [AGENT-11](../../architecture.md#agents) a little: today "what a MIDI keyboard would send" is only the musician's.
2. **A feed plays with the track's clips**, rather than instead of them.
3. **Feed time is song beats**, so a feed works with the loop and the Playground. The other choice is a clock that never wraps, which suits endless jams but needs a map to the song's bars.

## Order of work

| Track | What | Needs |
|---|---|---|
| `feeds` | Part 1: the contract (a FEED family of rules), the scheduler's second source, the routes, `m2 feed`, events, the Live mark, Keep, the guide | the owner's yes |
| `web-engine` | Part 2: the kernel host interface, `core/os` for the web, the engine in an AudioWorklet, the renders matched in Chrome | its own note |
| `web-player` | Part 3: `@m2/web`, songs by URL, packs from the library, web sample sets | the library live (R2) |
| `web-feeds` | Part 4: feeds in `@m2/web`, the same messages | `feeds` and `web-player` |
