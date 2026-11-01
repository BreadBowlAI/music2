# What music2 can't do yet

**In short**
- This page lists only what is missing. What the app does today is in the guide: [What you can do](guide/what-you-can-do.md).
- The gaps are grouped by area. The areas you notice first when making a song are at the top.
- Each line is one gap. The big ones have a number (#18, #24, …), which stays the same everywhere. Small loose ends are listed under them, without numbers.
- Size: **small** is one focused change; **medium** touches the window, the server and the core; **large** is new engine work.
- "Old app?" says whether the old version of music2 had it: **Yes**, **No** or **Partly**.

The old engine, with its own song files and screens, was removed in October 2026 (git commit `650f842` still has it). Its `.m2song` files can't be opened. Nor can song folders in the first folder format (`timeline/`, tracks named by their display name): the format changed once, in October 2026, to `data/` and keys ([Project format v1](architecture.md#project-format-v1)).

## Recording and playing live

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 24 | **The rest of recording notes**: recording the pitch and mod wheels and the sustain pedal into a clip (it needs controller data in a notes lane, a format bump: left for later on purpose; today the pedal plays live only, and a take keeps each of a keyboard's notes as long as its pedal held it, but a note from the computer's keys only as long as its key, RENDER-36); the computer keyboard's sustain. Overdub, a clip for each time round a loop and the keyboard piano's velocity are built (track `recording`, MON-11, MON-12, MON-15). | Expressive parts use the wheels and the pedal. | No | medium |
| 25 | **The rest of recording audio**: a recording offset for a device that reports its latency wrongly (the output's is taken as two buffers); comping (a clip plays one whole take, not parts of several). Arm, takes for each time round a loop, punch in and out, and inputs 3 and 4 are built (track `recording`, MON-9 to MON-14). | A singer builds the best take from the best lines. | No | medium |

- **Arms and each track's input channels last only while the song is open**: they aren't kept with it, and every window shares them.
- **One notes track records at a time**: the keyboards play one track, so arming a second notes track disarms the first. Audio tracks, any number.
- **A key held over a loop's wrap** ends at the loop's end, and isn't carried into the next pass.
- **The loop moved, turned off or on after a take's first pass** ends the take where it is heard.
- **An audio clip that can't be cut** (its pitch changed, or reversed and warped) stays whole under a punch: the take plays over it.
- **The take's level bar** shows the first armed audio track's input only.
- **An armed track stops hearing its input when the sound output changes** (a new player), until it is armed again or a take starts.
- **An input with many channels** keeps 4 s of each pair in memory, ready for any track: a 64-channel one takes about 50 MB.
- **A take waits while the code doesn't compile.** It is written once the code compiles; closing the song first loses it (a message says so).
- **Sound made during the count-in** isn't kept in the take.
- **An input on its own clock** (not the output's device) can drift a little against the song in a long take.
- **A take shows no waveform** until it is written.
- **A crash during a take loses it**: the part file is swept.
- **The input stays open** (a Mac shows its microphone light) from the first take until the song closes; a new choice in Settings opens at the next take.
- **Hearing yourself drops out** when the output buffer is more than about four times the input's (2048 against 128); the take is fine. It should lag by the larger buffer instead (the io review's r4).
- **Input overruns during a take** are counted in the log, not told.
- **A take whose microphone is turned off** in System Settings partway through is dropped whole, though its start had sound (ex-3).
- **An input's problem stays listed until the next take** (or until another song opens), even after it is plugged back in or allowed (ex-3).
- **Every MIDI keyboard, and every MIDI channel, plays the selected track**: no track per keyboard or per channel, and the pitch wheel bends all of it.
- **Aftertouch, expression (CC 11), breath and other controllers** do nothing; the pitch wheel always bends 2 semitones each way.
- **The wheels move in steps** once a block (2.7 ms at 48 kHz), with no smoothing.
- **Only the polysynth hears the mod wheel**: FM, the Sampler and the drums don't.
- **A track chosen while the wheel is held** starts with it at rest, until the wheel next moves.
- **The pitch wheel picks a different drum on the drum machine**: a kick (36) bent 2 up plays the snare (38) (the pm review's 2).
- **A plugin that takes only CLAP's own note events** may not hear the wheels: they are sent to every plugin as MIDI messages.

## Bringing in audio and MIDI files

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 18 | **The rest of audio clips**: place a clip's warp markers by hand (Follow tempo is the only way), find a clip's own tempo from its file, set a clip's pitch in the app (JSON only), join audio clips, clear out audio files no clip uses, and import MP3 or AIFF (FLAC comes in as a WAV of its samples, [FMT-24](architecture.md#assets-and-hashes); not yet a FLAC of 12 or 20 bits, which claxon reads and which would come in as a 16- or 24-bit WAV of the same sound, with the same `f32` bits, but that isn't written yet; nor a FLAC of 32 bits, nor one that uses a part of FLAC claxon 0.4.3 doesn't decode: a partition whose numbers are written plainly ("escaped"), or a frame that takes its bits from the header. Both are rare; they are told so in words. music2 doesn't patch claxon, so they wait for a decoder that reads them). | Every file stays in `assets/` until you delete it, and only WAV and FLAC come in. | Yes | medium |

- **After a tempo change, a followed clip is silent until its new stretch reaches the playhead**: up to half a second near its start, longer deep in a long clip or with several followed clips, as stretches are made one at a time, in build order, each from its clip's start. Later: the one under the playhead first.
- **A quarter second that holds a plain clip and a stretch being made** is silent, the plain clip too; the status may say "The disk was too slow" meanwhile.
- **Reverse with Follow tempo**: the clip's edges and Split are refused ("Change that in its lane file"), and after a tempo change it is silent until its whole stretch is made.
- **One stretch method for all clips** (`std:stretch`, WSOLA): no choice for drums, voice or pads.
- **The cache folder is never trimmed.** Each tempo tried leaves a whole stretch of every followed clip, about 23 MB a stereo minute (10 tempos on a 4-minute clip: about 0.9 GB).
- **A disk too slow for an audio clip** cuts it to silence for a moment, with a click; it doesn't fade. The status says "Disk too slow".
- **A WAV another program changes while it plays** may play a mix of old and new for a moment, until the song is built again.
- **A file that couldn't be read stays listed after it reads again** (a drive plugged back in), until the clip is edited (ex-2).
- **Two tracks whose audio lanes hold the very same clips** share one lane, so a file that can't be read there is told for one of them only (ex-2).
- **After a clip whose file is missing moves**, a problem in that lane may name the clip beside it: a clip is named by its place in its lane's data (ex-2).
- **A stretch the cache folder can't take** is made in memory, with a line in the log only (the sound is the same).
- **When the OS can't start the stretch thread or the disk thread**, warped or long clips are silent, with a line in the log only.
- **A MIDI file's controllers, pitch bends and later program changes** are skipped when it comes in: only notes, the first program, tempos and time signatures.
- **One MIDI file at a time**: dropping several says so, and brings none in.
- **The drop hint on a notes track says "Into Bass"** before it knows the file's parts; a file of several parts makes new tracks after Bass instead (the toast names them).

## Plugins

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 33 | **VST3 and AU** plugins: **not planned.** The owner chose CLAP only (2026-10-19), as the open format. | Many popular synths and mastering plugins come only as AU or VST3 on the Mac. | No | large |

- **A plugin effect can't be removed, turned off or moved from its card**: only in code (Undo works right after adding it) (#32's leftover).
- **A plugin installed while the app is open shows only after Look again** in a Plugins list.
- **A plugin removed since the list was made is named by its id** ("org.example.reverb isn't installed…"), not its name, when adding it is refused.
- **A plugin whose CLAP features say neither instrument nor effect** (an analyzer, a note effect) isn't listed; it can still be written in code.
- **All plugins share one plugin program**, so one crash silences them all for a moment (each comes back by itself). One program per plugin would silence only the one that crashed.
- **A plugin stuck in its window, its start or a settings load** holds up edits and saves for up to 30 seconds (2 minutes in a scan, or a read of its own presets); then music2 ends the plugin program and says so (the ph review's r1).
- **One broken `.clap` file that crashes the scan** makes every plugin say "not installed", and the bad file isn't named (the ph review's r4).
- **A plugin that fails a second time is left silent with no new message**; its listed words still say music2 starts it again. Open the song again to try it (the ex review's r3).
- **A plugin that fails in the same instant as a crash** may be told as "stopped working", not in the crash's words.
- **A song with plugins plays and exports on one core**, while other songs use several.
- **The plugin program's audio threads** don't join the Mac output's audio workgroup (they ask for real time on their own).
- **When the app and the plugin program die at once**, a plugin's shared memory can be left in `/dev/shm` until a restart.
- **No crash file** is written when a plugin crashes, to send to its maker.
- **A plugin's window floats on its own**: it isn't tied to the app's window, its place isn't kept, and it can't be resized by hand.
- **Plugin windows on Linux**: X11 only (Wayland through XWayland).
- **A window closes when its plugin is made again** (a restart, a crash, a change of shape): open it again from the box.
- **Latches last for the session**: none kept in `.music2/`, no "Write to code", no latch on std's knobs.
- **A knob turned in a plugin's window isn't recorded** as automation.
- **A plugin with no key can't have its settings saved**: the app says so once.
- **One plugin can hold or latch 32 parameters at once**; a gesture on one more is set back.
- **A plugin's knob in its box is labelled by its port name** (`A_filter_1_cutoff_4092842705`), not the plugin's name for it.
- **A number written in a `plugin()` call** reaches the plugin as a 32-bit number, as a lane's value does.
- **Latency inside a `poly` voice is refused**: put the late plugin or kernel after the voices.
- **Notes that a seek, a stop or a loop's wrap ends** still reach a late plugin late, like a tail.
- **The metronome isn't delayed by the song's latency**, so with a late plugin it clicks that much early.
- **Live keyboard notes into a late plugin** arrive late by its offset too.
- **`time` read inside the voices of a `poly` after a late node** isn't delayed (lanes, and `time` outside voices, are).
- **A poly's glide isn't delayed with its notes** behind a late plugin: a glide read from such a path is a few ms early.
- **A plugin given back by an export** starts its held notes again from its lane's notes, not the delayed ones.

## Sounds

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 22 | **The rest of the Sampler**. Since std 0.5.0 it plays `regions` (PKG-15, PKG-16): velocity layers, round robin, release sounds, loops, and each region's tune, volume, pan and offset. Since std 0.6.0 a turn for each key (`turns: "key"`, as SFZ counts) and the sustain pedal (the voices hold every instrument's notes, RENDER-38) are built. Still missing: choke groups (a closed hat cutting an open one: a voice can't hear the others yet); crossfades between velocity layers; a turn counted for each region alone (SFZ counts a region's `seq_position` from the notes that match it; music2 counts each key's); band-limited playback (a sample played far up aliases); streaming, for instruments bigger than memory (every file is decoded whole, so a file may hold at most 2^27 samples and a pack 2^28 in all, [FMT-24](architecture.md#assets-and-hashes): the full Salamander grand, four times the lite one, waits for it); from SFZ, an envelope and a velocity amount for each region (the Sampler's `attack`, `release`, `mode` and `velocity` are for all its regions), a loop that goes on after the note ends (`loop_continuous`), and a release sound that gets quieter the longer the note was held (`rt_decay`); end points and reverse; zones or regions made by dropping several WAVs (code only today); a waveform and a root picker on its card; files other than WAV and FLAC. | Sampled drums, pianos and vocal chops are pop staples. | Yes | medium |
| 47 | **The rest of ⌘F** ([LIB-1](architecture.md#library)): a sound (a preset) that makes a track of its own in one step (today Enter loads it onto the selected track's instrument it is for, and a drop onto a track onto that track's, and each says so when there is none); previews of plugins and of a plugin's presets; the kind filters to click as well as type (Loops, One-shots and Fits my song are buttons since track samples); (**What you have** and **From the library** with Get are built since track library-get, [REG-13](architecture.md#registry); a sound from the library adds a track of its instrument, then loads the sound: two undo steps.) Samples are built since track samples ([Samples](guide/samples.md)): see #48 for their rest. | Finding a sound and hearing it before adding it is how a beginner builds a song. | Partly (a browser with previews) | medium |
| 48 | **The rest of samples** ([LIB-4](architecture.md#library) to [LIB-8](architecture.md#library)): AIFF and MP3 samples (WAV and FLAC only for now, #18; FLAC since track flac, [FMT-24](architecture.md#assets-and-hashes)); a Samples group in the side browser, with Add a Folder… and Remove there too (today in ⌘F's panel); your folders watched, so a file added shows at once (today a folder is looked through again a minute after its last look, at the next search); `m2 search --fits` and Fits my song by the playhead as it moves (today the beat when the button was pressed); half and double time in Fits my song (a 60 bpm loop in a 120 bpm song); `m2 credits`, listing every sample's note in a song; and a waveform in a sample's row. (A pack's samples from the registry's catalog, with their measures, are in ⌘F since track library-get.) A sample dropped on a Sampler that refuses it (one playing `zones` or `regions`, found only once the file is in) leaves its copy and note in `assets/`, as a WAV dropped there leaves its copy. | A musician's own sample folders are where most of their sounds are. | Partly (a browser of files) | medium |

- **Your presets can't be renamed or deleted** in the app, and there is no Show in Finder: tidy the Presets folder by hand.
- **A card doesn't say which preset it came from**, or that its knobs changed since.
- **Presets hold a sound's numbers only**: not its settings (`voices`, `mode`), its choices or its switches (CARD-10), and not whole tracks, chains or the mixer.
- **music2's own presets are for five sounds** (Synth pluck, Synth pad, Synth bass, Reverb and Delay), two each.
- **A preset is for one sound**: a Synth pluck's doesn't load into the older Pluck (`@m2/synths:pluck`), nor the other way.
- **A control a load can't write into its call** (its options are a variable) is left as it is without a word.
- **A preset named as Windows reserves a name** (`CON`, `NUL`, …) isn't refused yet.
- **The Presets list reads every file in its folder** each time it opens, a plugin preset's whole state too.
- **A plugin call with no key can't save or load presets**: it has no state file. The app says to add a key; it could add one.
- **A plugin's own presets show by name only**: not their tags, favourites, packs or authors. Two with one name: the first loads.
- **A plugin's own presets are read once per app run** (2,000 files at most): new ones show after a restart. Look again doesn't read them again.
- **A failed read of a plugin's own presets sticks until restart**, even a short one (the host process was restarting): the Presets popover says it couldn't read them, and its presets won't load.
- **A plugin preset of yours keeps only the numbers its call's code wrote when you saved it**: load it into a call whose code writes another one, and the code's number stays, without a word.
- **A plugin preset saved by another version of the plugin** loads without a word.
- **A plugin whose own preset loads only once it plays** would give its old state (Surge XT's load at once).
- **No `m2 preset` commands** (save, load, list) for agents (#37). With the song open in the app, `m2 call` lists and loads presets (`/api/graph/presets`, `/api/graph/preset/load`); saving one is the musician's.
- **A Sampler played far above its root sounds harsh**: it isn't band-limited, so high notes alias, and past 6 octaves up it plays no faster.
- **A Sampler's files are held in memory**, decoded whole (clips stream from disk). The kernel ABI allows streaming them ([kernel-abi.md](kernel-abi.md#the-asset-import)).
- **A WAV dropped on a Sampler that then refuses it** (one with `zones` in its code) is still copied into `assets/`, though the page says "Nothing was changed" (the ab review's 3).
- **An audio file a kernel reads that changes on disk** is heard at the next build, not at once. Its name in the code has no hash pin, and when it is missing no near name is offered.
- **Glide in `"poly"` mode** isn't built: `glide` does nothing there.
- **The wavetable synth's tables are its five and a WAV** ([PACK-27](architecture.md#packs)): no drawing or editing a table, frames of 2048 samples only (a file of other frames is cut wrongly), at most 64 frames, a frame's harmonics up to the 512th only (a note below 39 Hz has nothing above 512 times its pitch: A0's stops at 14 kHz), and position only crossfades between two frames (no spectral morph, no warp, sync or FM between the oscillators). Its LFO counts in Hz from each note's start, never in beats; nothing else modulates (no matrix); there is no noise or sub oscillator.
- **A preset holds no table**: the tables are settings, so Slow pad, Round bass, Sharp lead and Short pluck play whatever tables the card has.
- **Changing a wavetable's table ends the notes that are playing**: a table is a setting, made when the sound is built, so picking another from the menu rebuilds the sound (RENDER-26a).
- **A file in `fileA` (or `fileB`) plays in place of the table menu, which still shows its pick**: changing the menu then changes nothing you hear, yet rebuilds the sound. Only the guide says that the file wins.
- **Every wavetable sound holds 11.8 MiB**, whatever tables it plays: its voices share their tables in the module's static memory, sized for two tables of 64 frames, since the kernel ABI gives a sandbox's states no other way to share what they make.
- **`steal: "none"` waits for a voice to fall silent**, not for its key to be let go.
- **The side browser lists no sounds**: its search is ⌘F's, but it shows only instruments, effects and plugins. Sounds are in ⌘F and on each card's Presets.
- **The + Track list and the Device view's + search on their own**, in the window: only the side browser and ⌘F ask the library's one search.
- **A preview is kept in the cache folder until its item changes**: one file an item (about 860 kB), never trimmed, and one of an item that went (a preset deleted) stays.
- **The song's own instruments and effects have no preview**: their code is the song's, and a preview renders outside it.
- **The Sampler has no preview**, nor any instrument that plays a file you give it: a preview has no file to give it. It could play a sample music2 or a pack ships.
- **No check counts dropouts while a preview renders beside playback**: a preview renders at background priority, as an export's workers do, but no gate measures it, as G13 does for `m2 listen` (LIB-3).
- **A sound for a plugin, on Enter**, looks for the selected track's call of that plugin by its component, which a plugin's call may not give: it may say the track has none.

## Mixing

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 20 | **The rest of meters and loudness.** A track that shares a mixer channel with another track, or goes into two, has no fader of its own: its header shows its own sound, before any fader (its strip reads after the channel's). The song's whole loudness is known only once it is exported: the Export dialog says it then, not before. A card can't show its sound's scope yet (`view.scope()`, [Typed sounds](designs/cards/design.md) §3.2, track `views`): the stream it will draw is built. Built since track `taps` ([TAP-1 to TAP-9](architecture.md#taps)): meters after the fader, on each strip and each track with a channel of its own; the song's loudness over the last 3 s and its true peak on Master's strip while the Mix view shows, measured as `m2 listen` measures them; the export's loudness in the Export dialog; and a scope's stream, `POST /api/graph/scope`. | A beginner mixes by the meters, and a shared channel's fader can't say what one of its tracks does. | Partly (a meter on each track) | small |

- **Soloing a bus plays its tracks' dry sound too**: a wet-only solo needs a cell per path.
- **A send has no pan of its own**, and can't be made mono.
- **No solo-safe buses**, and no "solo in place" choice.
- **A send starts at the track's output**: no send from inside a track's chain, before its effects.
- **A group's tracks don't fold under it** in the timeline.
- **The graph view shows a bus's feed** as a song export named `bus:verb`.
- **A bus channel must read the bus as it is** (`verb: verb.out`): one written `verb: eq(verb.out)` isn't seen as the bus's channel, so the bus is never fed, which the build says.
- **A send's knob in the graph view is named by its id** (`hats.sends.verb.db`), not in words ("Hats send to Reverb", as its lane is).
- **A send level written into `{ pre: true }` is a plain number** (`db: -6`), even where the other sends write `db(…)`.
- **A Group bus shows a Gain knob** in the graph view: the 0 dB gain that gives the group a node of its own.
- **A send's knob can't be typed into**: Return opens its menu. Its level is written to two decimals (`db(-1.44)`), not tenths as a fader's.
- **The output picker lists every bus**, a Reverb too, not only Groups: a bus doesn't record what kind it was made as. A track sent wholly into a Reverb plays only wet.
- **A bus strip doesn't show its effect**: its name says it (Reverb); its knobs are in the Device view.
- **Many buses make the faders short**: each bus is a row on every strip, so a short dock with five buses leaves little fader.
- **A new bus goes last** in the timeline, not near the tracks that feed it.
- **Change ID on a bus breaks its routes**: the other channels' `sends` and `to` keep the old key, so the edit is refused with std's message, which names keys, not tracks.
- **An automated send shows "—" in the Mix view, with no menu**: remove it, or change its before or after, in the code.

## Timeline and clips

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 23 | **The rest of sections**: dragging selected bars to move them (today: cut and paste, or `m2 sections move`); reordering sections by dragging a marker, the rest making room, as Logic's arrangement track does; a paste or a duplicate that makes room rather than replacing what is there (today: insert bars first); a copy of bars in the system's clipboard, to paste into another window. Built since track `sections` ([ARR-1 to ARR-9](architecture.md#sections), [Sections](guide/sections.md)): bars selected along the ruler's top band or by a marker's Select to next marker, on every track, lanes of names included; copy (the server's), cut, paste at the playhead's bar, duplicate, empty, insert bars and delete bars, each one undo step, with markers and tempo and time signature changes moving with insert and delete bars; `m2 sections`, `POST /api/graph/sections` and `/api/graph/sections/copy`. Bars put where the time signature makes them end inside a bar are refused rather than fitted. | Pop songs are arranged in sections; moving one by dragging is how other DAWs do it. | No | small |
| 9 | **The rest of clip editing**: a box drag that scrolls at the timeline's edge; copied clips in the system's clipboard, to paste into another window; a flag on the ruler where the song ends (taste.md TL-12). | Pop songs are built by copying parts. | Yes | small |

- **A nudged clip is heard once it lands** (← →), and so is a dragged audio clip; a dragged notes or automation clip is heard while it moves.
- **A tempo drag in a song with an audio clip** is heard only when you let go. Its tooltip says so.

## Notes

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 16 | **The rest of scale help: an arpeggiator, and scales in code.** An arpeggiator waits: with std kept small ([Typed sounds](designs/cards/design.md) §7) it belongs in a pack, and it needs kernels that make events, which the kernel ABI reserves but doesn't build ([ABI-2](architecture.md#kernel-abi-reservations)). `std/theory` (keys and scales for song code) and `std/gen` (generators) are designed ([song-as-program.md](song-as-program.md)) but not built; the song's key lives in `layout.json`, which code doesn't read. Built since track `notes` ([NOTE-1 to NOTE-9](architecture.md#notes-and-scales)): thirteen scales (#6), Lock to key and chord stamps (#16), copy and paste of notes between clips, ⌘ to place a note off the grid, more swing and grooves (#15), folded drum rows and taller rows (#3). | Beginners write better melodies and chords with a scale guide; an arpeggio is how a held chord becomes a part. | Partly (a key and scale, only in-key notes shown) | medium |

- **A groove of sixteenths after one of eighths** sees the swung eighths as off its grid, so it leaves those beats as they are: undo, or Quantize, before changing the step. Grooves of one step (the swings of a step; Laid back and Pushed) replace each other's places.
- **A stamp's chord names spell a note of the key with its own letter** (G♯ in A harmonic minor), but a key's root keeps the app's one spelling (E♭ minor, never D♯ minor), so a few keys spell notes as C♭, F♭ or E♯ (a note whose letter would need a double sharp or flat is spelled on its own: E♭ Locrian's fifth is A).

## Automation

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 14 | **Automation comforts**: curves between points (an ⌥-drag; it needs a field in the lane's file and new interpolation in the engine), a log scale for units other than Hz, and typing a point's value. | Sweeps sound smoother on a curve. | Partly (drawing points) | medium |

## Effects

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 21 | **The rest of the effect comforts**: the old Pump (ducking in time with the beat, with no sidechain), a compressor keyed from another track, and moving effects that aren't written one inside the other, or across files (refused in words today). | Pump is a pop staple. | Yes | medium |

- **Built since track effects2**: `@m2/mixing`, a pack music2 ships ([PACK-26](architecture.md#packs)): an eight-band parametric EQ, a gate that another track can key, a transient shaper, a de-esser and a multiband compressor, each with its own kernel and presets.
- **The EQ's card draws no curve**: no view draws an EQ's whole curve, every band's shape and gain together (that is a new view, a change to the card's closed vocabulary, CARD-16), and the filter view draws a low-pass, which no band is at the EQ's start.
- **The gate's key is declared `many()`** (`gate(pad.out, kick.out)`): core has no input a call may leave out, so the gate's descriptor says it takes any number of keys. TypeScript allows one, and the build refuses more. An input that may be left out (`port.audio().optional()`) is a minor of `@m2/core`, a change of its own.
- **A big change on a very resonant EQ band arrives late**: a new shape, or a jump of gain, Q or frequency, settles on a second filter before it is crossfaded in, so it never booms; at a high Q and a low frequency that takes up to a second.
- **A gate's key is written in code** (`gate(pad.out, kick.out)`): its card has no picker of tracks for it; the Device view's picker is the Duck's own.
- **A preset of the EQ holds its numbers only**: it can't turn a band on or change its shape, since a preset's values are numbers (FMT-22). CARD-10 promises a choice's name in a preset too; that isn't built yet.
- **The EQ's cuts are 12 dB per octave**, with no steeper slope to pick; the multiband compressor has three bands, and no way to hear one band alone; the de-esser has no way to hear only what it takes out.

## Export

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 36 | **The rest of export**: MP3 or AAC, stems of a song that plays plugins, and a range (the loop, a selection), bit depth and normalize. | Sharing ideas with other apps and people. | No | medium |

- **An export whose sound had NaN or infinite samples** silences them, and the app doesn't say so (`m2 render` warns).

## Tracks, knobs and the Device view

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 35 | **Knobs for more numbers**: a number held in a variable (`const cutoff = 800`) or worked out from others (`2 * 440`) shows **edit in code**, and so does a setting held in one (a card shows its settings since track `cards`, CARD-1). | A beginner who gives a number a name, as tutorials teach, loses its knob. | No | medium |
| 1 | **The rest of changing an instrument**: a track whose instrument is built from parts, has two instruments or a plugin, or whose settings code works out, is refused in words. Adding a track to a song that mixes its own way, or changing a track's instrument to a CLAP plugin, still needs code (+ Track can add a plugin track). | Changing your mind about a sound shouldn't mean a new track. | Yes | small |
| 10 | **The rest of changing a track's ID**: a track with a new ID restarts its sounds (its plugins' and instruments' state isn't carried over, design-8b §11.3), and the status popover has no Restore for a rename done by hand in code (DIAG-6). | A rename shouldn't cut a sound that is playing. | Yes | small |

- **Disconnecting a lane of names from the only choice it feeds is refused**, as the lane would have no names: take its automation out instead. A lane of names' header shows no value at the playhead, and Draw mode (B) paints only number lanes.
- **A view's drag of two knobs isn't kept while the code doesn't compile**: it is refused, said in words (one knob's drag waits, C5; CARD-22).
- **The filter view draws a low-pass only**, whatever filter the sound is, and at 12 dB per octave, whatever its slope (the polysynth's ladder falls 24): a view says nothing of the kind of filter or its slope (a `view.filter` with a shape and a slope is a contract change), so its words name neither. The envelope view holds its sustain for a time of its own choosing, a quarter of the rest. A part a lane drives is drawn at the lane's fallback, not at its value at the playhead; a part a cable drives, one the code works out, or one written in a unit the picture doesn't draw (a cutoff in semitones) greys the view, which says why: converting semitones to Hz for the picture isn't built.
- **The keys view reads what std's Sampler plays**: a sound whose regions reach another kernel draws no keys, and says so. Its keys can't be dragged to move a region: regions are changed in the code.
- **No std kernel reads a choice or a switch**: a choice reaches only a kernel whose port names its options (`control<choice<saw|square>>`, CARD-12). The polysynth's wave shape and the Drum's drum do since wave 3 (CARD-32, CARD-33), and the parametric EQ's band shapes and switches since `@m2/mixing` (PACK-26); std's kernels still take numbers (the LFO's shape is a setting). A kernel port that takes any choice (`control<choice<*>>`) isn't built either.
- **The Drum names no notes for the piano roll**: its hats play G#2 as a pedal hat and A#2 as an open one, but a lane's names come from the component (`editor.notes`), not from what its menu picks, and naming them would name a kick's rows "Pedal hat" too. The drum machine names every note; the single `drums.hats` did, before wave 3 (review Y14).
- **Drum kits from the side browser and ⌘F**: + Track and `m2 add-kit` add a kit (KIT-5); the side browser's Instruments and ⌘F's search list each Drum's instrument, not its kits.
- **A Drum has no level knob, and a kit sets only its Drums' knobs**: a kit's balance is the Drums' own levels (each drum alone peaks between −3 and −15 dB at full velocity) and its bus at −6 dB; to set one drum louder or quieter, move its track's fader. A kit that sets its tracks' faders, or a Drum level knob, waits for a need.
- **A MIDI file's percussion past the Drum's nine sounds** (bongos, congas, a triangle: General MIDI's notes 60 to 81) comes in on one Percussion track, played by the Drum's tom at each note's pitch (KIT-8): every note is heard, but not as the drum it names.
