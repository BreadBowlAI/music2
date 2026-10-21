# The music2 manual

**In short**
- This is the whole guide to music2: what you can do, the window, every key, how a song is written as code, the command line, and the local API for scripts and agents.
- New here? Start with [Your first song](#your-first-song). New to music? Read [Music basics](music-basics.md).
- What music2 can't do yet is in [What music2 can't do yet](gaps.md). How to build it and help is in [CONTRIBUTING.md](../CONTRIBUTING.md).

music2 is a music studio for the Mac and Linux: a DAW (digital audio workstation), the kind of app people make songs in, like GarageBand or Logic Pro. In music2 a song is a folder. Code in it says how the song sounds: the instruments, the effects and the mix. Data files say what plays: the notes, the automation (knob moves over time) and the audio.

The app shows that folder as a timeline, a graph of knobs and cables, a piano roll (a grid for drawing notes) and a code editor. Every change you make there is written into the files right away.

There's no AI inside the app. Scripts and AI agents (such as a Claude Code session) use the same files, and a local API (a way for programs on your computer to talk to the app).

## In short: what you can do

| You can | Where |
|---|---|
| Start a song from a template, or open an example | the start screen: **New Song…**, **Examples** |
| Add a track: 22 std sounds, a Sampler, or an empty audio track | **+ Track** (⌘T), or the browser on the left |
| Draw and edit notes | **Notes** (⌥2), the piano roll |
| Edit clips: move, copy, trim, split, join, repeat | the timeline |
| Turn knobs, add effects, connect cables | **Device** (⌥1) |
| Mix: faders, pans, mute, solo, and meters | **Mix** (⌥3), and each track's header |
| Share a reverb or a delay, or group tracks under one fader | **Mix** (⌥3) ▸ **Add bus** |
| Save a sound's knobs as a preset, or load one | **Presets** on an instrument's, an effect's or a plugin's card in **Device**. Yours are in `~/Music/music2/Presets`; a plugin's own are listed too |
| Draw automation (knob moves over time) | right-click a knob ▸ **Automate**, then draw on the timeline |
| Play live: the computer's keys, or a MIDI keyboard with its wheels | ⌘K; or plug a keyboard in |
| Hear a click on each beat | K |
| Record notes, or your voice or a guitar | R |
| Bring in WAV files, long ones too | File ▸ Import Audio… (⇧⌘I), or drag them onto the window |
| Keep an audio clip in time when the tempo changes | right-click the clip ▸ **Follow tempo** |
| Bring in a MIDI file as new tracks | File ▸ Import MIDI…, or drop the `.mid` file |
| Play a WAV file as an instrument | + Track ▸ Sampler, then drop a WAV on its card |
| Add a CLAP plugin, and open its own window | drag it from the browser's **Plugins** onto a track, or + Track ▸ **Plugins**; **Open window** on its box |
| Export the song, each track (stems), or the notes as MIDI | **Export** (⇧⌘E) |
| Edit the song's code, with type checks | **Code** (⌥4) |
| See what's wrong, in plain words, with fixes | the status pill in the top bar |
| Learn step by step | Help ▸ Make Your First Song |

Every change is written into the song's files at once, and Undo (⌘Z) takes it back.

## Run the app

