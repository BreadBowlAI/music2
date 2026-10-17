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
- Open a song folder (File → Open Project Folder…, ⌘O) and play it.
- See every track on a timeline: notes, automation, masks and audio clips, with the tempo shown above the tracks and bars counted by the song's time signatures (written in `data/arrangement.json`).
- Add, move and delete notes clips. Draw, move, resize and nudge notes, and set how hard each one is played. Every clip has an id, so an edit always lands on the clip it was made on.
- Drag a number written directly at an input (`0.3`, `hz(800)`, `{ pan: 0.3 }`) as a knob: you hear the change at once, even while the song compiles, and letting go writes it into the code. Saves to other files during the drag (another editor, an agent) don't interrupt it, and nor do saves to its own file that leave its line alone. A number held in a variable, or worked out from other numbers, shows **edit in code** instead (#35).
- Every control of an instrument or effect that isn't connected to a signal is a knob, even one the code leaves out: `bass(notes)` shows its `drive` at the default, its number in grey, and turning it writes `{ drive: … }` into the call. Each knob turns through the range the instrument or effect declares (300 pixels of drag, ⇧ for fine), a double-click types a value (`-3`, `2k`, `L20`), and an ⌥-click puts it back at its default.
- Connect cables inside one file, and add or remove 17 effects from a searchable browser. An effect is known by what it is, not by its name in the code: `import { delay as echo }` is still the Delay.
- Write your own instruments and effects in TypeScript, with knobs, defaults and ranges, the way std's are written ([Your own components](../README.md#your-own-components)).
- Edit the code in the app, with type checks. When the code breaks, the last working version keeps playing. A song whose code doesn't compile when it's opened still opens, in its code, and plays once a save makes it compile.
- Undo and redo every edit made in the app. An edit is refused, rather than written, when it would change anything but what it says, and an undo is refused only when what it puts back was changed since.
- Add a track without writing code: **+ Track** (⌘T) lists std's 22 sounds: the drums first, then the instruments, then the two synths with every knob. It adds the one you pick below the selected track. It writes `tracks/<key>.ts`, imports it in `song.ts`, gives it the mixer's last channel and an empty notes lane, as one edit Undo takes back. Duplicate a track (⌘D), with its clips, its plugins' saved settings and its mixer channel; delete one (⌘⌫), which asks first when another track's effect listens to it. Drag a track's name to reorder, and pick its colour from its menu: both are kept by name in `layout.json`, so colours never shift when tracks come and go. The same from `m2 add-track`, `duplicate-track`, `delete-track` and `layout`.
- Rename a track's label without moving or silencing anything: double-click its name in the timeline (or the dock's header), or press ⌘R, or write `track("kick", { name: "Big Kick" }, …)`. Data is filed under the track's key.
- Automate a knob: right-click it, **Automate**. It gets an automation lane of its own, which plays the knob's old value until it has points (drawing them is #14).
- See what needs attention. When a track's or a lane's key is renamed in code, its notes or automation stop playing; the status pill in the top bar says so, and its popover says it in plain words and offers to move the data where it belongs now (a renamed track's whole folder in one click), or to delete it. Undo takes either back. It also lists a data file that doesn't read (its lane keeps playing what it had), clips without ids, and a missing sample or plugin, each with its code and fixes. Every `m2` command that opens the song prints the same, with the commands that fix it (`m2 relink`, `m2 delete-data`, `m2 fix`).
- Edit the files in any other editor, from a script or with git. The app takes a change in once it has settled (no file changed for 150 ms), so a file caught half-written never plays and a lane never plays empty. Agents can also send checked edits (Tx) over HTTP or with `m2 tx`, written as one undo step.
- One program writes a song at a time: `m2` commands that write a song the app has open stop without changing anything, and a second window opens it read-only, with Retry. An edit cut off by a crash is offered back as Finish or Discard when the song opens next.
- Set the tempo and the time signature in the top bar. Drag the tempo up or down (⇧ for fine) or double-click it and type; click the time signature to pick it. Each changes the one in effect where the playhead is, and you hear a new tempo when you let go. Right-click the ruler to add a tempo or time signature change there, and click its mark to change or remove it. Double-click the song's name to rename it. New Song asks for the tempo too. Each is one edit Undo takes back; the same from `m2 tempo`, `meter` and `title`.
- Play and stop like Live: Space stops back where play started, and ⇧Space goes on from where it stopped. Click the ruler, or an empty spot in a lane while stopped, to start somewhere else. Loop with the loop button or C, a drag across the ruler, or ⌘L for the selected clips; the loop is kept for next time. ⌘. (Transport → Stop All Sound) stops every note at once.
- Export the song as a WAV file (the Export button, or File → Export Audio…, ⇧⌘E), then Show in Finder. Files go in `~/Music/music2/Exports` unless you pick another place. It's the same file `m2 render` writes. Quitting during an export asks first.
- From the command line (`m2`): make a new song, render a WAV, bounce a track to audio, and make the same edits as the app.
- Build `music2.app` and move it anywhere: std, the templates and the examples are inside it, so it no longer needs the source folder it was built from. It's signed ad hoc but not notarized, so on another Mac it opens only after **Open Anyway** in System Settings → Privacy & Security (and only on Apple silicon).
- Play CLAP plugins such as Surge XT, with automation and saved settings. A plugin is added by writing it into a track's code (#32). A song whose plugin isn't installed still plays: only that plugin's node is silent, or passes its audio through.
- Play audio clips with offset, gain, fades, reverse, pitch and warp (written by hand in JSON).
- Play pop sounds from std: a supersaw, a gliding lead, a pad, a pluck, an 808 sub, a synth bass and strings on one polysynth (`std/synth`), FM keys, bells and metal (`std/fm`), and an 808/909 drum machine whose kit plays kick, snare, clap, hats, toms and more from one lane by General MIDI note, with an open hat cut off by the next hat and a level for each group of drums (`std/drums`). Each has a few named knobs; `synth.custom` and `fm.custom` have every one. One note at a time, a note still held sounds again when the one over it ends, as on a monosynth ([Sounds and effects](../README.md#sounds-and-effects-std)).
- Hear a song as it exports. std's chorus, phaser and tremolo wobble only while the song plays, and the drums' noise follows the hits, so they sound as in the export when you press Play, however long the song sat first. An oscillator or noise written straight into a track's code, outside a voice, or a plugin, may still run on while stopped. Going back to bar 1 after playing is a jump: the wobble and the drums' noise carry on from where they were, so that play may differ a little from the export.

## The top ten

1. **Swap a track's instrument** (adding a track without code works now). (small)
2. ~~More instruments~~: done, in `std/synth` and `std/fm` (see What works today).
3. **Drum names in the piano roll** for the drum kit, the part of it that's left. (medium)
4. **A mixer panel**: volume, pan, mute and solo for each track. (medium)
5. **Fast seeking and fast edits** in a full-length song. (large)
6. **Key and scale** in the top bar (the tempo, the time signature and the song's name work now). (small)
7. ~~Loop~~: done (see What works today).
8. **Hear notes** as you draw them in the piano roll (Stop All Sound works now). (medium)
9. **Clip editing**: duplicate, copy and paste, resize, split and move clips on the timeline. (medium)
10. **Rename a track's key** from the app (delete, duplicate, reorder and recolour work now). (small)

## Everything, by area

### Projects and files

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 12 | **Recent projects** on the start screen and in File → Open Recent, and reopen the last project at launch. A list in `~/.music2/recent.json` is updated whenever a project opens. | Without it, reopening a song means finding its folder with Open Project Folder… after every launch. | Yes (the Song Library) | small |
| 28 | **Save As or Duplicate Project, Rename Project**, and opening a project by dropping its folder on the window or the Dock. | These let you keep versions of a song. (File → Show in Finder already opens the project folder.) | Yes | small |

### Packages

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 39 | **Shared sound packages**: list the packages a song uses in `music2.toml`, fetch them (for example `github:owner/repo@tag`), pin them in `music2.lock`, and browse and add them from the app. | Today only std can be loaded, so sounds other people make can't be used, except as CLAP plugins. A package already declares its instruments and effects in TypeScript, with their knobs and effect-browser categories, so what's missing is fetching and loading it. | Partly (single sounds from GitHub, with `music2-cli plugin add`) | large |

### Export

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 36 | **MIDI files**: import a `.mid` into a notes lane, and export notes as `.mid`. Also export stems (one file per track) and MP3 or AAC from the app. Stems exist only as `m2 bounce`, one track at a time. | Sharing ideas with other apps and people. | No | medium |

### Tracks and instruments

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 1 | **Swap a track's instrument** later. (Adding a track without code works: see [What works today](#what-works-today).) Adding one to a song that mixes its tracks without std's mixer, or picking a CLAP instrument (#32), still needs code. | Changing your mind about a sound shouldn't mean a new track. | Yes | small |
| 10 | **Rename a track's key** from the app. (Renaming its label, deleting, duplicating, reordering and recolouring work: see [What works today](#what-works-today).) | Keeping a song tidy. A key is changed in `track()` today; after that, **Needs attention** offers to move the track's whole data folder to its new key. | Yes | small |

### Sounds

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 3 | **Drum names in the piano roll**: `drums.kit` plays each drum on its General MIDI note, but the piano roll shows those notes as C2, D2, F#2 rather than kick, snare, closed hat. A component would declare its note names in its descriptor, and the piano roll would show them. | Drawing a beat by note name means knowing the General MIDI map. | Yes | medium |
| 22 | **A sampler**: play a recording or a one-shot as an instrument, with a root note, plus drum samples. Kernels can't read assets yet, so the core needs work first. | Sampled drums and vocal chops are pop staples. | Yes | large |
| 26 | **Presets**: save and load an instrument's or an effect's settings by name, a preset browser, and presets for CLAP plugins. | Beginners start from presets, not from blank knobs. | No | medium |

### Mixing

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 4 | **A mixer panel**: a volume fader, pan, mute and solo for each track, plus master volume. The faders can sit on the existing `mixer({ db, pan })` knobs. Mute and solo are engine monitoring overlays that work on any track and never change the export, and "mute in song" is a track option written in code, `track("kick", { muted: true }, fn)`; their back ends are built (8b.4: `POST /api/graph/overlay`, `POST /api/graph/track/mute`, `m2 mute-in-song`), and the M and S buttons are still to come ([Monitoring overlays](architecture.md#monitoring-overlays-and-mute-in-song)). | Balancing the parts is the heart of mixing. Today levels are knobs on the Song module's graph, and there's no mute or solo at all. | Yes | medium |
| 20 | **Level meters**: a master meter with a clipping warning and the loudness for export, and a meter on each track again. Their back end is built (8b.8): the engine keeps the peak of each track and of the song's output, and the server sends them to every window about 30 times a second while the song plays, with the DSP load (`{"t": "meters"}` on the WebSocket). The meters in the UI and the loudness for export are still to come. | Without meters you can't see whether the mix is too loud or clipping. | Partly (a meter on each track) | medium |
| 27 | **Sends and buses** in the UI: a shared reverb bus, group buses, and sending a track to a bus. | It's standard mixing practice, such as one reverb shared by every track. Each change is a structural code edit in `song.ts`. | Yes | large |

### Song settings

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 6 | **Key and scale** in the top bar. (The tempo, the time signature and the song's name work: see [What works today](#what-works-today).) A key and scale have no home in the song's files yet; the field comes when they do, and scale help (#16) builds on it. | A key tells you which notes fit. | Yes | small |

### Playback

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 5 | **Fast seeking and fast edits** in a full-length song. A persistent live engine where a seek is a transport event, never a replay, and a notes edit swaps only that lane's data (step 3 of the [order of work](architecture.md#order-of-work); [RENDER-8](architecture.md#live-engine-and-its-two-clocks), [RENDER-9](architecture.md#seek-and-loop)). | Today a seek replays the song from 0, and every notes, cable, effect or code edit rebuilds the player at the playhead the same way. The example renders about 30 times faster than real time, so late in a busy 3-minute song each seek or edit could stall for several seconds. | Yes | large |
| 19 | **A metronome**: a click with an on/off button in the top bar. The player makes it from the tempo; it isn't part of the song. A count-in can come with recording. | It helps you keep time and check a beat. | No | small |

### Timeline and clips

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 9 | **Clip editing on the timeline**: duplicate (⌘D) and copy/paste; drag the right edge to resize; split at the playhead; move a clip to another track; repeat a clip N times; add clips on automation and audio lanes. Their back end is built (8b.5): one clip gesture each, by clip id, for notes, automation and audio clips (create, move, copy, resize either edge, split, join, duplicate, paste, delete, quantize), each one undo step, through `POST /api/graph/clips` and `m2 clips`. The timeline doesn't send them yet. | Pop songs are built by copying parts. Today notes clips only move along their own lane, and ⌘D only works on notes inside the piano roll. | Yes, except split | medium |
| 17 | **Timeline zoom** (⌘= and ⌘−, pinch), and **right-click menus** on clips, lanes, tracks and notes. The old UI's native menu code can come back from git history. What a clip's menu does (cut, copy, paste, duplicate, split, join, quantize, delete) is built as clip gestures (8b.5, #9). | At a fixed 24 pixels per beat, a 3-minute song takes a lot of scrolling. Right-click menus are how beginners find options. | Yes | small |
| 23 | **Sections**: name the parts (intro, verse, chorus), show markers on the ruler, copy or move a whole section across all tracks, and insert or delete bars. | Pop songs are arranged in sections, and copying a chorus is one gesture in other DAWs. Sections are designed but not built, and inserting bars touches every lane and the tempo. | No | large |

### Notes

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 8 | **Hear notes** when you click, draw or move them in the piano roll (audition). (Stop All Sound, ⌘., works: see [What works today](#what-works-today).) The engine and server half is built (8b.6): a window plays live notes on a lane over its WebSocket or `POST /api/graph/audition`, and they keep sounding through edits, stop and loops until let go ([MON-7](architecture.md#monitoring-overlays-and-mute-in-song)). The piano roll doesn't send them yet. | You can't write a melody without hearing it. MIDI input will take the same path. | Yes | medium |
| 15 | **Note tools**: triplet snap (1/8T and 1/16T, three notes in the space of two), quantize, transpose a clip, humanize, swing, legato, and copy/paste notes between clips. | These clean up a drawn or played part, and triplets are common in pop hats and rolls. The piano roll snaps only to 1/4, 1/8, 1/16 and 1/32, and has only nudging with the arrow keys and ⌘D. | Partly (triplet snap, quantize, humanize, and octave up and down) | small |
| 16 | **Scale help**: choose a key and scale, highlight or lock the notes in the key, chord stamps and an arpeggiator. `std/theory` and `std/gen` are designed but not built. | Beginners write better melodies and chords with a scale guide. | Partly (a key and scale, and showing only in-key notes) | medium |

### Automation

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 14 | **Draw automation**: click and drag points on automation lanes in the timeline. Its back end is built (8b.5): a clip gesture makes an automation clip holding the lane's value, and draws one clip's points (`points`). "Automate" on a knob's menu already makes the lane and reads the knob from it, and names it after the control (`decay`); **Remove automation** puts the value back in the code (`POST /api/graph/automate/remove`, `m2 unautomate`); the view says which lane drives an input. The timeline and the knob menu don't use them yet. | Filter sweeps, fades and build-ups are everywhere in pop. Today automation lanes can only be looked at in the app, and their points are written by hand in JSON. | Yes | medium |

### Audio

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 18 | **Audio in the app**: import a WAV from a menu, or drag it onto a bar; add an audio track; show waveforms; move, trim, fade and set the gain of audio clips. The engine already plays offset, gain, fades, reverse, pitch and warp. | Vocals, loops and samples. Today this means copying files into `assets/` and writing clip JSON by hand. | Yes | large |

### Recording and MIDI

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 24 | **Play instruments live** from a MIDI keyboard or the computer keyboard, and record what you play into a notes clip, with a count-in and quantize. | Most people write melodies by playing them. It needs MIDI input and low-latency live notes in the player. | No | large |
| 25 | **Record audio** (vocals, guitar) from an input into an audio clip, and hear yourself while recording. | Vocals are central to pop. It needs an input stream, a low-latency path for hearing yourself, and the audio clip work (#18). | No | large |

### Effects and plugins

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 21 | **Effect comforts**: bypass, reordering, a picker for duck's sidechain source (today written in code or connected by a cable), effect presets, and the old Pump (ducking in time with the beat, with no sidechain). | Switching an effect on and off is how beginners learn what it does. | Yes | medium |
| 32 | **Add an installed plugin from the app**: list installed CLAP plugins (such as Surge XT) in the effect browser and when adding a track, and put one on a track as an instrument or an effect. | Today a plugin goes on a track only by writing `plugin("clap:…")` into its code, after finding the plugin's id with `m2 plugins`. | No | medium |
| 33 | **Plugin windows** for CLAP plugins, **VST3 and AU** support, and **latency compensation** (plugins that report latency are refused today). | Many popular synths and mastering plugins report latency, or come only as AU or VST3 on the Mac. | No | large |

### Safety and undo

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 29 | **Undo that survives closing the app**, and readable undo messages, such as "Undid: Hats pan" instead of "Undid: song.ts 0.5 → 0.2". | Closing the app loses all undo, and today's undo messages are technical. | Partly (readable messages; its undo was lost on closing too) | medium |
| 30 | **Crash safety**: keep unsaved code-editor text through a crash, run CLAP plugins in a separate process so a crashing plugin can't take the app down, and simple project snapshots or backups. | Edits are on disk at once, and an edit cut off by a crash is offered back as Finish or Discard, so no file is left half written. But a plugin crash closes the whole app, and unsaved code is lost. | No | large |

### Settings

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 13 | **Open a project without an audio device**: fall back to the silent player and say so. | Opening a project fails with "no audio output device", so an unplugged audio interface locks you out of your song. | Yes | small |
| 31 | **A Settings window**: choose the audio output and buffer size, show the current device and its rate, and set where new projects go. The engine already runs at each project's `sample_rate` (in `music2.toml`), converting at the device when the device runs at another rate ([RENDER-31](architecture.md#sample-rate)), so the sample rate is a project setting, not an app one. | People with an audio interface or headphones need to pick the output. Today the player always uses the system default. | Partly (it showed the device and the folders, with nothing to choose) | medium |

### Help

| # | What's missing | Why it matters | Did the old app have it? | Size |
|---|---|---|---|---|
| 11 | **Plain-language help**: a "make your first song" guide in the app, tooltips on every control, and friendlier error messages. Today a unit mistake reads like ``input `cutoff` expects control<hz>, got control<db>``, and it points at `std/src/filter.ts` rather than at your own line. | Many people who open music2 are new to making music. [Music basics](music-basics.md) and the README's first sections are a start; the rest of the docs are technical. | Yes | medium |

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
| 37 | **CLI basics**: `m2 import <wav>`. (`m2 tempo`, `meter`, `title` and `add-track` work.) | Scripts and agents need the same basics as the app. Each is small once the matching session edit exists. | Yes (in `music2-cli`) | small |
| 38 | **A self-describing API**: every `/api/graph` operation listed with a JSON Schema, a short summary of the song, and MCP. (Export over HTTP is there: `POST /api/graph/export`; so are checked edits of any files as one undo step: `POST /api/tx/check` and `/api/tx/commit`.) | The old command API let Claude Code drive the app. The new routes work, but an agent has to read the README or the source code to learn them, and a Tx's JSON by the [architecture contract](architecture.md#the-two-write-primitives). | Yes | medium |

## Never in either version

Every row marked **No** above was missing from the old app too. Of the rows marked **Partly** or **Yes, except**, the old app also lacked: splitting clips (#9), swing, legato and copying notes between clips (#15), chord stamps and an arpeggiator (#16), a master meter and loudness (#20), undo that survives closing (#29) and choosing the audio device (#31).

## Loose ends

- **Undo steps have no names on the server** (#29). The top bar's Undo and Redo name the steps this window made ("Undo Reverb size"), and an undo of a part added or removed says so ("Undid adding Reverb"); a step made elsewhere (another editor, an agent's Tx) leaves them unnamed until this window edits again, and the native Edit menu never names them.
- **A knob let go while the code doesn't compile** keeps playing its new value, and is written into the code once the code compiles again; after 30 seconds it is dropped, with a message saying so. Writing it at once, when the broken file is another one, is the owner's call (C21 of the step 6 design): the rule today is that an edit of code waits for all the code to compile.
- **The code editor reloads a file an edit changed** rather than following the edit in place; unsaved text in it is kept, with the "changed on disk" choice.
- **A file added to a renamed track's folder afterwards** (a Finder copy, a `notes.txt`) isn't noticed until something else changes: the status popover still offers "Move all", and moving the folder is then refused, naming the file and saying to move it out or delete it. Added before the rename, the folder is offered file by file, and the problem names it.
- **Two literals edited on neighbouring lines by another program** count as a change to the calls between them, so the song is rebuilt (crossfaded) rather than having its numbers written into what plays. A change a line or more apart from the next is written in place.
- **A missing plugin reads technically**: ``no CLAP plugin `org.surge-synth-team.surge-xt` is installed (looked in [...])``. It no longer silences the song: only that plugin's node is silent, or passes its audio through ([FMT-18a](architecture.md#plugin-state-and-parameters)). Open an Example still names missing plugins plainly, from the example's `example.toml`, and refuses to copy the example (#11).
- **An error in the browser console with no project open.** `GET /api/graph` answers 400 when nothing is open, which is how the window knows to show the start screen. The browser logs that request as an error even though nothing is wrong. Answering "no project" without an error status would be quieter.
- **The browser suites** live in `e2e/` (puppeteer, with its own `package.json`), rebuilt after a reboot wiped their old scratch folder.
- **Space, then Space again, jumps back** to where play started. std's chorus, phaser and tremolo, and the drums' noise, then carry on from where they were rather than starting over, so that second play may differ a little from the export. The export is always right.

## Removed with the old version

| What was removed | Where it comes back |
|---|---|
| New Song, Open and Save of `.m2song` files | A song is now a folder, and every change is written at once. New Song… is back (File → New Song…, ⌘N) and makes a song folder from a template. Old `.m2song` files can't be opened. Recent projects are #12, Save As #28. |
| The Song Library | Recent projects (#12) |
| Import Audio | Audio in the app (#18) |
| Export as WAV | Back as Export Audio (File → Export Audio…, ⇧⌘E) |
| Settings | A Settings window (#31) |
| The Track inspector (volume, pan, output, sends, effects, sidechain) | The mixer panel (#4), sends and buses (#27), effect comforts (#21) |
| A level meter on each track | Level meters (#20) |
| The Sounds panel | Audio in the app (#18), the sampler (#22) |
| Drawing automation | Draw automation (#14) |
| Loop | Back: the loop button or C, a drag across the ruler, or ⌘L for the selected clips |
| Hearing notes as you draw them | Audition (#8) |
| Triplet snap (1/8T, 1/16T), quantize, humanize, and octave up and down in the piano roll | Note tools (#15) |
| The key and scale picker, and showing only in-key notes | Scale help (#16) |
| The command API: `/api/commands`, `/api/describe`, `/api/exec`, `/api/steps` | A self-describing API (#38) |
| `music2-cli` | `m2` replaces it; the missing commands are #37 |
| Installing sounds from GitHub (`music2-cli plugin add github:…`) and the Plugins folder | Shared sound packages (#39) |
| m2plug plugins (the JSON sound files that were in `plugins/`) | std replaces most of them: Shimmer is now `chorus`, Space is `reverb`, Echo is `delay`, Tone is `eq3` and Glue is `compress`. Lead, keys and strings are in `std/synth` and `std/fm`, and the drum kit in `std/drums`. Still missing: Pump (#21) and the sampler (#22). |
