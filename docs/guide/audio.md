# Audio, recording and MIDI files

**In short**
- Bring in WAV, FLAC and MIDI files, edit audio clips, and keep audio in time when the tempo changes.
- Record notes from a MIDI keyboard or the computer's keys, or your voice or a guitar.
- Arm tracks to record several at once, punch in to fix one part, and keep a take for each time round a loop.

- **Bring in audio:** File → Import Audio… (⇧⌘I), or drag WAV or FLAC files onto the window: onto an audio track (at the bar under the pointer), onto another track (a new audio track right after it), or below the tracks (a new audio track for each file, named after it). An outline and a few words say where they go. + Track → Audio track adds an empty one. Each file is copied into the song's `assets/` folder (the same file twice is kept once). A FLAC (the same sound as a WAV, about half the size) comes in as a WAV of the very same sound, `kick.flac` as `kick.wav`, with a note beside it (`kick.wav.json`) saying which FLAC it was made from: a song's clips play WAV files. Files up to 3 hours long play from the disk, never whole in memory.
- **Edit an audio clip:** drag its edges to trim it, and the small squares at its top to fade it in or out. Its menu sets its volume, plays it backwards, or removes the fades. Each is one undo step.
- **Follow tempo:** right-click an audio clip and turn on **Follow tempo**. Change the song's tempo later, and the clip stretches to keep its beats; its pitch stays the same. The stretch is made in the background while the song plays on. Turn it off and the clip plays at its own speed again.
- **Bring in a MIDI file:** File → Import MIDI…, or drop a `.mid` file. Each part becomes a new track with a fitting sound, its notes in one clip; the drums (channel 10) come in as a [drum kit](sounds.md#drum-kits), a track for each drum. If the file's tempo differs from the song's, it asks: **Keep** the song's (Return) or **Use** the file's. One Undo takes the whole import back.

## Recording

- **Record notes:** select a track that plays notes and press R (or the red **Record** button after Play, or Transport → Record). Stopped, it counts in one bar of clicks (the button says 4, 3, 2, 1), then plays; playing, it starts at once. Play a MIDI keyboard or the keyboard piano: the notes show as you play. R again, or Space, ends it: one clip over whole bars, one undo step. Right-click the button for **Count in first** and **Snap notes to the grid**. With the loop on, recording starts at the loop's start: the song plays up to it first, and a note struck just before it lands on its first beat. On the keyboard piano, − and = make the notes softer and louder (10% a step; the piano shows how loud).
- **Record your voice or a guitar:** select an audio track and press R. It counts in, then records from the sound input into a new file in `assets/` ("Vox take.wav"). The take grows as a red clip, with a bar showing how loud the input is. R again ends it: one clip, one undo step; the file stays if you undo.
- **Settings → Sound input** picks the input. **Hear yourself while recording** plays the input through the track's effects and fader while it records, and on an armed track (below) before you record too. It is off at first: use headphones, as speakers feed back into the microphone. An export never has it.

### Arm, punch, takes and overdub

| To do this | Do this | What happens |
|---|---|---|
| Choose the tracks that record | Click **R** on each track's header (it turns red) | R records into every armed track at once. With none armed, it records the selected track, as before. Arming is not an edit: there is nothing to undo. A track with two notes lanes records the one its open clip is on. |
| Hear yourself before you record | Arm an audio track, with **Hear yourself** on in Settings | You hear the input through the track, with no take. |
| Play a notes track with the keyboards | Arm it | The MIDI keyboards and the keyboard piano play it, whatever you select. One notes track is armed at a time: arming another moves it. |
| Record input 3–4, or one input alone | Right-click an audio track's **R** | It lists the input's channels: a pair (1–2, 3–4, …) records in stereo, one alone records in mono. |
| Fix one part of a take (punch in and out) | Set a loop over the part, turn on **Punch in and out** (right-click Record), and press R before it | The song plays up to the loop's start, records to its end, and plays on. What was there is replaced: a clip that crosses an edge is cut there, as Split cuts it. The loop's bar on the ruler is red while Punch is on. |
| Keep a take for each time round | Turn the loop on and record over it | Notes: each time round is its own clip, and the next time round plays it, so a part builds in layers. Audio: each time round is a take; the newest whole one plays. |
| Choose another audio take | Right-click the clip: **Take 1**, **Take 2**, … | The take ticked plays. One undo step. |
| Add notes into a clip | Turn on **Overdub** (right-click Record) | The notes go into the clip under them, which grows to hold them. |

- Each time round of notes, and each take's end, is one undo step. Choosing a take is one too.
- **Pass** means one time round the loop. A **take** is one recording of a part.
- **When the input has a problem**, a message says so, and the status pill lists it: an input chosen in Settings that isn't plugged in records from another and says which; one unplugged during a take keeps the take up to there; a Mac that doesn't allow the microphone drops the take (it is only silence) and says where to allow it. With the disk full, or a take over 3.1 hours, the take is kept up to there.
- The song plays on through all of these.