You need a Mac, or Linux (Ubuntu 24.04), with [Rust](https://rustup.rs) and [Node.js](https://nodejs.org). On Linux, install the system libraries first:

```bash
sudo apt install pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libdbus-1-dev
```

Then, on either:

```bash
(cd web && npm ci && npm run build)                            # the window's UI: once, and after UI changes
cargo run --release -p music2-desktop                           # the app, on its start screen
cargo run --release -p music2-desktop -- ~/Music/music2/Demo    # or opened on a song folder
```

The start screen has three ways in:

- **New Song…** makes a new song from a template and opens it (see [Your first song](#your-first-song)).
- **Open Project Folder…** opens a song you already have. A song folder is any folder with a `music2.toml` file in it (see [The folder](#the-folder)). File → Open Project Folder… (⌘O) does the same at any time.
- **Examples** lists the examples. Picking one opens a copy of it in your songs folder, so the example itself stays as it is.

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

1. **Add a track.** Click **+ Track** above the track names (or press ⌘T), type to find an instrument such as Synth pluck, and press Enter. The new track appears below the selected one, ready to play.
2. **Turn a knob.** Click the **Song** row at the foot of the timeline. The dock below shows the mix as boxes with knobs. Drag a knob while the song plays and you hear it change. Let go, and the new number is written into `song.ts`.
3. **Add a clip.** In the timeline, double-click the Bass row past the end of the song. A one-bar clip appears, and opens in the dock's **Notes** view (a piano roll).
4. **Draw notes.** Double-click the grid to add a note (or press B for Draw mode, where a click adds one, a drag draws one as long as you drag, and ⌥-drag paints one in each grid step), drag a note to move it, and drag its right edge to make it longer.
5. **Add an effect.** Press ⌥1 (or click **Device**), then click **+** on a box that makes sound and pick an effect such as reverb, or drag one from the browser on the left onto a track.
6. **Undo.** Press ⌘Z to take these back, one at a time.
7. **Export it.** Click **Export** in the top bar (or File → Export…, ⇧⌘E), keep **Song**, and save. The song is written to a WAV file you can play anywhere or share; click **Show in Finder** when it's done (**Show in Folder** on Linux).

To learn it step by step in the app, open **Help → Make Your First Song**.

## The window

### The top bar

From left to right: the browser's toggle, back to the start, play/stop, record, loop, the metronome, where the song is (bar.beat.sixteenth, then minutes and seconds), the tempo, time signature and key, the song's name, a keyboard mark while a MIDI keyboard is plugged in, the status pill (only when something needs saying: see [Saving and undo](#saving-and-undo)), undo, redo, and **Export** (it shows how far along an export is). File → Close Song (⌘W) goes back to the start screen.

| To | Do this |
|---|---|
| Change the tempo | Drag it up or down: 1 BPM for every 4 pixels, a tenth with ⇧. You hear it as you drag, playing or not; letting go saves it, and Esc before you let go puts it back. (A song with an audio clip hears it when you let go.) Or double-click it and type `128`, `128.5` or `128 bpm` (20 to 999). |
| Hear a click on each beat | Click the **Metronome** button after Loop, or press K (Transport → Metronome). While the song plays it clicks each beat, higher on the first beat of a bar. Each song keeps its own setting; an export never has the click. |
| Change the time signature | Click it, then pick the beats in a bar (1 to 16 with the buttons, up to 64 typed) and the note a beat is (2, 4, 8 or 16). |
| Set the song's key | Click **Key** after the time signature, then pick the root (C to B) and Major or Minor; **No key** takes it away. Notes then shades the rows not in the key. It is kept in `layout.json`, and changes nothing you hear. |
| Rename the song | Double-click its name, type, then Enter. Escape cancels. The folder keeps its name. **File → Rename Song…** renames its folder too ("Also rename its folder", on at first): the song closes and opens again from there, so Undo can't go back past it. With unsaved code in the editor it says so first. |

Both fields change the tempo or time signature in effect where the playhead is. Each change is one Undo step, written in `data/arrangement.json` (the song's name in `music2.toml`). There is no Alt-click reset on the tempo: one stray click shouldn't change the song.

Play and stop work as in GarageBand: Space plays from the playhead, and Space again stops and leaves the playhead where it stopped, so the next Space plays on from there. Return (or the ⏮ button) goes back to bar 1; while playing, it plays on from there. Transport → Stop All Sound (⌘.) stops every note at once.

### Playing live

- **The keyboard piano:** ⌘K (Ctrl+K off the Mac), from anywhere, or View → Keyboard Piano, or the keyboard button in Notes. The computer's keys then play the selected track: A S D F G H J K L ; ' are the white keys, W E T Y U O P the black ones, and A is middle C at first. Z goes down an octave, X up. A card at the lower right of the timeline shows the keys and what plays. While it is on, those keys only play notes (Space, C and M work as always); typing in a field never plays. ⌘K again turns it off.
- **A MIDI keyboard:** plug one in, before or while the app runs. A message says it is connected, and a small keyboard mark shows in the top bar. Its keys play the selected track, as hard as you press them, and the sustain pedal holds notes. The pitch wheel bends the track's notes, up to 2 semitones each way; the mod wheel adds vibrato to the polysynth's sounds, and plugins hear both. Unplugged, its notes stop, the wheels go back to rest, and a message says so.
- To record what you play, press R: see [Recording](#recording).

Stop ends the notes, and echoes and reverb ring out as they would on a real instrument. Moving the playhead, looping, editing notes and adding an effect all happen while the song plays, without it starting over or jumping back. Notes already sounding where the playhead lands aren't picked up: playback starts with the next note.

### The timeline

- Each track has a row that draws its first notes (or audio) lane, and a row under it for each of its other notes lanes. Its automation lanes fold under it: the arrow by its name, or A, shows them, each with the value it plays at the playhead (see [Words you'll see](#words-youll-see)). The Song's own row is pinned at the foot. The ruler on top counts bars, following the song's time signatures.
- Tracks are listed in the order the song mixes them, until you move one. Drag a track's name up or down to move it; its colour stays its own. Both are kept in `layout.json`, which never changes the sound (see [The folder](#the-folder)).
- Click a track's row to select it: the dock below shows that track. Selecting never changes which view the dock is in.
- **+ Track** (⌘T) adds a track playing one of std's sounds: the drums first (the whole kit, then each drum), then the instruments, then the two synths with every knob, under Sound design. Typing filters the list, ↑↓ move, Enter adds; the list opens on the instrument you added last, so ⌘T then Enter adds another of the same. The new track goes below the selected one, in the first colour no other track has. It writes `tracks/<its key>.ts`, an empty notes lane in `data/`, and two lines in `song.ts`: its import, and its channel at the end of the mixer. One Undo takes it all back.
- Right-click a track's name (or click ⋯ beside it) for its menu: Rename, Change instrument…, Duplicate (⌘D), Delete (⌘⌫), Colour, its automation, and Edit in code. **Change instrument…** (also Track → Change Instrument…) opens the + Track list for that track: the one you pick replaces its instrument in its code, starting at its own settings, as one undo step. When code works out one of the old instrument's settings, it says so and leaves the code as it is. Duplicate copies the track's code, its clips, its plugins' saved settings and its mixer channel (a duck stays a duck), below it in its colour. Delete takes out its file, its import, its channel and its data. When another track's effect listens to it (the bass ducking to the kick), it asks first, and takes that effect off with it.
- Zoom with ⌘= and ⌘− (around the playhead), a pinch or ⌘-scroll (around the pointer), or the two buttons at the right end of the ruler. Z fits the selected clips, or the whole song; Z again goes back.
- Make a track taller or shorter: drag the bottom edge of its name (28 to 160 pixels), or press ⌘↓ and ⌘↑ (the selected track, or every track with none selected). A double-click on the edge puts it back. An automation lane has the same edge. **Hide this lane** in a lane's menu hides one lane; the line "1 hidden lane · Show" under the track brings it back.
- While the song plays, the view follows the playhead a page at a time. Scrolling by hand pauses it until play starts again. The Follow button at the right end of the ruler (⇧⌘F) turns it off. The zoom, the heights, Follow and hidden lanes are kept per song, in `.music2/view.json`.
- **Markers** (Intro, Verse, Chorus…): right-click the ruler, **Add marker**, or Transport → Add Marker at the playhead's bar. Type a name or pick one from the list. Click a marker to go there, drag it to move it (hold ⌘ to place it off the grid), double-click it to rename it, and right-click it to delete it or loop to the next one. ⌥← and ⌥→ go to the marker before or after. They are kept in `layout.json`.
- Click the ruler to move the playhead there. Drag across the ruler to loop those beats: playback goes round them until you turn the loop off with the loop button or C (it keeps the range, also when the song is opened again). With no loop set, the loop button loops the 4 bars from the playhead. ⌘L loops the selected clips (with none, the bar the playhead is in).
- While stopped, a click on an empty spot in a lane moves the playhead there too. While playing it doesn't, so a click to deselect never makes the song jump.
- The tempo and time signature changes are marks on the ruler. Click one to change it or remove it. Right-click the ruler to add a tempo change, or a time signature change at the start of that bar.
- While the song plays, a clip you drag (or its edge), an automation point or a note in Notes is heard where it is before you let go. Letting go saves it, as one undo step; Esc before you let go puts it back. (An audio clip is heard when you let go.)
- Double-click an empty spot in a notes lane to add a one-bar clip. Drag a clip to move it along its lane. Click a clip and press Delete to remove it. **Repeat…** in a clip's menu makes copies one after another (1 to 64), as one undo step.
- Click a notes clip to select it (Notes shows it); double-click it to open it in Notes.
- Double-click a track's name (or press ⌘R) to rename it: type, then press Enter (or click away) to save, or Escape to cancel. This changes only the name the app shows (the track's label, written in its `track()` call); its key, its data and its sound stay as they are, and Undo takes it back.
- **Mute and solo** while you listen: **M** and **S** on each track's header (and on its Mix strip), or press M or S for the selected track; ⌥-click S hears only that track. A pill at the top counts them ("1 muted · 1 solo"); click it for the list and Clear all. They change only what you hear, so Export asks first while one is on. **Mute in song** in a track's menu writes `muted: true` into its code, so the export leaves it out too: its name is struck through.
- **Meters:** a thin bar at the right of each track's name, and the Song row's, shows how loud it is. The status pill says **Heavy load** when the computer is near its limit, and **Sound dropped out** after a gap.
- **Edit clips:** drag a clip to move it (onto another track too), ⌥-drag to copy it, drag an edge to resize it. ⇧-click, ⌘-click or a drag across empty lanes picks several. ⌘E splits at the playhead, ⌘J joins, ⌘X ⌘C ⌘V cut, copy and paste at the playhead, Q quantizes, ← → nudge. Each clip has a right-click menu. The timeline's grid is in its corner (Auto, Bar to 1/64, triplets; ⌘1 to ⌘4); hold ⌘ to place a clip off the grid.
- **Draw automation** on the timeline: double-click an automation lane to make a clip, click its line to add a point, drag a point to move it, Delete removes it, and B paints. A frequency is drawn on a log scale. **Remove automation** in the lane's menu writes the value at the playhead back into the code.
- **Audio clips** show their waveform: see [Audio, recording and MIDI files](#audio-recording-and-midi-files).

### The browser

Down the left (⌥⌘B shows or hides it): the instruments (Drums, Instruments, Sound design) and the effects, by group. Type to search. Enter or a double-click on an instrument adds a track with it below the selected one, and one dragged onto the space below the tracks adds it there. Enter or a double-click on an effect adds it to the end of the selected track's sound, and an effect dragged onto a track or the Device view goes at the end of that sound.

### The dock below the timeline

It shows the selected track (or the Song) in one of four views, picked with the switcher in its header or ⌥1 to ⌥4: **Device**, **Notes**, **Mix** and **Code**. The view stays what you picked while you select other tracks. Drag the line between the timeline and the dock to resize it; double-click it, or press E, to hide or show the dock.

- **Device:** the track's sound as boxes and knobs.
  - Drag a knob up or down to hear it (300 pixels take it across its whole range; hold ⇧ for a tenth of that, to fine-tune); let go to write the number into the code. ⌥-click a knob to put it back at its default (the middle of its range when its control declares no default).
  - Double-click a knob or its number to type a value: `-3`, `2k`, `120ms`, `50%`, `L20` for a pan, or a pitch such as `C4` for a frequency. Enter writes it, Escape leaves it, Tab writes it and moves to the next knob. A knob you've clicked or tabbed to also takes ↑ and ↓ (with ⇧, ten steps), Return to type and Delete to reset; the scroll wheel steps it too. A run of steps is written as one edit.
  - Numbers read the way a mixing desk writes them: "−6.0 dB", "350 Hz", "1.2 kHz", "120 ms", and pans as "C", "L 20" or "R 35".
  - Right-click a knob for **Automate** and **Reset to default**. Automate gives the knob an automation lane of its own, under its track in the timeline: the code reads the knob from the lane (`pan: lane.control("hats-pan", "bipolar", 0.3)`), and the lane plays the old number until it has points, so nothing sounds different. Undo takes the lane out again. When a knob can't be automated (it's shared by several tracks, say), the item is greyed out and says why.
  - Double-click the track's name in the dock's header to rename it, as in the timeline.
  - Select the Song, and its mixer is one small box that says how many channels it has: the levels and pans are in the Mix view (its **Open Mix** link, or ⌥3). The master's volume and ceiling stay knobs there.
  - A knob whose number is in grey is a control the code leaves out, such as a bass's `drive` when the code says only `bass(notes)`: it plays the instrument's or effect's default. Turning it writes the number into the call (`bass(notes, { drive: 9 })`), and undo takes it out again.
  - A row that says **edit in code** is a value the app can't turn into a knob (see [Editing from the app](#editing-from-the-app)).
  - Cables: drag from a named output (the dot at the right edge of a box whose result has a `const` name in the code) onto a knob. The ✕ on a connected input disconnects it.
  - An effect card's ⋯ menu has **Turn off** (the sound goes on without it; the card stays, dimmed, "Reverb · Off", and `bypass: true` goes into its code) and **Turn on**, and **Move left** and **Move right**, which swap it with the effect next to it. Each is one undo step. A Duck's card says what it listens to ("Listens to: Kick ▾"): click it to pick another track.
  - **+** on a box that makes sound opens the effect browser: 17 effects in seven groups (Filter, EQ, Dynamics, Distortion, Delay & Reverb, Modulation, Utility), as the effects themselves declare them. Type to search, then press Enter or click. The bin icon on an effect removes it.
  - Zoom with ⌘ and scroll, or pinch. The buttons at the bottom right zoom and fit the graph to the panel.
  - An instrument's card says how it plays under its name, such as "Plays one note at a time." (the rest is in its tooltip).
  - A **Sampler**'s card takes a WAV file: drop one on it (see [Sounds and effects](#sounds-and-effects-std)).
  - A CLAP plugin's box has **Open window**, for the plugin's own window (see [CLAP plugins](#clap-plugins)).
- **Mix** (⌥3, or X): a strip for each track, in the timeline's order, and Master on the right.
  - Each strip has the track's colour and name, a pan knob, **M** and **S** (as on the track's header; a strip shared by two tracks acts on both), and a fader with its level under it. Beside the fader, two bars (left and right) show how loud the track is. They read the track before its fader, as the header's meter does. A small red dot over them means the track went over the top: click it to clear it. Master's meter is the whole song.
  - Drag a fader to hear it; let go to write it into the code. 0 dB sits three quarters of the way up. Hold ⇧ to fine-tune; ⌥-click puts it back at 0 dB; the very bottom is silence ("−∞", written as −96 dB). Scroll up or down over a fader to move it 0.1 dB a notch (scrolling sideways moves the strips). Double-click the level to type one: `-3`, `-3 dB` or `-inf`. A fader you've clicked or tabbed to also takes ↑ and ↓ (with ⇧, 1 dB) and Return to type.
  - A level in grey is one the code leaves out: it plays at 0 dB, and the fader's first move writes it into the mixer, in the style the mixer already uses (`kick: kick.out` becomes `kick: { in: kick.out, db: db(-3) }`).
  - A pan reads "C", "L 20" or "R 35". In grey, it isn't in the code yet: the first drag writes `pan: 0` into the channel once it has moved 4 pixels, then goes on, and letting go writes the pan. Undo once puts it back at C; undo again takes it out of the code.
  - Click a strip to select its track. Right-click it to rename or recolour the track, or for **Edit in code**, which opens `song.ts` at its channel. Right-click a fader, or a pan once it's written, for **Automate** and **Reset to default** (a pan in grey has the strip's menu).
  - **Automate** on a fader gives it a lane under the Song in the timeline, "Hats volume". The fader then follows the lane: it moves with playback, shows the lane's dot, and can't be dragged. Right-click it for **Show Hats volume lane** or **Remove automation**, which writes the level at the playhead back into the code.
  - A track that doesn't reach the mixer gets a strip that says so, with **Edit in code**. So does a song that mixes its tracks its own way.
  - **Buses.** A bus is a track with no notes. It plays what other tracks send it. **Add bus** (after the strips) makes one:

    | Bus | What it is for | How a track uses it |
    |---|---|---|
    | **Reverb** | one shared room for every track | turn up the track's **Reverb** knob |
    | **Delay** | shared echoes | turn up the track's **Delay** knob |
    | **Group** | one fader for several tracks (all the drums) | set the track's output to it |

    It writes `tracks/<its key>.ts` with `bus(…)` and a channel in the mixer. A bus strip comes after the tracks, with a merge mark before its name. It has M, S and a meter, like a track.
  - **Sends.** Every strip has a row for each bus. Click **+** to send the track there, at −12 dB, after its fader. Then the row is a small knob: drag it, as a fader. Right-click it for **Before the fader** (the track's fader no longer turns it down; a dot on the knob), **Automate**, **Remove send** and **Edit in code**.
  - **Output.** Under each fader: where the strip goes, **Master** or a bus. Click it to pick. A choice that would make a loop (a bus feeding itself) is greyed, and says why.
  - Each of these is one undo step. In the timeline, a bus is a row marked **Bus**, with no lanes.
- **Code:** the selected track's TypeScript, with type checks. The list on the left shows the song's files. Right-click a track's file for **Change track ID…**: the ID is the name code and files use (`track("bass", …)`, `data/bass/`). Changing it moves both, and the track's view settings, as one undo step; the track's sounds restart. ⌘-click a name to jump to where it's defined, even inside std (std's files open read-only). ⌘S (or the Save button) saves. When the code has an error, the last working version keeps playing, and the status pill in the top bar says so; its popover offers **Show in code**. A song whose code doesn't compile when you open it still opens: in its code, at the error, with its tracks and clips greyed in the timeline and nothing to play until a save makes it compile, and then it plays.
- **Notes:** the selected clip in a piano roll; with none selected, it says how to pick one.
  - Pick a snap grid (1/4 to 1/64, triplets, or off): new notes are a grid step long. A click selects; a double-click adds a note; a drag across empty space selects with a box. In Draw mode (B, or the pencil) a click adds a note, sounding while you hold it, and dragging sets how long it is (moving up or down keeps its pitch); ⌥-drag paints a note in each grid step it crosses. Drag a note to move it, its right edge to resize it.
  - The strip at the bottom sets velocity (how hard each note is played).
  - Set the clip's length in bars in the dock's header.
  - While the clip's data file doesn't read (the status pill lists it), the piano roll is read only, with a note in the dock's header saying which file: it shows the notes the lane played before, and editing them would overwrite the file. Fix the file and it can be edited again.
  - ⌘A selects all notes, ⌘D duplicates the selection, the arrow keys move it (with ⇧, by an octave or a bar), and Delete removes it. ⌘= and ⌘− zoom the roll while it has the keyboard.
  - Right-click a note (or empty grid) for the note menu: up or down a semitone or an octave, **Quantize** (Q: onto the grid), **Legato** (each note lasts until the next starts), **Humanize** (timing and loudness a little off, so it sounds played), **Swing** (Light, Medium, Heavy: every second note a little later), Duplicate and Delete. It acts on the selected notes, or with none, on every note of the clip, as one undo step.
  - A drum kit's rows say their drum (Kick, Snare, Closed hat) instead of C2, and so do its notes. With the song's key set, the rows not in the key are shaded (never on a drum kit).

### Help

- **Help → Make Your First Song** opens a guide at the top of the browser: seven steps that tick themselves as you do them.
- Hold the mouse over any button, knob or fader: its tooltip says what it does, in plain words, with its key.
- A mistake in the code is said in plain words with a fix, pointing at your own line: "Cutoff wants a frequency (Hz), but gets a level (dB). Write it in Hz, such as hz(800)." The original message is under **Details**.

## Audio, recording and MIDI files

- **Bring in audio:** File → Import Audio… (⇧⌘I), or drag WAV files onto the window: onto an audio track (at the bar under the pointer), onto another track (a new audio track right after it), or below the tracks (a new audio track for each file, named after it). An outline and a few words say where they go. + Track → Audio track adds an empty one. Each file is copied into the song's `assets/` folder (the same file twice is kept once). Files up to 3 hours long play from the disk, never whole in memory.
- **Edit an audio clip:** drag its edges to trim it, and the small squares at its top to fade it in or out. Its menu sets its volume, plays it backwards, or removes the fades. Each is one undo step.
- **Follow tempo:** right-click an audio clip and turn on **Follow tempo**. Change the song's tempo later, and the clip stretches to keep its beats; its pitch stays the same. The stretch is made in the background while the song plays on. Turn it off and the clip plays at its own speed again.
- **Bring in a MIDI file:** File → Import MIDI…, or drop a `.mid` file. Each part becomes a new track with a fitting sound (channel 10 plays the Drum kit), its notes in one clip. If the file's tempo differs from the song's, it asks: **Keep** the song's (Return) or **Use** the file's. One Undo takes the whole import back.

### Recording

- **Record notes:** select a track that plays notes and press R (or the red **Record** button after Play, or Transport → Record). Stopped, it counts in one bar of clicks (the button says 4, 3, 2, 1), then plays; playing, it starts at once. Play a MIDI keyboard or the keyboard piano: the notes show as you play. R again, or Space, ends it: one clip over whole bars, one undo step. Right-click the button for **Count in first** and **Snap notes to the grid**. While looping, the first time round is kept.
- **Record your voice or a guitar:** select an audio track and press R. It counts in, then records from the sound input into a new file in `assets/` ("Vox take.wav"). The take grows as a red clip, with a bar showing how loud the input is. R again ends it: one clip, one undo step; the file stays if you undo.
- **Settings → Sound input** picks the input. **Hear yourself while recording** plays the input through the track's effects and fader while it records. It is off at first: use headphones, as speakers feed back into the microphone. An export never has it.
- **When the input has a problem**, a message says so, and the status pill lists it: an input chosen in Settings that isn't plugged in records from another and says which; one unplugged during a take keeps the take up to there; a Mac that doesn't allow the microphone drops the take (it is only silence) and says where to allow it. With the disk full, or a take over 3.1 hours, the take is kept up to there.
- The song plays on through all of these.

## New songs, the songs folder and export

- **New Song…** (the start screen, or File → New Song…, ⌘N, even while a song is open) asks for a name, a tempo (the template's until you type one; Tab moves to it) and how the song starts:
  - **Starter beat**: six tracks (kick, clap, hats, bass, chords and a hook), 8 bars at 116 BPM, ready to play and change.
  - **Empty**: no tracks yet, so it plays silence. Its timeline offers **Add a track**. `song.ts` also has a drum track ready, switched off: delete the `//` in front of its two lines and save, and a Drums lane appears.
- New songs go in the **songs folder**, `~/Music/music2`, each in a folder named after the song. If the name is taken, the new one gets a number ("My song 2"): nothing is ever replaced. `Exports` there is taken by exports, so a song called "Exports" becomes "Exports 2". In the app, **Choose Folder…** in the New Song dialog saves it somewhere else.
- The start screen lists the songs you opened last, newest first, with their tempo and when you opened them; so does File → Open Recent. A click opens one. A song whose folder was moved or deleted says so, with × to take it off the list. The song that was open when you quit opens again at the next launch (not after Close Song, ⌘W).
- **Save As…** (File → Save As…, ⇧⌘S) saves a copy of the song under a new name and opens the copy; the song itself stays as it was. The copy has the song's files, but not its `.music2/` (mutes, view settings, undo). **Choose Folder…** puts it somewhere else (not inside the song's own folder).
- **Settings** (⌘, on a Mac, Ctrl+, elsewhere; music2 → Settings… on a Mac, File → Settings… elsewhere) chooses the sound output and its buffer size, the sound input and **Hear yourself while recording**, which MIDI keyboards play (untick one and it plays and records nothing), shows what plays now, and sets where new songs go (and their exports). A folder reached through a link is shown at its real place, as the songs made in it are. They are kept for this computer in `~/.music2/settings.json`, and apply at once: a change of output stops playback where it was. A song opens with no sound device too: it plays silently, and the status pill says **No sound output**. Plug one in, and within a few seconds the song moves to it and stops.
- **Examples** on the start screen lists the examples. Picking one copies it into the songs folder and opens the copy. An example that needs plugins you don't have (With plugins) says which, and nothing is copied.
- **Export** (the button in the top bar, or File → Export…, ⇧⌘E) asks what to make:
  - **Song**: the whole song as one WAV file (below).
  - **Stems**: each track as its own WAV file, before its volume and pan, in a new folder. Not yet for a song that plays plugins.
  - **MIDI**: the notes, as a MIDI file other music apps open, with a fitting General MIDI sound for each part and drums on channel 10. A notes clip's menu has **Export as MIDI…** too.
- **Song** writes a WAV file: 24-bit, stereo, at the song's sample rate (48 kHz unless its `music2.toml` says otherwise). It starts at bar 1 and goes to the end of the last clip, then on until the sound has died away (silent for half a second, at most 10 seconds more). A song longer than 3 hours (most likely a clip far past the rest) isn't exported: the message says in which bar the last clip ends.
  - The app asks where to save it, starting at `~/Music/music2/Exports/<song>.wav`. Opened in a browser, it saves there straight away, adding " 2", " 3"… rather than replacing a file, even one that appeared while it was exporting.
  - The button shows how far along it is, and you can keep playing and editing meanwhile. It exports the song as it was when you clicked. If you close the song or open another, the export carries on, and a message in the corner shows how far along it is.
  - A song with plugins pauses while it exports, as a tape would: the export borrows the plugins that play, so they don't need a second copy. The top bar says **Paused for export**. Afterwards play goes on where it paused, and a note held across the pause starts again. Edits made meanwhile are heard once the export is done.
  - If any part breaks while it exports (a sound, a plugin that crashes or stays stuck for 30 seconds, a file that can't be read, a stretch), the export fails and names the part and its track. No file is written with a part silent. Playback goes on.
  - Quitting while a song is being exported asks first: quitting stops the export, and no file is saved.
  - The file is written under a hidden name and renamed when it's complete. A hidden file left by an export that was cut off (the app was killed) is removed by the next export into that folder.
  - When it's done, a message gives the file's name and length, with **Show in Finder** (**Show in Folder** on Linux; in a browser, it says which folder).
  - `m2 render` writes the same file.

## Saving and undo

- There's no Save for the song: every change is written to the files at once. ⌘S only saves the code editor's text.
- A note edit is heard at once, while it's being written to disk. If the disk refuses it (it's full, say, or another program replaced the file at that moment), the edit is taken back and a message says why; if the disk is slow to answer, a message says the edit isn't saved yet, and another says when it is. The same goes for an edit an agent sends while the song plays: the app says when it was taken back.
- Every edit made in the app is checked before it's written: the app works out what the change does to the song and refuses it if it would do anything else. An edit that touches several files (automating a knob writes its code and a new lane file) is written as one, all or nothing.
- Undo (⌘Z) and redo (⇧⌘Z) cover knobs, cables, effects, notes, renames, automation, fixes and code saves. Their tooltips, and the Edit menu, say what they take back ("Undo Hats volume"), also for an edit made in another window or by an agent ("Undo Bass notes"). The history ends when you close the app. Undo is refused only when what it would put back has changed since (the same number, or the same clip, edited elsewhere); a change somewhere else in the file doesn't block it.
- Changes made in another editor, or by a script or git, show up once the other program has stopped writing for a moment (150 milliseconds, so usually within a quarter of a second). A file caught half-written never plays: a lane keeps the notes it had until its file reads again, and a code change that doesn't compile yet holds back the data saved with it, so a lane never plays empty in between. The app hears of changes from the system (FSEvents on a Mac, inotify on Linux), so an open song nobody is changing costs next to nothing, however many samples it has. On a network drive, which doesn't report other computers' changes, it reads the song's folder instead: every 50 milliseconds for a small song, less often for one with thousands of files, and at least once a second.
- If another program changes a clip while you're editing it in the piano roll, your edit of that clip is refused, saying which clip, and the other program's version stays. Edits to different clips of one lane both stay.
- One program writes a song at a time. While the app has a song open, `m2` commands that write it stop without changing anything and say so; ones that only read it, render it or bounce a track work. A song that's open for writing elsewhere (another music2 window, say) opens read-only: the status pill says **Read only**, and its popover says where it is open, with **Retry** to take it over once it's free. You can play it and see its changes; nothing offers to edit it meanwhile.
- The lock and the edits being written live in the song's `.music2/` folder, which git ignores. If the app or the Mac stops in the middle of writing an edit, the status pill says so when the song opens next, with **Finish** and **Discard**: either leaves every file as it was before the edit or after it, never half.
- Problems that don't stop the song, such as data no lane reads after a key was renamed in code, a data file that doesn't read, or a missing sample or plugin, are counted by the status pill in the top bar ("2 problems"); click it for each one, with its fixes (and its code, under Details). So is a part that breaks while the song plays: a sound, a plugin, an audio file that can't be read, a stretch that failed, or the sound input. That part goes silent alone, the rest plays on, a message says so once, and it stays listed, with its track, until it plays again (Transport → Stop All Sound starts a stopped sound again). A fix runs only when you click it, and Undo takes it back. When a track's key was renamed in code, **Move all to …** moves its whole data folder to the new key at once.

## Shortcuts

Every key is listed in the app too: Help → Keyboard Shortcuts (⌥K on a Mac, Alt+K on Linux). Single keys work when you aren't typing in a field or the code editor. On Linux, Ctrl does what ⌘ does on a Mac, Alt what ⌥ does, Shift what ⇧ does, and the app's labels spell them that way.

| Mac | Linux | What it does |
|---|---|---|
| Space | Space | Play, or stop where it is (the next play carries on from there) |
| Return or Home | Enter or Home | Back to the start (playing, it plays on from there) |
| C | C | Loop on or off |
| K | K | Metronome on or off |
| R | R | Record (again: stop recording) |
| M / S | M / S | Mute / solo the selected track |
| ⌘L | Ctrl+L | Loop the selected clips, or the bar the playhead is in (in the code editor, its own: select the line) |
| ⌘. | Ctrl+. | Stop all sound (in the code editor, its own: quick fix) |
| ⌘Z / ⇧⌘Z | Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| ⌘N | Ctrl+N | New song |
| ⌘O | Ctrl+O | Open a song folder |
| ⌘W | Ctrl+W | Close the song |
| ⌘S | Ctrl+S | Save the code editor's files |
| ⇧⌘S | Ctrl+Shift+S | Save a copy of the song (Save As) |
| ⇧⌘E | Ctrl+Shift+E | Export: the song, stems or MIDI |
| ⇧⌘I | Ctrl+Shift+I | Import audio |
| ⌘, | Ctrl+, | Settings |
| ⌘K | Ctrl+K | The keyboard piano on or off |
| ⌘Q | Ctrl+Q | Quit (on Linux, File → Quit) |
| ⌘T | Ctrl+T | New track |
| ⌘⌫ | Ctrl+Backspace | Delete the selected track. In a field or the code editor it edits the text instead: on a Mac up to the line's start, on Linux the word before |
| ⌘R or ⇧Return | Ctrl+R or Shift+Enter | Rename the selected track |
| ⌘D | Ctrl+D | Duplicate the selected notes, or with none, the selected track (in the code editor, its own: the next match) |
| ⌘A | Ctrl+A | Select all notes in the open clip |
| ⌘X / ⌘C / ⌘V | Ctrl+X / Ctrl+C / Ctrl+V | Cut / copy / paste clips (at the playhead) |
| ⌘E | Ctrl+E | Split the selected clips at the playhead |
| ⌘J | Ctrl+J | Join the selected clips |
| ← → (⇧: a bar) | ← → (Shift: a bar) | Move the selected clips or notes by the grid |
| ↑ ↓ (⇧: an octave) | ↑ ↓ (Shift: an octave) | Move the selected notes a semitone |
| ⌘1 / ⌘2 / ⌘3 / ⌘4 | Ctrl+1 / 2 / 3 / 4 | Narrower grid / wider grid / triplets / snap on or off |
| Delete | Backspace | Remove the selected clip or notes |
| Escape | Escape | Deselect |
| ⌘= / ⌘− | Ctrl+= / Ctrl+− | Zoom in / out: the timeline, or the roll while it has the keyboard |
| Z | Z | Fit the selected clips, or the song; again to go back |
| ⌘↓ / ⌘↑ | Ctrl+↓ / Ctrl+↑ | Taller / shorter tracks: the selected one, or all |
| ⇧⌘F | Ctrl+Shift+F | Follow the playhead on or off |
| ⌥← / ⌥→ | Alt+← / Alt+→ | Go to the marker before / after |
| Q | Q | Quantize: in Notes the selected notes (or with none, every note), else the selected clips' notes |
| B | B | Draw mode: notes, and automation points |
| ⌥-drag | Alt-drag | In Draw mode in Notes: paint a note in each grid step |
| A | A | Show or hide the selected track's automation (every track's, with none selected) |
| ⌥1 / ⌥2 / ⌥3 / ⌥4 | Alt+1 / Alt+2 / Alt+3 / Alt+4 | Device / Notes / Mix / Code (X: Mix) |
| E | E | Show or hide the dock |
| ⌥⌘B | Ctrl+Alt+B | Show or hide the browser |
| ⌥K | Alt+K | Keyboard shortcuts |
| ⇧-drag, ⌥-click, double-click | Shift-drag, Alt-click, double-click | On a knob: fine-tune, reset to its default, type a value |
| ↑ ↓, Return, Delete | ↑ ↓, Enter, Backspace | On a focused knob: step it (⇧ or Shift: ten steps), type a value, reset it |

## Words you'll see

New to music itself? [Music basics](music-basics.md) explains notes, beats, scales, synths, effects and mixing in plain words.

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
- **Stems:** each track exported as its own audio file, to mix or share elsewhere.
- **Take:** one recording, from Record to stop.
- **Sampler:** an instrument that plays an audio file on each note, higher or lower by the note.
- **Plugin:** an instrument or effect from another maker, installed on your computer (music2 plays CLAP plugins).

## How a song is written

The rest of this page is for writing songs as code, and for scripts and agents.

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
  layout.json          # the tracks' order and colours: shown, never heard
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
std = "0.4"
```

`arrangement.json` holds `tempo` (a list of `{ "beat", "bpm" }` changes, the first at beat 0) and `meter` (a list of `{ "beat", "beats", "unit" }` time signatures; empty is 4/4). A beat is a quarter note.

`layout.json` is written the first time you move, recolour or add a track: `{"order": ["kick", "hats", …], "tracks": {"kick": {"colour": "rose"}, …}}`. A colour is one of the twelve names (rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet, lime). A track that is missing for a while (commented out of `song.ts`) keeps its place and colour there, and keys the app doesn't know are kept as they are. If it stops being valid JSON (a git merge can leave `<<<<<<<` lines in it), the tracks show in their own order and colours, the status pill says so, and **Reset layout** there writes the default one over it (Undo brings the old text back).

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
| `time.beats`, `time.seconds`, `time.playing` | the song's position, and 1 while it plays (0 while stopped). A track gets `time` in its `fn`; a component's body imports it from `@m2/core` |
| `poly(notes, voice, { voices, mode, retrigger, steal, glide })` | plays a voice for each note at once (`voices`, 16); or one note at a time, each starting again (`mode: "mono"`) or sliding from the one held (`"legato"`) over `glide` seconds; `retrigger: false` carries a voice on into its next note; `steal` says which note stops when too many are held: `"oldest"`, `"quietest"` or `"none"` |
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

The app shows the same problems in the status pill's popover (click **Code doesn't compile** in the top bar) and in the code editor's list, with the file, the line and what's wrong, but without the part in square brackets.

Problems that don't stop the song (data no lane reads, a lane file that doesn't read, a missing asset or plugin) are printed by every `m2` command that opens the song, each with its code in square brackets, such as `[data.unclaimed]`, and the commands that fix it.

### Editing from the app

The code stays the source of truth; the app edits it for you, one small checked change at a time.

- A knob appears for a number written directly at an input: `0.3`, `hz(800)`, `db(-6)`, or a plain property like `{ pan: 0.3 }`.
- A knob also appears for every control a call leaves out, at the default its component declares (its number in grey). Its first turn writes the number into the call: as the next argument when that is where it goes (`lowpass(x, 1000)` becomes `lowpass(x, 1000, 0.8)`), else in the call's options (`{ release: ms(150), attack: 0.01 }`), else in new ones. A knob's range is the one its component declares.
- Computed values (`2 * 440`, a variable holding a plain number) and objects with spreads, getters or computed keys still work, but show **edit in code** instead of a knob.
- A shared value such as `const cutoff = hz(800)` shows as one box wired to every place it's used. Turning it changes all of them.
- Parentheses and comments around a number are kept.
- Before writing anything, the app compiles the new code and checks that only the intended thing changed: a knob changes only its number, a cable only what reads it, a rename nothing that plays.
- While the code doesn't compile, the last working version keeps playing and the knobs are locked.
- Cables inside one file, and adding or removing effects, also work. Cables between tracks, and adding or deleting other boxes, don't yet.
- Renaming a track writes, changes or takes out the `name` in its `track()` call (a name equal to the key takes it out, since the label defaults to the key). A name written as a variable is left alone: the app says to change it there.
- Automating a knob writes `lane.control("<key>", "<unit>", <the number as written>)` where the number was, adds `lane` to the track function's parameters when it isn't there (`() =>` becomes `({ lane }) =>`), and makes the empty lane file `data/<track>/<key>.json`. The key comes from the control's name (`decay`, or `hats-pan` for a mixer channel's), with the call's name in front only when the track has that key already (`pad-decay`); `{ name: … }` is written only when the key, read as words, isn't the control's label.

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
  - With `voices: 1` any of them, and any FM sound, plays one note at a time: a note that starts before the last one ends carries on from it, without starting again (legato), and the ones with a `glide` knob slide to it. When that note ends while an older one is still held, the older one sounds again, sliding back: hold A1 on the 808 and tap A2 over it for the classic slide up and back. Notes that start together count as one, the last of them, and each note keeps its own velocity. `synth.lead`, `synth.sub` and `synth.bass` start that way, and their card in Device says "Plays one note at a time". For a voice of your own, `mono(notes, voice, { glide })` is `poly(notes, voice, { mode: "legato", retrigger: false, glide })` (std 0.3.0).
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
- **Sampler** (`std/sampler`, std 0.2.1): `sampler` plays a WAV file from the song's `assets/` folder on every note, higher or lower by how far the note is from `root` (60, C4): `sampler(lane.notes("notes"), { sample: "pluck.wav" })`. Or one file for each range of notes, each from its root up to the next one's: `{ zones: { 36: "kick.wav", 38: "snare.wav" } }`. It plays the whole file however short the note (`mode: "oneshot"`), or stops with the note, fading over `release` (`mode: "gate"`). Knobs: `tune` (semitones), `level`, `attack`, `release`. In the app, drop a WAV on its card in Device. [`examples/sampler`](../examples/sampler) plays drums and a melody from three short files.
- **The first instruments** (`std/instruments`): `bass`, `kick`, `hat`, `pluck` and `pad`, simple versions built from the building blocks below, short enough to read and copy. For a song, the ones above sound fuller: `synth.bass` or `synth.sub`, `drums.kick`, `drums.hats`, `synth.pluck` and `synth.pad`. The templates still start new songs with the first ones.

[`examples/sounds`](../examples/sounds) plays every synth and drum, and the FM keys, bell and metal, in a short pop song, mastered near -11.5 LUFS.

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

Chorus, phaser and tremolo move only while the song plays. Stopped, they wait, and Play carries them on from there. The bitcrusher keeps crushing a tail after Stop, and at Play it picks up where it stopped. So these effects, and std's drums, sound as in the export when you press Play, however long the song sat first. Going back to bar 1 after playing is a jump: the wobble carries on from where it stopped, so that play may differ a little from the export. An oscillator or `noise` written straight into a track's code, outside a voice, runs on while the song is stopped, so it may start from another place than in the export. A plugin may run on too.

**`duck`** (`std/fx`) is the sidechain: `duck(bass.out, kick.env)` turns the bass down while the kick plays. It needs a second signal, so it's written in code rather than added from the browser.

**`mixer` and `master`** (`std/mix`): `mixer` adds tracks together. Each channel has a level, `db` (0 when left out), and can have a `pan`, `sends` to buses and a `to` (a group bus instead of the master): see [Buses, sends and groups](song-as-program.md#buses-sends-and-groups). `master` has a level too, then a limiter, so the song never clips. A level of −96 dB or lower is silence.

**Building blocks:** `std/dsp` (oscillators, noise, envelopes, filters, gain, pan), `std/math` (add, scale, blend, sum, pitch to frequency, ranges), `std/control` (`lfo`, `when`, `choose`) and `std/filter`.

`m2 kernels` lists every kernel with its typed inputs and outputs, and `m2 effects <song>` lists the effects (by their ids, such as `std:delay`) and where they can go.

## CLAP plugins

music2 can play installed CLAP plugins, such as [Surge XT](https://surge-synthesizer.github.io) and [Airwindows Consolidated](https://www.airwindows.com), as boxes in the graph:

```ts
const synth = plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("notes"), key: "lead", a_filter_1_cutoff_4092842705: 0.6 });
```

- The app lists the installed plugins: the browser's **Plugins** groups (one under the instruments, one under the effects), the Device view's **+**, and **+ Track**. Drag an effect plugin onto a track, or double-click it, and it goes after the track's sound. Pick an instrument plugin in + Track, and you get a new track playing it. Each is one undo step, and writes the `plugin(…)` call with a `key`, so its settings are saved. A plugin installed while the app is open shows after **Look again**.
- `m2 plugins` lists installed plugins; `m2 plugins clap:<id>` shows one plugin's inputs and knobs.
- A parameter is written as its name, then its CLAP id: `a_filter_1_cutoff_4092842705`. Only the id says which parameter it is, so a plugin update that renames its parameters changes nothing (music2 notes the new name). Written without its id (`a_filter_1_cutoff`), it is an error that names the full address.
- A song whose plugin isn't installed still plays: that plugin's node is silent (an instrument) or passes its audio through (an effect), and the song says which plugin is missing.
- `m2 plugin-state <song> <track>/<key> clap:<id>` saves the plugin's default settings (not the values your code gives it) to `data/<track>/<key>.state`, which the plugin keyed `<key>` on that track loads. Beside it, `<key>.state.json` says which plugin wrote it; a state without it, or written by another plugin, isn't loaded.
- Plugins run in a second program, beside the app. If a plugin crashes or hangs, its sound goes quiet, the app says so once, and it comes back by itself; the song plays on. A plugin stuck for 30 seconds is ended the same way.
- **Open window** on a plugin's box in Device opens the plugin's own window. A knob turned there writes its number into the code (one undo step); a knob a lane drives stays where you put it ("Latched"; **Re-enable** gives it back to the lane); any other change saves the plugin's settings a second later (one undo step).
- A plugin that plays late (a look-ahead limiter, say) is made up for: every other track waits for it, live and in exports.
- Plugins can't go inside `poly` (an instrument plugin plays all the notes itself).
- See [`examples/with-plugins`](../examples/with-plugins), which needs both plugins installed.

## The command line (`m2`)

`m2` works on song folders without the app. Build it once with `cargo build --release -p m2`. The program is `target/release/m2`; the examples below call it `m2`. It finds std, the templates and the examples the way the app does: at `$M2_STD`, `$M2_TEMPLATES` and `$M2_EXAMPLES`, else in this source tree.

| Command | What it does |
|---|---|
| `m2 new <folder>` | Makes a new song in exactly that folder (it refuses one that exists) from a template: `--template starter` (the default) or `--template empty`. `--example first-song` copies an example instead. |
| `m2 check <song> [--json]` | Runs the code and checks the graph and the timeline. `--json` prints the compiled graph. `m2 build` is the same. |
| `m2 render <song> <out.wav>` | Renders the song to a 24-bit WAV, the same file the app's Export (Song) writes: to the end of the last clip, then on until the sound has been silent for half a second (10 s past the end at most). It renders at the song's `sample_rate`, from `music2.toml`. `--from S` starts S seconds in (or `--from-bar N` at bar N, counted from 1), replaying the song up to there; `--seconds N` renders exactly N seconds. 3 hours at most. `--preroll P` starts only P seconds before instead of replaying: much faster late in a long song, for listening, but approximate, as it says (notes held from before then, and earlier tails, are missing), and never written into `assets/`. `--threads N` renders on N threads, the same file on any number. If a part breaks while it renders, it fails and names it. |
| `m2 kernels [--json]` | Every kernel and its typed ports. |
| `m2 bounce <song> <name>` | Renders a track, named by its key (`--track keys`), or the whole song to a new `assets/<name>.wav`; it never replaces one that's there. `--place keys.lane@beat` also puts it on an audio lane. |
| `m2 graph <song> [--json]` | The graph the app shows, as text. |
| `m2 controls <song>` | Every number the app can turn into a knob, with its id, including the controls a call leaves out (`tracks/hats.ts#1/@decay`, marked inherited). |
| `m2 set <song> <id> <value>` | Changes one, checked by the compiler. `--expect V` refuses unless it's V now. |
| `m2 automate <song> <id>` | Gives a knob an automation lane of its own, as the app's Automate does: the knob's number becomes the lane's fallback, so the song sounds the same. |
| `m2 label <song> <track> <name>` | Shows a track, named by its key, as `<name>`: only its `track()` call's name changes. |
| `m2 import <song> <file.wav>… [--track key] [--at bar]` | Brings WAV files into `assets/` and places their clips: on a track's audio lane, or each on a new audio track. One undo step. |
| `m2 mute-in-song <song> <track>… on\|off` | Mutes tracks in the song (every render and export), or unmutes them. |
| `m2 clips <song> <gesture> <clip>…` | One clip edit: `create`, `move`, `resize`, `split`, `join`, `duplicate`, `quantize`, `delete`. A clip is `track/lane:id` or `track/lane@9.3.1` (where it starts). |
| `m2 unautomate <song> <track>/<lane> [--at place]` | Takes a knob's automation out: the lane's value there goes back into the code. |
| `m2 add-track <song> <instrument>` | Adds a track playing an instrument (`synth.pluck` or `std:synth-pluck`), as + Track does. `--name "Lead Vox"` names it, `--key` gives its key, `--after kick` puts it below that track. |
| `m2 duplicate-track <song> <track>` | Copies a track, named by its key: its code, its clips (with new ids), its plugins' saved settings and its mixer channel. |
| `m2 delete-track <song> <track> [--cascade]` | Takes a track out: its file, its imports, its channel, its data and its place in `layout.json`. When an effect elsewhere listens to it (a duck), it lists them and changes nothing; `--cascade` takes the track out of them too. |
| `m2 layout <song> order <keys…>` / `colour <key> <colour>` / `reset` | Reorders or recolours tracks in `layout.json` (rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet, lime), or writes the default layout over one that doesn't read. No revision: the layout never changes the sound. |
| `m2 tempo <song> <bpm> [--at <bar>]` | Sets the song's tempo (20 to 999 BPM) in `data/arrangement.json`, or with `--at 9` a tempo change from bar 9. |
| `m2 meter <song> <n/d> [--at <bar>]` | Sets the song's time signature (`7/8`: 1 to 64 beats of a 1, 2, 4, 8, 16 or 32 note), or with `--at 9` a change from bar 9. |
| `m2 title <song> <name>` | Renames the song: its name in `music2.toml`. The folder keeps its own. |
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

While the app has a song open, the commands that write it (`set`, `automate`, `unautomate`, `label`, `add-track`, `import`, `mute-in-song`, `clips`, `duplicate-track`, `delete-track`, `layout`, `tempo`, `meter`, `title`, `connect`, `disconnect`, `add-effect`, `remove-effect`, `relink`, `delete-data`, `fix`, `tx`, `plugin-state` and `bounce --place`) stop at once, change nothing, and say which program has it. Make the edit in the app, or close the song there first. The other commands, and `bounce` without `--place` (it only makes a new file), work while it's open.

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

An agent can also send any edit as a **Tx**: the files it reads, each with the hash (or the text) it was planned on, the changes, and what they may do to the song. The app checks it as it checks its own edits, writes it as one undo step, and refuses it if a file changed under it or the song would change in any other way ([WRITE-3](architecture.md#the-two-write-primitives)). For example, deleting a data file no lane reads:

```bash
sha=$(shasum -a 256 data/kick/old.json | cut -d' ' -f1)       # in the song's folder
tx='{"id": "tx-delete-old", "base": {"data/kick/old.json": "sha256:'$sha'"},
     "ops": [{"op": "remove", "file": "data/kick/old.json"}], "expect": "identical"}'
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/check -d "{\"tx\": $tx}" | jq .checked.hash
curl -s -H "$H" -H 'content-type: application/json' $URL/api/tx/commit -d "{\"tx\": $tx}"
```

| Route | What it does |
|---|---|
| `GET /api/graph` | The open song: name, folder, `session`, the graph view (with `revision` and every knob's id), the timeline, the Mix view (`mix`: a strip per track with its fader's and pan's knob ids, `pan_insert` when its first pan can be written, and the lane a level follows), the effects, the instruments + Track offers (`instruments`), what is muted or soloed while listening (`overlays`), problems, where playback is, and the knob drags in progress (`leases`, each with its knob's id in that view). `has_code` is false while the song opened with code that doesn't compile; `read_only` is null, or `{why, message, holder}` when another program holds the song |
| `POST /api/graph/open` `{path}` | Open a song folder |
| `POST /api/graph/close` | Close it |
| `GET /api/graph/status` | Playing or not, seconds and beat, the tempo playing there (`bpm`), and the loop (`{start, end, on}` in beats, or null). A play, a stop or a seek just sent reads as done |
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
| `POST /api/graph/automate/remove` `{session, module, lane, beat?}` | Remove automation: the lane's value at `beat` (the playhead; else its fallback) goes back into the code where it was read, and the lane's file goes |
| `POST /api/graph/track/add` `{session, component, label?, key?, after?}` | Add a track playing an instrument from `instruments` (`std:synth-pluck`): its file, its import and mixer channel, its lane, and its place below `after` (else last). Answers with the new track's `module` and `label` |
| `POST /api/graph/track/duplicate` `{session, module}` | Duplicate a track: its code, clips (with new ids), plugin settings and channel, below it in its colour |
| `POST /api/graph/track/delete` `{session, module, cascade?}` | Delete a track: its file, import, channel, data and layout. When other code reads it, nothing is written and the answer is 409 with the reads (`dependents`, and `blocked` when they can't be taken out from here); with `cascade`, an effect that only listens to it (a duck) goes too |
| `POST /api/graph/track/mute` `{session, modules, muted}` | Mute tracks in the song, written in their code (or unmute them) |
| `POST /api/graph/overlay` `{session, mute?, solo?, exclusive?, clear?}` | Mute or solo tracks while listening (`{kick: true}`). Not a write: it changes nothing in the song, and works read-only |
| `POST /api/graph/layout` `{session, order? \| colour? \| reset?}` | Reorder the tracks (`order`: every track, top to bottom), recolour one (`colour: {module, colour}`), or write the defaults over a `layout.json` that doesn't read. Writes `layout.json` only, with no revision |
| `POST /api/graph/mix/insert` `{session, channel, control, value}` | Write a mixer channel's first pan (`control: "pan"`) into its entry; answers with the new knob's id (`control`) |
| `POST /api/graph/mix/bus` `{session, kind, label?, after?}` | Add a bus: `reverb`, `delay` or `group`. Answers with its `module`, `label` and the new Mix view (`mix`) |
| `POST /api/graph/mix/send` `{session, channel, bus, action, pre?}` | A send from a channel to a bus channel: `add` (before the fader with `pre`), `remove`, or `pre` (set before or after the fader). A loop is refused in words. Answers with `mix` |
| `POST /api/graph/mix/route` `{session, channel, to}` | Send a channel to a bus channel instead of the master, or (`to: null`) back to the master. A loop is refused in words. Answers with `mix` |
| `POST /api/graph/clips` `{session, gesture}` | A clip gesture (create, move, resize, split, join, duplicate, paste, delete, quantize), by clip id; 409 when a clip it names changed since |
| `POST /api/graph/audition` `{client, module, lane, on?, off?, keep?}` | Play or let go of notes on a lane, as a window does. Not a write |
| `POST /api/tx/check` `{session?, tx}` | Check a Tx: `{checked: {delta, hash, diagnostics}}`, writing nothing (409 when a file it's based on changed, 422 when it would do something else) |
| `POST /api/tx/commit` `{session?, tx}` | Write a Tx: `{committed: {revision, delta, hash, …}}`, or `{conflict}`, `{rejected}` or `{io}`. With `"expect": {"seen": hash}` it is written only while it makes the change its check showed |
| `POST /api/graph/lock/retry` `{session}` | Try again to take a song that opened read-only; answers like `GET /api/graph` |
| `POST /api/graph/undo`, `/api/graph/redo` `{session}` | Undo or redo |
| `POST /api/graph/transport` `{action}` | `play`, `stop`, `toggle` or `restart`. A stop leaves the playhead where it stopped |
| `POST /api/graph/panic` | Stop all sound: every note off, at once. It changes nothing in the song |
| `POST /api/graph/tempo` `{session, at, bpm?, add?, remove?, meter?}` | Set the tempo in effect at beat `at`; with `add`, a tempo change there; with `remove`, take out the change there, and with `meter` too the time signature change there, in one step. `loop_off` says when it turned the loop off |
| `POST /api/graph/meter` `{session, at, beats?, unit?, add?, remove?}` | The same for the time signature; a new change lands on the bar line at or before `at`, and later changes keep their bar |
| `POST /api/graph/title` `{session, name}` | Rename the song (`[project] name` in `music2.toml`) |
| `POST /api/graph/seek` `{beat}` | Move the playhead: a jump at the next block, playing or not; nothing is rebuilt |
| `POST /api/graph/loop` `{start?, end?, on?}` | Set the loop's range in beats (it turns the loop on unless `on` says otherwise), or turn it on or off over the range set. Answers like `/status` |
| `GET /api/graph/files` | The song's TypeScript files |
| `PUT /api/graph/file` `{session?, path, base, text}` | Save one file, only if it still says `base` |
| `GET /api/graph/types` | The SDK and package sources, for type checks |
| `POST /api/graph/unsaved` `{session, files}` | Tell the app which files have unsaved edits in an editor |
| `POST /api/graph/export` `{path?}` | Export the song: render it to a WAV file on a thread of its own, to `path` or to a new file in `~/Music/music2/Exports`. Answers `{path}` at once; one export at a time |
| `GET /api/project/templates` | What a new song can start from: `{templates, examples}` (each `{id, name, description}`), the songs folder (`songs`) and `home` |
| `POST /api/project/new` `{name, template?, example?, dir?, tempo?}` | Make a song from a template (`starter` if neither is given) or a copy of an example, in a new folder named after it in `dir` (a full path; default `~/Music/music2`), and open it. A taken name gets " 2", " 3"…, and so does `Exports` in the songs folder. An example whose `example.toml` lists `[plugins]` that aren't installed is refused, naming them. Answers like `GET /api/graph`, for the song it made |

The table lists the routes scripts use most. The rest (recording, imports, the Sampler, plugin windows, settings, the view, MIDI) are in `crates/server/src/routes*.rs`.

`/api/ws` is a WebSocket with three events: `{"t":"toast","text":…}` for messages, `{"t":"graph"}` when the song changed and should be fetched again, and `{"t":"export",…}` while a song is exported, each with the `song`'s name and its folder (`dir`): `progress` (0 to 1, at most ten a second), then `done` with `path`, `seconds` and `peak_db`, or `error`. Toasts sent while nothing was connected, such as why the folder given at launch didn't open, go to the next client that connects.

Or just edit the files: the app takes the change in once it has settled (no file changed for 150 ms), usually within a quarter of a second.

## Working on music2

How to build it, run the checks and find your way around the repository: [CONTRIBUTING.md](../CONTRIBUTING.md). The project's goals: [goals.md](goals.md).

## Design docs

- [Song as a program](song-as-program.md): the design of music2, and what's built so far.
- [Code and graph editing design](code-graph-editing-design.md): how the app edits code.
- [Code and graph editing brief](code-graph-editing-brief.md): the request that design answered (historical).
- [Architecture contract](architecture.md): the rules every part is built and reviewed against, and the order of work.
- [What music2 can't do yet](gaps.md): the gap list, most important first.
- [Music basics](music-basics.md): music for beginners.

## Credits

| What | Where | Licence |
|---|---|---|
| [Inter](https://rsms.me/inter/) 4.1, by the Inter Project Authors: the app's font on Linux and Windows (the Mac uses its own) | `web/public/fonts/` | SIL Open Font License 1.1 (`web/public/fonts/OFL.txt`) |
