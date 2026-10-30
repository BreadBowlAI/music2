# Words you'll see

**In short**
- The app's own words, each in one line: track, key, lane, clip, knob, cable, render, bounce, stems and more.
- New to music itself? [Music basics](music-basics.md) explains notes, beats, scales, synths, effects and mixing.

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
  - **Region:** one recording in a Sampler's map, with the notes, how hard and in which turn it plays ([Sounds](sounds.md)).
  - **Velocity layer:** the recordings of one note played at one strength; a soft note plays the soft layer.
  - **Round robin:** several takes of one note, played in turn, so a repeated note doesn't sound the same each time.
- **Plugin:** an instrument or effect from another maker, installed on your computer (music2 plays CLAP plugins).
