# What music2 can't do yet

music2 is now only the song-as-program app: a song is a folder of code and data files, and the app is a view of that folder. The old engine, with its own song files, screens and command API, was removed in October 2026. Git commit `650f842` still has it. Songs saved by the old version (`.m2song` files) can't be opened by this one. Nor can song folders in the first folder format (`timeline/`, tracks named by their display name): the format changed once, in October 2026, to `data/` and keys ([Project format v1](architecture.md#project-format-v1)), and the examples and templates were converted by a script.

This page lists what's missing for making a whole pop song in the app, most important first. It starts with what already works, so it reads as a roadmap.

Each gap has a rough size:
- **small**: one focused change.
- **medium**: several pieces across the UI (the window), the server (the part of the app the window talks to) and the core (the part that compiles and plays the song).
- **large**: new engine work.

The numbers (#1, #2, …) are the order of importance. They stay the same in every table on this page.

The column "Did the old app have it?" says **Yes**, **No**, or **Partly**, with what the old app had.

## What works today

- Start a new song from a template, **Starter beat** (six tracks, ready to play) or **Empty**, with **New Song…** on the start screen or File → New Song… (⌘N). Songs go in `~/Music/music2`. **Open an Example** opens a copy of an example there.
- Open a song folder (File → Open Project Folder…, ⌘O) and play it. The start screen and File → Open Recent list the songs you opened last, newest first; a song whose folder is gone says "Moved or deleted". The song open when you quit opens again at the next launch.
- Keep versions of a song: **Save As…** (⇧⌘S) saves a copy under a new name and opens it, and **Rename Song…** renames the song and its folder. Both ask first when the code editor has unsaved edits.
- See every track on a timeline: notes, automation, masks and audio clips, with the tempo shown above the tracks and bars counted by the song's time signatures (written in `data/arrangement.json`).
- Draw, move, resize and nudge notes in the piano roll, and set how hard each one is played. Its grid goes from 1/4 to 1/64, with triplets. Every clip has an id, so an edit always lands on the clip it was made on. Right-click a note for its menu: up or down a semitone or an octave, Quantize (Q), Legato, Humanize, Swing (Light, Medium, Heavy), Duplicate and Delete, on the selected notes or, with none, the whole clip. A drum kit's rows say their drum (Kick, Snare, Closed hat) instead of C2.
- Edit clips on the timeline. Double-click empty lane space to make one. Drag a clip to move it (onto another track too), ⌥-drag to copy it, and drag either edge to resize it. ⇧-click, ⌘-click or drag a box to pick several, and ⌫ deletes them. ⌘D duplicates, ⌘E splits at the playhead, ⌘J joins, ⌘C ⌘X ⌘V copy and paste at the playhead, Q quantizes a notes clip's notes, and ← → nudge. Each clip has a right-click menu. The timeline has its own grid and snap, in its corner: Auto, Bar to 1/64, and triplets; ⌘1 to ⌘4 change it, and holding ⌘ places a clip off the grid. Each edit is one undo step; the same from `m2 clips`.
- Hear notes as you click, drag or draw them in the piano roll, and play the keys at its left. A speaker button in Notes turns this off. When a mute or a solo keeps the track silent, Notes says so ("Silenced by the Bass solo · audition is silent").
- Drag a number written directly at an input (`0.3`, `hz(800)`, `{ pan: 0.3 }`) as a knob: you hear the change at once, even while the song compiles, and letting go writes it into the code. Saves to other files during the drag (another editor, an agent) don't interrupt it, and nor do saves to its own file that leave its line alone. A number held in a variable, or worked out from other numbers, shows **edit in code** instead (#35).
- Every control of an instrument or effect that isn't connected to a signal is a knob, even one the code leaves out: `bass(notes)` shows its `drive` at the default, its number in grey, and turning it writes `{ drive: … }` into the call. Each knob turns through the range the instrument or effect declares (300 pixels of drag, ⇧ for fine), a double-click types a value (`-3`, `2k`, `L20`), and an ⌥-click puts it back at its default.
- Connect cables inside one file, and add or remove 17 effects from a searchable browser. An effect is known by what it is, not by its name in the code: `import { delay as echo }` is still the Delay.
- Write your own instruments and effects in TypeScript, with knobs, defaults and ranges, the way std's are written ([Your own components](../README.md#your-own-components)).
- Edit the code in the app, with type checks. When the code breaks, the last working version keeps playing. A song whose code doesn't compile when it's opened still opens, in its code, and plays once a save makes it compile.
- Undo and redo every edit made in the app. An edit is refused, rather than written, when it would change anything but what it says, and an undo is refused only when what it puts back was changed since. Undo and Redo say what they take back ("Undo Hats volume"), in their tooltips and in the Edit menu, for every step: one made in this window, another window, an agent or another editor ("Undo Bass notes").
- Change a track's instrument: Track → Change Instrument…, or the track's or its strip's menu, opens the same list and writes the new instrument into the track's code as one undo step. It is refused, in words, when code works out one of the old instrument's settings.
- Add a track without writing code: **+ Track** (⌘T) lists std's 22 sounds: the drums first, then the instruments, then the two synths with every knob. It adds the one you pick below the selected track. It writes `tracks/<key>.ts`, imports it in `song.ts`, gives it the mixer's last channel and an empty notes lane, as one edit Undo takes back. Duplicate a track (⌘D), with its clips, its plugins' saved settings and its mixer channel; delete one (⌘⌫), which asks first when another track's effect listens to it. Drag a track's name to reorder, and pick its colour from its menu: both are kept by name in `layout.json`, so colours never shift when tracks come and go. Change a track's ID (the name code and files use) from the Code view: right-click its file, **Change track ID…**; its code, its data folder and its view settings follow, as one undo step. The same from `m2 add-track`, `duplicate-track`, `delete-track` and `layout`.
- Mute and solo while you listen: **M** and **S** on every track's header and on every strip of the Mix view (a strip shared by two tracks acts on both, and shows a dash when they differ). Click them, ⌥-click S to hear only that track, drag across several, or press M or S for the selected track. A pill in the top bar counts them ("1 muted · 1 solo"); click it for the list and Clear all. They change only what you hear: the export still has every track, so Export asks first while one is on. **Mute in song**, in a track's menu (or `m2 mute-in-song`), writes `muted: true` into the track's code, so the export leaves it out too: its name is struck through and its clips are hatched. That is one undo step.
- See how loud it is: a level meter in every track's header, one on the Song row (the whole song), and two bars (left and right) beside every fader in the Mix view and on Master, with a peak hold, amber from −6 dB and red at 0 dB. They read each track before its fader. A small red dot over a strip's meter says it went over the top; click it to clear it. While the song plays, the status pill says **Heavy load** when the computer is close to its limit, and **Sound dropped out** after a gap in the sound.
- Mix with faders: the Mix view (⌥3) has a strip for each track, with its pan and a fader, and Master's fader. Drag a fader to hear it, let go to write it; the bottom is silence. A level or a pan the code leaves out is written into the mixer on its first move, as one edit Undo takes back.
- Rename a track's label without moving or silencing anything: double-click its name in the timeline (or the dock's header), or press ⌘R, or write `track("kick", { name: "Big Kick" }, …)`. Data is filed under the track's key.
- Automate a knob: right-click it, **Automate**. It gets an automation lane of its own, named after the control ("Decay"), which plays the knob's old value until it has points. Draw them on the timeline: double-click the lane to make a clip, click its line to add a point, drag a point to move it, ⌫ deletes it, and B paints. A frequency (a lane in Hz) is drawn on a log scale, as you hear it. **Hide this lane** in a lane's menu hides one lane; a line under the track says "1 hidden lane · Show". **Remove automation** (in the lane's menu, or the input's in the Device view) writes the value playing at the playhead back into the code; the same from `m2 unautomate`.
- See what needs attention. When a track's or a lane's key is renamed in code, its notes or automation stop playing; the status pill in the top bar says so, and its popover says it in plain words and offers to move the data where it belongs now (a renamed track's whole folder in one click), or to delete it. Undo takes either back. It also lists a data file that doesn't read (its lane keeps playing what it had), clips without ids, and a missing sample or plugin, each with its code and fixes. Every `m2` command that opens the song prints the same, with the commands that fix it (`m2 relink`, `m2 delete-data`, `m2 fix`).
- Edit the files in any other editor, from a script or with git. The app takes a change in once it has settled (no file changed for 150 ms), so a file caught half-written never plays and a lane never plays empty. Agents can also send checked edits (Tx) over HTTP or with `m2 tx`, written as one undo step.
- One program writes a song at a time: `m2` commands that write a song the app has open stop without changing anything, and a second window opens it read-only, with Retry. An edit cut off by a crash is offered back as Finish or Discard when the song opens next.
- Set the tempo and the time signature in the top bar. Drag the tempo up or down (⇧ for fine) or double-click it and type; click the time signature to pick it. Each changes the one in effect where the playhead is. You hear a tempo as you drag it, playing or not; it is saved when you let go, and Esc puts it back (a song with an audio clip hears it when you let go). Right-click the ruler to add a tempo or time signature change there, and click its mark to change or remove it. Click **Key** after the time signature to set the song's key (major or minor): Notes then shades the rows not in it, and it is kept in `layout.json`. Double-click the song's name to rename it. New Song asks for the tempo too. Each is one edit Undo takes back; the same from `m2 tempo`, `meter` and `title`.
- Zoom the timeline: ⌘= and ⌘−, a pinch or ⌘-scroll, the buttons at the right end of the ruler, or Z to fit the selected clips or the song (Z again goes back). In Notes, ⌘= and ⌘− zoom the roll. Make a track taller by dragging the bottom edge of its name, or with ⌘↓ and ⌘↑; an automation lane the same. While the song plays the view follows the playhead; scrolling by hand pauses it, and the ruler's Follow button (⇧⌘F) turns it off. The zoom, the heights, Follow, hidden lanes, each view's grid and snap and the audition toggle are kept per song, in `.music2/view.json`.
- Put markers on the ruler (Intro, Verse, Chorus…): right-click the ruler, **Add marker**, or Transport → Add Marker at the playhead's bar. Click one to go there, drag it to move it (⌘ held: off the grid), double-click to rename it, and ⌥← and ⌥→ go from one to the next. Its menu loops to the next marker. They are kept in `layout.json`.
- Repeat a clip: **Repeat…** in its menu makes 1 to 64 copies one after another, as one undo step.
- Hear a drag while the song plays: a clip or its edge, an automation point, or notes in Notes play where they are before you let go. Letting go saves it once, as one undo step; Esc puts it back. (An audio clip is heard when you let go.)
- Turn on the metronome: the **Metronome** button after Loop, K, or Transport → Metronome. While the song plays it clicks each beat, higher on the first beat of a bar. Each song keeps its own setting, and an export never has the click.
- Play the selected track live. ⌘K (Ctrl+K off the Mac) turns the computer's keys into a piano: A is middle C, the row above has the black keys, and Z and X change the octave; a card shows the keys. A MIDI keyboard plugged in plays it too, as hard as you press, with the sustain pedal, the pitch wheel (2 semitones each way) and the mod wheel (more vibrato on the polysynth's sounds; plugins hear it too), and a mark in the top bar says it is connected. **Settings → MIDI keyboards** turns one off: it then plays and records nothing.
- Record what you play: R, the **Record** button after Play, or Transport → Record. Stopped, it counts in one bar of clicks (the button says 4, 3, 2, 1), then the song plays and the take starts; playing, it starts at once. The notes show in the track as you play them, from a MIDI keyboard or the keyboard piano. R again, or Space, ends it: it becomes one clip over whole bars, selected, as one undo step ("Undo recording on Bass"). Right-click the button for **Count in first** and **Snap notes to the grid**. While looping, the first time round is kept. While the code doesn't compile, R says to fix it first.
- Record your voice or a guitar: select an audio track (+ Track → Audio track makes one) and press R, or the **Record** button. It counts in one bar, then records from the sound input into a new file in the song's `assets/` ("Vox take.wav"). The take grows on the track as a red clip, with a bar showing how loud the input is. R again, or Space, ends it: one clip, one undo step ("Undo recording on Vox"); the file stays if you undo. **Settings → Sound input** picks the input (the output's own device unless you choose one). **Hear yourself while recording** plays the input through the track's effects and fader while it records; it is off unless you turn it on, as speakers feed back into the microphone. An export never has it. With no input, or on a Mac that doesn't allow the microphone, R says so in plain words.
- Turn an effect off and on, or move it before or after the effect next to it, from its card's ⋯ menu in the Device view; pick what a Duck listens to from its card. Each is one undo step, kept in the code (`bypass: true`).
- Play and stop like Live: Space stops back where play started, and ⇧Space goes on from where it stopped. Click the ruler, or an empty spot in a lane while stopped, to start somewhere else. Loop with the loop button or C, a drag across the ruler, or ⌘L for the selected clips; the loop is kept for next time. ⌘. (Transport → Stop All Sound) stops every note at once.
- Choose the sound output and its buffer size, and the sound input, in **Settings** (⌘,), see what plays now, and set where new songs go. They are kept for this computer, in `~/.music2/settings.json`. A song opens with no sound device too: it plays silently, the status says "No sound output", and a device plugged in is used within a few seconds.
- Bring a MIDI file in: File → Import MIDI…, or drop a `.mid` file. Each part becomes a new track with a fitting sound (channel 10 plays the Drum kit), its notes in one clip. Dropped below the tracks, the new tracks go at the end; on a track, right after it; a one-part file dropped on a notes track goes into it. An empty song takes the file's tempo; otherwise a question: "groove.mid is at 128 BPM. The song is at 120 BPM." **Keep 120** (Return) or **Use 128**. One Undo takes the whole import back.
- Export (the Export button, or File → Export…, ⇧⌘E) asks what to make: **Song**, one WAV file, the same file `m2 render` writes; **Stems**, each track as its own WAV file, before its volume and pan, in a new folder (not yet for a song that plays plugins); or **MIDI**, the notes as a MIDI file other music apps open, with a fitting General MIDI sound for each part and drums on channel 10. A notes clip's menu has **Export as MIDI…**. Files go in `~/Music/music2/Exports` unless you pick another place in the app's save window; then Show in Finder. Quitting during an export asks first.
- From the command line (`m2`): make a new song, render a WAV, bounce a track to audio, and make the same edits as the app.
- Build `music2.app` and move it anywhere: std, the templates and the examples are inside it, so it no longer needs the source folder it was built from. It's signed ad hoc but not notarized, so on another Mac it opens only after **Open Anyway** in System Settings → Privacy & Security (and only on Apple silicon).
- Play CLAP plugins such as Surge XT, with automation and saved settings. A plugin is added by writing it into a track's code (#32). A song whose plugin isn't installed still plays: only that plugin's node is silent, or passes its audio through. Open a plugin's own window from its box (**Open window**). A knob turned there changes the number in the code (one Undo step); a knob a lane drives stays where you put it ("Latched", **Re-enable** gives it back to the lane); any other change saves the plugin's settings a second later (one Undo step). A preset the plugin loads by itself can't overrule the code: music2 sets those knobs back and says so.
- Keep playing when one sound breaks. If a sound fails while the song plays (a bug in it, or it asks for more than the 32 MB a sound may use), only that sound goes silent. The rest plays on, and a message says which sound, on which track, and that Transport → Stop All Sound starts it again. An export never leaves such a sound out: it stops, and names it.
- Play and export big songs on several cores. Playback uses the fast cores that share a cache (4 on an M1 Pro) as real-time audio threads; where the computer refuses real time (some Linux setups), it plays on one core. An export with nothing playing uses every fast core. Either sounds exactly as on one core. `M2_THREADS`, or `--threads` on `m2 render` and `m2 bounce`, sets how many. A song with CLAP plugins plays on one core for now.
- Bring in audio: File → Import Audio… (⇧⌘I), or drag WAV files from the Finder onto the window: onto an audio track (at the bar under the pointer), onto another track (a new audio track right after it), or below the tracks (a new audio track for each file, named after it). While you drag, an outline and a few words say where they go. + Track → Audio track adds an empty one. Each file is copied into the song's `assets/` (the same file twice is kept once), and its clip shows its waveform. Drag a clip's edges to trim it and the small squares at its top to fade it; its menu sets its volume, plays it backwards, or removes the fades. Each is one undo step. The same from `m2 import`. The engine also plays pitch and warp, written by hand in JSON.
- Keep an audio clip in time when the tempo changes: right-click it and turn on **Follow tempo**. It sounds the same at the tempo now; change the tempo later and it stretches to keep its beats, its pitch the same. The new stretch is made in the background while the song plays on. Turn it off and the clip plays at its own speed again. Each is one undo step.
- Learn as you go: **Help → Make Your First Song** opens a guide at the top of the side browser, seven steps that tick themselves as you do them. Every button, knob and fader has a tooltip in plain words, with its key. A mistake in the code is said in plain words with a fix, pointing at your own line ("Cutoff wants a frequency (Hz), but gets a level (dB). Write it in Hz, such as hz(800)."); the original message is under Details. [Music basics](music-basics.md) explains the words.
- Play pop sounds from std: a supersaw, a gliding lead, a pad, a pluck, an 808 sub, a synth bass and strings on one polysynth (`std/synth`), FM keys, bells and metal (`std/fm`), and an 808/909 drum machine whose kit plays kick, snare, clap, hats, toms and more from one lane by General MIDI note, with an open hat cut off by the next hat and a level for each group of drums (`std/drums`). Each has a few named knobs; `synth.custom` and `fm.custom` have every one. One note at a time, a note still held sounds again when the one over it ends, as on a monosynth ([Sounds and effects](../README.md#sounds-and-effects-std)).
- Play a WAV file as an instrument with std's **Sampler** (+ Track ▸ Sampler): every note plays the file, higher or lower by how far the note is from its root (C4 unless the code says). Drop a WAV on its card in Device and it plays that file, as one undo step. In code it can play one file for each range of notes (`zones`), stop with the note (`mode: "gate"`), and be tuned and faded in and out. The example **Sampler** plays drums and a melody from three short files.
- Hear a song as it exports. std's chorus, phaser and tremolo wobble only while the song plays, and the drums' noise follows the hits, so they sound as in the export when you press Play, however long the song sat first. An oscillator or noise written straight into a track's code, outside a voice, or a plugin, may still run on while stopped. Going back to bar 1 after playing is a jump: the wobble and the drums' noise carry on from where they were, so that play may differ a little from the export.

## The top ten

1. ~~Swap a track's instrument~~: done (see What works today).
2. ~~More instruments~~: done, in `std/synth` and `std/fm` (see What works today).
3. ~~Drum names in the piano roll~~: done. Showing only the drums a clip uses is left (#3).
4. ~~Mute and solo on the Mix view's strips~~: done, with a meter on each strip.
5. **Fast seeking and fast edits** in a full-length song. (large)
6. ~~Key and scale~~: the song's key is done, major and minor. Other scales come with scale help (#16).
7. ~~Loop~~: done (see What works today).
8. ~~Hear notes as you draw them~~: done (see What works today).
9. ~~Clip editing~~: done, repeating a clip too (see What works today).
10. ~~Rename a track's key~~: done, as **Change track ID…** in the Code view.

## Everything, by area

### Projects and files

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 12 | **A picture of each song in the recent list** (taste.md SS-2). The list itself, File → Open Recent and reopening at launch are done (`~/.music2/recent.json`). | A small picture of each song's arrangement helps find the right one. | Yes (the Song Library) | small |
| 28 | **Open a song by dropping its folder** on the window or the Dock. (Save As and Rename Song, with its folder, are done.) | Finding a song in the Finder is how many people open it. | Yes | small |

### Packages

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 39 | **Shared sound packages**: list the packages a song uses in `music2.toml`, fetch them (for example `github:owner/repo@tag`), pin them in `music2.lock`, and browse and add them from the app. | Today only std can be loaded, so sounds other people make can't be used, except as CLAP plugins. A package already declares its instruments and effects in TypeScript, with their knobs and effect-browser categories, so what's missing is fetching and loading it. | Partly (single sounds from GitHub, with `music2-cli plugin add`) | large |

### Export

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 36 | **The rest of export**: MP3 or AAC export, and stems of a song that plays plugins. (Saving the song or a clip as a MIDI file, bringing a MIDI file in as new tracks, stems, and the Export dialog work: see [What works today](#what-works-today).) | Sharing ideas with other apps and people. | No | medium |

### Tracks and instruments

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 1 | **The rest of changing an instrument**: a track whose instrument is built from parts, has two instruments or a plugin, or whose settings code works out, is refused in words and changed in code. Adding a track to a song that mixes its tracks without std's mixer, or picking a CLAP instrument (#32), still needs code. (Change Instrument… and + Track work: see [What works today](#what-works-today).) | Changing your mind about a sound shouldn't mean a new track. | Yes | small |
| 10 | **The rest of changing a track's ID**: a track with a new ID restarts its sounds (its plugins' and instruments' state isn't carried over, design-8b §11.3), and the status popover has no Restore for a rename done by hand in code (DIAG-6). (Change track ID… works: see [What works today](#what-works-today).) | A rename shouldn't cut a sound that is playing. | Yes | small |

### Sounds

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 3 | **Drum rows folded to the drums a clip uses**, and the piano roll's zoom up and down (taste.md NT-5). The names are done: an instrument names its notes in its descriptor (`editor.notes`), and Notes shows them. | A kit has 30 rows; a beat uses five. | Yes | small |
| 22 | **The rest of the sampler** (std's Sampler is done: see What works today): loop points, start and end, reverse, velocity layers, round robin and choke groups; zones made by dropping several WAVs (they are code only), and a waveform and a root picker on its card; files other than WAV. | Sampled drums and vocal chops are pop staples. | Yes | medium |
| 26 | **Presets**: save and load an instrument's or an effect's settings by name, a preset browser, and presets for CLAP plugins. | Beginners start from presets, not from blank knobs. | No | medium |

### Mixing

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 20 | **Level meters**: the loudness for export, and meters after the fader. A meter in every track header, on the Song row and on every Mix strip and Master is built, with a peak hold and red above 0 dB. They read each track before its fader, so a fader move doesn't move them: after-fader taps need the engine (`core/runtime`). | Without meters after the fader you can't see what a fader does to a track's level. | Partly (a meter on each track) | medium |
| 27 | **Sends and buses** in the UI: a shared reverb bus, group buses, and sending a track to a bus. | It's standard mixing practice, such as one reverb shared by every track. Each change is a structural code edit in `song.ts`. | Yes | large |

### Song settings

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 6 | **More scales** than major and minor (dorian, pentatonic…). The song's key is done: the top bar's Key field, kept in `layout.json` as `key`, and Notes shades the rows not in it. Scale help (#16) builds on it. | A key tells you which notes fit. | Yes | small |

### Playback

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 5 | **Fast seeking and fast edits** in a full-length song. A persistent live engine where a seek is a transport event, never a replay, and a notes edit swaps only that lane's data (step 3 of the [order of work](architecture.md#order-of-work); [RENDER-8](architecture.md#live-engine-and-its-two-clocks), [RENDER-9](architecture.md#seek-and-loop)). | Today a seek replays the song from 0, and every notes, cable, effect or code edit rebuilds the player at the playhead the same way. The example renders about 30 times faster than real time, so late in a busy 3-minute song each seek or edit could stall for several seconds. | Yes | large |
| 19 | **The click's level and sound.** (The click, its button and the count-in before recording are done: see [What works today](#what-works-today).) | Some songs need a quieter or a different click. | No | small |

### Timeline and clips

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 9 | **The rest of clip editing**: a box drag that scrolls the timeline at its edge; copied clips in the system's clipboard, so they paste into another window; a flag on the ruler where the song ends (taste.md TL-12). (Making, moving, copying, resizing, splitting, joining, pasting, quantizing and repeating clips work: see [What works today](#what-works-today). Audio clips come with #18.) | Pop songs are built by copying parts. | Yes | small |
| 23 | **Sections**: copy or move a whole section (a chorus) across all tracks, and insert or delete bars. Markers on the ruler are done. | Pop songs are arranged in sections, and copying a chorus is one gesture in other DAWs. Sections are designed but not built. Inserting bars touches every lane and the tempo. | No | large |

### Notes

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 15 | **The rest of the note tools**: copy and paste notes between clips, holding ⌘ to place a note off the grid, swing amounts other than Light, Medium and Heavy, and groove templates. (The note menu's transpose, Quantize, Legato, Humanize and Swing work: see [What works today](#what-works-today).) | These clean up a drawn or played part. | Partly (humanize, and octave up and down) | small |
| 16 | **Scale help**: lock the notes to the key, chord stamps and an arpeggiator. (Choosing the key and shading the rows not in it work: #6.) `std/theory` and `std/gen` are designed but not built. | Beginners write better melodies and chords with a scale guide. | Partly (a key and scale, and showing only in-key notes) | medium |

### Automation

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 14 | **Automation comforts**: curves between points (an ⌥-drag, which needs a field in the lane's file format and new interpolation in the engine), a log scale for other units than Hz (times), and typing a point's value. (Drawing, a frequency on a log scale, and hiding one lane work: see [What works today](#what-works-today).) | Sweeps sound smoother on a curve. | Partly (drawing points) | medium |

### Audio

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 18 | **The rest of audio in the app**: place an audio clip's warp markers by hand (Follow tempo works: the clip menu), join audio clips, clear out audio files no clip uses, and import MP3, AIFF or FLAC. (Importing WAV files up to 3 hours long, which play from the disk, drops, audio tracks, waveforms, trims, fades, volume and reverse work: see [What works today](#what-works-today).) | A loop at another tempo has to be stretched by hand today, and every file stays in `assets/` until you delete it. | Yes | medium |

### Recording and MIDI

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 24 | **The rest of recording notes**: pitch bend and the mod wheel; recording over a clip into it (overdub), and one clip for each time round a loop; the computer keyboard's velocity and sustain. (Recording from a MIDI keyboard or the computer's keys, with a count-in and snap, and choosing which keyboards play work: see [What works today](#what-works-today).) | Expressive parts use the pitch wheel; loop recording builds a part in layers. | No | medium |
| 25 | **The rest of recording audio**: an arm button (hear yourself without recording), takes stacked per loop pass, punch in and out, inputs 3 and 4, and a recording offset. (Recording from a sound input onto an audio track, and hearing yourself while recording, work: see [What works today](#what-works-today).) | Singers stack takes and fix one line by punching in. | No | medium |

### Effects and plugins

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 21 | **The rest of the effect comforts**: effect presets, the old Pump (ducking in time with the beat, with no sidechain), and moving effects that aren't written one inside the other, or across files (refused in words today). (Turning an effect off and on, moving it next to its neighbour, and picking what a Duck listens to work: see [What works today](#what-works-today).) | Presets are how beginners start; Pump is a pop staple. | Yes | medium |
| 32 | **Add an installed plugin from the app**: list installed CLAP plugins (such as Surge XT) in the effect browser and when adding a track, and put one on a track as an instrument or an effect. | Today a plugin goes on a track only by writing `plugin("clap:…")` into its code, after finding the plugin's id with `m2 plugins`. | No | medium |
| 33 | **VST3 and AU** support. (A CLAP plugin's own window opens from its box since ph-2, and a plugin that reports latency is made up for since ph-3: see Loose ends.) | Many popular synths and mastering plugins come only as AU or VST3 on the Mac. | No | large |

### Safety and undo

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 29 | **Undo that survives closing the app.** (Readable undo names, "Undo Hats pan", work for every step: see [What works today](#what-works-today).) | Closing the app, or the song, loses all undo. | Partly (its undo was lost on closing too) | medium |
| 30 | **Crash safety**: keep unsaved code-editor text through a crash, run CLAP plugins in a separate process so a crashing plugin can't take the app down, and simple project snapshots or backups. | Edits are on disk at once, and an edit cut off by a crash is offered back as Finish or Discard, so no file is left half written. But a plugin crash closes the whole app, and unsaved code is lost. | No | large |

### Settings

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 31 | **The rest of Settings**: view settings that hold for every song (today zoom, grids and the audition toggle are kept per song). (Choosing the output and buffer size, the sound input and hearing yourself, seeing what plays, where new songs go, and which MIDI keyboards play work: see [What works today](#what-works-today).) The sample rate stays the song's (`music2.toml`, [RENDER-31](architecture.md#sample-rate)). | Most people want one zoom for every song. | Partly (it showed the device and the folders, with nothing to choose) | small |

### Help

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 11 | **The rest of plain-language help**: a missing plugin still reads technically (Loose ends), and the docs past [Music basics](music-basics.md) and the README's first sections are technical. (The first-song guide, a tooltip on every control, and code mistakes in plain words work: see [What works today](#what-works-today).) | Many people who open music2 are new to making music. | Yes | small |

### Code editor

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 34 | **Create, rename and delete files** in the code editor (today it only opens existing `.ts` files), and edit the data JSON and `music2.toml` there. | A track can be written in `song.ts` (the Empty template shows how), but giving it a file of its own, as the Starter beat's tracks have, still needs another editor. | No | small |

### Knobs

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 35 | **Knobs for more numbers**: a number held in a variable (`const cutoff = 800`) or worked out from others (`2 * 440`) shows **edit in code** today, and settings such as an instrument's number of `voices` aren't shown at all. Follow a variable back to the number written in it, and show settings as fields. | A beginner who gives a number a name, as many tutorials teach, loses its knob. | No | medium |

### Scripts, agents and CLI

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 37 | **CLI basics**: `m2` commands for what the app's polish added: change instrument, markers, the song's key, a track's ID, Save As. Agents can send these as JSON to the app's routes, or as a Tx. (`m2 import`, `tempo`, `meter`, `title` and `add-track` work.) | Scripts and agents need the same basics as the app. Each is small once the matching session edit exists. | Yes (in `music2-cli`) | small |
| 38 | **A self-describing API**: every `/api/graph` operation listed with a JSON Schema, a short summary of the song, and MCP. (Export over HTTP is there: `POST /api/graph/export`; so are checked edits of any files as one undo step: `POST /api/tx/check` and `/api/tx/commit`.) | The old command API let Claude Code drive the app. The new routes work, but an agent has to read the README or the source code to learn them, and a Tx's JSON by the [architecture contract](architecture.md#the-two-write-primitives). | Yes | medium |

## Never in either version

Every row marked **No** above was missing from the old app too. Of the rows marked **Partly**, the old app also lacked: copying notes between clips (#15), chord stamps and an arpeggiator (#16), loudness and meters after the fader (#20), and undo that survives closing (#29).

## Loose ends

- **A knob let go while the code doesn't compile** keeps playing its new value, and is written into the code once the code compiles again; after 30 seconds it is dropped, with a message saying so. Writing it at once, when the broken file is another one, is the owner's call (C21 of the step 6 design): the rule today is that an edit of code waits for all the code to compile.
- **The code editor reloads a file an edit changed** rather than following the edit in place; unsaved text in it is kept, with the "changed on disk" choice.
- **A file added to a renamed track's folder afterwards** (a Finder copy, a `notes.txt`) isn't noticed until something else changes: the status popover still offers "Move all", and moving the folder is then refused, naming the file and saying to move it out or delete it. Added before the rename, the folder is offered file by file, and the problem names it.
- **Two literals edited on neighbouring lines by another program** count as a change to the calls between them, so the song is rebuilt (crossfaded) rather than having its numbers written into what plays. A change a line or more apart from the next is written in place.
- **A missing plugin reads technically**: ``no CLAP plugin `org.surge-synth-team.surge-xt` is installed (looked in [...])``. It no longer silences the song: only that plugin's node is silent, or passes its audio through ([FMT-18a](architecture.md#plugin-state-and-parameters)). Open an Example still names missing plugins plainly, from the example's `example.toml`, and refuses to copy the example (#11).
- **An error in the browser console with no project open.** `GET /api/graph` answers 400 when nothing is open, which is how the window knows to show the start screen. The browser logs that request as an error even though nothing is wrong. Answering "no project" without an error status would be quieter.
- **The browser suites** live in `e2e/` (puppeteer, with its own `package.json`), rebuilt after a reboot wiped their old scratch folder.
- **The dock and the browser panel start over at each launch of the app** (which view the dock shows, and its height). They are kept in the browser's storage, which starts empty since the app's page gets a new address each time. Every other view setting is kept per song in `.music2/view.json`.
- **Renaming a song's folder starts Undo over**: the song closes and opens again from the new folder (Rename Song… says so). Renaming only the name is one undo step.
- **Checks owed on a Mac**: a trackpad pinch to zoom, Open Recent in the menu bar, reopening at launch, ⇧⌘S, and a Ctrl-click on a marker (a right-click there); a real MIDI keyboard (plug, play, the pedal, unplug, and recording from it), the metronome and the count-in against the beat, switching the sound output in Settings; dropping files from the Finder, the Mac's own open and save windows (Import Audio, Export MIDI), and stems opened in another app. They are checked on Linux and in the browser suites, with notes posted as a keyboard would send them (see HANDOFF).
- **A recording waits while the code doesn't compile.** A take that ends then is kept, and written once the code compiles; closing the song before that loses it (a message says so).
- **A nudged clip is heard once it lands** (← →), and so is a dragged audio clip (it needs renders); a dragged notes or automation clip is heard while it moves.
- **A tempo drag in a song with an audio clip** is heard only when you let go: the clip's audio is timed in the tempo before the drag. Its tooltip says so.
- **The output list on Linux** shows each card once, by its name, from the device a person means (the sound server's, else the card's shared one). An output that won't open plays on the default, and Settings says "the output chosen can't be used now".
- **An output chosen in Settings that comes back** after it was unplugged isn't taken again by itself, and choosing it again while the default plays for it does nothing: choose another output, then it.
- **A stretch is made from the clip's start**: after a tempo change deep inside a long followed clip, it is silent until its new stretch reaches the playhead (on Linux about 300 times faster than real time).
- **A tempo change while a followed clip plays** may wait up to half a second before the new tempo is heard, while its new stretch reaches the playhead.
- **A quarter second that holds a plain clip and a stretch being made** is silent, the plain clip too, until the stretch reaches it.
- **A stretch being made reads as a slow disk**: while a followed clip's new stretch hasn't reached the playhead, the status may say "The disk was too slow" for its track.
- **Warp markers can't be placed by hand** in the app: Follow tempo is the only way.
- **A clip's own tempo isn't found from its file**: Follow tempo takes the clip as it plays when you turn it on.
- **No `m2` command for Follow tempo**: agents use `POST /api/graph/clips` (`{"op": "audio", "follow": true}`).
- **One stretch method for all clips** (`std:stretch`, WSOLA). No choice per clip (drums, voice, pads).
- **Stretches after a tempo change are made one at a time**, in build order (lane, then clip), each from its clip's start: with several long followed clips, the one under the playhead may stay silent until those before it are done. Later: make the one nearest the playhead first.
- **Reverse with Follow tempo** (the clip menu allows both): the clip's edges and Split are refused ("Change that in its lane file"), and after a tempo change it is silent from its start until its whole stretch is made.
- **A stretch that fails plays silence** live, with a line in the log only (an export fails and names the clip). A stretch the cache folder can't take is made in memory, also with a line in the log only.
- **The plugin host process** (HOST-3, since ph-1). Plugins run in a second program, so a crash no longer ends the app. What it doesn't do yet:
  - All plugins share one host process, so one crash silences them all (each comes back by itself). One process per plugin would silence only the one that crashed.
  - Its audio threads don't join the Mac output's audio workgroup (they ask for real time on their own).
  - When the app and the host process die at the same moment, a plugin's shared memory can be left in `/dev/shm` until a restart.
  - It writes no core dump when a plugin crashes, so there is no crash file to send to the plugin's maker.
  - The app still starts its own plugin main loop, which now serves nothing: it can be folded away.
  - A plugin that fails in the same instant as a crash may be told as "stopped working" rather than in the crash's notice.
  - Tests that use it need `m2-plugin-host` built first (`cargo build -p m2-plugins --bin m2-plugin-host`); `ci/check.sh` builds it.
  - macOS: its shared memory and wake are lint-checked only, until the owner's Mac runs them.
  - A plugin stuck on the host's main loop (in its window, its start or a settings load) holds up edits and saves for up to 30 seconds (2 minutes in a scan); then music2 ends the host process and says so (the ph review's r1).
  - One broken `.clap` file that crashes the host while it scans makes every plugin say "not installed", and the bad file isn't named. Scanning one file at a time would name it and skip it (the ph review's r4).
  - An export during which the host process crashes, or is ended for not answering, finishes with those plugins silent from there and says only "Exported" (seen in the ph review's pictures).
- **Plugin windows** (HOST-7, since ph-2). A plugin's own window opens from its box; a knob turned there writes the code, latches a lane, or saves the plugin's settings (one Undo step). What it doesn't do yet:
  - A plugin's window floats on its own: it isn't tied to the app's window, its place isn't remembered, and you can't resize it by hand.
  - Linux: X11 only (Wayland through XWayland).
  - macOS: the window, the accessory app and its event pump are lint-checked only, until the owner's Mac runs them.
  - A window closes when its plugin is made again (a restart, a crash, a change of shape): open it again from the box.
  - Latches last for the session: none in `.music2/` yet, no "Write to code", no latch on std's knobs.
  - A gesture in a plugin's window isn't recorded as automation.
  - A plugin with no key can't have its settings saved: the app says so once.
  - One plugin can hold or latch 32 parameters at once; a gesture on one more is set back.
  - A plugin's parameter knob in its box is labelled by its port name (`A_filter_1_cutoff_4092842705`), not the plugin's name for it.
  - A number written in a `plugin()` call now reaches the plugin as a 32-bit number, as a lane's value does.
- **Latency compensation** (RENDER-17 to 21, since ph-3). A plugin that plays late (a look-ahead limiter) keeps every track on the beat, live and in exports. What it doesn't do yet:
  - Latency inside a `poly` voice is still refused: put the latent plugin or kernel after the voices.
  - Notes that a seek, a stop or a loop's wrap ends still reach a late plugin late, like a tail.
  - The metronome isn't delayed by the song's latency, so with a latent plugin it clicks that much early.
  - `time` read inside the voices of a `poly` that sits after a latent node isn't delayed (lanes and `time` outside voices are).
  - Live keyboard notes into a late plugin arrive late by its offset too.
  - `m2-gates realtime --offline`'s step 7 loop (`How::Old`) gives a late plugin or poly no notes (timing only).
  - A plugin given back by an export, with notes to start again, starts them from its lane's notes, not the delayed ones.
- **The cache folder is never trimmed**: warped clips' renders and waveforms stay in it until it is deleted by hand. Follow tempo makes it grow fast: each tempo tried leaves a whole stretch of every followed clip, about 23 MB a stereo minute at 48 kHz (trying 10 tempos on a 4-minute stereo clip leaves about 0.9 GB).
- **A disk too slow for an audio clip** cuts it to silence for a moment, with a click; it doesn't fade. The status says "Disk too slow".
- **A WAV another program changes while it plays** may play a mix of old and new for a moment, until the song is built again.
- **`m2-os`'s two `fake_sysfs` tests** share one temp folder, so run together they sometimes fail; alone they pass.
- **Recording audio:** sound made during the count-in isn't kept in the take.
- **Recording audio:** no arm button: you hear yourself only while recording.
- **Recording audio:** one clip per take: no takes stacked per loop pass, no punch in and out.
- **Recording audio:** an input on its own clock (not the output's device) can drift a little against the song in a long take.
- **Recording audio:** only the input's first two channels: no choosing inputs 3 and 4.
- **Recording audio:** no recording offset for a device that reports its latency wrongly; the output's latency is taken as two buffers.
- **Recording audio:** the take shows no waveform until it is written.
- **Recording audio:** a crash during a take loses it: the part file is swept.
- **Recording audio:** the input stays open (a Mac shows its microphone light) from the first take until the song closes; a new choice in Settings opens at the next take.
- **Recording audio:** a Mac asks about the microphone at the first take; a take running while you click Don't Allow records silence. The next R says why.
- **Recording audio:** with an output buffer more than about four times the input's (Settings' 2048 against a 128 input), hearing yourself drops out; the take is fine. Hearing should lag by the larger of the two buffers (the io review's r4).
- **Recording audio:** an input chosen in Settings that is unplugged records from another input without saying so; outputs say so (the io review's r7).
- **A streamed file that can't be read** (a drive unplugged, a file cut short) plays silence there with no message, live or in an export; the count is kept but nothing reads it (the io review's r8).
- **Recording audio:** checks owed on a Mac: a real microphone, the permission question, the take's timing against the click, and hearing yourself in headphones (see HANDOFF).
- **Space, then Space again, jumps back** to where play started. std's chorus, phaser and tremolo, and the drums' noise, then carry on from where they were rather than starting over, so that second play may differ a little from the export. The export is always right.
- **An audio file a kernel reads** (kernel ABI v1) that changes on disk is heard at the next build, not at once: only files a lane names are watched by their hash. Its name in the code has no hash pin, unlike a clip's, and when it is missing no near name is offered.
- **Kernel ABI v1's reserved parts aren't built**: event ports, `k_save`/`k_load` and `k_latency`/`k_tail` ([kernel-abi.md](kernel-abi.md#reserved-shapes)). Each comes with the first kernel that needs it.
- **A sound part that crashed is reset when it starts again**, not built new. A crash can leave its memory wrong, so it may crash again sooner.
- **Kernels from other people have no time limit**: one that loops forever stalls the sound (wasmtime's epochs would stop it). A song near the 3,999 sound parts limit has no cheaper way to run yet (a `sum` kernel, a checked engine).
- **The Mac's numbers for the one-sandbox-a-box layout are owed**: G7f's run, the footprint and the address space with two renderers (PLAT-1).
- **A Sampler played far above its root sounds harsh**: it isn't band-limited, so high notes alias, and past 6 octaves up it plays no faster.
- **A Sampler's files are held in memory**, decoded whole, once while the song plays them (clips stream from disk; offline kernels read from disk). The ABI allows streaming them ([kernel-abi.md](kernel-abi.md#the-asset-import)); the host needs to load each file's start ahead and read ahead per voice.
- **A WAV dropped on a Sampler that then refuses it** (one with `zones` in its code) is still copied into assets/, though the page says "Nothing was changed"; while dragging, its card still says "Play it in Sampler" (the ab review's 3).
- **A Sampler file that is in assets/ but won't decode** (`sample: "loop.mp3"`, a damaged WAV) plays silence with no message; a missing one is named (`asset.missing`) (the ab review's 4).
- **A kernel that asks for memory while playing** is told it went over 256 MB, even when it asked for a few MB; the words should say kernels may not take memory while playing (the ab review's 5).
- **A song asking for std "0.2" in an older app that has std 0.2.0** and calls the Sampler gets a missing import, not `pkg.unsatisfied`: a song's std requirement names no patch version. `examples/sampler` asks for "0.2.1".
- **std's kernel test `odd_db_values` fails in a debug build**: a NaN's sign bit differs from the release build's. `ci/check.sh` runs those tests in release, where it passes.
- **An instrument's voices, mode and glide can't be changed on its card**: the Device card says how it plays, read-only; they are set in its code (with #35).
- **Glide in `"poly"` mode** isn't built: each new note sliding from the last on many voices. `glide` does nothing there.
- **The Device card shows the first sentence** of how an instrument plays ("Plays one note at a time."); the rest is in its tooltip.
- **`steal: "none"` waits for a voice to fall silent**, not for its key to be let go: a note with a long release keeps its voice until it ends.
- **A song asking for std "0.2"** gets `pkg.unsatisfied` in an app with std 0.3.0; its message names the line to write (`std = "0.3"`). The song may sound a little different: notes in one-note sounds keep their own velocity.
- **`a_second_session_is_read_only`** (core/session) failed once in a full test run under load (its `refresh` after the lock came back); alone, 3 runs of 3 pass.
- **The `st-errors` browser suite** once waited over 30 s for the Needs attention row of a missing sound file, in a full run at load 9; alone it passes.
- **Every MIDI keyboard plays the selected track**: choosing a track for each keyboard isn't built.
- **Every MIDI channel plays the same track**, and the pitch wheel bends all of it: a channel per track isn't built.
- **The pitch wheel and the mod wheel aren't recorded** into clips (with #24): it needs note expression data, a format bump.
- **Aftertouch, expression (CC 11), breath and other controllers** do nothing; the pitch wheel always bends 2 semitones each way.
- **The pitch wheel and the mod wheel move in steps** once a block (2.7 ms at 48 kHz), with no smoothing.
- **Only the polysynth hears the mod wheel**: FM, the Sampler and the drums don't.
- **A track chosen while the wheel is held** starts with it at rest, until the wheel next moves.
- **A plugin that takes only CLAP's own note events** may not hear the wheels: they are sent as MIDI messages to every plugin.
- **A MIDI file's controllers, pitch bends and later program changes** are skipped when it is brought in: only notes, the first program, tempos and time signatures come in.
- **The drop hint on a notes track says "Into Bass"** before it knows the file's parts: a file of several parts makes new tracks after Bass instead (the toast names them). A browser gives no file until the drop.
- **One MIDI file at a time**: dropping several says so, and brings none in.
- **The pitch wheel picks a different drum on a drum kit**: the bend is added to every voice's pitch, and std's kit picks the drum by its pitch, so a kick (36) bent 2 up plays the snare (38); a wheel resting a little off centre changes a hit's noise (the pm review's 2).
- **A poly's glide isn't delayed with its notes** behind a latent plugin (RENDER-18): a glide read from such a path is a few ms early. A glide is almost always a number (found merging `ph`).

## Removed with the old version

| What was removed | Where it comes back |
|---|---|
| New Song, Open and Save of `.m2song` files | A song is now a folder, and every change is written at once. New Song… is back (File → New Song…, ⌘N) and makes a song folder from a template. Old `.m2song` files can't be opened. Recent songs and Save As are back (File → Open Recent, File → Save As…, ⇧⌘S). |
| The Song Library | Back as the recent list on the start screen and File → Open Recent. A picture of each song is #12 |
| Import Audio | Back: File → Import Audio… (⇧⌘I) and dropping WAV files. Warp and other file types are #18 |
| Export as WAV | Back: File → Export… (⇧⌘E): the song, stems or MIDI |
| Settings | Back: Settings (⌘,) chooses the sound output and its buffer size, the sound input, the songs folder, and which MIDI keyboards play. View settings for every song are #31 |
| The Track inspector (volume, pan, output, sends, effects, sidechain) | Volume and pan: back in the Mix view. Effects on and off, and their order: back on the effect cards. The sidechain: back, as a Duck's "Listens to". Output and sends: sends and buses (#27) |
| A level meter on each track | Back: a meter in every track header, on the Song row and on every Mix strip. Meters after the fader and loudness are #20 |
| The Sounds panel | Audio files: back (File → Import Audio…, drops). A sampler: back, std's Sampler; the rest of it is #22 |
| Drawing automation | Back: drawn on the timeline, a frequency on a log scale. Curves are #14 |
| Loop | Back: the loop button or C, a drag across the ruler, or ⌘L for the selected clips |
| Hearing notes as you draw them | Back: audition in Notes |
| Triplet snap (1/8T, 1/16T), quantize, humanize, and octave up and down in the piano roll | Back: triplet snap, and the note menu's Quantize, Humanize and octave up and down |
| The key and scale picker, and showing only in-key notes | The key is back (major and minor), and Notes shades the rows not in it. Other scales are #6, locking notes to the key #16 |
| The command API: `/api/commands`, `/api/describe`, `/api/exec`, `/api/steps` | A self-describing API (#38) |
| `music2-cli` | `m2` replaces it; the missing commands are #37 |
| Installing sounds from GitHub (`music2-cli plugin add github:…`) and the Plugins folder | Shared sound packages (#39) |
| m2plug plugins (the JSON sound files that were in `plugins/`) | std replaces most of them: Shimmer is now `chorus`, Space is `reverb`, Echo is `delay`, Tone is `eq3` and Glue is `compress`. Lead, keys and strings are in `std/synth` and `std/fm`, and the drum kit in `std/drums`. Still missing: Pump (#21) and the sampler (#22). |
