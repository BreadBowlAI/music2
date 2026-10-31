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
| Add a track: 22 std sounds, a Sampler, or an empty audio track | **+ Track** (⌘T), or the browser on the left |
| Draw and edit notes | **Notes** (⌥2), the piano roll |
| Edit clips: move, copy, trim, split, join, repeat | the timeline |
| Copy a chorus, or insert and delete bars, on every track at once | drag across the ruler, then ⌘C and ⌘V, or right-click ([Sections](sections.md)) |
| Turn knobs, add effects, connect cables | **Device** (⌥1) |
| Mix: faders, pans, mute, solo, and meters | **Mix** (⌥3), and each track's header |
| Share a reverb or a delay, or group tracks under one fader | **Mix** (⌥3) ▸ **Add bus** |
| Save a sound's knobs as a preset, or load one | **Presets** on an instrument's, an effect's or a plugin's card in **Device**. Yours are in `~/Music/music2/Presets`; a plugin's own are listed too |
| Draw automation (knob moves over time) | right-click a knob ▸ **Automate**, then draw on the timeline |
| Play live: the computer's keys, or a MIDI keyboard with its wheels | ⌘K; or plug a keyboard in |
| Hear a click on each beat | K |
| Record notes, or your voice or a guitar | R |
| Bring in WAV or FLAC files, long ones too | File ▸ Import Audio… (⇧⌘I), or drag them onto the window |
| Keep an audio clip in time when the tempo changes | right-click the clip ▸ **Follow tempo** |
| Bring in a MIDI file as new tracks | File ▸ Import MIDI…, or drop the `.mid` file |
| Play a WAV file as an instrument | + Track ▸ Sampler, then drop a WAV (or a FLAC) on its card |
| Add a CLAP plugin, and open its own window | drag it from the browser's **Plugins** onto a track, or + Track ▸ **Plugins**; **Open window** on its box |
| Export the song, each track (stems), or the notes as MIDI | **Export** (⇧⌘E) |
| Edit the song's code, with type checks | **Code** (⌥4) |
| See what's wrong, in plain words, with fixes | the status pill in the top bar |
| Learn step by step | Help ▸ Make Your First Song |

Every change is written into the song's files at once, and Undo (⌘Z) takes it back.
