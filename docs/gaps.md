# What music2 can't do yet

**In short**
- This page lists only what is missing. What the app does today is in the README: [In short: what you can do](../README.md#in-short-what-you-can-do).
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
| 18 | **The rest of audio clips**: place a clip's warp markers by hand (Follow tempo is the only way), find a clip's own tempo from its file, set a clip's pitch in the app (JSON only), join audio clips, clear out audio files no clip uses, and import MP3, AIFF or FLAC. | Every file stays in `assets/` until you delete it, and only WAV comes in. | Yes | medium |

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
| 32 | **Add an installed plugin from the app**: list installed CLAP plugins in the effect browser and in + Track, and put one on a track. | Today a plugin goes on a track only by writing `plugin("clap:…")` in its code, after finding its id with `m2 plugins`. | No | medium |
| 33 | **VST3 and AU** plugins. | Many popular synths and mastering plugins come only as AU or VST3 on the Mac. | No | large |

- **All plugins share one plugin program**, so one crash silences them all for a moment (each comes back by itself). One program per plugin would silence only the one that crashed.
- **A plugin stuck in its window, its start or a settings load** holds up edits and saves for up to 30 seconds (2 minutes in a scan); then music2 ends the plugin program and says so (the ph review's r1).
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
| 22 | **The rest of the Sampler**: loop points, start and end, reverse, velocity layers, round robin and choke groups; zones made by dropping several WAVs (code only today); a waveform and a root picker on its card; files other than WAV. | Sampled drums and vocal chops are pop staples. | Yes | medium |
| 26 | **Presets**: save and load an instrument's or an effect's settings by name, a preset browser, and presets for CLAP plugins. | Beginners start from presets, not from blank knobs. | No | medium |

- **A Sampler played far above its root sounds harsh**: it isn't band-limited, so high notes alias, and past 6 octaves up it plays no faster.
- **A Sampler's files are held in memory**, decoded whole (clips stream from disk). The kernel ABI allows streaming them ([kernel-abi.md](kernel-abi.md#the-asset-import)).
- **A WAV dropped on a Sampler that then refuses it** (one with `zones` in its code) is still copied into `assets/`, though the page says "Nothing was changed" (the ab review's 3).
- **An audio file a kernel reads that changes on disk** is heard at the next build, not at once. Its name in the code has no hash pin, and when it is missing no near name is offered.
- **Glide in `"poly"` mode** isn't built: `glide` does nothing there.
- **`steal: "none"` waits for a voice to fall silent**, not for its key to be let go.

## Mixing

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 20 | **Meters after the fader, and loudness for export.** Today's meters read each track before its fader, so a fader move doesn't move them: after-fader taps need the engine (`core/runtime`). | You can't see what a fader does to a track's level. | Partly (a meter on each track) | medium |
| 27 | **Sends and buses in the app**: buses, sends and groups work in song code (`bus()` and std's mixer, [RENDER-34](architecture.md#buses)), but the app can't add a bus, a send or an output yet, and the Mix view shows no bus strips or send knobs. | Standard mixing, such as one reverb shared by every track, without writing code. | Yes | large |

- **Soloing a bus plays its tracks' dry sound too**: a wet-only solo needs a cell per path.
- **A send has no pan of its own**, and can't be made mono.
- **No solo-safe buses**, and no "solo in place" choice.
- **A send starts at the track's output**: no send from inside a track's chain, before its effects.
- **A group's tracks don't fold under it** in the timeline.
- **The graph view shows a bus's feed** as a song export named `bus:verb`.
- **A bus channel must read the bus as it is** (`verb: verb.out`): one written `verb: eq(verb.out)` isn't seen as the bus's channel, so the bus is never fed, which the build says.

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
| 1 | **The rest of changing an instrument**: a track whose instrument is built from parts, has two instruments or a plugin, or whose settings code works out, is refused in words. Adding a track to a song that mixes its own way, or picking a CLAP instrument (#32), still needs code. | Changing your mind about a sound shouldn't mean a new track. | Yes | small |
| 10 | **The rest of changing a track's ID**: a track with a new ID restarts its sounds (its plugins' and instruments' state isn't carried over, design-8b §11.3), and the status popover has no Restore for a rename done by hand in code (DIAG-6). | A rename shouldn't cut a sound that is playing. | Yes | small |

- **A knob let go while the code doesn't compile** keeps playing its new value, and is written once the code compiles; after 30 seconds it is dropped, with a message. Writing it at once, when the broken file is another one, is the owner's call (the step 6 design's C21).
- **Two numbers edited on neighbouring lines by another program** count as a change to the calls between them, so the song is rebuilt (crossfaded) rather than the numbers written into what plays.

## Problems and help

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 11 | **The rest of plain-language help**: a missing plugin reads technically (``no CLAP plugin `org.surge-synth-team.surge-xt` is installed (looked in [...])``; Open an Example names it plainly), and the docs past [Music basics](music-basics.md) and the README's first sections are technical. | Many people who open music2 are new to making music. | Yes | small |

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
- **Kernels from other people have no time limit**: one that loops forever stalls the sound (wasmtime's epochs would stop it).
- **A song near the 3,999 sound parts limit** has no cheaper way to run yet (a `sum` kernel, a checked engine).
- **An output chosen in Settings that comes back** after it was unplugged isn't taken again by itself; choose another output, then it.

## Settings and projects

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 30 | **Crash safety**: keep unsaved code-editor text through a crash, and simple project snapshots or backups. | Edits are on disk at once and a cut-off edit is offered back, but unsaved code is lost. | No | large |
| 29 | **Undo that survives closing the app.** | Closing the app, or the song, loses all undo. | Partly (lost on closing too) | medium |
| 31 | **The rest of Settings**: view settings that hold for every song (zoom, grids and the audition toggle are kept per song). The sample rate stays the song's ([RENDER-31](architecture.md#sample-rate)). | Most people want one zoom for every song. | Partly (it showed the device and the folders) | small |
| 12 | **A picture of each song in the recent list** (taste.md SS-2). | A small picture of each arrangement helps find the right song. | Yes (the Song Library) | small |
| 28 | **Open a song by dropping its folder** on the window or the Dock. | Finding a song in the Finder is how many people open it. | Yes | small |

- **The dock and the browser panel start over at each launch** (which view the dock shows, and its height): the browser's storage starts empty, as the page gets a new address each time.
- **Renaming a song's folder starts Undo over**: the song closes and opens again from there (Rename Song… says so).
- **A song asking for std "0.2"** gets `pkg.unsatisfied` in this app (std 0.3.0); its message names the line to write (`std = "0.3"`). It may sound a little different: one-note sounds now keep each note's own velocity.
- **A song's std line names no patch version**, so a song that needs a later patch (`examples/sampler` asks for "0.2.1") gets a missing import in an app with an earlier one, not `pkg.unsatisfied`.

## Code editor

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 34 | **Create, rename and delete files** in the code editor (today it opens existing `.ts` files only), and edit the data JSON and `music2.toml` there. | Giving a track a file of its own still needs another editor. | No | small |

- **The code editor reloads a file an edit changed** rather than following the edit in place; unsaved text in it is kept, with the "changed on disk" choice.

## Packages

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 39 | **Shared sound packages**: list the packages a song uses in `music2.toml`, fetch them (`github:owner/repo@tag`), pin them in `music2.lock`, and browse and add them from the app. | Only std can be loaded, so sounds other people make can't be used, except as CLAP plugins. | Partly (single sounds from GitHub) | large |

- **Kernel ABI v1's reserved parts aren't built**: event ports, `k_save`/`k_load` and `k_latency`/`k_tail` ([kernel-abi.md](kernel-abi.md#reserved-shapes)). Each comes with the first kernel that needs it.

## Scripts, agents and the command line

| # | What's missing | Why it matters | Old app? | Size |
|---|---|---|---|---|
| 37 | **`m2` commands** for change instrument, markers, the song's key, a track's ID, Save As and Follow tempo. Agents can send these to the app's routes, or as a Tx (Follow tempo: `POST /api/graph/clips` with `{"op": "audio", "follow": true}`). | Scripts and agents need the same basics as the app. | Yes (in `music2-cli`) | small |
| 38 | **A self-describing API**: every `/api/graph` operation listed with a JSON Schema, a short summary of the song, and MCP. | An agent has to read the README or the source to learn the routes. | Yes | medium |

## Checked on Linux only

- **Owed on a Mac**: every step by ear in [mac-run.md](mac-run.md), and the Mac-only code that is only lint-checked here: the plugin program's shared memory and wake, plugin windows, real-time threads in the audio workgroup, the microphone and its permission, and G7f's numbers for one sandbox a box. HANDOFF has the table.

## For developers

- **The app still starts its own plugin main loop**, which now serves nothing: it can be folded away.
- **Tests that use the plugin program** need `m2-plugin-host` built first (`cargo build -p m2-plugins --bin m2-plugin-host`); `ci/check.sh` builds it.
- **`m2-gates realtime --offline`'s step 7 loop** gives a late plugin or poly no notes (timing only).
- **std's kernel test `odd_db_values` fails in a debug build** (a NaN's sign bit); `ci/check.sh` runs those tests in release, where it passes.
- **The browser logs an error with no song open**: `GET /api/graph` answers 400, which is how the window knows to show the start screen. Answering "no project" without an error status would be quieter.

## The old app's parts not back yet

Every row marked **No** was missing from the old app too. Of the rows marked **Partly**, the old app also lacked: copying notes between clips (#15), chord stamps and an arpeggiator (#16), loudness and meters after the fader (#20), and undo that survives closing (#29).

| From the old app | Still missing |
|---|---|
| The Song Library | a picture of each song (#12) |
| Import Audio | other file types, warp by hand (#18) |
| Settings | view settings for every song (#31) |
| The Track inspector | output and sends (#27) |
| The Sounds panel, and m2plug's sampler | the rest of the Sampler (#22) |
| m2plug's Pump | #21 |
| Drawing automation | curves (#14) |
| The key and scale picker | other scales (#6), locking notes to the key (#16) |
| The command API, `music2-cli` | a self-describing API (#38), the missing commands (#37) |
| Installing sounds from GitHub | shared sound packages (#39) |
