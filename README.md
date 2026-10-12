# music2

music2 is a music studio for the Mac: a DAW (digital audio workstation), the kind of app people make songs in, like GarageBand or Logic Pro. In music2 a song is a folder. Code in it says how the song sounds: the instruments, the effects and the mix. Timeline files say what plays: the notes, the automation (knob moves over time) and the audio.

The app shows that folder as a timeline, a graph of knobs and cables, a piano roll (a grid for drawing notes) and a code editor. Every change you make there is written into the files right away.

There's no AI inside the app. Scripts and AI agents (such as a Claude Code session) use the same files, and a local API (a way for programs on your Mac to talk to the app). music2 is part of **suite2**, alongside film2 and dj2. Status: early. A lot is missing: tracks are added in code, there are only a few instruments, and there's no mixer panel yet. See [What music2 can't do yet](docs/gaps.md).

## Run the app

You need a Mac, [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```bash
(cd web && npm install && npm run build)                        # the window's UI: once, and after UI changes
cargo run --release -p music2-desktop                           # the app, on its start screen
cargo run --release -p music2-desktop -- ~/Music/music2/Demo    # or opened on a song folder
```

The start screen has three ways in:

- **New Song…** makes a new song from a template and opens it (see [Your first song](#your-first-song)).
- **Open Project Folder…** opens a song you already have. A song folder is any folder with a `song.ts` file in it. File → Open Project Folder… (⌘O) does the same at any time.
- **Open an Example** opens a copy of one of the examples in your songs folder, so the example itself stays as it is.

Everything you change is written into the song's folder right away.

To work on the UI with hot reload:

```bash
cd web && npm run dev                                                 # the UI on :5173 (it passes /api on to :4100)
MUSIC2_DEV_URL=http://localhost:5173 cargo run -p music2-desktop      # the app, its server on :4100, the UI from Vite
```

To build `music2.app`:

```bash
(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --bundles app)   # → target/release/bundle/macos/music2.app
```

The app carries everything it needs in `music2.app/Contents/Resources`: the UI (`web/`), the standard sounds (`std/`: `m2pkg.toml`, `kernels/std.wasm` and `src/`), the song templates (`templates/`) and the examples (`examples/`). So it runs from anywhere, without this source folder. `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` point it at other copies; when it runs outside a bundle (`cargo run`), it uses the ones in this source folder.

The build signs the app ad hoc (with no developer certificate) and isn't notarized by Apple. On the Mac that built it, it just opens. A copy downloaded, AirDropped or sent in Messages to another Mac is held back the first time: macOS says it can't check the app. Open **System Settings → Privacy & Security** there and click **Open Anyway** (once). It runs on Apple silicon Macs only. The hardened runtime is off (`tauri.conf.json`): under it, macOS kills the app as soon as it compiles std's kernels. Notarizing will need it on, with entitlements that allow that (and loading other developers' plugins).

## Your first song

On the start screen, click **New Song…** (or use File → New Song…, ⌘N, at any time). Give the song a name, keep **Starter beat** selected, and press **Create** (or Enter). The song is made in `~/Music/music2/<its name>` and opens.

Press **Space** to play. It's an 8-bar pop beat at 116 BPM: kick, clap, hi-hats, bass, chords and a hook that comes in halfway.

Things to try:

1. **Turn a knob.** Below the timeline, click the **Song** tab. It shows the mix as boxes with knobs. Drag a knob while the song plays and you hear it change. Let go, and the new number is written into `song.ts`.
2. **Add a clip.** In the timeline, double-click the Bass **notes** lane past the end of the song. A one-bar clip appears.
3. **Draw notes.** Double-click the new clip. It opens below in the Notes view (a piano roll). Click the grid to add a note, drag a note to move it, and drag its right edge to make it longer.
4. **Add an effect.** Go back to the Graph view, click **+** on a box that makes sound, and pick an effect such as reverb.
5. **Undo.** Press ⌘Z to take these back, one at a time.
6. **Export it.** Click **Export** in the top bar (or File → Export Audio…, ⌘E). The song is written to a WAV file you can play anywhere or share; click **Show in Finder** when it's done.

## The window

### The top bar

From left to right: play/stop, back to the start, where the song is (bar.beat, then minutes and seconds), the song's name and revision (every accepted change is a new revision), **Export** (it shows how far along an export is), undo, redo, and close (✕), which goes back to the start screen.

### The timeline

- Each track has a row for each of its lanes: its notes, its automation and its audio (see [Words you'll see](#words-youll-see)). The ruler on top counts bars, and the song's tempo is shown at the top left.
- Click the ruler to move the playhead there.
- Double-click an empty spot in a notes lane to add a one-bar clip. Drag a clip to move it along its lane. Click a clip and press Delete to remove it.
- Double-click a notes clip to edit its notes.
- Automation and audio clips are shown, but they can't be edited in the app yet. Tempo changes appear on the ruler.

### The panel below the timeline

There's a tab for each module (one file's sound): one for each track file, and one for the Song (the mix). Drag the bar between the timeline and the panel to resize it. Each module has three views:

- **Graph:** the module as boxes and knobs.
  - Drag a knob to hear it; let go to write the number into the code. Double-click a knob to set it to the middle of its range.
  - A row that says **edit in code** is a value the app can't turn into a knob (see [Editing from the app](#editing-from-the-app)).
  - Cables: drag from a named output (the dot at the right edge of a box whose result has a `const` name in the code) onto a knob. The ✕ on a connected input disconnects it.
  - **+** on a box that makes sound opens the effect browser: 17 effects in seven groups (Filter, EQ, Dynamics, Distortion, Delay & Reverb, Modulation, Utility). Type to search, then press Enter or click. The bin icon on an effect removes it.
  - Zoom with ⌘ and scroll, or pinch. The buttons at the bottom right zoom and fit the graph to the panel.
- **Code:** the module's TypeScript, with type checks. The list on the left shows the song's files. ⌘-click a name to jump to where it's defined, even inside std (std's files open read-only). ⌘S (or the Save button) saves. When the code has an error, the last working version keeps playing, and a yellow banner offers **Show in code**.
- **Notes:** the open clip in a piano roll.
  - Pick a snap grid (1/4 to 1/32, or off). Click to draw a note, drag to move, drag the right edge to resize.
  - The strip at the bottom sets velocity (how hard each note is played).
  - Set the clip's length in bars at the top.
  - ⌘A selects all notes, ⌘D duplicates the selection, the arrow keys move it (with ⇧, by an octave or a bar), and Delete removes it.

## New songs, the songs folder and export

- **New Song…** (the start screen, or File → New Song…, ⌘N, even while a song is open) asks for a name and how the song starts:
  - **Starter beat**: six tracks (kick, clap, hats, bass, chords and a hook), 8 bars at 116 BPM, ready to play and change.
  - **Empty**: no tracks yet, so it plays silence. `song.ts` has a drum track ready, switched off: delete the `//` in front of its two lines and save, and a Drums lane appears.
- New songs go in the **songs folder**, `~/Music/music2`, each in a folder named after the song. If the name is taken, the new one gets a number ("My song 2"): nothing is ever replaced. `Exports` there is taken by exports, so a song called "Exports" becomes "Exports 2". In the app, **Choose Folder…** in the New Song dialog saves it somewhere else.
- **Open an Example** on the start screen copies an example into the songs folder and opens the copy. An example that needs plugins you don't have (With plugins) says which, and nothing is copied.
- **Export** (the button in the top bar, or File → Export Audio…, ⌘E) writes the song to a WAV file: 48 kHz, 24-bit, stereo. It starts at bar 1 and goes to the end of the last clip, then on until the sound has died away (silent for half a second, at most 10 seconds more). A song longer than 3 hours (most likely a clip far past the rest) isn't exported: the message says in which bar the last clip ends.
  - The app asks where to save it, starting at `~/Music/music2/Exports/<song>.wav`. Opened in a browser, it saves there straight away, adding " 2", " 3"… rather than replacing a file, even one that appeared while it was exporting.
  - The button shows how far along it is, and you can keep playing and editing meanwhile. It exports the song as it was when you clicked. If you close the song or open another, the export carries on, and a message in the corner shows how far along it is.
  - Quitting while a song is being exported asks first: quitting stops the export, and no file is saved.
  - The file is written under a hidden name and renamed when it's complete. A hidden file left by an export that was cut off (the app was killed) is removed by the next export into that folder.
  - When it's done, a message gives the file's name and length, with **Show in Finder** (in a browser, it says which folder).
  - `m2 render` writes the same file.

## Saving and undo

- There's no Save for the song: every change is written to the files at once. ⌘S only saves the code editor's text.
- Undo (⌘Z) and redo (⇧⌘Z) cover knobs, cables, effects, notes and code saves. The history ends when you close the app.
- Changes made in another editor, or by a script, show up within a moment.

## Shortcuts

| Key | What it does |
|---|---|
| Space | Play or stop |
| Enter or Home | Back to the start |
| ⌘Z / ⇧⌘Z | Undo / redo |
| ⌘N | New song |
| ⌘O | Open a song folder |
| ⌘S | Save the code editor's files |
| ⌘E | Export the song as a WAV file |
| ⌘D | Duplicate the selected notes |
| ⌘A | Select all notes in the open clip |
| Arrow keys | Move the selected notes (⇧: by an octave or a bar) |
| Delete | Remove the selected clip or notes |
| Escape | Deselect |

## Words you'll see

New to music itself? [Music basics](docs/music-basics.md) explains notes, beats, scales, synths, effects and mixing in plain words.

- **Track:** one part of the song, such as the bass or the drums. In music2 each track is a file in `tracks/`.
- **Lane:** a row of a track on the timeline. There are four kinds:
  - **notes**: notes for an instrument to play.
  - **automation** (a control lane): a value that changes over time, like a filter opening up.
  - **mask**: a 0-to-1 switch over time, which turns something on in some bars and off in others.
  - **audio**: recordings.
- **Clip:** a block on a lane, with a start and a length. A notes clip holds notes, an automation clip holds points, and an audio clip plays part of a recording.
- **Instrument:** something that turns notes into sound, such as `bass` or `kick`.
- **Effect:** something that changes a sound, such as reverb or delay.
- **Knob:** a number from the code that you can drag in the app.
- **Cable:** a connection that lets one signal control another, such as a slow wobble moving a filter.
- **Module:** one file's sound graph. Each track is a module, and so is the Song (the mix in `song.ts`).
- **std:** the standard package of instruments, effects and building blocks that comes with music2.
- **Kernel:** a small compiled sound-processing program (WebAssembly), such as an oscillator or a filter. Instruments and effects are built from kernels.
- **Render:** making the audio for the song, either to hear it or to write a WAV file.
- **Export:** rendering the whole song to a WAV file you can play anywhere or share.
- **Template:** what a new song starts from, such as the Starter beat.
- **Bounce:** rendering one track to an audio file in the song's `assets/`, often to chop it up again.

## How a song is written

The rest of this page is for writing songs as code, and for working on music2 itself.

### The folder

```
my-song/
  music2.toml          # the song's name
  song.ts              # the mix: which tracks play, how loud, and the master
  tracks/
    bass.ts            # one file per track: its lanes, instrument and effects
  timeline/
    song.json          # tempo and beats per bar
    bass.notes.json    # one file per lane: <track>.<lane>.json
    bass.cutoff.json
  assets/              # audio files (WAV) and saved plugin settings
```

`song.json` holds `tempo` (a list of `{ "beat", "bpm" }` changes) and `beats_per_bar`.

In a lane file's name, `<track>` is the track's name in lower case, with spaces and punctuation turned into `-`: a track called `Lead Vox` has `lead-vox.notes.json`.

### A track

This is a whole track, the shape of `tracks/hats.ts`:

```ts
import { track } from "@m2/core";
import { hat } from "std/instruments";

export default track("Hats", ({ lane }) => hat(lane.notes("hits")));
```

- `track("Hats", …)` makes a track called Hats.
- `lane.notes("hits")` is a notes lane called hits. Its clips are in `timeline/hats.hits.json`.
- `hat(…)` is the hi-hat instrument from std. It plays those notes.

### The mix

`song.ts` says what you hear. Here the bass ducks (dips in volume) each time the kick hits, which is called a sidechain. The numbers are only examples.

```ts
import { db, song } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import hats from "./tracks/hats";
import kick from "./tracks/kick";

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      hats: { in: hats.out, db: db(-8), pan: 0.2 }, // quieter, a little to the right
      bass: duck(bass.out, kick.env, { depth: db(-10) }),
    }),
  ),
);
```

`kick.env` is how loud the kick is right now. The kick track exports it next to its sound; see `examples/first-song/tracks/kick.ts`.

### Notes

```json
{
  "clips": [
    {
      "start": 0,
      "length": 4,
      "notes": [
        { "pitch": "F#2", "start": 0.5, "length": 0.25, "velocity": 0.9 },
        { "pitch": 42, "start": 1.5, "length": 0.25 }
      ]
    }
  ]
}
```

- `pitch` is a note name (`"F#2"`, `"Bb3"`) or a number (60 is middle C, so `42` is F#2).
- A clip's `start` and `length` are in beats from the start of the song. A note's `start` and `length` are in beats from the start of its clip.
- `velocity` goes from 0 to 1. Without it, a note gets 0.8.

Automation and mask clips hold `points` instead of notes: `[beat, value]` pairs, with straight lines in between. Audio clips name a file in `assets/` and can have `offset`, `gain_db`, `fade_in`, `fade_out`, `reverse`, `pitch` and `warp`; see `examples/resample/timeline/chops.clips.json`.

### Units

A number in the code can carry a unit, written as a function. Plugging a value with the wrong unit into an input is an error. A plain number fits any unit.

| Write | Means |
|---|---|
| `hz(440)`, `khz(2.5)` | a frequency |
| `db(-6)` | a level in decibels |
| `linear(0.5)` | a gain as a plain multiplier |
| `semis(12)` | semitones (12 is an octave) |
| `secs(0.3)`, `ms(30)` | a time |
| `beats(0.5)` | a length in beats |
| `frac(0.5)` | a fraction from 0 to 1 |
| `bipolar(-0.3)` | a value from -1 to 1, like pan |
| `scalar(2)` | a number with no unit |

### The SDK (`@m2/core`)

| Name | What it is |
|---|---|
| `track(name, fn)` | a track: `fn` gets `{ lane, time }` and returns its sound, or an object of named outputs such as `{ out, env }` |
| `song(fn)` | the root module in `song.ts`: whatever it returns is what you hear |
| `lane.notes(id)`, `lane.control(id, unit, fallback)`, `lane.mask(id)`, `lane.audio(id)` | the timeline lanes |
| `time.beats`, `time.seconds` | the song's position |
| `poly(notes, voice, { voices })` | plays a voice for each note at once |
| `feedback(kind)` | a loop with a short delay; give its input later with `.set()` |
| `kernel(id, inputs)` | one kernel, such as `kernel("std:svf", { … })` |
| `plugin("clap:<id>", inputs)` | a CLAP plugin (see [CLAP plugins](#clap-plugins)) |
| `component({ controls }, fn)` | marks which arguments of a function are knobs |

### When something's wrong

`m2 check` reports a problem as the file, line and column, then what's wrong. A problem in the sound graph also names the module and the node in square brackets.

```
song.ts:14:30: snare is not defined
std/src/filter.ts:10:108: [Hats/n22] input `cutoff` expects control<hz>, got control<db>
```

The second one came from `lowpass(hat(…), db(-6))` in `tracks/hats.ts`: a filter's cutoff wants a frequency, not a level. When the wrong value goes into a std function, the line shown is inside std, so look for the call in your own file.

The app shows the same problems in a yellow banner and in the code editor's list, with the file, the line and what's wrong, but without the part in square brackets.

### Editing from the app

The code stays the source of truth; the app edits it for you, one small checked change at a time.

- A knob appears for a number written directly at an input: `0.3`, `hz(800)`, `db(-6)`, or a plain property like `{ pan: 0.3 }`.
- Computed values (`2 * 440`, a variable holding a plain number) and objects with spreads, getters or computed keys still work, but show **edit in code** instead of a knob.
- A shared value such as `const cutoff = hz(800)` shows as one box wired to every place it's used. Turning it changes all of them.
- Parentheses and comments around a number are kept.
- Before writing anything, the app compiles the new code and checks that only the intended thing changed.
- While the code doesn't compile, the last working version keeps playing and the knobs are locked.
- Cables inside one file, and adding or removing effects, also work. Cables between tracks, and adding or deleting other boxes, don't yet.

## Sounds and effects (std)

std is an ordinary package in `std/`: TypeScript in `std/src`, kernels in `std/kernels/std.wasm`, built from Rust in `std/kernels-src`.

**Instruments** (`std/instruments`): `bass`, `kick`, `hat`, `pluck` and `pad`. Each takes a notes lane and returns sound.

**Effects**, as the effect browser groups them:

| Group | Effects |
|---|---|
| Filter | `lowpass`, `highpass`, `bandpass`, `notch` |
| EQ | `eq3` |
| Dynamics | `compress`, `limit` |
| Distortion | `drive`, `bitcrush` |
| Delay & Reverb | `delay`, `reverb` |
| Modulation | `chorus`, `phaser`, `tremolo` |
| Utility | `gain`, `pan`, `widen` |

**`duck`** (`std/fx`) is the sidechain: `duck(bass.out, kick.env)` turns the bass down while the kick plays. It needs a second signal, so it's written in code rather than added from the browser.

**`mixer` and `master`** (`std/mix`): `mixer` adds tracks together, each with an optional `db` and `pan`. `master` is a limiter at the end, so the song never clips.

**Building blocks:** `std/dsp` (oscillators, noise, envelopes, filters, gain, pan), `std/math` (add, scale, blend, sum, pitch to frequency, ranges), `std/control` (`lfo`, `when`, `choose`) and `std/filter`.

`m2 kernels` lists every kernel with its typed inputs and outputs, and `m2 effects <song>` lists the effects and where they can go.

## CLAP plugins

music2 can play installed CLAP plugins, such as [Surge XT](https://surge-synthesizer.github.io) and [Airwindows Consolidated](https://www.airwindows.com), as boxes in the graph:

```ts
const synth = plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("notes"), state: "lead" });
```

- `m2 plugins` lists installed plugins; `m2 plugins clap:<id>` shows one plugin's inputs and knobs.
- `m2 plugin-state <song> <name> clap:<id>` saves the plugin's default settings (not the values your code gives it) to `assets/plugin-state/<name>.bin`, which `state: "<name>"` loads.
- Plugins can't go inside `poly` (an instrument plugin plays all the notes itself), and there are no plugin windows yet.
- See [`examples/with-plugins`](examples/with-plugins), which needs both plugins installed.

## The command line (`m2`)

`m2` works on song folders without the app. Build it once with `cargo build --release -p m2`. The program is `target/release/m2`; the examples below call it `m2`. It finds std, the templates and the examples the way the app does: at `$M2_STD`, `$M2_TEMPLATES` and `$M2_EXAMPLES`, else in this source tree.

| Command | What it does |
|---|---|
| `m2 new <folder>` | Makes a new song in exactly that folder (it refuses one that exists) from a template: `--template starter` (the default) or `--template empty`. `--example first-song` copies an example instead. |
| `m2 check <song> [--json]` | Runs the code and checks the graph and the timeline. `--json` prints the compiled graph. `m2 build` is the same. |
| `m2 render <song> <out.wav>` | Renders the song to a 24-bit WAV, the same file the app's Export Audio writes: to the end of the last clip, then on until the sound has been silent for half a second (10 s past the end at most). `--sr 48000` sets the sample rate, `--from S` starts S seconds in, `--seconds N` renders exactly N seconds. 3 hours at most. |
| `m2 kernels [--json]` | Every kernel and its typed ports. |
| `m2 bounce <song> <name>` | Renders a track (`--track Keys`) or the whole song to `assets/<name>.wav`. `--place Track.lane@beat` also puts it on an audio lane, and `--force` replaces a file that's there. |
| `m2 graph <song> [--json]` | The graph the app shows, as text. |
| `m2 controls <song>` | Every number the app can turn into a knob, with its id. |
| `m2 set <song> <id> <value>` | Changes one, checked by the compiler. `--expect V` refuses unless it's V now. |
| `m2 cables <song>` | The named signals a cable can start from, and the inputs that cables reach. |
| `m2 connect <song> <signal> <control>` | Writes a signal's name in place of a number. |
| `m2 disconnect <song> <input>` | Writes the input's default back in place of the name. |
| `m2 effects <song>` | The effects packages offer, and the calls they can go after. |
| `m2 add-effect <song> <call> <module>.<effect>` | Wraps a call's sound in an effect, such as `std/fx.delay`. |
| `m2 remove-effect <song> <call>` | Takes an effect out, keeping what it was applied to. |
| `m2 plugins [clap:<id>]` | Installed CLAP plugins, or one plugin's ports. |
| `m2 plugin-state <song> <name> clap:<id>` | Saves a plugin's default settings as an asset. |

Commands that change a song write into its folder, so try them on a copy, not on the examples:

```bash
m2 new /tmp/m2-scratch --example first-song                    # a copy of the example
m2 check /tmp/m2-scratch
m2 render /tmp/m2-scratch out.wav
m2 controls /tmp/m2-scratch                                     # ids look like song.ts#2/0.hats.pan
m2 set /tmp/m2-scratch 'song.ts#2/0.hats.pan' 0.45
m2 add-effect /tmp/m2-scratch 'tracks/hats.ts#1' std/fx.delay

m2 new /tmp/m2-resample --example resample
m2 bounce /tmp/m2-resample keys-bounce --track Keys --force     # render the Keys track to assets/keys-bounce.wav
```

An id names a file, a call in it and an argument. `song.ts#2/0.hats.pan` means: in `song.ts`, call number 2 (here the mixer), its first argument (`0`), and `hats.pan` inside that.

## Scripts and agents

While the app runs, it writes `~/.music2/session.json` (only you can read it). It holds the app's local URL and a token that changes at every launch. Every `/api` call needs that token in an `x-m2-token` header (or `?token=`).

```bash
S=~/.music2/session.json; URL=$(jq -r .url $S); H="x-m2-token: $(jq -r .token $S)"
curl -s -H "$H" $URL/api/graph | jq '{name, session, revision: .view.revision}'
body=$(curl -s -H "$H" $URL/api/graph | jq '{session, revision: .view.revision, id: "song.ts#2/0.hats.pan", gesture: "script", value: 0.45}')
curl -s -H "$H" -H 'content-type: application/json' $URL/api/graph/set -d "$body"
```

Edits name the open project's `session` and the `revision` they were planned against, so a stale edit is refused rather than landing on the wrong thing.

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the effects, problems, and where playback is |
| `POST /api/graph/open` `{path}` | Open a song folder |
| `POST /api/graph/close` | Close it |
| `GET /api/graph/status` | Playing or not, seconds and beat |
| `POST /api/graph/preview` `{session, revision, id, gesture, value}` | Change a knob in the sound only, while dragging |
| `POST /api/graph/preview/cancel` `{session, id, gesture}` | Drop a preview |
| `POST /api/graph/set` `{session, revision, id, gesture, value, expect?}` | Write a knob's value into the code |
| `POST /api/graph/connect` `{session, revision, from, to}` | Connect a cable |
| `POST /api/graph/disconnect` `{session, revision, id}` | Disconnect one |
| `POST /api/graph/effect/add` `{session, revision, call, module, name}` | Add an effect |
| `POST /api/graph/effect/remove` `{session, revision, call}` | Remove one |
| `POST /api/graph/lane` `{session, revision, module, lane, clips}` | Replace a notes lane's clips |
| `POST /api/graph/undo`, `/api/graph/redo` `{session}` | Undo or redo |
| `POST /api/graph/transport` `{action}` | `play`, `stop`, `toggle` or `restart` |
| `POST /api/graph/seek` `{beat}` | Move the playhead |
| `GET /api/graph/files` | The song's TypeScript files |
| `PUT /api/graph/file` `{session?, path, base, text}` | Save one file, only if it still says `base` |
| `GET /api/graph/types` | The SDK and package sources, for type checks |
| `POST /api/graph/unsaved` `{session, files}` | Tell the app which files have unsaved edits in an editor |
| `POST /api/graph/export` `{path?}` | Export Audio: render the song to a WAV file on a thread of its own, to `path` or to a new file in `~/Music/music2/Exports`. Answers `{path}` at once; one export at a time |
| `GET /api/project/templates` | What a new song can start from: `{templates, examples}` (each `{id, name, description}`), the songs folder (`songs`) and `home` |
| `POST /api/project/new` `{name, template?, example?, dir?}` | Make a song from a template (`starter` if neither is given) or a copy of an example, in a new folder named after it in `dir` (a full path; default `~/Music/music2`), and open it. A taken name gets " 2", " 3"…, and so does `Exports` in the songs folder. An example whose `example.toml` lists `[plugins]` that aren't installed is refused, naming them. Answers like `GET /api/graph`, for the song it made |

`/api/ws` is a WebSocket with three events: `{"t":"toast","text":…}` for messages, `{"t":"graph"}` when the song changed and should be fetched again, and `{"t":"export",…}` while a song is exported, each with the `song`'s name and its folder (`dir`): `progress` (0 to 1, at most ten a second), then `done` with `path`, `seconds` and `peak_db`, or `error`. Toasts sent while nothing was connected, such as why the folder given at launch didn't open, go to the next client that connects.

Or just edit the files: the app picks up the change within about 300 ms.

## Building and testing

- `cargo test --workspace` runs the tests.
- To test the UI with no window and no sound: `M2_AUDIO=none cargo run -p m2-server --example headless -- <song-folder | -> <session.json> <token>`, then point a headless browser at the URL in the session file. `-` opens no song, for the start screen. Outside the app, the UI reads its token from `window.__M2__ = {native: false, token}`.
- `std/build.sh` rebuilds std's kernels (it needs the `wasm32-unknown-unknown` Rust target). The built `std/kernels/std.wasm` is checked in.
- `cargo run --release -p m2-runtime --example bench` compares WebAssembly and native speed.

## Repository layout

| Folder | What's in it |
|---|---|
| `core/ir` | The graph's compiled form, signal kinds and units, and the checker |
| `core/kernel` | The kit for writing kernels in Rust |
| `core/song` | The song folder: manifest, timeline files, tempo |
| `core/runtime` | Loads packages, runs kernels (wasmtime), `poly`, `feedback`, audio clips; the renderer |
| `core/script` | Runs the TypeScript (oxc + QuickJS) and the `@m2/core` SDK |
| `core/session` | Compiling, revisions, knobs, cables, effects, notes edits, undo, and the offline render that Export Audio and `m2 render` share |
| `core/plugins` | CLAP plugin hosting |
| `core/play` | Plays a renderer on the audio device |
| `std/`, `std/kernels-src` | The standard package, and its kernels' Rust source |
| `apps/m2` | The command line |
| `apps/music2-desktop` | The Mac app (Tauri): window, menus, dialogs |
| `crates/server` | The app's local server: `graph.rs` (the open song), `files.rs` (the code editor's files), `project.rs` (new songs and the songs folder), `export.rs` (Export Audio), `resources.rs` (finding std, the templates and the examples), `routes.rs` (the API) |
| `web/` | The UI: `StartScreen`, `NewSongDialog`, `GraphCanvas` (top bar and panel), `SongTimeline`, `NodeGraph`, `NoteView`, `CodeView`, `EffectBrowser` |
| `templates/` | What new songs start from: `starter` and `empty`, each a whole song plus a `template.toml` (its name and description) |
| `examples/` | `first-song`, `with-plugins` and `resample`, each with an `example.toml` for the start screen (and the plugins it needs) |
| `docs/` | Design docs and the gap list |

## Design docs

- [Song as a program](docs/song-as-program.md): the design of music2, and what's built so far.
- [Code and graph editing design](docs/code-graph-editing-design.md): how the app edits code.
- [Code and graph editing brief](docs/code-graph-editing-brief.md): the request that design answered (historical).
- [What music2 can't do yet](docs/gaps.md): the gap list, most important first.
- [Music basics](docs/music-basics.md): music for beginners.
