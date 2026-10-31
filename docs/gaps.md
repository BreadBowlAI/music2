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
| 24 | **The rest of recording notes**: recording the pitch and mod wheels (it needs note expression data, a format bump); recording over a clip into it (overdub), and one clip for each time round a loop; the computer keyboard's velocity and sustain. | Expressive parts use the wheels; loop recording builds a part in layers. | No | medium |
| 25 | **The rest of recording audio**: an arm button (hear yourself without recording); takes stacked per loop pass; punch in and out; inputs 3 and 4 (only the first two channels record); a recording offset for a device that reports its latency wrongly (the output's is taken as two buffers). | Singers stack takes and fix one line by punching in. | No | medium |

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
- **The pitch wheel picks a different drum on a drum kit**: a kick (36) bent 2 up plays the snare (38) (the pm review's 2).
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
| 22 | **The rest of the Sampler**. Since std 0.5.0 it plays `regions` (PKG-15, PKG-16): velocity layers, round robin, release sounds, loops, and each region's tune, volume, pan and offset. Still missing: a sustain pedal (needs controller events in kernels); choke groups (a closed hat cutting an open one: a voice can't hear the others yet); crossfades between velocity layers; a turn counted for each key or region alone, as SFZ does (std counts the lane's note starts, so on one lane notes that alternate keep their takes: give a drum whose takes must alternate a lane of its own; a count for each key needs one that sees each note's pitch, and one signal outside the voices can't say which keys of a chord started, so it waits for event ports in kernels, ABI-2, or for the poly to give each note its key's count); band-limited playback (a sample played far up aliases); streaming, for instruments bigger than memory (every file is decoded whole, so a file may hold at most 2^27 samples and a pack 2^28 in all, [FMT-24](architecture.md#assets-and-hashes): the full Salamander grand, four times the lite one, waits for it); from SFZ, an envelope and a velocity amount for each region (the Sampler's `attack`, `release`, `mode` and `velocity` are for all its regions), a loop that goes on after the note ends (`loop_continuous`), and a release sound that gets quieter the longer the note was held (`rt_decay`); end points and reverse; zones or regions made by dropping several WAVs (code only today); a waveform and a root picker on its card; files other than WAV and FLAC. | Sampled drums, pianos and vocal chops are pop staples. | Yes | medium |
| 47 | **The rest of ⌘F** ([LIB-1](architecture.md#library)): a sound (a preset) that makes a track of its own in one step (today Enter loads it onto the selected track's instrument it is for, and a drop onto a track onto that track's, and each says so when there is none); previews of plugins and of a plugin's presets; the kind filters to click as well as type (Loops, One-shots and Fits my song are buttons since track samples); (**What you have** and **From the library** with Get are built since track library-get, [REG-13](architecture.md#registry); a sound from the library adds a track of its instrument, then loads the sound: two undo steps.) Samples are built since track samples ([Samples](guide/samples.md)): see #48 for their rest. | Finding a sound and hearing it before adding it is how a beginner builds a song. | Partly (a browser with previews) | medium |
| 48 | **The rest of samples** ([LIB-4](architecture.md#library) to [LIB-8](architecture.md#library)): AIFF and MP3 samples (WAV and FLAC only for now, #18; FLAC since track flac, [FMT-24](architecture.md#assets-and-hashes)); a Samples group in the side browser, with Add a Folder… and Remove there too (today in ⌘F's panel); your folders watched, so a file added shows at once (today a folder is looked through again a minute after its last look, at the next search); `m2 search --fits` and Fits my song by the playhead as it moves (today the beat when the button was pressed); half and double time in Fits my song (a 60 bpm loop in a 120 bpm song); `m2 credits`, listing every sample's note in a song; and a waveform in a sample's row. (A pack's samples from the registry's catalog, with their measures, are in ⌘F since track library-get.) A sample dropped on a Sampler that refuses it (one playing `zones` or `regions`, found only once the file is in) leaves its copy and note in `assets/`, as a WAV dropped there leaves its copy. | A musician's own sample folders are where most of their sounds are. | Partly (a browser of files) | medium |

- **Your presets can't be renamed or deleted** in the app, and there is no Show in Finder: tidy the Presets folder by hand.
- **A card doesn't say which preset it came from**, or that its knobs changed since.
- **Presets hold a sound's numbers only**: not its settings (`voices`, `mode`, #35), and not whole tracks, chains or the mixer.
- **music2's own presets are for five sounds** (Synth pluck, Synth pad, Synth bass, Reverb and Delay), two each.
- **A preset is for one sound**: a Synth pluck's doesn't load into the older Pluck (`std:pluck`), nor the other way.
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
| 20 | **Meters after the fader, and loudness in the app.** The app's meters read each track and bus before its fader, so a fader move doesn't move them: after-fader taps need the engine (`core/runtime`). The Export dialog says nothing of loudness. (`m2 listen` already measures each track after its fader, and the mix's loudness and true peak, from the command line.) | You can't see what a fader does to a track's level, or how loud a song is. | Partly (a meter on each track) | medium |

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
| 23 | **Sections**: copy or move a whole section (a chorus) across all tracks, and insert or delete bars. | Pop songs are arranged in sections, and copying a chorus is one gesture in other DAWs. Inserting bars touches every lane and the tempo. | No | large |
| 9 | **The rest of clip editing**: a box drag that scrolls at the timeline's edge; copied clips in the system's clipboard, to paste into another window; a flag on the ruler where the song ends (taste.md TL-12). | Pop songs are built by copying parts. | Yes | small |

- **A nudged clip is heard once it lands** (← →), and so is a dragged audio clip; a dragged notes or automation clip is heard while it moves.
- **A tempo drag in a song with an audio clip** is heard only when you let go. Its tooltip says so.

## Notes

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 16 | **Scale help**: lock notes to the key, chord stamps and an arpeggiator. `std/theory` and `std/gen` are designed but not built. | Beginners write better melodies and chords with a scale guide. | Partly (a key and scale, only in-key notes shown) | medium |
| 15 | **The rest of the note tools**: copy and paste notes between clips, hold ⌘ to place a note off the grid, other swing amounts, and groove templates. | These clean up a drawn or played part. | Partly (humanize, octave up and down) | small |
| 3 | **Drum rows folded to the drums a clip uses**, and zooming the piano roll up and down (taste.md NT-5). | A kit has 30 rows; a beat uses five. | Yes | small |
| 6 | **More scales** than major and minor (dorian, pentatonic…). | A key tells you which notes fit. | Yes | small |

## Automation

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 14 | **Automation comforts**: curves between points (an ⌥-drag; it needs a field in the lane's file and new interpolation in the engine), a log scale for units other than Hz, and typing a point's value. | Sweeps sound smoother on a curve. | Partly (drawing points) | medium |

## Effects

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 21 | **The rest of the effect comforts**: the old Pump (ducking in time with the beat, with no sidechain), a compressor keyed from another track, and moving effects that aren't written one inside the other, or across files (refused in words today). | Pump is a pop staple. | Yes | medium |

## Export

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 36 | **The rest of export**: MP3 or AAC, stems of a song that plays plugins, and a range (the loop, a selection), bit depth and normalize. | Sharing ideas with other apps and people. | No | medium |

- **An export whose sound had NaN or infinite samples** silences them, and the app doesn't say so (`m2 render` warns).

## Tracks, knobs and the Device view

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 35 | **Knobs for more numbers**: a number held in a variable (`const cutoff = 800`) or worked out from others (`2 * 440`) shows **edit in code**; settings such as an instrument's `voices`, `mode` and `glide` aren't on its card (the card says how it plays, read-only). | A beginner who gives a number a name, as tutorials teach, loses its knob. | No | medium |
| 1 | **The rest of changing an instrument**: a track whose instrument is built from parts, has two instruments or a plugin, or whose settings code works out, is refused in words. Adding a track to a song that mixes its own way, or changing a track's instrument to a CLAP plugin, still needs code (+ Track can add a plugin track). | Changing your mind about a sound shouldn't mean a new track. | Yes | small |
| 10 | **The rest of changing a track's ID**: a track with a new ID restarts its sounds (its plugins' and instruments' state isn't carried over, design-8b §11.3), and the status popover has no Restore for a rename done by hand in code (DIAG-6). | A rename shouldn't cut a sound that is playing. | Yes | small |

- **A knob let go while the code doesn't compile** keeps playing its new value, and is written once the code compiles; after 30 seconds it is dropped, with a message. Writing it at once, when the broken file is another one, is the owner's call (the step 6 design's C21).
- **Two numbers edited on neighbouring lines by another program** count as a change to the calls between them, so the song is rebuilt (crossfaded) rather than the numbers written into what plays.

## Problems and help

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 40 | **The guide from inside the app**: Help ▸ music2 Guide, opening the guide's website (it is public once the repo is), the site in the app's look (one CSS file for mdBook), and a copy of the site for each release once there are releases (today it follows `main`, and `m2 docs` its own version). | A musician looks for help in the Help menu, not on GitHub. | No | small |
| 41 | **More of the guide made from code**: the shortcuts page from the web app's own table, the WebSocket events (in [The local API](guide/api.md)) from the server's, and std's sounds and their knobs from std's descriptors. Today these are written by hand; `commands.md` and `routes.md` are made by `m2 docs` and a test keeps them fresh (AGENT-12). | A page written twice drifts from what the app does. | No | medium |
| 11 | **The rest of plain-language help**: a missing plugin reads technically (``no CLAP plugin `org.surge-synth-team.surge-xt` is installed (looked in [...])``; the start screen's Examples names it plainly), and the docs past [Music basics](guide/music-basics.md) and the guide's manual part is technical. | Many people who open music2 are new to making music. | Yes | small |

- **A problem from a sound or plugin that stopped can't be dismissed**: it stays in the status popover until the part plays again.
- **A sound that stops in the moment between Stop All Sound and its reset** stays listed, though the reset starts it again (ex-1).
- **Two sounds with the same words** ("The delay on Bass", twice on one track) share one toast; each is listed (ex-1).
- **A sound or plugin that stops just as two edits come quickly** (a tempo drag, quick code changes) may never be told: faults are named by the last two plans only (the ex review's r1).
- **A stretch that fails while the song plays** is told without why (the kernel's own words) (ex-2).
- **A kernel that asks for memory while playing** is told it went over 256 MB, even when it asked for a few MB (the ab review's 5).
- **`m2 check` doesn't find a file in `assets/` that won't decode**; only a build does, as `asset.unreadable`.
- **A file added to a renamed track's folder afterwards** (a Finder copy) isn't noticed until something else changes: "Move all" is then refused, naming the file.

## Playback

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 19 | **The click's level and sound.** | Some songs need a quieter or a different click. | No | small |

- **Going back after Stop is a jump** (Space then Space, or Return then Space): std's chorus, phaser and tremolo, and the drums' noise, carry on from where they were, so that play may differ a little from the export. The export is always right. An open question for the owner (HANDOFF).
- **An oscillator or noise written straight into a track's code**, outside a voice, or a plugin, runs on while the song is stopped, so it may start from another place than in the export.
- **A sound part that crashed is reset when it starts again**, not built new, so it may crash again sooner.
- **A song near the 3,999 sound parts limit** has no cheaper way to run yet (a `sum` kernel, a checked engine).
- **An output chosen in Settings that comes back** after it was unplugged isn't taken again by itself; choose another output, then it.

## Settings and projects

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 30 | **Crash safety**: keep unsaved code-editor text through a crash, and simple project snapshots or backups. | Edits are on disk at once and a cut-off edit is offered back, but unsaved code is lost. | No | large |
| 29 | **Undo that survives closing the app.** | Closing the app, or the song, loses all undo. | Partly (lost on closing too) | medium |
| 31 | **The rest of Settings**: view settings that hold for every song (zoom, grids, the audition toggle and the metronome are kept per song). The sample rate stays the song's ([RENDER-31](architecture.md#sample-rate)). | Most people want one zoom for every song. | Partly (it showed the device and the folders) | small |
| 12 | **A picture of each song in the recent list** (taste.md SS-2). | A small picture of each arrangement helps find the right song. | Yes (the Song Library) | small |
| 28 | **Open a song by dropping its folder** on the window or the Dock. | Finding a song in the Finder is how many people open it. | Yes | small |

- **The dock and the browser panel start over at each launch** (which view the dock shows, and its height): the browser's storage starts empty, as the page gets a new address each time.
- **Renaming a song's folder starts Undo over**: the song closes and opens again from there (Rename Song… says so).
- **A song asking for std "0.2", "0.3" or "0.4"** gets `pkg.unsatisfied` in this app (std 0.5.0); its message names the line to write (`std = "0.5"`). A "0.2" song may sound a little different (one-note sounds now keep each note's own velocity); a "0.3" or "0.4" song sounds the same, though the message says it may not.
- **A song's std line names no patch version** (`std = "0.5"`), so a song that needs a later patch gets a missing import in an app with an earlier one, not `pkg.unsatisfied`.

## Code editor

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 34 | **Create, rename and delete files** in the code editor (today it opens existing `.ts` files only), and edit the data JSON and `music2.toml` there. | Giving a track a file of its own still needs another editor. | No | small |

- **The code editor reloads a file an edit changed** rather than following the edit in place; unsaved text in it is kept, with the "changed on disk" choice.

## Packages

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 39 | **Shared sound packages, from the registry**: the registry repo itself and its bucket (the owner's: D10, the bucket's address, which `DEFAULT_REGISTRY` waits for: until then the library is only where `M2_REGISTRY` or the settings' `registry` say, and no window sets it); in the app, the cache's previews and blobs pruned (they stay until the cache is cleared), the catalog signed (today it is checked by its hash against `catalog.json` from the same host, and an older one refused, so the registry's address is trusted for what it lists, `@m2/` packs too), the downloads search ranks by (design §9.2: the catalog counts none), an effect's sound from the library added as a new effect with its sound (today it loads as an installed one does, onto the selected track's effect), Get from the side browser (⌘F's only), and a browser suite for the status popover's Get of a song's missing pack (the route and `m2 pack get` are tested); and `m2 pack publish` (today an author packs up the `.tar.gz` and makes the release by hand, then `m2 registry add`); a later version of a pack merging by itself once the robot passes (today a person merges every pull request: it needs a check the pull request can't change); and a take-down's purge from Cloudflare's cache (by hand today). Built since track registry: the registry's format and its robot, `m2 registry check`, `add`, `pending` and `build` ([REG-1 to REG-10](architecture.md#registry)), with the registry repo's template in `registry/`. Built since packs: packs from a folder, `m2 pack install` into the store (`~/Music/music2/Packs`), `music2.lock`, scoped names (`@owner/name`), a pack's own recordings, and the browser and + Track offering what is installed ([Packs](guide/packs.md)). Since track `library-get`, the registry in the app ([REG-11 to REG-17](architecture.md#registry), [Getting packs from the library](guide/library.md)): the catalog fetched and kept, ⌘F's From the library with its previews, Get (a pack's whole version, or one sample) with progress and Cancel, Get for a song missing a locked pack, `m2 pack get`, and the library in `m2 search`. Since track `fetch`, the downloads (`m2_editor::fetch`, [NET-1](architecture.md#network) to NET-6), which the robot and the app's Get use; they use no proxy (no `HTTPS_PROXY`, no system proxy), so a network that needs one can't download, and a `.part` file a crash left stays until whoever owns its folder removes it. Since track `limits`, a time limit on every call into a pack's kernel ([ABI-6, ABI-7](architecture.md#kernel-abi-reservations); std's, music2's own, have none): a pack's sound that never returns is stopped after 100 ms while the song plays, and stays silent until the song is built again; an export, a render or a preview stops it after 5 s and fails, naming it; one that never gets ready is refused once, then at once; the registry's robot refuses a version whose preview it stopped. The checks cost a pack's kernel's call about a fifth more. | A pack has to be passed around as a folder. | Partly (single sounds from GitHub) | large |
| 46 | **The rest of packs from a folder**: the lists (the browser, + Track, presets) and the code editor's types are made as a song opens and when its packs change, so a pack installed while it is open (by `m2 pack install`) shows when it opens again or its packs change (a Get in the app makes them again at once); a path pack's files aren't watched, so an edit to one (its TypeScript, its `m2pkg.toml`, its wasm) reaches the song with the song's next build; the store keeps each version whole, not each file once by its hash (design §11); `m2 pack check` loads each kernel as a song does rather than in a sealed sandbox of its own, takes any licence named rather than the design's list (D6), checks a preset against its sound's controls rather than loading it onto a call, and checks no tags (P7). | A pack's author and a musician who installs while a song is open both have to open it again. | No | small |
| 49 | **The rest of instruments as data, and of SFZ import**: a data instrument's `tags` reach no list, search or registry catalog yet (the robot's facts, REG-4, carry the library's tags, which don't include them); only std's Sampler plays data (`engine = "std:sampler"`), and its `attack`, `release`, `mode` and `velocity` are for all its regions; `m2 pack import-sfz` leaves out what the Sampler doesn't play (a sustain pedal and other controllers, regions chosen at random, crossfades between layers, an envelope and a velocity amount for each region, `loop_continuous` after the note ends, a loop the WAV itself holds: gap #22) and says so; the Sampler's `velocity` follows a straight line where SFZ's `amp_veltrack` follows the velocity squared (at the piano's 73 %, soft notes are up to 3.3 dB louder), and the import says so; a region whose root is the WAV's own (`pitch_keycenter=sample`) or whose value has a unit (`tune=+5c`) is left out; an incomplete round robin (a take missing, silent in SFZ) stops the import; the import reads only the SFZ's own folder, so a library whose SFZ sits beside its samples' folder (`default_path=../Samples/`) must have the SFZ moved up first; it reads no Decent Sampler (`.dspreset`). The first pack made this way, the Salamander grand (lite: 4 of its 16 velocity layers, 208 WAVs, 300 MB), takes about 430 MB of memory while it plays, every recording decoded whole (streaming, #22). | Most free sample instruments are SFZ, and a musician should never have to write code for a recorded instrument. | No | medium |

- **Kernel ABI v1's reserved parts aren't built**: event ports, `k_save`/`k_load` and `k_latency`/`k_tail` ([kernel-abi.md](kernel-abi.md#reserved-shapes)). Each comes with the first kernel that needs it.

## Scripts, agents and the command line

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 37 | **`m2` commands** for change instrument, markers, the song's key, a track's ID, Save As and Follow tempo, with the app closed. With the song open in the app, `m2 call` reaches each but Save As, which is the musician's (AGENT-11): `/api/graph/track/instrument`, `/api/graph/markers`, `/api/graph/song-key`, `/api/graph/track/rekey`, and Follow tempo as `/api/graph/clips` with `{"op": "audio", "follow": true}`. With it closed, `m2 clips <song> json` takes that Follow tempo gesture too; the others need a Tx or the files. | Scripts and agents need the same basics as the app. | Yes (in `music2-cli`) | small |
| 42 | **`m2` on your PATH on a Mac**: a link in `/usr/local/bin` to the `m2` inside `music2.app` (or from source, `target/release/m2`), or a menu row that makes one. The Linux `.deb` installs `/usr/bin/m2` since 0.1.0. Today a song's `AGENTS.md` names its `m2` by its full path, so an agent needs no PATH, but you type the whole path. | Typing `m2` alone in Terminal. | No | small |
| 38 | **A schema for the API**: `m2 routes --json` lists every route with its words and an example body (AGENT-8), but no JSON Schema of what each takes and answers; and a short summary of the song, and MCP. | An agent learns a body from one example, not every field it may give. | Yes | medium |

- **A song's `AGENTS.md` names the `m2` it was made with, by its path then** (FMT-23): moving `music2.app`, or a first launch macOS ran from a copy it moved aside (App Translocation: an app downloaded and not yet moved to Applications), leaves it naming an old path. `m2 docs agents > AGENTS.md` writes it again, as does a song made before 2026-10-25, which has none.
- **`m2 listen` reads its words twice** (AGENT-13; the review of `flags`, W8): `words.rs` takes them by its usage, then `listen::args` parses them again, for its own tests' sake. Every other command reads its flags through `words::value` and `words::on`; handing each command a typed result of the one reading would make that one place.
- **While a song's code doesn't build, only `m2 edit` and `m2 write` of a code file that is there work** (the agents design's review, m8; the review of `ag-fwd`, F12): they save it as the code editor does, and only if the song then builds, as one undo step. Every other `m2` write waits for the code to build: `m2` can't plan without built code, though the app plays the last version that built and would take a data edit (WRITE-14). A fix that needs two files changed at once can't be made from `m2`, since each save must build on its own, and neither can a new code file (the app's save route saves only files that are there): fix those in the app's code editor, or write the files directly (an outside change, one undo step).
- **A knob drag holds its whole file for agents** (AGENT-3): while you drag a fader in `song.ts`, every agent write to `song.ts` waits, `add-track` included. Matching by the call the drag holds would be finer; a drag lasts seconds, and `m2` waits up to 30 s.
- **On arrival, an agent's write through a gesture route** (`/api/graph/set`, `/connect`, …, as `m2 call` sends them) **is checked only against a take and the Compare pill**: its files aren't known before it is planned. A drag or unsaved code in its file is found at the settle, after the compile, and the change is taken back then, answered busy. `m2`'s own write commands send Tx, whose files are known, so they are refused at once.
- **An agent's import taken back at its settle leaves its file in `assets/`** (the review of `ag-routes`, F2): when you begin a drag on its lane, or a plugin window its change would end, in the milliseconds it lands, the change is taken back, but the file it named into `assets/` stays, named by nothing. music2 never deletes an asset (Q4), and an import of the same bytes uses it again. Refused on arrival, nothing is left: that is the case you meet (V, a take).
- **`POST /api/tx/commit` still takes `assets`**: new files under `assets/` a forwarded Tx names, which its sender put there first. No `m2` command sends any since `m2 import` goes through the app's own import (the review of `ag-routes`, F2); a script that did would write them itself, an outside write AGENT-3 can't refuse. Staging them as the app's import does, or taking `assets` out, would close it.
- **`m2 bounce` and `m2 render` into `assets/` write their file themselves** while the app has the song open: an outside write, which closes the Compare pill and can't be refused (AGENT-3). `bounce --place` then waits while busy for its clip. Handing the file to the app, as `m2 import` does now, would fix that.
- **The page names its own window** (`client`, one per page load): any socket opened with the page key that gives another window's id ends that window's drags when it closes. Only the musician's own pages have that key; a socket opened with the tools key names no window (the review of `ag-routes`, F1). The server could hand each page its id instead.
- **One list of unsaved code for the app, not one per window** (the review's m5): the newest post wins. The app has one window, so it is that window's.
- **A plugin window's change reaches the app a few milliseconds after it is made** (AGENT-3): the plugin host passes on what plugins ask for (`mark_dirty`) every 5 ms, and the host process sends it down its pipe. An agent's write of that plugin's state landing in that moment isn't taken back: the settle's check hears what the windows told the app up to then. Found when `a_window_change_made_while_an_agents_state_lands_takes_it_back` raced the sweep (the review of `ag-see`'s full check).
- **A tempo drag is checked by its file**: an agent's change to `data/arrangement.json` is busy while you drag the tempo, as AGENT-3 says. The arrangement lives only in that file today; a change elsewhere that moved it would need the carry's check that knob and clip drags have now.
- **A plugin window not saved yet is checked against the plan an agent's change makes at once**: while an export holds playback, that plan waits for the export, and the app's own settle after it could drop the window's instance, with no agent's change left to take back. Rare: an export, a window not saved, and an agent's change that takes its plugin out, all at the same time.
- **A `git checkout` that finishes between two looks of the watcher** (50 ms) is taken as an outside edit, one undo step: a batch is git's only when git's `index.lock` was seen while it settled.
- **The glow outlines every clip of a changed lane**, not only the clips that changed; notes in the piano roll and automation points don't glow. A rewrite of more than 500 lines, or of more than 500 tokens in one run of changed lines, is one change to the carry (`DIFF_LINES` in `core/session/src/text.rs`), so no knob inside it glows.
- **Outside saves aren't merged into one step**: an editor that saves every second as you type (autosave) makes a step each time, and 500 of them push the app's own changes out of undo. Merging close saves of the same files, with no other step between, would fix that.
- **Only undo's newest step can be skipped**: a redo whose file changed since stays refused until the next change clears redo.
- **One focus record for the app, not one per window** (AGENT-6): the newest post wins, as with the unsaved list. The app has one window, so it is that window's.
- **The focus record names a note by its pitch, start and length**: notes have no ids, so two notes alike in all three can't be told apart.
- **`m2 watch` reads the transport every quarter second**, as the page does: no event says it, and a seek of less than half a beat isn't said.
- **A pointer at an automation lane hidden one by one** (A's list) waits at its track's header; Show unfolds the track's automation but doesn't bring the hidden lane back.
- **A pointer's note looks where its target is every frame** while it shows: a pointer is rare and goes with OK, and the timeline's zoom says nothing when it changes, so events alone would miss it (the review of `ag-see`, F10).
- **Compare's Before is heard, not shown**: while Before plays, the window shows the song as it is (After), with the pill saying Before. Showing Before in every view would be a second world (the agents design's out of scope).
- **V compares only the newest change**: an agent's change under a newer one of yours can't be heard before and after on its own (taking back an older step alone is out of the design's scope).
- **`m2 listen` finds a track as it reaches the mix in std's mixer only**: in a song that mixes in code of its own, or a channel two tracks share, it measures each track's own sound, and says its masking isn't measured.
- **An automated fader is found by its channel's place** among the faders std's mixer made, in the order the channels are written: a channel whose key reads as a whole number (`"808"`) comes first in JavaScript's own order, and the wrong fader would be read for it.
- **`m2 listen` measures on one thread**, after each piece of its render: the render itself takes most of its time (8 bars of p50: about 13 s on Linux, on one thread beside playback).
- **A track inside a group bus masks only as that bus**: masking is among what the master adds up, so a kick in a drums group clashes with the bass as `drums`. Comparing each grouped track at the master would need its part of the bus's sound, which an effect on the bus (a compressor) doesn't keep apart (the review of `ag-listen`, F5).
- **`m2 listen`'s masking lists every pair that ever meets**: p50's 50 tracks, many playing the same notes, give 7,622 pairs (1.3 MB of JSON for 8 bars). The words show the 12 that meet the most; `jq` filters the rest by `share`.
- **On Linux, a listen costs playback's callbacks about what any one busy core costs** (G12 beside it on the build server's 7950X3D: p50 +14 to +22%, p99 +12%; a bare busy loop at the same class costs as much). `SCHED_IDLE` gives way at once, but the cores share their clocks and caches, which no scheduling class keeps; and Linux's engine threads aren't pinned, so no core is playback's to keep the listen off (the review of `ag-listen`, F6). Recorded, as Linux timings are. And `SCHED_IDLE` runs only on a core nothing else wants: on a machine busy on every core with other work (a build), a listen waits for one.
- **On a Mac, `m2 listen` gets only what the efficiency cores have left**: its background state runs it on them (two on an M1 Pro), below all other work there. On the Mac's first full check (`a1e3c77`) ten listens at once, before the tests took turns, took 734 s where two Linux cores take 16 s, and one listen of 8 bars of p50 didn't end in G13's minute (G13 now listens to 2 bars). How long one listen alone takes there, the next Mac run's G13 lines say. Whether the background state is the right lowest priority on a Mac is the owner's call: the utility class, for one, may use a fast core when the slow ones are busy.
- **A background plugin host's audio threads still ask for real time** (`m2_os::rt::engine_thread` in `core/plugins/src/remote/child.rs`), as the app's host does. On Linux with real-time rights (`RLIMIT_RTPRIO`, an audio group), such a thread would leave `SCHED_IDLE` and run above playback's other work; the build server grants none, so no test sees it. A host started with `--m2-background` shouldn't ask. Found while fixing the Mac's reading of a host's background state (`mf3`).
- **`m2 listen` names a plugin window's unsaved changes only when it asks the app**: with `--bars` and without `--as-heard` it asks the app nothing, so it can't say that it hears a plugin's saved state while you hear your window's change (the review of `ag-listen`, F17).
- **`m2 listen --png`'s picture has no labels**: the JSON says which block is which (`png.blocks`) and which row which band (`png.rows_hz`).
- **`m2 listen` has no loudness range** (LRA, EBU Tech 3342) and no momentary loudness: integrated and the short-term maximum only, as the design asks.
- **`m2 listen` stops at the song's last clip**: bars past it are refused, so a song with no clips (a drone in code) can't be listened to by bars.
- **G13 measures the gate's own player, not the app's**: the app in its process takes the forwarded edits and compiles them beside the live run, but what plays is G12's player (G12 through the app's server, `--server`, isn't built). Its listens of `examples/with-plugins` run their plugin host beside it only when the test plugins are installed.

## Releases and installing

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 43 | **A Mac app signed with a Developer ID and notarized by Apple.** 0.1.0 is signed ad hoc, so macOS asks for Open Anyway once. The switch is built (the repo's Apple secrets; `ci/release.sh`, PLAT-8): it turns the hardened runtime on with `apps/music2-desktop/entitlements.plist`, which has never been tried on a Mac. Before the secrets go in: keep them in a GitHub environment `release` that only `v*` tags may use, with the owner as required reviewer, not in the repo's secrets; give the signing its own job with `environment: release`; and add a ruleset that lets only the owner create `v*` tags (the release review's L5; `release.yml`'s head says the same). | A first launch with no trip to System Settings; some schools and companies allow only notarized apps. | No | small |
| 44 | **More downloads**: Intel Macs (a universal or x86_64 build), Linux on ARM, an `.rpm`, Windows. | People on those computers can only build from source, or not at all. | No | medium |
| 45 | **Updates**: the app doesn't say when a newer release is out, nor update itself. | Bugs fixed in a release reach only people who look. | No | medium |

- **The first Mac build of the release workflow hasn't run yet**: what it must show is in [the release build notes](designs/release/build-notes.md#what-the-first-mac-run-must-show).
- **Gatekeeper and the `m2` inside the app**: whether Open Anyway on `music2.app` also lets Terminal run `music2.app/Contents/MacOS/m2` from a download is untried; `xattr -dr com.apple.quarantine` on the app clears both.
- **The AppImage can't put `m2` on your shell**: `m2` comes in its own `.tar.gz`. A song made in the AppImage's app keeps the general `m2` line in its `AGENTS.md` (the AppImage's own folder goes away when it quits).
- **The `.deb` is tried on Ubuntu 24.04 only**: it asks for glibc 2.35 (what its programs need, computed by `ci/release.sh check`), so apt takes it on Ubuntu 22.04 and Debian 12 too, whose WebKitGTK 4.1 no one has opened it with yet. The AppImage needs glibc 2.38, for the libraries it carries from ubuntu-24.04.
- **The AppImage's tools are pinned by checksum** (`ci/appimage-tools.sha256`), but its GTK and WebKit libraries are copied from the build machine, so a rebuild on a newer runner image carries newer ones.
- **The AppImage's system libraries** (GTK, WebKitGTK and the rest linuxdeploy copies) carry only the Debian copyright files linuxdeploy brings; `THIRD-PARTY-NOTICES.txt` lists music2's crates, npm packages and font.

## Checked on Linux only

- **Owed on a Mac**: every step by ear in [mac-run.md](mac-run.md) (steps 7 to 133), and one more `ci/check.sh` with the output at 48 kHz. The Mac's two full checks (at `e7518f4` and `a1e3c77`) ran its own code for the plugin program, plugin windows, the engine's real-time threads and G7f, but macOS isn't green yet. The microphone and its permission are checked only by ear. HANDOFF has the table.

## For developers

- **The app still starts its own plugin main loop**, which now serves nothing: it can be folded away.
- **Tests that use the plugin program** need `m2-plugin-host` built first (`cargo build --locked -p m2-plugins --bin m2-plugin-host`); `ci/check.sh` builds it.
- **`m2-gates realtime --offline`'s step 7 loop** gives a late plugin or poly no notes (timing only).
- **std's kernel test `odd_db_values` fails in a debug build** (a NaN's sign bit); `ci/check.sh` runs those tests in release, where it passes.
- **The Confined check's rewire to a node made inside the edit** (WRITE-8, `core/session/src/tx/expect.rs` `made_inside`) has no test in `core/session`: only the editor's bus tests cover it.
- **`--gates=auto` sees only what git sees**: what the gates take from outside the repo (`RUSTFLAGS` and other settings, a `.cargo/config.toml` above the repo or in `CARGO_HOME`, the machine itself) isn't in its record, so a skip trusts that these are as they were at the recorded pass.
- **A check whose gates `--gates=auto` skipped still begins `music2 check: PASS`** (it goes on "without running the gates", and the gates row says SKIP): a script that must know the gates ran at this very commit has to read the gates row.
- **The pull request tier's time on GitHub is still a guess.** It was timed on 8 threads of the server: about 10 minutes cold, 7.5 warm. GitHub's runner has 4 slower cores, so a warm run likely takes 10 to 11 minutes and a cold Rust job 16 to 17 (the goals: 10 and 15). Time the first real runs; if they miss, split `rust` into 4 parts.
- **The push to `main` runs the whole pull request tier again**, on the tree the pull request (or the merge queue) just tested, only to save the build caches. Running just the parts that save caches there (`quick`, `clippy`, `rust 1/3`, `suites 1/2`) would cut a merged pull request from about 18 jobs to 15 (27 to 24 with a merge queue).
- **A hang in the nightly Linux run waits 165 minutes** before "Nightly failed" opens: the full check's `cargo test` has no limit per test, so only `timeout` in `nightly.yml` ends it (its logs are kept). The Mac's nightly job runs nextest, which ends a test after 3 minutes (10 for the five slow tests `.config/nextest.toml` names).
- **CI saves the test plugins' cache only when a job passes** (`actions/cache` saves at the job's end). After a failed run on `main`, later runs download the plugins again, from upstream or the `test-plugins` release. Saving it right after `ci/plugins.sh` (`actions/cache`'s separate restore and save steps) would fix that.
- **The browser suites keep a list of their own** of the page's requests that write nothing (`unwritten` in `e2e/fixlib.mjs`: naming a step, the focus record, a pointer), to tell the page's writes apart. The route table knows which routes write (AGENT-8); `m2 routes --json` could give it to them.
- **A stretch's part file stays in the cache folder when its program is killed mid-stretch** (`renders/<hash>.f32.<pid>.<n>.part`, RENDER-30): a program that ends normally removes it, but nothing sweeps old ones. Since D17 of track samples no test writes there (PLAT-7: before, a plain `cargo test` left them in your own cache). Removing part files untouched for an hour when a program keeps the cache would end it.
- **The browser logs an error with no song open**: `GET /api/graph` answers 400, which is how the window knows to show the start screen. Answering "no project" without an error status would be quieter.

## The old app's parts not back yet

Every row marked **No** was missing from the old app too. Of the rows marked **Partly**, the old app also lacked: copying notes between clips (#15), chord stamps and an arpeggiator (#16), loudness and meters after the fader (#20), and undo that survives closing (#29).

| From the old app | Still missing |
|---|---|
| The Song Library | a picture of each song (#12) |
| Import Audio | other file types, warp by hand (#18) |
| Settings | view settings for every song (#31) |
| The Sounds panel, and m2plug's sampler | the rest of the Sampler (#22) |
| m2plug's Pump | #21 |
| Drawing automation | curves (#14) |
| The key and scale picker | other scales (#6), locking notes to the key (#16) |
| The command API, `music2-cli` | a schema for each route (#38), the missing commands with the app closed (#37) |
| Installing sounds from GitHub | shared sound packages (#39) |
