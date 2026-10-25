# Audio, recording and MIDI files

**In short**
- Bring in WAV files and MIDI files, edit audio clips, and keep audio in time when the tempo changes.
- Record notes from a MIDI keyboard or the computer's keys, or your voice or a guitar.

- **Bring in audio:** File → Import Audio… (⇧⌘I), or drag WAV files onto the window: onto an audio track (at the bar under the pointer), onto another track (a new audio track right after it), or below the tracks (a new audio track for each file, named after it). An outline and a few words say where they go. + Track → Audio track adds an empty one. Each file is copied into the song's `assets/` folder (the same file twice is kept once). Files up to 3 hours long play from the disk, never whole in memory.
- **Edit an audio clip:** drag its edges to trim it, and the small squares at its top to fade it in or out. Its menu sets its volume, plays it backwards, or removes the fades. Each is one undo step.
- **Follow tempo:** right-click an audio clip and turn on **Follow tempo**. Change the song's tempo later, and the clip stretches to keep its beats; its pitch stays the same. The stretch is made in the background while the song plays on. Turn it off and the clip plays at its own speed again.
- **Bring in a MIDI file:** File → Import MIDI…, or drop a `.mid` file. Each part becomes a new track with a fitting sound (channel 10 plays the Drum kit), its notes in one clip. If the file's tempo differs from the song's, it asks: **Keep** the song's (Return) or **Use** the file's. One Undo takes the whole import back.

## Recording

- **Record notes:** select a track that plays notes and press R (or the red **Record** button after Play, or Transport → Record). Stopped, it counts in one bar of clicks (the button says 4, 3, 2, 1), then plays; playing, it starts at once. Play a MIDI keyboard or the keyboard piano: the notes show as you play. R again, or Space, ends it: one clip over whole bars, one undo step. Right-click the button for **Count in first** and **Snap notes to the grid**. While looping, the first time round is kept.
- **Record your voice or a guitar:** select an audio track and press R. It counts in, then records from the sound input into a new file in `assets/` ("Vox take.wav"). The take grows as a red clip, with a bar showing how loud the input is. R again ends it: one clip, one undo step; the file stays if you undo.
- **Settings → Sound input** picks the input. **Hear yourself while recording** plays the input through the track's effects and fader while it records. It is off at first: use headphones, as speakers feed back into the microphone. An export never has it.
- **When the input has a problem**, a message says so, and the status pill lists it: an input chosen in Settings that isn't plugged in records from another and says which; one unplugged during a take keeps the take up to there; a Mac that doesn't allow the microphone drops the take (it is only silence) and says where to allow it. With the disk full, or a take over 3.1 hours, the take is kept up to there.
- The song plays on through all of these.
