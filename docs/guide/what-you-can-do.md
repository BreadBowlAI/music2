# What you can do

**In short**
- music2 is a music studio where a song is a folder: code says how it sounds, data files say what plays.
- The table below lists what you can do, and where in the app.
- There's no AI inside the app. Scripts and agents use the same files and a local API.

music2 is a music studio for the Mac and Linux: a DAW (digital audio workstation), the kind of app people make songs in, like GarageBand or Logic Pro. In music2 a song is a folder. Code in it says how the song sounds: the instruments, the effects and the mix. Data files say what plays: the notes, the automation (knob moves over time) and the audio.

The app shows that folder as a timeline, a graph of knobs and cables, a piano roll (a grid for drawing notes) and a code editor. Every change you make there is written into the files right away.

There's no AI inside the app. Scripts and AI agents (such as a Claude Code session) use the same files, and a local API (a way for programs on your computer to talk to the app).

| You can | Where |
|---|---|
| Start a song from a template, or open an example | the start screen: **New Song…**, **Examples** |
| Add a track: music2's own sounds (synths, FM keys, drums), a Sampler, a pack's sound, or an empty audio track; or a drum kit, a track for each drum and a bus | **+ Track** (⌘T), or the browser on the left |
| Draw and edit notes | **Notes** (⌥2), the piano roll |
| Edit clips: move, copy, trim, split, join, repeat | the timeline |
| Copy a chorus, or insert and delete bars, on every track at once | drag across the ruler, then ⌘C and ⌘V, or right-click ([Sections](sections.md)) |
| Turn knobs, add effects, connect cables | **Device** (⌥1) |
| Mix: faders, pans, mute, solo, meters, and how loud the song is (LUFS) | **Mix** (⌥3), and each track's header |
| Share a reverb or a delay, or group tracks under one fader | **Mix** (⌥3) ▸ **Add bus** |
| Save a sound's knobs as a preset, or load one | **Presets** on an instrument's, an effect's or a plugin's card in **Device**. Yours are in `~/Music/music2/Presets`; a plugin's own are listed too |
| Draw automation (knob moves over time) | right-click a knob ▸ **Automate**, then draw on the timeline |
| Play live: the computer's keys, or a MIDI keyboard with its wheels | ⌘K; or plug a keyboard in |
| Hear a click on each beat | K |
| Record notes, or your voice or a guitar | R |
| Record several tracks at once, each from its own input (3–4 too) | **R** on each track's header; right-click it for its input |
| Fix one part by punching in, or keep a take each time round a loop | right-click Record ▸ **Punch in and out**; the clip's menu ▸ **Take 1**, **Take 2**, … |
| Build one take from the best parts of several (comping) | **Takes** on the track's header, then swipe along a take ([how](audio.md#comp-the-best-parts-of-several-takes)) |
| Tune a voice onto the song's key, gently or as the "robot" pop sound | **+** on the vocal track's lane in its **Device**, then **Tune** ([how](audio.md#tune-a-voice)) |
| Make a voice deeper or higher, or a chipmunk | **+** on the vocal track's lane in its **Device**, then **Voice changer** ([how](audio.md#change-a-voice)) |
| Make a synth talk with a voice (a vocoder) | **+** after the synth in its **Device**, then **Vocoder**, then pick the vocal track in its card's **Talks with** ([how](audio.md#make-a-synth-talk)) |
| Add the jet's whoosh (a flanger), or break a sound into tiny pieces to smear or stretch it (granular) | **+** after the sound in its **Device**, then **Flanger** (Modulation) or **Grains** (Delay & Reverb) ([what each knob does](sounds.md)) |
| Bring in WAV or FLAC files, long ones too | File ▸ Import Audio… (⇧⌘I), or drag them onto the window |
| Keep an audio clip in time when the tempo changes | right-click the clip ▸ **Follow tempo** |
| Turn a track into audio, to chop it, reverse it or pitch it; the song sounds the same | the track's menu ▸ **Bounce to audio** ([how](audio.md#bounce-a-track-to-audio)) |
| Bring in a MIDI file as new tracks | File ▸ Import MIDI…, or drop the `.mid` file |
| Play a WAV file as an instrument | + Track ▸ Sampler, then drop a WAV (or a FLAC) on its card |
| Add a CLAP plugin, and open its own window | drag it from the browser's **Plugins** onto a track, or + Track ▸ **Plugins**; **Open window** on its box |
| Export the song, each track (stems), or the notes as MIDI | **Export** (⇧⌘E) |
| Edit the song's code, with type checks | **Code** (⌥4) |
| See what's wrong, in plain words, with fixes | the status pill in the top bar |
| Learn step by step | Help ▸ Make Your First Song |

Every change is written into the song's files at once, and Undo (⌘Z) takes it back, also after you close the song. Code you typed and didn't save is offered back after a crash ([Saving and undo](saving.md)).
