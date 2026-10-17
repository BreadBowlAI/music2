# music2

music2 is a music studio for the Mac: a DAW (digital audio workstation), the kind of app people make songs in, like GarageBand or Logic Pro. In music2 a song is a folder. Code in it says how the song sounds: the instruments, the effects and the mix. Data files say what plays: the notes, the automation (knob moves over time) and the audio.

The app shows that folder as a timeline, a graph of knobs and cables, a piano roll (a grid for drawing notes) and a code editor. Every change you make there is written into the files right away.

There's no AI inside the app. Scripts and AI agents (such as a Claude Code session) use the same files, and a local API (a way for programs on your Mac to talk to the app). music2 is part of **suite2**, alongside film2 and dj2. Status: early. A lot is missing: tracks are added in code, there's no sampler, and there's no mixer panel yet. See [What music2 can't do yet](docs/gaps.md).

## Run the app

You need a Mac, [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```bash
(cd web && npm install && npm run build)                        # the window's UI: once, and after UI changes
cargo run --release -p music2-desktop                           # the app, on its start screen
cargo run --release -p music2-desktop -- ~/Music/music2/Demo    # or opened on a song folder
```

The start screen has three ways in:

- **New Song…** makes a new song from a template and opens it (see [Your first song](#your-first-song)).
- **Open Project Folder…** opens a song you already have. A song folder is any folder with a `music2.toml` file in it (see [The folder](#the-folder)). File → Open Project Folder… (⌘O) does the same at any time.
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

From left to right: play/stop, back to the start, loop, where the song is (bar.beat, then minutes and seconds), the song's name and revision (every accepted change is a new revision), **Export** (it shows how far along an export is), undo, redo, and close (✕), which goes back to the start screen.

Stop ends the notes, and echoes and reverb ring out as they would on a real instrument. Moving the playhead, looping, editing notes and adding an effect all happen while the song plays, without it starting over or jumping back. Notes already sounding where the playhead lands aren't picked up: playback starts with the next note.

### The timeline

- Each track has a row for each of its lanes: its notes, its automation and its audio (see [Words you'll see](#words-youll-see)). The ruler on top counts bars, following the song's time signatures, and the song's tempo is shown at the top left.
- Click the ruler to move the playhead there. Drag across the ruler to loop those beats: playback goes round them until you turn the loop off with the loop button (it keeps the range for next time). With no loop set, the loop button loops the 4 bars from the playhead.
- Double-click an empty spot in a notes lane to add a one-bar clip. Drag a clip to move it along its lane. Click a clip and press Delete to remove it.
- Double-click a notes clip to edit its notes.
- Double-click a track's name to rename it: type, then press Enter (or click away) to save, or Escape to cancel. This changes only the name the app shows (the track's label, written in its `track()` call); its key, its data and its sound stay as they are, and Undo takes it back.
- Automation and audio clips are shown, but they can't be edited in the app yet. Tempo and time-signature changes appear on the ruler.

### The panel below the timeline

There's a tab for each module (one file's sound): one for each track file, and one for the Song (the mix). Drag the bar between the timeline and the panel to resize it. Each module has three views:

- **Graph:** the module as boxes and knobs.
  - Drag a knob to hear it; let go to write the number into the code. Double-click a knob to put it back at its default (the middle of its range when its control declares no default).
  - Right-click a knob for **Automate** and **Reset to default**. Automate gives the knob an automation lane of its own, under its track in the timeline: the code reads the knob from the lane (`pan: lane.control("hats-pan", "bipolar", 0.3)`), and the lane plays the old number until it has points, so nothing sounds different. Undo takes the lane out again. When a knob can't be automated (it's shared by several tracks, say), the item is greyed out and says why.
  - Double-click the module's name at the top of the graph to rename the track, as in the timeline.
  - A knob whose number is in *italics* is a control the code leaves out, such as a bass's `drive` when the code says only `bass(notes)`: it plays the instrument's or effect's default. Turning it writes the number into the call (`bass(notes, { drive: 9 })`), and undo takes it out again.
  - A row that says **edit in code** is a value the app can't turn into a knob (see [Editing from the app](#editing-from-the-app)).
  - Cables: drag from a named output (the dot at the right edge of a box whose result has a `const` name in the code) onto a knob. The ✕ on a connected input disconnects it.
  - **+** on a box that makes sound opens the effect browser: 17 effects in seven groups (Filter, EQ, Dynamics, Distortion, Delay & Reverb, Modulation, Utility), as the effects themselves declare them. Type to search, then press Enter or click. The bin icon on an effect removes it.
  - Zoom with ⌘ and scroll, or pinch. The buttons at the bottom right zoom and fit the graph to the panel.
- **Code:** the module's TypeScript, with type checks. The list on the left shows the song's files. ⌘-click a name to jump to where it's defined, even inside std (std's files open read-only). ⌘S (or the Save button) saves. When the code has an error, the last working version keeps playing, and a yellow banner offers **Show in code**. A song whose code doesn't compile when you open it still opens: in its code, at the error, with nothing to show or play until a save makes it compile, and then it plays.
- **Notes:** the open clip in a piano roll.
  - Pick a snap grid (1/4 to 1/32, or off). Click to draw a note, drag to move, drag the right edge to resize.
  - The strip at the bottom sets velocity (how hard each note is played).
  - Set the clip's length in bars at the top.
  - While the clip's data file doesn't read (see **Needs attention**), the piano roll is read only, with a note at the top saying which file: it shows the notes the lane played before, and editing them would overwrite the file. Fix the file and it can be edited again.
  - ⌘A selects all notes, ⌘D duplicates the selection, the arrow keys move it (with ⇧, by an octave or a bar), and Delete removes it.

## New songs, the songs folder and export

- **New Song…** (the start screen, or File → New Song…, ⌘N, even while a song is open) asks for a name and how the song starts:
  - **Starter beat**: six tracks (kick, clap, hats, bass, chords and a hook), 8 bars at 116 BPM, ready to play and change.
  - **Empty**: no tracks yet, so it plays silence. `song.ts` has a drum track ready, switched off: delete the `//` in front of its two lines and save, and a Drums lane appears.
- New songs go in the **songs folder**, `~/Music/music2`, each in a folder named after the song. If the name is taken, the new one gets a number ("My song 2"): nothing is ever replaced. `Exports` there is taken by exports, so a song called "Exports" becomes "Exports 2". In the app, **Choose Folder…** in the New Song dialog saves it somewhere else.
- **Open an Example** on the start screen copies an example into the songs folder and opens the copy. An example that needs plugins you don't have (With plugins) says which, and nothing is copied.
- **Export** (the button in the top bar, or File → Export Audio…, ⌘E) writes the song to a WAV file: 24-bit, stereo, at the song's sample rate (48 kHz unless its `music2.toml` says otherwise). It starts at bar 1 and goes to the end of the last clip, then on until the sound has died away (silent for half a second, at most 10 seconds more). A song longer than 3 hours (most likely a clip far past the rest) isn't exported: the message says in which bar the last clip ends.
  - The app asks where to save it, starting at `~/Music/music2/Exports/<song>.wav`. Opened in a browser, it saves there straight away, adding " 2", " 3"… rather than replacing a file, even one that appeared while it was exporting.
  - The button shows how far along it is, and you can keep playing and editing meanwhile. It exports the song as it was when you clicked. If you close the song or open another, the export carries on, and a message in the corner shows how far along it is.
  - A song with plugins pauses while it exports, as a tape would: the export borrows the plugins that play, so they don't need a second copy. The top bar says **Paused for export**. Afterwards play goes on where it paused, and a note held across the pause starts again. Edits made meanwhile are heard once the export is done.
  - Quitting while a song is being exported asks first: quitting stops the export, and no file is saved.
  - The file is written under a hidden name and renamed when it's complete. A hidden file left by an export that was cut off (the app was killed) is removed by the next export into that folder.
  - When it's done, a message gives the file's name and length, with **Show in Finder** (in a browser, it says which folder).
  - `m2 render` writes the same file.

## Saving and undo

- There's no Save for the song: every change is written to the files at once. ⌘S only saves the code editor's text.
- A note edit is heard at once, while it's being written to disk. If the disk refuses it (it's full, say, or another program replaced the file at that moment), the edit is taken back and a message says why; if the disk is slow to answer, a message says the edit isn't saved yet, and another says when it is. The same goes for an edit an agent sends while the song plays: the app says when it was taken back.
- Every edit made in the app is checked before it's written: the app works out what the change does to the song and refuses it if it would do anything else. An edit that touches several files (automating a knob writes its code and a new lane file) is written as one, all or nothing.
- Undo (⌘Z) and redo (⇧⌘Z) cover knobs, cables, effects, notes, renames, automation, fixes and code saves. The history ends when you close the app. Undo is refused only when what it would put back has changed since (the same number, or the same clip, edited elsewhere); a change somewhere else in the file doesn't block it.
- Changes made in another editor, or by a script or git, show up once the other program has stopped writing for a moment (150 milliseconds, so usually within a quarter of a second). A file caught half-written never plays: a lane keeps the notes it had until its file reads again, and a code change that doesn't compile yet holds back the data saved with it, so a lane never plays empty in between. The app hears of changes from macOS, so an open song nobody is changing costs next to nothing, however many samples it has. On a network drive, which doesn't report other computers' changes, it reads the song's folder instead: every 50 milliseconds for a small song, less often for one with thousands of files, and at least once a second.
- If another program changes a clip while you're editing it in the piano roll, your edit of that clip is refused, saying which clip, and the other program's version stays. Edits to different clips of one lane both stay.
- One program writes a song at a time. While the app has a song open, `m2` commands that write it stop without changing anything and say so; ones that only read it, render it or bounce a track work. A song that's open for writing elsewhere (another music2 window, say) opens read-only, with a blue banner naming who has it: you can play it and see its changes, and **Retry** takes it over once it's free.
- The lock and the edits being written live in the song's `.music2/` folder, which git ignores. If the app or the Mac stops in the middle of writing an edit, **Needs attention** says so when the song opens next, with **Finish** and **Discard**: either leaves every file as it was before the edit or after it, never half.
- Problems that don't stop the song, such as data no lane reads after a key was renamed in code, a data file that doesn't read, or a missing sample or plugin, are listed in a **Needs attention** strip above the timeline, each with its fixes. A fix runs only when you click it, and Undo takes it back. When a track's key was renamed in code, **Move all to …** moves its whole data folder to the new key at once.

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
- **Key:** the short name a track, a lane or a plugin's saved settings are filed under, written in the code: lowercase letters, digits and hyphens, such as `lead-vox`. A track also has a **label**, the name the app shows (`Lead Vox`), which can change freely.
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
  music2.toml          # the song's name, its format and sample rate, and the SDK and packages it uses
  song.ts              # the mix: which tracks play, how loud, and the master
  tracks/
    bass.ts            # one file per track: its lanes, instrument and effects
  data/
    arrangement.json   # tempo and time signature
    bass/              # one folder per track, named by its key
      notes.json       # one file per lane: data/<track key>/<lane key>.json
      cutoff.json      # (a key that is a Windows device name, such as aux, is written aux-)
  assets/              # audio files (WAV), never overwritten
  .music2/             # this machine's: not part of the song, and not in git
```

`music2.toml` looks like this:

```toml
[project]
name = "My song"
format = 1
sample_rate = 48000

[dependencies]
"@m2/core" = "1"
std = "0.2"
```

`arrangement.json` holds `tempo` (a list of `{ "beat", "bpm" }` changes, the first at beat 0) and `meter` (a list of `{ "beat", "beats", "unit" }` time signatures; empty is 4/4). A beat is a quarter note.

Tracks, lanes and plugins are filed under their keys: short names in lowercase letters, digits and hyphens, such as `lead-vox`. A key is written in the code, so renaming a track's label never moves its data.

### A track

This is a whole track, the shape of `tracks/hats.ts`:

```ts
import { track } from "@m2/core";
import { hat } from "std/instruments";

export default track("hats", { name: "Hats" }, ({ lane }) => hat(lane.notes("hits")));
```

- `track("hats", { name: "Hats" }, …)` makes a track with the key `hats`, shown as Hats. Without the options, `track("hats", fn)`, it is shown by its key. Its data is filed under the key, so changing the name moves nothing; changing the key leaves its data behind until you move it (the app offers to).
- `lane.notes("hits")` is a notes lane with the key hits. Its clips are in `data/hats/hits.json`. `lane.notes("hits", { name: "Hits" })` gives it a label too.
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
{"clips": [
  {"id": "k3f9q2zt", "start": 0, "length": 4, "notes": [
    {"pitch": "F#2", "start": 0.5, "length": 0.25, "velocity": 0.9},
    {"pitch": 42, "start": 1.5, "length": 0.25}
  ]}
]}
```

- `pitch` is a note name (`"F#2"`, `"Bb3"`) or a number (60 is middle C, so `42` is F#2).
- A clip's `start` and `length` are in beats from the start of the song. A note's `start` and `length` are in beats from the start of its clip.
- `velocity` goes from 0 to 1. Without it, a note gets 0.8.
- `id` names the clip. music2 gives every clip it writes one; a clip without one still plays.
- music2 writes these files in one form, one clip and one note per line, so they diff well. The order of clips and notes in a file never changes what plays.

Automation and mask clips hold `points` instead of notes: `[beat, value]` pairs, with straight lines in between, and their file records the lane's unit (`{"unit": "hz", "clips": [...]}`). Audio clips name a file in `assets/`, pin it with `hash` (the first 16 hex digits of its SHA-256), and can have `offset`, `gain_db`, `fade_in`, `fade_out`, `reverse`, `pitch` and `warp`; see `examples/resample/data/chops/clips.json`.

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
| `track(key, { name }?, fn)` | a track: `fn` gets `{ key, lane, time }` and returns its sound, or an object of named outputs such as `{ out, env }`. `name` is its label; code never sees it |
| `song(fn)` | the root module in `song.ts`, with the key `song`: whatever it returns is what you hear |
| `lane.notes(key)`, `lane.control(key, unit, fallback)`, `lane.mask(key)`, `lane.audio(key)` | the timeline lanes, each with an optional last argument `{ name, shared }` |
| `key(k, fn)` | runs `fn` in a frame named `k`, which files the saved settings of the plugins built inside it under `k` |
| `time.beats`, `time.seconds` | the song's position |
| `poly(notes, voice, { voices })` | plays a voice for each note at once |
| `feedback(kind)` | a loop with a short delay; give its input later with `.set()` |
| `kernel(id, inputs)` | one kernel, such as `kernel("std:svf", { … })` |
| `plugin("clap:<id>", inputs)` | a CLAP plugin; `key` in its inputs names its saved settings (see [CLAP plugins](#clap-plugins)) |
| `component({ id, inputs, controls, settings, output, positional, editor }, body)` | declares a component: the signals it reads, its controls (knobs, each with a unit, a default and a range), its settings, and how the editor offers it. Every std function is one (see `std/src`); the older `component({ controls }, fn)`, which only marks which arguments are knobs, still works |

### Your own components

A function of your own becomes a box with knobs when it's declared as a component, as every std function is:

```ts
// lib/crunch.ts
import { component, control, kernel, port, signature, type Audio, type Params, type Value } from "@m2/core";

/** Drive, then a lowpass that tames the fizz. */
export const crunch = component(
  {
    id: "crunch",
    label: "Crunch",
    signature: signature<(x: Audio, amount?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      amount: control.db(12).range(0, 36).smooth(),
      tone: control.hz(3000).range(200, 20000).log().smooth(),
    },
    output: port.audio(),
    positional: ["amount"],
  },
  (x: Audio, { amount, tone }: Params) => kernel("std:svf", { in: kernel("std:drive", { in: x, amount }), cutoff: tone }).port("lp"),
);
```

- `id` is what the app knows the component by, whatever name a file imports it as. In a package it is `<package>:<id>` (`std:delay`).
- `inputs` are the signals it reads. `port.audio({ sidechain: true })` marks one that only listens, like a compressor's key input.
- Each control has a unit and a default (`control.db(12)` is 12 dB), written in that unit, like every number in the descriptor. `.range(lo, hi)` is the knob's range (a hint: a number outside it is never changed), `.log()` turns it on a log scale, `.smooth()` glides a change over one block (for a control the sound reads every sample, so a jump would click), and `.optional()` means a call that leaves it out gets no node at all. The body always gets a signal for a control, whether the call wrote a number, connected a signal or left it out.
- `settings` are values that shape the graph, such as `setting.int(4)` voices: never knobs.
- `positional` lets calls write those controls as arguments, in order: `crunch(x, 18)`. Any control can also go in an options object after them: `crunch(x, { tone: 1500 })`. A misspelt option is an error that lists the right ones.
- `editor: { category, description, insert }` puts a package's component in the effect browser, under its category, and `insert` is what adding it writes (`{ amount: 6 }`).

### When something's wrong

`m2 check` reports a problem as the file, line and column, then what's wrong. A problem in the sound graph also names the module's key and the node in square brackets.

```
song.ts:14:30: snare is not defined
std/src/filter.ts:26:40: [hats/n23] input `cutoff` expects control<hz>, got control<db>
```

The second one came from `lowpass(hat(…), db(-6))` in `tracks/hats.ts`: a filter's cutoff wants a frequency, not a level. When the wrong value goes into a std function, the line shown is inside std, so look for the call in your own file.

The app shows the same problems in a yellow banner and in the code editor's list, with the file, the line and what's wrong, but without the part in square brackets.

Problems that don't stop the song (data no lane reads, a lane file that doesn't read, a missing asset or plugin) are printed by every `m2` command that opens the song, each with its code in square brackets, such as `[data.unclaimed]`, and the commands that fix it.

### Editing from the app

The code stays the source of truth; the app edits it for you, one small checked change at a time.

- A knob appears for a number written directly at an input: `0.3`, `hz(800)`, `db(-6)`, or a plain property like `{ pan: 0.3 }`.
- A knob also appears for every control a call leaves out, at the default its component declares (shown in italics). Its first turn writes the number into the call: as the next argument when that is where it goes (`lowpass(x, 1000)` becomes `lowpass(x, 1000, 0.8)`), else in the call's options (`{ release: ms(150), attack: 0.01 }`), else in new ones. A knob's range is the one its component declares.
- Computed values (`2 * 440`, a variable holding a plain number) and objects with spreads, getters or computed keys still work, but show **edit in code** instead of a knob.
- A shared value such as `const cutoff = hz(800)` shows as one box wired to every place it's used. Turning it changes all of them.
- Parentheses and comments around a number are kept.
- Before writing anything, the app compiles the new code and checks that only the intended thing changed: a knob changes only its number, a cable only what reads it, a rename nothing that plays.
- While the code doesn't compile, the last working version keeps playing and the knobs are locked.
- Cables inside one file, and adding or removing effects, also work. Cables between tracks, and adding or deleting other boxes, don't yet.
- Renaming a track writes, changes or takes out the `name` in its `track()` call (a name equal to the key takes it out, since the label defaults to the key). A name written as a variable is left alone: the app says to change it there.
- Automating a knob writes `lane.control("<key>", "<unit>", <the number as written>)` where the number was, adds `lane` to the track function's parameters when it isn't there (`() =>` becomes `({ lane }) =>`), and makes the empty lane file `data/<track>/<key>.json`. The key comes from the knob's name, such as `hats-pan`.

## Sounds and effects (std)

std is an ordinary package in `std/`: TypeScript in `std/src`, kernels in `std/kernels/std.wasm`, built from Rust in `std/kernels-src`.

**Instruments.** Each takes a notes lane and returns sound. The `voices` setting is how many notes can sound at once (`synth.lead(notes, { voices: 4 })` plays chords). A typical note, at velocity 0.8, peaks between -15 and -12 dBFS, so a four-note chord still has room, and the drums sit as on a drum machine: the kick near -6, snare and clap near -9, hats near -16. Notes played harder are louder, and most sounds get brighter too. On the synths and FM sounds, `velSens` ("vel sens") is how much: the level follows velocity^(2 × velSens), so a note at velocity 0 is silent, and at 0.5 a note at 0.3 is 10 dB below one at 1. The keys start widest (0.8), the 808 narrowest (0.3).

- **Synths** (`std/synth`), all one polysynth:
  - `synth.supersaw`: seven detuned saws, bright and wide, for big chords and hooks. Its filter starts wide open, as on a JP-8000; turn `cutoff` down to tame it. Knobs: `detune`, `spread` (how wide), `cutoff`, `attack`, `release`, `velSens`.
  - `synth.lead`: a singing lead that plays one note at a time. A note that starts before the last one ends slides into it, and a vibrato fades in on long notes. Knobs: `cutoff`, `glide`, `vibrato`, `drive`, `velSens`.
  - `synth.pad`: a big, slow, wide pad that swells open. Knobs: `cutoff`, `attack`, `release`, `spread`, `velSens`.
  - `synth.pluck`: a bright, wide pluck for arps and chords, whose filter snaps shut. Knobs: `brightness`, `decay`, `spread`, `velSens`.
  - `synth.sub`: an 808 bass, one note at a time: a deep, warm sine that drops into each note (by `drop` semitones), rings out over `decay` (to -60 dB: 0.2 s for a tight hit, 4 s by default, longer for long slides) and slides between overlapping notes. Knobs: `drop`, `drive`, `glide`, `decay`, `velSens`.
  - `synth.bass`: a synth bass, one note at a time: a saw and a pulse with a sine under them, through a filter that bites as each note starts. Knobs: `cutoff`, `bite` (how many octaves the filter opens), `decay` (how fast it closes), `glide`, `velSens`.
  - `synth.strings`: a string machine, soft saws through an ensemble chorus. Knobs: `cutoff`, `attack`, `release`, `ensemble`, `velSens`.
  - `synth.custom`: the synth itself with every knob, to make your own sound: saw, pulse, sub and noise, a ladder filter with its own envelope, the amp envelope, drive, vibrato, a pitch drop and glide. `unison` (1 to 7 stacked voices) is a setting: `synth.custom(notes, { unison: 5 })`.
  - With `voices: 1` any of them, and any FM sound, plays one note at a time: a note that starts before the last one ends carries on from it, without starting again (legato), and the ones with a `glide` knob slide to it. When that note ends while an older one is still held, the older one sounds again, sliding back: hold A1 on the 808 and tap A2 over it for the classic slide up and back. Notes that start together count as one, the last of them. `synth.lead`, `synth.sub` and `synth.bass` start that way. For a voice of your own, `mono(notes, voice, { glide })` is `poly` with one note at a time.
- **FM** (`std/fm`), one FM synth: `fm.keys`, an electric piano with a warm body and a tine that barks when you play hard (knobs `tone`, `tine`, `decay`, `release`, `velSens`); `fm.bell`, a glassy bell (`tone`, `decay`, `velSens`); `fm.metal`, a short, clangy metallic hit that zaps down into its note (`tone`, `decay`, `feedback`, `drop`, `velSens`); and `fm.custom`, the FM synth with every knob. Its carriers and modulators are frequency ratios to the note: whole numbers sound harmonic, halves (the bell's 3.5) like a bell, and others such as 3.17 clang like metal. A ratio typed into the code is exact; dragged on a knob it lands near a value (2.03, say), so type the musical ones.
- **Drums** (`std/drums`), an 808/909-style drum machine. `drums.kit` plays a whole beat from one notes lane, each drum on its General MIDI note, the numbers drum parts usually use:

  | Note | Drum | Note | Drum |
  |---|---|---|---|
  | C2 (36), B1 (35) | kick (B1 lower and softer) | F#2 (42) | closed hat |
  | D2 (38), E2 (40) | snare (E2 higher and snappier) | G#2 (44) | pedal hat |
  | D#2 (39) | clap | A#2 (46) | open hat |
  | C#2 (37) | rim | C#3 (49), A3 (57) | crash |
  | F2, G2, A2, B2, C3, D3 (41 to 50) | toms, low to high | D#3 (51), B3 (59) | ride |
  | G#3 (56) | cowbell | F#3 (54) | tambourine |
  | A4, A#4 (69, 70) | shakers | D#5 (75) | claves |

  A hit cuts off the drum before it, as on a drum machine: a kick the kick still ringing, a closed hat an open one. Toms, rim, cowbell, ride and shakers ring on. No two hits are quite the same, as on an analog machine (each catches its noise and metal at a new place), yet every render of a song is. The kit's knobs are `kickTune`, `kickDecay`, `punch` (from a deep 808 boom to a hard 909 thump), `snareTune`, `snap`, `hatDecay`, `openDecay`, `hatTone`, `tomTune`, `drive`, and a level in dB for each group: `kickLevel`, `snareLevel` (with the clap and rim), `hatLevel`, `tomLevel` and `percLevel` (cymbals and the rest). Each drum is also an instrument of its own, on every note: `drums.kick`, `drums.snare`, `drums.clap`, `drums.hats` (closed, with G#2 pedal and A#2 open), `drums.tom` (tuned by the note: A2 plays at its `tune`), `drums.rim`, `drums.cowbell`, `drums.cymbal` and `drums.shaker`. The kit comes out as one sound, so a drum you want to treat on its own (a kick for the bass to duck under, a snare with a reverb of its own) goes on a lane of its own, played by that drum alone, as `examples/sounds` does with its kick.
- **The first instruments** (`std/instruments`): `bass`, `kick`, `hat`, `pluck` and `pad`, simple versions built from the building blocks below, short enough to read and copy. For a song, the ones above sound fuller: `synth.bass` or `synth.sub`, `drums.kick`, `drums.hats`, `synth.pluck` and `synth.pad`. The templates still start new songs with the first ones.

[`examples/sounds`](examples/sounds) plays every synth, FM sound and drum in a short pop song, mastered near -11.5 LUFS.

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

`m2 kernels` lists every kernel with its typed inputs and outputs, and `m2 effects <song>` lists the effects (by their ids, such as `std:delay`) and where they can go.

## CLAP plugins

music2 can play installed CLAP plugins, such as [Surge XT](https://surge-synthesizer.github.io) and [Airwindows Consolidated](https://www.airwindows.com), as boxes in the graph:

```ts
const synth = plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("notes"), key: "lead", a_filter_1_cutoff_4092842705: 0.6 });
```

- `m2 plugins` lists installed plugins; `m2 plugins clap:<id>` shows one plugin's inputs and knobs.
- A parameter is written as its name, then its CLAP id: `a_filter_1_cutoff_4092842705`. Only the id says which parameter it is, so a plugin update that renames its parameters changes nothing (music2 notes the new name). Written without its id (`a_filter_1_cutoff`), it is an error that names the full address.
- A song whose plugin isn't installed still plays: that plugin's node is silent (an instrument) or passes its audio through (an effect), and the song says which plugin is missing.
- `m2 plugin-state <song> <track>/<key> clap:<id>` saves the plugin's default settings (not the values your code gives it) to `data/<track>/<key>.state`, which the plugin keyed `<key>` on that track loads. Beside it, `<key>.state.json` says which plugin wrote it; a state without it, or written by another plugin, isn't loaded.
- Plugins can't go inside `poly` (an instrument plugin plays all the notes itself), and there are no plugin windows yet.
- See [`examples/with-plugins`](examples/with-plugins), which needs both plugins installed.

## The command line (`m2`)

`m2` works on song folders without the app. Build it once with `cargo build --release -p m2`. The program is `target/release/m2`; the examples below call it `m2`. It finds std, the templates and the examples the way the app does: at `$M2_STD`, `$M2_TEMPLATES` and `$M2_EXAMPLES`, else in this source tree.

| Command | What it does |
|---|---|
| `m2 new <folder>` | Makes a new song in exactly that folder (it refuses one that exists) from a template: `--template starter` (the default) or `--template empty`. `--example first-song` copies an example instead. |
| `m2 check <song> [--json]` | Runs the code and checks the graph and the timeline. `--json` prints the compiled graph. `m2 build` is the same. |
| `m2 render <song> <out.wav>` | Renders the song to a 24-bit WAV, the same file the app's Export Audio writes: to the end of the last clip, then on until the sound has been silent for half a second (10 s past the end at most). It renders at the song's `sample_rate`, from `music2.toml`. `--from S` starts S seconds in (or `--from-bar N` at bar N, counted from 1), replaying the song up to there; `--seconds N` renders exactly N seconds. 3 hours at most. `--preroll P` starts only P seconds before instead of replaying: much faster late in a long song, for listening, but approximate, as it says (notes held from before then, and earlier tails, are missing), and never written into `assets/`. |
| `m2 kernels [--json]` | Every kernel and its typed ports. |
| `m2 bounce <song> <name>` | Renders a track, named by its key (`--track keys`), or the whole song to a new `assets/<name>.wav`; it never replaces one that's there. `--place keys.lane@beat` also puts it on an audio lane. |
| `m2 graph <song> [--json]` | The graph the app shows, as text. |
| `m2 controls <song>` | Every number the app can turn into a knob, with its id, including the controls a call leaves out (`tracks/hats.ts#1/@decay`, marked inherited). |
| `m2 set <song> <id> <value>` | Changes one, checked by the compiler. `--expect V` refuses unless it's V now. |
| `m2 automate <song> <id>` | Gives a knob an automation lane of its own, as the app's Automate does: the knob's number becomes the lane's fallback, so the song sounds the same. |
| `m2 label <song> <track> <name>` | Shows a track, named by its key, as `<name>`: only its `track()` call's name changes. |
| `m2 cables <song>` | The named signals a cable can start from, and the inputs that cables reach. |
| `m2 connect <song> <signal> <control>` | Writes a signal's name in place of a number. |
| `m2 disconnect <song> <input>` | Writes the input's default back in place of the name. |
| `m2 effects <song>` | The effects packages offer, by id (`std:delay`), and the calls they can go after. |
| `m2 add-effect <song> <call> <effect>` | Wraps a call's sound in an effect, named by its id (`std:delay`) or where it's exported (`std/fx.delay`). |
| `m2 remove-effect <song> <call>` | Takes an effect out, keeping what it was applied to. |
| `m2 plugins [clap:<id>]` | Installed CLAP plugins, or one plugin's ports. |
| `m2 plugin-state <song> <track>/<key> clap:<id>` | Saves a plugin's default settings as its saved state. |
| `m2 relink <song> <from> <to>` | Moves data no lane or plugin reads to where it's read now, after a key was renamed in code: a track's folder (`data/kick data/kicks`), a lane file (`data/kick/hits.json data/kick/beats.json`) or a plugin's state. |
| `m2 delete-data <song> <path>` | Deletes data no lane or plugin reads. |
| `m2 fix <song> <path> <fix>` | Applies a fix a problem offers, such as `assign-clip-ids` or `repin-asset`, or `finish-tx` and `discard-tx` for an edit that was cut off (its path is `.music2/tx/<id>.json`). Every command prints the problems it finds with the commands that fix them. |
| `m2 tx <song> <tx.json \| -> [--check] [--seen HASH]` | A checked edit written as Tx JSON (see [Scripts and agents](#scripts-and-agents)). `--check` prints what it would change and that change's hash, writing nothing; without it, it is written, and `--seen HASH` writes it only while it still makes the change with that hash. The answer is JSON; the exit status is 1 when nothing was written. |

While the app has a song open, the commands that write it (`set`, `automate`, `label`, `connect`, `disconnect`, `add-effect`, `remove-effect`, `relink`, `delete-data`, `fix`, `tx`, `plugin-state` and `bounce --place`) stop at once, change nothing, and say which program has it. Make the edit in the app, or close the song there first. The other commands, and `bounce` without `--place` (it only makes a new file), work while it's open.

Commands that change a song write into its folder, so try them on a copy, not on the examples:

```bash
m2 new /tmp/m2-scratch --example first-song                    # a copy of the example
m2 check /tmp/m2-scratch
m2 render /tmp/m2-scratch out.wav
m2 controls /tmp/m2-scratch                                     # ids look like song.ts#2/0.hats.pan
m2 set /tmp/m2-scratch 'song.ts#2/0.hats.pan' 0.45
m2 set /tmp/m2-scratch 'tracks/hats.ts#1/@decay' 0.08            # a control the call leaves out: writes { decay: 0.08 }
m2 add-effect /tmp/m2-scratch 'tracks/hats.ts#1' std:delay

m2 new /tmp/m2-resample --example resample
m2 bounce /tmp/m2-resample keys-again --track keys              # render the keys track to assets/keys-again.wav
```

An id names a file, a call in it and an argument. `song.ts#2/0.hats.pan` means: in `song.ts`, call number 2 (here the mixer), its first argument (`0`), and `hats.pan` inside that. A control the call leaves out is named by its call and `@` with the control's name: `tracks/hats.ts#1/@decay`.

## Scripts and agents

While the app runs, it writes `~/.music2/session.json` (only you can read it). It holds the app's local URL and a token that changes at every launch. Every `/api` call needs that token in an `x-m2-token` header (or `?token=`).

```bash
S=~/.music2/session.json; URL=$(jq -r .url $S); H="x-m2-token: $(jq -r .token $S)"
curl -s -H "$H" $URL/api/graph | jq '{name, session, revision: .view.revision}'
body=$(curl -s -H "$H" $URL/api/graph | jq '{session, revision: .view.revision, id: "song.ts#2/0.hats.pan", gesture: "script", value: 0.45}')
curl -s -H "$H" -H 'content-type: application/json' $URL/api/graph/set -d "$body"
```

Edits name the open project's `session` and the `revision` they were planned against, so a stale edit is refused rather than landing on the wrong thing. While the song is open read-only (another program holds it), every route that writes answers 423 with the reason, and changes nothing.

An agent can also send any edit as a **Tx**: the files it reads, each with the hash (or the text) it was planned on, the changes, and what they may do to the song. The app checks it as it checks its own edits, writes it as one undo step, and refuses it if a file changed under it or the song would change in any other way ([WRITE-3](docs/architecture.md#the-two-write-primitives)). For example, deleting a data file no lane reads:

```bash
sha=$(shasum -a 256 data/kick/old.json | cut -d' ' -f1)       # in the song's folder
tx='{"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:'$sha'"},
     "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}'
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/check -d "{\"tx\": $tx}" | jq .checked.hash
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/commit -d "{\"tx\": $tx}"
```

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the effects, problems, where playback is, and the knob drags in progress (`leases`, each with its knob's id in that view). `has_code` is false while the song opened with code that doesn't compile; `read_only` is null, or `{why, message, holder}` when another program holds the song |
| `POST /api/graph/open` `{path}` | Open a song folder |
| `POST /api/graph/close` | Close it |
| `GET /api/graph/status` | Playing or not, seconds and beat, and the loop (`{start, end, on}` in beats, or null) |
| `POST /api/graph/preview` `{session, revision, id, gesture, value}` | Change a knob in the sound only, while dragging |
| `POST /api/graph/preview/cancel` `{session, id, gesture}` | Drop a preview |
| `POST /api/graph/set` `{session, revision, id, gesture, value, expect?}` | Write a knob's value into the code |
| `POST /api/graph/connect` `{session, revision, from, to}` | Connect a cable |
| `POST /api/graph/disconnect` `{session, revision, id}` | Disconnect one |
| `POST /api/graph/effect/add` `{session, revision, call, component}` | Add an effect, by its id (`std:delay`) as `effects` lists it |
| `POST /api/graph/effect/remove` `{session, revision, call}` | Remove one |
| `POST /api/graph/lane` `{session, revision, module, lane, clips}` | Replace a notes lane's clips (`module` and `lane` are keys; each clip keeps its `id`) |
| `POST /api/graph/fix` `{session, revision, fix}` | Apply one of the fixes a problem in `problems` offers, as it was offered |
| `POST /api/graph/relink` `{session, revision, from, to}` | Move data no lane or plugin reads (`data/kick` to `data/kicks`, or a lane file) |
| `POST /api/graph/data/delete` `{session, revision, file}` | Delete data no lane or plugin reads |
| `POST /api/graph/label` `{session, module, label}` | Rename a track's label (its key stays) |
| `POST /api/graph/automate` `{session, revision, id}` | Give a knob an automation lane; answers with the `lane` it made |
| `POST /api/tx/check` `{session?, tx}` | Check a Tx: `{checked: {delta, hash, diagnostics}}`, writing nothing (409 when a file it's based on changed, 422 when it would do something else) |
| `POST /api/tx/commit` `{session?, tx}` | Write a Tx: `{committed: {revision, delta, hash, …}}`, or `{conflict}`, `{rejected}` or `{io}`. With `"expect": {"seen": hash}` it is written only while it makes the change its check showed |
| `POST /api/graph/lock/retry` `{session}` | Try again to take a song that opened read-only; answers like `GET /api/graph` |
| `POST /api/graph/undo`, `/api/graph/redo` `{session}` | Undo or redo |
| `POST /api/graph/transport` `{action}` | `play`, `stop`, `toggle` or `restart` |
| `POST /api/graph/seek` `{beat}` | Move the playhead: a jump at the next block, playing or not; nothing is rebuilt |
| `POST /api/graph/loop` `{start?, end?, on?}` | Set the loop's range in beats (it turns the loop on unless `on` says otherwise), or turn it on or off over the range set. Answers like `/status` |
| `GET /api/graph/files` | The song's TypeScript files |
| `PUT /api/graph/file` `{session?, path, base, text}` | Save one file, only if it still says `base` |
| `GET /api/graph/types` | The SDK and package sources, for type checks |
| `POST /api/graph/unsaved` `{session, files}` | Tell the app which files have unsaved edits in an editor |
| `POST /api/graph/export` `{path?}` | Export Audio: render the song to a WAV file on a thread of its own, to `path` or to a new file in `~/Music/music2/Exports`. Answers `{path}` at once; one export at a time |
| `GET /api/project/templates` | What a new song can start from: `{templates, examples}` (each `{id, name, description}`), the songs folder (`songs`) and `home` |
| `POST /api/project/new` `{name, template?, example?, dir?}` | Make a song from a template (`starter` if neither is given) or a copy of an example, in a new folder named after it in `dir` (a full path; default `~/Music/music2`), and open it. A taken name gets " 2", " 3"…, and so does `Exports` in the songs folder. An example whose `example.toml` lists `[plugins]` that aren't installed is refused, naming them. Answers like `GET /api/graph`, for the song it made |

`/api/ws` is a WebSocket with three events: `{"t":"toast","text":…}` for messages, `{"t":"graph"}` when the song changed and should be fetched again, and `{"t":"export",…}` while a song is exported, each with the `song`'s name and its folder (`dir`): `progress` (0 to 1, at most ten a second), then `done` with `path`, `seconds` and `peak_db`, or `error`. Toasts sent while nothing was connected, such as why the folder given at launch didn't open, go to the next client that connects.

Or just edit the files: the app takes the change in once it has settled (no file changed for 150 ms), usually within a quarter of a second.

## Building and testing

- `cargo test --workspace` runs the tests.
- To test the UI with no window and no sound: `M2_AUDIO=none cargo run -p m2-server --example headless -- <song-folder | -> <session.json> <token>`, then point a headless browser at the URL in the session file. `-` opens no song, for the start screen. Outside the app, the UI reads its token from `window.__M2__ = {native: false, token}`.
- The browser suites are in `e2e/`: each drives the UI in headless Chrome against the headless server, on a fresh copy of a song (see `e2e/README.md`).
- `std/build.sh` rebuilds std's kernels (it needs the `wasm32-unknown-unknown` Rust target). The built `std/kernels/std.wasm` is checked in.
- `cargo run --release -p m2-runtime --example bench` compares WebAssembly and native speed.

## Repository layout

| Folder | What's in it |
|---|---|
| `core/ir` | The graph's compiled form, signal kinds and units, and the checker |
| `std/kernels-src/kernel` | The kit for writing kernels in Rust (`m2-kernel`), a member of the kernels' own workspace |
| `core/song` | The song folder (format 1): the manifest, `DataPath` (every project path), keys, lane and arrangement files in their canonical form, asset hashes, and the problems data can have |
| `core/runtime` | Loads packages, runs kernels (wasmtime), `poly`, `feedback`, audio clips; the renderer |
| `core/script` | Runs the TypeScript (oxc + QuickJS) and the `@m2/core` SDK |
| `core/session` | Compiling, revisions, and the write path every edit takes: checking and writing Tx (the change a Tx makes to the graph, what it may change, the journal), the code editor's saves, taking in outside changes once they settle, the project lock, and undo. It knows no gesture |
| `crates/editor` | Every gesture as a Tx: knobs and their ids, cables, effects, notes, relinks and fixes, renames, automate; the views the app shows; and the offline render that Export Audio and `m2 render` share |
| `core/plugins` | CLAP plugin hosting |
| `core/play` | Plays a renderer on the audio device |
| `std/`, `std/kernels-src` | The standard package, and its kernels' Rust source |
| `apps/m2` | The command line |
| `apps/music2-desktop` | The Mac app (Tauri): window, menus, dialogs |
| `crates/server` | The app's local server: `graph.rs` (the open song), `files.rs` (the code editor's files), `project.rs` (new songs and the songs folder), `export.rs` (Export Audio), `resources.rs` (finding std, the templates and the examples), `routes.rs` (the API) |
| `web/` | The UI: `StartScreen`, `NewSongDialog`, `GraphCanvas` (top bar and panel), `SongTimeline`, `NodeGraph`, `NoteView`, `CodeView`, `EffectBrowser` |
| `templates/` | What new songs start from: `starter` and `empty`, each a whole song plus a `template.toml` (its name and description) |
| `examples/` | `first-song`, `resample`, `with-plugins` and `sounds`, each with an `example.toml` for the start screen (and the plugins it needs) |
| `e2e/` | The browser suites |
| `docs/` | Design docs and the gap list |

## Design docs

- [Song as a program](docs/song-as-program.md): the design of music2, and what's built so far.
- [Code and graph editing design](docs/code-graph-editing-design.md): how the app edits code.
- [Code and graph editing brief](docs/code-graph-editing-brief.md): the request that design answered (historical).
- [Architecture contract](docs/architecture.md): the rules every part is built and reviewed against, and the order of work.
- [What music2 can't do yet](docs/gaps.md): the gap list, most important first.
- [Music basics](docs/music-basics.md): music for beginners.
