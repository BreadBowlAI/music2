# Audio, recording and MIDI files

**In short**
- Bring in WAV, FLAC and MIDI files, edit audio clips, and keep audio in time when the tempo changes.
- Record notes from a MIDI keyboard or the computer's keys, or your voice or a guitar.
- Arm tracks to record several at once, punch in to fix one part, and keep a take for each time round a loop.
- Comp: build one take from the best parts of several, by swiping across the take you like.
- Tune a voice: move each sung note onto the song's key, gently or as the hard "robot" pop sound.
- Change a voice: make it deeper, or chipmunk-like, by moving its notes and its colour, each on its own.
- Make a synth talk: the synth plays the notes, and a voice gives them its words (a vocoder).
- Bounce a track to audio, to chop it, play it backwards or change its pitch: the song sounds the same.

- **Bring in audio:** File → Import Audio… (⇧⌘I), or drag WAV or FLAC files onto the window: onto an audio track (at the bar under the pointer), onto another track (a new audio track right after it), or below the tracks (a new audio track for each file, named after it). An outline and a few words say where they go. + Track → Audio track adds an empty one. Each file is copied into the song's `assets/` folder (the same file twice is kept once). A FLAC (the same sound as a WAV, about half the size) comes in as a WAV of the very same sound, `kick.flac` as `kick.wav`, with a note beside it (`kick.wav.json`) saying which FLAC it was made from: a song's clips play WAV files. Files up to 3 hours long play from the disk, never whole in memory.
- **Edit an audio clip:** drag its edges to trim it, and the small squares at its top to fade it in or out. Its menu sets its volume, plays it backwards, or removes the fades. Each is one undo step.
- **Follow tempo:** right-click an audio clip and turn on **Follow tempo**. Change the song's tempo later, and the clip stretches to keep its beats; its pitch stays the same. The stretch is made in the background while the song plays on. Turn it off and the clip plays at its own speed again.
- **Bring in a MIDI file:** File → Import MIDI…, or drop a `.mid` file. Each part becomes a new track with a fitting sound, its notes in one clip; the drums (channel 10) come in as a [drum kit](sounds.md#drum-kits), a track for each drum. If the file's tempo differs from the song's, it asks: **Keep** the song's (Return) or **Use** the file's. One Undo takes the whole import back.

## Bounce a track to audio

Bouncing turns a track into a recording of itself ("resampling"). Then you can chop it, play it backwards or change its pitch like any audio clip.

- **How:** open a track's menu (the ⋯ on its header, or right-click its strip in Mix) and pick **Bounce to audio**. A bus works too: bounce the drums bus to get the whole kit as one file.
- **What it does,** as one undo step:

| What | Where |
|---|---|
| The track is rendered to a new file | `assets/keys-bounce.wav`, with a note beside it saying how it was made |
| A new audio track plays the file | right under it, called "Keys audio", one clip from the start to its last sound |
| The new track gets the same mixer settings | its volume, pan, effects and where it goes are copied from the track |
| The track is muted in the song | it stays, with its notes, so you can go back |

- **The song sounds the same** afterwards, sample for sample, and is as long. The bounce goes on until the track falls silent, as an export does, so a long reverb tail is kept (up to 10 s past the song's end). A track that never falls silent fades out there, as an export does.
- **While it renders**, a box says how far it is. **Stop** in it adds nothing and leaves no file. If you change the song while it renders, it stops at once, adds nothing and says so: bounce again.
- **Undo** (⌘Z) takes it all back: the new track goes, and the track plays again. The file stays in `assets/`.
- **Agents** do the same with `m2 bounce <song> keys-bounce --track keys --in-place`.
- **It doesn't save the computer's power yet:** the muted track still runs. Delete it once you're happy with the bounce.
- **Not yet:** only the bars you selected, and a track that plays a plugin. Nor a track that is muted in the song, shares its mixer channel, or that another track plays too (muting it would silence that). Each says why.

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
| Record a drum kit from a pad controller | Arm any one of the kit's drum tracks, then R | Each pad plays the kit's drum that answers to it (a kick pad the Kick, a snare pad the Snare), and the take puts each hit on that drum's own track: one take, one undo step, "recording on Drums". |
| Record input 3–4, or one input alone | Right-click an audio track's **R** | It lists the input's channels: a pair (1–2, 3–4, …) records in stereo, one alone records in mono. |
| Fix one part of a take (punch in and out) | Set a loop over the part, turn on **Punch in and out** (right-click Record), and press R before it | The song plays up to the loop's start, records to its end, and plays on. What was there is replaced: a clip that crosses an edge is cut there, as Split cuts it. The loop's bar on the ruler is red while Punch is on. |
| Keep a take for each time round | Turn the loop on and record over it | Notes: each time round is its own clip, and the next time round plays it, so a part builds in layers. Audio: each time round is a take; the newest whole one plays. |
| Choose another audio take | Right-click the clip: **Take 1**, **Take 2**, … | The take ticked plays. One undo step. |
| Add notes into a clip | Turn on **Overdub** (right-click Record) | The notes go into the clip under them, which grows to hold them. |

- Each time round of notes, and each take's end, is one undo step. Choosing a take is one too.
- **Pass** means one time round the loop. A **take** is one recording of a part.
- **When the input has a problem**, a message says so, and the status pill lists it: an input chosen in Settings that isn't plugged in records from another and says which; one unplugged during a take keeps the take up to there; a Mac that doesn't allow the microphone drops the take (it is only silence) and says where to allow it. With the disk full, or a take over 3.1 hours, the take is kept up to there.
- The song plays on through all of these.

### Comp: the best parts of several takes

Singers often record a line a few times and keep the best bits of each. **Comping** builds one take from those bits. Record round a loop first, so each time round is a take (above).

| To do this | Do this | What happens |
|---|---|---|
| See the takes | Click **Takes** on the track's header | One row opens under the lane for each take: Take 1, Take 2, … Each row shows that take's waveform. The parts that play are lit; the rest is faint. Opening the rows is not an edit. |
| Use one take for a few bars | Swipe along that take's row, across the bars (press, drag, let go) | That take plays there; the rest stays as it was. It snaps to the grid (hold ⌘ for no snap). One undo step: "Comp on Vox". |
| Use a take for one part | Click a faint part of its row | That take plays for that part. |
| Go back to one take | Swipe that take across the whole loop | It is one clip again, as it was. |
| Close the rows | Click **Takes** again | The lane shows the comp as clips. |

- **A comp is only clips.** Each part is a normal audio clip that plays its take from the one file. You can move, split or delete them as any clip. A swipe changes only the bars you swiped: a split or a fade you made elsewhere stays.
- **Each join crossfades** over 10 ms (a hundredth of a second), so you hear no click.
- **Agents comp too:** `POST /api/graph/take` with `from` and `to` (beats) plays the take there only ([routes](routes.md)).
- A comp works on the clips of one recording that sit in a row, in time with the loop they were recorded on. A copy of the clip, even straight after it, is a comp of its own. A swipe that runs into the next comp changes only the one it started in.
- **When a take stopped early** (you pressed stop partway round), it has no sound for the rest of the loop. A swipe there says where the take stops, and changes nothing.
- **A cut inside a fade** is refused: make the fade shorter first, or swipe outside it.

## Tune a voice

**Tune** moves each note a singer sings onto the nearest note of the song's [key](music-basics.md) (pitch correction, as Auto-Tune does). It comes with music2, in the pack `@m2/vocals`.

| To do this | Do this | What happens |
|---|---|---|
| Add Tune to a vocal | Select the vocal track (Vox), and in its Graph click the **+** on the wire out of its lane, then **Tune** (under Voice) | `tune(lane.audio("clips"), { glide: 0.1, amount: 1 })` is written into the track's own file, so everything that hears the track hears it tuned. It starts on **Natural**. |
| Set the notes it moves to | Click **Key** in the top bar, or **Set the key** on Tune's card | Tune's card says the key, such as "Key: A minor". |
| The hard, stepped pop sound | Load the **Robot** preset (Presets on its card), or turn **Glide** to 0 | Each note jumps to its pitch at once. |
| A gentle fix | Turn **Glide** up to about 0.1 s (Natural) or more | Each note slides to its pitch, and the singer's wobble (vibrato) stays. |
| Fix only part of each note's error | Turn **Amount** down | At 0.5, half of each note's error is fixed; at 0, none. |
| Change the voice's colour | Turn **Formant** down or up | Down is deeper and darker, a bigger singer; up is smaller and brighter. The notes stay where Tune puts them. At 0, the voice's own colour. |
| See what it does | Play the song and look at its card | Its **pitch line** shows the last 3 seconds: how far your voice is from its note, faint, and how far Tune leaves it, solid. The middle line is the note; up is sharp, down is flat, a semitone each way. Its words say how fast and how far it moves the voice. |

- **No key set:** every note is allowed, so Tune moves each sung note to the nearest of all twelve. Its card says "Every note allowed" with **Set the key**.
- **Glide** is how long a note takes to reach its pitch: 0 to 0.4 seconds. **Amount** is how much of the error it fixes: 0 to 1. **Formant** moves the colour (see [Change a voice](#change-a-voice)): −12 to 12 semitones.
- **The voice keeps its colour.** Tune moves the pitch without making the singer sound like a chipmunk or a giant, until you turn Formant.
- **A breath or an "s"** has no pitch, so Tune lets it through as it is. On the pitch line, both lines rest on the middle line.
- **The pitch line comes from Tune itself**: how far off it hears each note, and how far off it leaves it, as it plays. A note sung more than a semitone off is drawn at the edge. At Amount 0 the two lines are one; with Glide at 0 and Amount at 1, the solid line stays on the middle line.
- **The whole song plays 15 ms later** (15 thousandths of a second) while Tune is on any track, so every track stays in time with the tuned voice. You won't hear the delay, but a singer listening to themselves may feel it. A take you record still lands where you heard it.
- **An export sounds the same as playback.**
- **Tune works from about D2 (73 Hz) up to about D6 (1,200 Hz).** A lower note, such as a bass singer's low C, comes through untuned.
- **It works best on one voice at a time.** A choir, or a voice with a loud band behind it, confuses it.
- **Agents:** `tune(x, { glide, amount, formant })` from `"@m2/vocals"` in any track's code; `m2 listen` measures the result ([listening](listening.md)).

## Change a voice

The **Voice changer** makes a voice deeper or higher, or sound like another singer: **Pitch** moves its notes, **Formant** its colour, each on its own. It comes with music2, in the pack `@m2/vocals`.

| Word | Meaning |
|---|---|
| **Formant** | the hills in a voice's sound that make it an "ah" or an "ee", and a big or a small singer: its colour |

| To do this | Do this | What happens |
|---|---|---|
| Add it to a vocal | Select the vocal track (Vox), and in its Graph click the **+** on the wire out of its lane, then **Voice changer** (under Voice) | `voiceChanger(lane.audio("clips"), { pitch: 0, formant: 0 })` is written into the track's own file. At 0 and 0 the voice sounds as sung. |
| A deep, big voice | Load **Deep** (Presets on its card), or turn **Pitch** and **Formant** down | Its notes go 5 semitones down and its colour 5 semitones darker, like a tape played slower. |
| A chipmunk | Load **Chipmunk**, or turn both up | Its notes go 7 semitones up and its colour 7 brighter, like a tape played faster. |
| Higher or lower notes, the same singer | Turn **Pitch** only | From 7 semitones down to 12 up: each note moves, the colour stays. |
| The same notes, another singer | Turn **Formant** only | From 12 semitones down to 12 up: down sounds bigger and darker, up smaller and brighter. |
| See what it does | Play the song and look at its card | Its picture is your voice's spectrum: faint as it comes in, solid as it goes out. Pitch moves the spikes (the notes); Formant moves the hills (the colour). Its words say what the knobs did. |

- **It follows no key.** Every note moves by the same number of semitones. To put the notes on the key, use Tune.
- **A breath or an "s"** passes through as it is.
- **The whole song plays 15 ms later** while it is on any track, as with Tune.
- **One voice at a time**, from about D2 (73 Hz) to D6 (1,200 Hz), as with Tune.
- **Agents:** `voiceChanger(x, { pitch, formant })` from `"@m2/vocals"`.

## Make a synth talk

The **Vocoder** makes a synth talk with a voice: the synth plays the notes, and the voice gives them its words. It comes with music2, in the pack `@m2/vocals`.

| Word | Meaning |
|---|---|
| **Vocoder** | an effect that gives one sound (a synth) the shape of another (a voice), band by band: the "robot voice" of pop and dance songs |
| **Band** | a slice of the sound, from low to high: the Vocoder has 20, from 100 Hz to 8 kHz |

| To do this | Do this | What happens |
|---|---|---|
| Add it to a synth | Select the synth's track, and in its Graph click the **+** on the wire out of its sound, then **Vocoder** (under Voice) | `vocoder(…, { clarity: 0.5, smooth: 0.05, formant: 0 })` is written into the track's own file. Until you pick a voice, the synth sounds as it did. |
| Pick the voice | On its card, click **Talks with: No other track**, and pick the vocal track in Talks with (Vox) | `vox.out` is written into the call, and Vox is imported at the top of the file. One undo step: "Lead talks with Vox". |
| Make it talk | Play the synth's chords while the voice speaks or sings | Each band of the synth plays as loud as the voice is there: the synth takes the shape of the voice, its words on the synth's notes. |
| Clearer words | Turn **Clarity** up (0 to 1) | More of the voice's own S, T and breath sounds come through. A synth has little that high, so without them the words mumble. |
| Smoother or crisper | Turn **Smooth** (10 to 200 ms) | How fast the bands let go once the voice moves on: short is crisp, long is smooth. Very short makes the low notes buzz. |
| A deeper or brighter robot | Turn **Formant** (−12 to 12 semitones) | Down moves the voice's colour on the synth lower: a bigger, deeper robot. Up: smaller and brighter. The notes stay the synth's. |
| See what it does | Play the song and look at its card | Its picture is the synth's spectrum: faint as it comes in, solid as it goes out, in the voice's shape. |

- **The synth sounds only while the voice does.** A held chord under a silent voice is silent.
- **Clarity's S and T sounds are the voice's own**, so they come through even where the synth is silent. Turn Clarity down if they stand out.
- **To hear only the robot**, mute the vocal track in the song: the Vocoder still hears it (a sidechain hears its track, muted or not).
- **It listens to how loud the voice has been over the last quarter second or so.** Its loudest sounds play the synth at nearly its own level, quieter ones quieter, and the gaps between words stay quiet. So a quiet voice and a loud one come out about as loud.
- **While the voice sounds, it is about 6 to 11 dB quieter than the synth alone** with a voice recorded at a normal level: only the bands where the voice is loud play. Turn its track up. A whisper comes out about 16 dB under the synth, more than the fader's +12 dB can give back, so add a **Gain** after it. No band is ever louder than the synth.
- **It adds no delay.**
- **Any track can be the voice**: a drum loop makes the synth pulse with the drums. The kick sets the level, so the hats play steadily under it.
- **Agents:** `vocoder(x, voice, { clarity, smooth, formant })` from `"@m2/vocals"`; `vocoder(x)`, with no voice, plays `x` as it is.

## Edit a voice's notes

**Find notes** finds each note a singer sang in a recorded clip, so you can fix one note without singing it again (as Melodyne and Logic's Flex Pitch do). It comes with music2, in the pack `@m2/vocals`.

| To do this | Do this | What happens |
|---|---|---|
| See the notes | Right-click a vocal clip, then **Find notes** | Each sung note shows in the **Notes** view as a bar, over the clip's waveform. The line through it is the pitch the singer sang. One undo step. The song gets `@m2/vocals` if it didn't have it. |
| Fix a flat or sharp note | Drag the note up or down | It moves by whole semitones (one piano key). Hold **⌥** to move it by cents (a hundredth of a semitone). With **Lock to key** on, it lands on a note of the song's key. |
| Calm a wobble | Right-click the note, then **Straighten** | At 100% the note sings one straight pitch; at 0% it keeps the singer's wobble (vibrato). |
| Move a word onto the beat | Drag the note sideways | Only that note and the gaps beside it stretch. The clip now follows the song's tempo. |
| Make a note longer or shorter | Drag its left or right edge | The same: only the note and its gaps change. |
| Find the notes again | **Find notes** in the Notes view | Your edits stay on the notes they were on. |

- **Each edit is one undo step**, and you hear it at once, even while the song plays.
- **A note moves at most 7 semitones down and 12 up.** Further, the voice would sound broken, so the drag stops and says so.
- **The voice keeps its colour** when a note moves, as with Tune.
- **Only the notes you edit change.** After a pitch edit or Straighten, the rest of the clip plays exactly as it did. After a timing edit the whole clip follows the song's tempo, so the notes you didn't move may sit up to 8 ms (8 thousandths of a second) off.
- **Copies are separate.** Fixing a note in one copy of a chorus doesn't change the other copies.
- **Split a clip, and each piece keeps its notes.** A note across the cut is in both pieces.
- **One voice at a time.** Find notes doesn't hear chords, or a voice with a loud band behind it.
- **Find notes needs the clip played forwards, at its own pitch.** If edited notes can't play (the clip turned around, or its pitch changed by hand), the song's problems say so, and they play as sung.
- **An older song** may play `@m2/vocals` 1.0.0, which can't find notes: Find notes says so, and `m2 pack update @m2/vocals` updates it.
- **Agents:** the notes are the clip's `sung` field in its lane file, `{"at", "length", "pitch", "move", "straighten"}` (seconds of the file; MIDI numbers with cents), with `"render": "@m2/vocals:voice"`. Find notes is the clip gesture `{"op": "find_notes", "clip", "measure": "@m2/vocals:notes", "render": "@m2/vocals:voice"}`; `GET /api/graph/clip/measure` gives the pitch line, as bytes ([routes](routes.md)). A `set` of the clip from an older `revision` is refused when another changed the lane since.
