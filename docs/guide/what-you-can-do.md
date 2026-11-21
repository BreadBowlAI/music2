# What you can do

**In short**
- music2 is a music studio where a song is a folder: code says how it sounds, data files say what plays.
- The table below lists what you can do, and where in the app.
- There's no AI inside the app. Scripts and agents use the same files and a local API.

music2 is a music studio for the Mac and Linux: a DAW (digital audio workstation), the kind of app people make songs in, like GarageBand or Logic Pro. In music2 a song is a folder. Code in it says how the song sounds: the instruments, the effects and the mix. Data files say what plays: the notes, the automation (knob moves over time) and the audio.

The app shows that folder as a timeline, a graph of its sounds (a box for each part, joined by wires), a piano roll (a grid for drawing notes) and a code editor. Every change you make there is written into the files right away.

There's no AI inside the app. Scripts and AI agents (such as a Claude Code session) use the same files, and a local API (a way for programs on your computer to talk to the app).

| You can | Where |
|---|---|
| Start a song from a template, or open an example | the start screen: **New Song…**, **Examples** |
| Add a track: music2's own sounds (synths, FM keys, drums), a Sampler, a pack's sound, or an empty audio track; or a drum kit, a track for each drum and a bus | **+ Track** (⌘T), or the browser on the left |
| Find an instrument, a sound, an effect or a sample by typing a word, and hear it first | ⌘F ([Finding sounds](finding-sounds.md)) |
| Add a sample (a kick, a loop) from music2's small drum kit, a pack, or your own folders | ⌘F: type a word, Space to hear it, Enter to add it ([Samples](samples.md)) |
| Install a pack: instruments, effects and sounds someone else made | `m2 pack install <folder>` in a terminal, then open the song again: its sounds are in the browser and **+ Track** ([Packs](packs.md#installing-a-pack)) |
| Draw and edit notes | **Notes** (⌥2), the piano roll |
| Write a drum beat by clicking boxes, a row for each drum of a kit | **Beat** (⌥5), with a drum kit's track selected ([The beat grid](window.md#the-beat-grid)) |
| Set the song's key, keep notes in it, and place whole chords at once | **Key** in the top bar; then in **Notes**, **Lock to key** and **Chord ▾** ([Key, scale and chords](music-basics.md#key-scale-and-chords)) |
| Play a pattern, a chord or echoes from each note you play: an arpeggiator, Chord, Note echo and Lock to key | **+** on the notes wire into the instrument, in its **Graph** ([how](sounds.md#note-effects)) |
| Edit clips: move, copy, trim, split, join, repeat | the timeline |
| Copy a chorus, or insert and delete bars, on every track at once | drag across the ruler, then ⌘C and ⌘V, or right-click ([Sections](sections.md)) |
| Keep several ideas for one clip's notes, and pick one | `m2 versions` in a terminal; an agent proposes ideas this way ([Versions](versions.md)) |
| Change values, add effects, connect wires | **Graph** (⌥1) |
| Mix: faders, pans, mute, solo, meters, and how loud the song is (LUFS) | **Mix** (⌥3), and each track's header |
| Share a reverb or a delay, or group tracks under one fader | **Mix** (⌥3) ▸ **Add bus** |
| Make one track dip while another plays (a sidechain: the bass under the kick) | **+** on the sound's wire in the **Graph**, then **Duck**; pick the other track in its node's **Listens to** ([Sidechain](sounds.md#sidechain)) |
| Use the mixing tools: shape the tone, silence a track between its hits, make hits punchier, tame a voice's S sounds, compress lows, mids and highs each on its own, or dip on every beat | **+** on the sound's wire in the **Graph**, then **Parametric EQ**, **Gate**, **Transient shaper**, **De-esser**, **Multiband compressor** or **Pump** ([what each knob does](sounds.md)) |
| Make a drum bus or the whole mix louder without going over | **+** on the sound's wire in the **Graph**, then **Clipper** (Dynamics) ([how](sounds.md)) |
| Save a sound's knobs as a preset, or load one | **Presets** on an instrument's, an effect's or a plugin's node in the **Graph** (expand it with ⤢). Yours are in `~/Music/music2/Presets`; a plugin's own are listed too |
| Draw automation (knob moves over time) | right-click a knob ▸ **Automate**, then draw on the timeline |
| Play live: the computer's keys, or a MIDI keyboard with its wheels | ⌘K; or plug a keyboard in |
| Hear a click on each beat | K |
| Record notes, or your voice or a guitar | R |
| Record several tracks at once, each from its own input (3–4 too) | **R** on each track's header; right-click it for its input |
| Fix one part by punching in, or keep a take each time round a loop | right-click Record ▸ **Punch in and out**; the clip's menu ▸ **Take 1**, **Take 2**, … |
| Build one take from the best parts of several (comping) | **Takes** on the track's header, then swipe along a take ([how](audio.md#comp-the-best-parts-of-several-takes)) |
| Tune a voice onto the song's key, gently or as the "robot" pop sound | **+** on the wire out of the vocal track's lane in its **Graph**, then **Tune** ([how](audio.md#tune-a-voice)) |
| Fix one sung note without singing it again | right-click a vocal clip ▸ **Find notes** ([how](audio.md#edit-a-voices-notes)) |
| Make a voice deeper or higher, or a chipmunk | **+** on the wire out of the vocal track's lane in its **Graph**, then **Voice changer** ([how](audio.md#change-a-voice)) |
| Make a synth talk with a voice (a vocoder) | **+** on the wire out of the synth in its **Graph**, then **Vocoder**, then pick the vocal track in its node's **Talks with** ([how](audio.md#make-a-synth-talk)) |
| Add the jet's whoosh (a flanger), or break a sound into tiny pieces to smear or stretch it (granular) | **+** on the wire out of the sound in its **Graph**, then **Flanger** (Modulation) or **Grains** (Delay & Reverb) ([what each knob does](sounds.md)) |
| Bring in WAV or FLAC files, long ones too | File ▸ Import Audio… (⇧⌘I), or drag them onto the window |
| Keep an audio clip in time when the tempo changes | right-click the clip ▸ **Follow tempo** |
| Turn a track into audio, to chop it, reverse it or pitch it; the song sounds the same | the track's menu ▸ **Bounce to audio** ([how](audio.md#bounce-a-track-to-audio)) |
| Bring in a MIDI file as new tracks | File ▸ Import MIDI…, or drop the `.mid` file |
| Play a WAV file as an instrument | + Track ▸ Sampler, then drop a WAV (or a FLAC) on its card |
| Add a CLAP plugin, and open its own window | drag it from the browser's **Plugins** onto a track, or + Track ▸ **Plugins**; **Open window** on its box |
| Export the song, each track (stems), or the notes as MIDI | **Export** (⇧⌘E) |
| Edit the song's code, with type checks | **Code** (⌥4) |
| See what's wrong, in plain words, with fixes | the status pill in the top bar |
| Open a song made with an older music2 and move it to this one | the status pill ▸ **Upgrade it** ([how](songs.md#opening-a-song-made-with-an-older-music2)) |
| Learn step by step | Help ▸ Make Your First Song |

Every change is written into the song's files at once, and Undo (⌘Z) takes it back, also after you close the song. Code you typed and didn't save is offered back after a crash ([Saving and undo](saving.md)).
