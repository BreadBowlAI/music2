# The Mac run by ear

**In short**
- These are the checks no test can do: they need your ears, a real microphone, a MIDI keyboard, the Finder and the Mac's own windows.
- Do them after the check in [HANDOFF.md](../HANDOFF.md) ("The owner's Mac command") has passed.
- About two hours in all. You can do one part at a time, on different days.
- After each part, tell the lead what you saw and heard. That is the last step of each part.
- The steps are numbered 7 to 136, as they always were: steps 1 to 6 are the check itself, in HANDOFF.
- **Examples** is a button on the start screen. If a song is open, press ⌘W (Close Song) first.

**What you need**

| Thing | For steps |
|---|---|
| The test plugins (Surge XT and Airwindows), as the check uses them | 8 to 10, 30, 88 to 94, 117 to 124 |
| Headphones (or AirPods) | 11, 12, 58, 59, 78 |
| A MIDI keyboard with a sustain pedal, a pitch wheel and a mod wheel | 51 to 53, 65 to 68, 103 to 107 |
| A USB microphone | 79, 81, 111 to 115 |
| A WAV file 10 minutes or longer, and a few short ones (a song, a loop, one drum hit) | 61 to 64, 73, 74, 84 |
| A MIDI file (`.mid`) made in another app, with drums and other parts | 108 to 110 |
| GarageBand or QuickTime Player | 69, 70 |

**The parts**

| Steps | What they check | Built in |
|---|---|---|
| 7 to 17 | an export with plugins, the output changing, a big song, no allocation | step 7 |
| 18 to 21 | the new sounds, and Stop then Play | the `sounds` merge |
| 22 to 31 | the look, + Track, the Mix view, tempo changes | `taste-next` |
| 32 to 38 | mute, solo, mute in song, hearing notes, meters | `8b-ui` |
| 39 to 50 | the Mix strips, zoom, markers, Save As, Open Recent | `polish` |
| 51 to 60 | a MIDI keyboard, the click, the keyboard piano, Settings | `next` |
| 61 to 72 | audio from the Finder, recording notes, stems, MIDI export, the guide | `studio` |
| 73 to 87 | long audio from the disk, recording your voice, the Sampler | tracks `io` and `ab` |
| 88 to 95 | plugins in their own program, a plugin's own window | track `ph` |
| 96 to 102 | stretched clips, Follow tempo | track `sx` |
| 103 to 110 | one-note sounds, the wheels, MIDI files | track `pm` |
| 111 to 116 | the sound input's problems | track `ex` |
| 117 to 125 | adding an installed plugin, and presets | track `br` |
| 126 to 132 | buses: a shared reverb, a send before the fader, a group | track `bus` |
| 133 | drawing notes: a drag draws one long note, ⌥-drag paints | track `dn` |

---

**Then a check by ear** (about 10 minutes; no test can do it, as the tests play no sound):

| # | Do this | You should hear or see |
|---|---|---|
| 7 | Type `cargo run --locked --release -p music2-desktop`. | The app's start screen. |
| 8 | Click **Examples**, then **With plugins**. Click the loop button, then press Space. | It plays round and round 4 bars. |
| 9 | Click **Export**, then **Save**. | "Paused for export" in the top bar, and the sound stops for a moment. |
| 10 | Listen as it comes back. | It goes on where it paused, with no click, and the chord sounds again at once. |
| 11 | While it plays, plug in headphones (or connect AirPods). | Playback stops, and a message says the sound output moved. |
| 12 | Press Space. | It plays on, in the headphones. |
| 13 | Quit the app. Type `cargo run --locked --release -p m2-server --example reference -- /tmp/p50 --tracks 50`. | It makes a 50-track test song: more than the Mac plays smoothly. |
| 14 | Type `cargo run --locked --release -p music2-desktop -- /tmp/p50`, press Space, and let it play for a minute. | Gaps or clicks are fine, but it keeps playing: no "the audio device stopped responding". |
| 15 | Look at the Terminal. | It may say `music2: the audio device reported … dropouts`: the count at work. |
| 16 | Quit. Type `cargo run --locked -p music2-desktop` (a slower test build), and play With plugins for a minute. | The Terminal never says "the audio thread allocated". |
| 17 | Tell the lead what you saw and heard at 9 to 16. | |

**A check by ear of the new sounds** (about 5 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 18 | Quit the app. Type `cargo run --locked --release -p music2-desktop`. Click **Examples**, then **Pop sounds**. Press Space. | A short pop song: supersaws, an 808, FM keys, bells and drums. It sounds full, with no clicks. |
| 19 | While it still plays (in its first 20 seconds), press Space to stop. Wait about 10 seconds. Press Space again. | It plays on from where it stopped. The wobble on the keys and the strings (their chorus) carries on smoothly, with no jump. |
| 20 | Press Space, then Return, then Space. | It stops, goes back to bar 1, and plays from there, with no click. The wobble and the drums' noise carry on from where they were, so this play may differ a tiny bit from the export. That is expected for now: it is the open question in Open owner decisions. |
| 21 | Tell the lead what you heard at 18 to 20, and whether 20 sounded right to you. | |

**A look and a listen at the new screens** (`taste-next`, about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 22 | Quit the app. Type `cargo run --locked --release -p music2-desktop`. Open **first-song**. | The new look: one bar at the top (play, the tempo "124 BPM", "4/4", the song's name in the middle, Export), the tracks, and the dock below. The Mac's own font, and the Mac's menus as before (music2, File, Edit, Track, View, Transport, Window, Help). |
| 23 | Press ⌘T. | + Track's list: Drums, Instruments, Sound design. Pick **Synth pad**: a new track below the selected one, and it plays with the rest. ⌘Z takes it out. |
| 24 | Press ⌥3. Press C to loop, then Space. While it plays, drag Kick's fader up and down. | The Mix view: a strip per track, Master on the right. The level changes as you drag, smoothly, with no clicks; let go and it is written into `song.ts`. |
| 25 | Drag Kick's pan knob a little (it reads "C" in grey). | Nothing changes until it moves; then the kick moves left or right, with no click. Two ⌘Z put the code back. |
| 26 | Still playing, drag the tempo in the top bar up by about 10. | The tempo changes when you let go, with no gap. |
| 27 | Press ⌘. (Command and full stop). | All sound stops at once; play goes on, silent until the next notes. |
| 28 | Press Space to stop, then Space again. | It plays on from where it stopped. Chorus and drum details may not match the export exactly then; the export is always right. |
| 29 | Right-click the ruler at bar 5, **Add tempo change at 5.1**, type 140, Return. | A mark "♩140" on the ruler; the song speeds up from bar 5. Click the mark, then **Remove this tempo change**. |
| 30 | Open the example **With plugins** (it plays Surge XT), press Space, then click **Export**. | While it exports, the grey pill in the top bar says "Paused for export"; then a toast says where the file is, with **Show in Finder**. |
| 31 | Tell the lead what you saw and heard at 22 to 30. | |

**A check by ear of 8b's screens** (about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 32 | Quit the app. Type `cargo run --locked --release -p music2-desktop`. Click **Examples**, then **First song**. Press Space. | Kick, hats, bass and keys play. |
| 33 | While it plays, click **M** on Kick's row (right of its name). Then click it again. | The kick goes quiet at once, and a pill at the top says "1 muted". The second click brings it back. |
| 34 | Click **S** on Bass's row. Then click it again. | Only the bass plays. The second click brings the others back. |
| 35 | Press Space to stop. Click Kick's **⋯** (or right-click its name), then **Mute in song**. Click **Export**, then **Save**, then **Show in Finder** in the message, and play the file. Then press ⌘Z. | Kick's name is crossed out. The file has no kick. ⌘Z brings Kick back. |
| 36 | Double-click Bass's clip. Notes opens. Click a note, then click a key on the piano at the left. | You hear each one while you hold it. |
| 37 | Press Space. Watch the thin bars at the right edge of each track's name, and the Song row's. Press Space again. | They move with the sound, and fall to nothing within about a second after Stop. |
| 38 | Tell the lead what you heard at 33 to 37. | |

**A look and a listen at `polish`** (about 15 minutes). These are what only a Mac can show: its trackpad, its menu
bar, its Ctrl-click, and the sound.

| # | Do this | You should hear or see |
|---|---|---|
| 39 | Quit the app. Type `cargo run --locked --release -p music2-desktop`. Open **First song**. Press ⌥3, then Space. | The Mix view. Each strip has M, S and two thin bars beside its fader that move with the sound. |
| 40 | While it plays, click **M** on the Keys strip, then **S** on the Bass strip. Click each again. | Keys goes quiet; then only the bass plays. The second clicks bring them back. The headers' M and S light up too. |
| 41 | Drag the Hats fader down a lot. | The hats get quieter, but the Hats strip's bars don't move: they show the track before its fader. |
| 42 | Press Space to stop. On the trackpad, pinch out over the timeline, then pinch in. | The timeline zooms in and out around your fingers. The window itself never zooms. |
| 43 | Press ⌘= twice, then Space. Watch the timeline. | It zooms in around the playhead; while it plays, the view pages along with it. Scroll by hand: it stops following until you play again. |
| 44 | Right-click Hats' name, **Change instrument…**, type `kit`, Return. Press Space. | Hats plays the drum kit now, and a message says so. ⌘Z puts the old hat back. |
| 45 | Double-click the Kick's clip, then Ctrl-click a note in Notes. | The note menu opens, and the note doesn't move. Pick **Humanize**: the kick sounds a little less stiff. ⌘Z. |
| 46 | Right-click the ruler at bar 5, **Add marker**, type Chorus, Return. Then Ctrl-click the Chorus marker. | Chorus sits on the ruler. The Ctrl-click opens its menu, and the playhead doesn't jump there. |
| 47 | Press ⇧⌘S. | Save As, with the song's name and a 2 after it. Press Return: the copy opens, and a message says the first one is kept. |
| 48 | Open the **File** menu, then **Open Recent**. | Both songs are listed, the copy first. |
| 49 | Quit the app (⌘Q) and start it again with step 39's command. | The song you had open opens again by itself. |
| 50 | Tell the lead what you saw and heard at 39 to 49. | |
| | **`next`**: what only a Mac with real devices can show. Start the app with step 39's command. | |
| 51 | Press Space. While the song plays, plug in a MIDI keyboard. | A message says the keyboard is connected. A small keyboard mark shows in the top bar. |
| 52 | Click Bass's name. Play some keys, soft, then hard. Hold the sustain pedal while you play. | Bass plays what you press, at once. Soft keys are quiet, hard keys loud. The pedal holds the notes. |
| 53 | Hold a key down and unplug the keyboard. | The note stops. The mark goes, and a message says the keyboard was unplugged. |
| 54 | Press K, then Space. Then press C to loop. | A click on each beat, a higher one on the first beat of each bar. It stays with the drums, also each time the loop jumps back. |
| 55 | Press ⇧⌘E to export. Play the file in the Finder (select it, press Space). | No click in the file. |
| 56 | Press ⌘K. Press A, S, D, F. Then Z, then X. Press ⌘K again. | Bass plays: A is middle C. Z goes an octave down, X back up. A card shows the keys. ⌘K again hides it. |
| 57 | While the song plays, drag a note in Notes, then a point of an automation line. Then drag the tempo number up and down. | You hear each where it is while you drag. The song speeds up and slows down with the tempo. Let go: it stays. ⌘Z goes back. |
| 58 | Press ⌘, for Settings. Plug in headphones. Under Sound output, pick them. Then under Buffer size, pick 128. | The sound moves to the headphones and stops: press Space to go on. The Now row says the headphones and 128 samples (2.7 ms). No crackles. |
| 59 | Unplug the headphones while the song plays. Press Space. Look at Settings. | It plays on the Mac's speakers. Sound output says the headphones "(not connected)". The Now row says "(the output chosen can't be used now)". |
| 60 | Tell the lead what you saw, heard and felt at 51 to 59. | |
| | **`studio`**: what only a Mac can show: the Finder, a real MIDI keyboard, and the sound. Start the app with step 39's command, and open **First song**. | |
| 61 | In the Finder, drag a WAV file (a song or a loop) onto the window, below the tracks, at bar 3. | While you drag, a blue outline and a few words say where it goes ("New audio track · bar 3"). Let go: a new track, named after the file, with its waveform. |
| 62 | Drag another WAV file onto that new track, at bar 9. Press Space. | It lands on the same track at bar 9. Both play with the song, in time. |
| 63 | Press Space to stop. Drag the right edge of a clip to the left. Then drag the small square at its top left to the right. Press Space. | The clip gets shorter. It fades in: the sound starts quiet and grows. No clicks. Two ⌘Z put it back. |
| 64 | Press ⇧⌘I and pick a WAV file. | The Mac's own open window, "Import Audio", shows WAV files. The file lands at the playhead's bar. |
| 65 | Plug in a MIDI keyboard. Click Bass's name. Press R. | The Record button counts 4, 3, 2, 1, with a click on each beat. Then the song plays. |
| 66 | Play a few notes, then press R. | Each note shows in Bass's row as you play it. R stops recording, and the song plays on. The notes are one clip, where you played them. ⌘Z takes it back. |
| 67 | Press Space to stop. Drag across the ruler from bar 1 to bar 3 to loop it. Press R and play until the loop goes round. | The take ends where the loop goes back: only the first time round is kept. The loop plays on. |
| 68 | Press ⌘, for Settings. Under MIDI keyboards, untick your keyboard. Play a key. Then tick it again. | Unticked, the keyboard plays nothing, and Record would record nothing from it. Ticked, it plays again. |
| 69 | Press ⇧⌘E. Click **Stems**, then **Export…**. When it is done, click **Show in Finder**. | A new folder in Exports, with one WAV file for each track. Open one in QuickTime Player or GarageBand: it is that track alone. |
| 70 | Press ⇧⌘E. Click **MIDI**, then **Export…**. Save. Open the file in GarageBand. | The Mac's own save window opens in Exports. In GarageBand the tracks and their notes are there. |
| 71 | Click **Help**, then **Make Your First Song**. Click **Start from an empty song…**, and follow the steps. | Each step ticks itself when you do it. At the end, the card says where your song was saved. |
| 72 | Tell the lead what you saw, heard and felt at 61 to 71. | |
| | **`io`**: long audio from the disk, and recording your voice. Have headphones, and a WAV file 10 minutes or longer. Build the app: `(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --bundles app)`, then open `target/release/bundle/macos/music2.app` (the bundle, so macOS asks about the microphone in music2's own words). Open **First song**. | |
| 73 | Drag the long WAV file onto the window, below the tracks. Press Space. Click the ruler near its end, then near its start. | It plays at once from each place you click: no gap, no click. The status pill never says "Disk too slow". |
| 74 | Drag across the ruler over 4 bars of that clip to loop it. Let it go round 10 times. | No gap where the loop jumps back. |
| 75 | Press Space to stop. + Track ▸ Audio track. Press R. | The first time, macOS asks to let music2 use the microphone: "music2 records from your microphone when you press Record on an audio track." Click Allow. The button counts 4, 3, 2, 1, with clicks. |
| 76 | Press K (the click). Count "one, two, three, four" out loud on the clicks for 2 bars. Press R. | A red clip grows as you speak, with a thin bar at its right end that moves with your voice. R ends it: a clip "Audio take.wav" with your voice's waveform. Play it from bar 1: your words land on the clicks, not late. |
| 77 | Press ⌘Z. | The clip goes. "Audio take.wav" stays in the song's `assets` folder. |
| 78 | Put on headphones. Press ⌘, for Settings. Tick **Hear yourself while recording**. Close Settings, press R and speak. | You hear yourself in the headphones while it records, the count included. It sounds a little late at most, never like an echo. Untick it after. |
| 79 | In Settings, under **Sound input**, pick another input (a USB microphone, if you have one). Record again. | It records from that input. |
| 80 | In System Settings ▸ Privacy & Security ▸ Microphone, turn music2 off. Back in music2, press R on the audio track. | A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone." Nothing records. Turn it back on. |
| 81 | With a USB microphone picked in Settings, press Space, then R, and speak. While it records, unplug the microphone. | The song plays on. A message (since track `ex`): "(the microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. Plug it back in and press R: it records again. |
| 82 | Tell the lead what you saw, heard and felt at 73 to 81. | |
| 83 | Click **Examples**, then **Sampler**. Press Space. | Keys play a plucked sound, Drums a kick and a snare, in time. No clicks. |
| 84 | Press Space to stop. + Track ▸ Sampler. In the Finder, drag a short WAV (one sound, like a drum hit) onto the Sampler's card in Device. | While you drag, the card says "Play it in Sampler". Let go: the card names the file. |
| 85 | Press ⌘K. Press A, then S, D, F. | A plays the file as it is (A is middle C). S, D and F play it a little higher each time. |
| 86 | Press ⌘K again, then ⌘Z. | The card says "Drop a WAV here" again. The file stays in the song's `assets` folder. |
| 87 | Tell the lead what you saw, heard and felt at 83 to 86. | |

**A check by ear of plugins in their own program** (`ph`, about 10 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 88 | Quit the app. Type `cargo run --locked --release -p music2-desktop`. Click **Examples**, then **With plugins**. Press Space. | It plays the Surge XT lead as before, with no clicks. |
| 89 | Open Activity Monitor and type `music2` in its search box. | Two music2 programs: the app, and a second one that runs the plugins. |
| 90 | Back in music2, click the **lead** track, then **Device** at the bottom. On the Surge XT box, click **Open window**. | Surge XT's own window opens. The button now says **Close window**. |
| 91 | In Surge XT's window, turn the filter cutoff knob while it plays, and let go. | You hear the sound change as you turn. |
| 92 | Press ⌘Z. | The sound goes back to how it was. |
| 93 | In the Terminal, type `pkill -9 -f -- --m2-plugin-host` while it plays. | The song plays on. The lead goes quiet for a moment, a message says "The plugin host crashed…", and the lead comes back by itself. Surge XT's window closes: open it again from the box. |
| 94 | Click **Export**, then **Save**, while it plays. | "Paused for export" in the top bar, then it plays on. The file sounds like the song. |
| 95 | Tell the lead what you saw, heard and felt at 88 to 94. | |

**A check by ear of stretched clips and Follow tempo** (`sx`, about 5 minutes):

| # | Do this | You should hear or see |
|---|---|---|
| 96 | Click **Examples**, then **Resample**. Press Space. Listen from 3 seconds in (bar 2, beat 3) to 5 seconds, then from 6 seconds (bar 4). | At 3 s the keys come back slower, stretched over 4 beats. Their pitch is the same as at the start. No wobble, no clicks, no echo. At 6 s they come back higher and stretched, still on the beat. |
| 97 | Tell the lead what you heard at 96. | |
| 98 | Open **Resample** again. Right-click the first clip on Chops (bar 1). Click **Follow tempo**. Press Space from the start. | It sounds as before. |
| 99 | Double-click the tempo (120), type 90, press Return. Press Space from the start. | The first clip keeps in time with the kick, a little slower, at the same pitch. The second clip (not followed) plays more of its sound in the same time. |
| 100 | While it plays, drag the tempo up and down a little, then let go. | The followed clip goes on in time. At most a short silence, no click, no wrong sound. |
| 101 | Press ⌘Z twice. | The tempo is 120 again, and Follow tempo is off. |
| 102 | Tell the lead what you heard at 98 to 101. | |

**A check by ear of one-note sounds, the wheels and MIDI files** (`pm`, about 10 minutes, with a MIDI keyboard
that has a pitch wheel and a mod wheel):

| # | Do this | You should hear or see |
|---|---|---|
| 103 | **Examples** ▸ First song. Click the **Bass** track, then **Device**. | Its card says "Plays one note at a time." under its name. Hold the mouse over that line: the rest of the sentence. |
| 104 | Plug in the MIDI keyboard. Keep Bass selected. Hold a low key. While holding it, press a higher key, then let it go. | One note at a time. The higher key slides up from the low one without starting again; let go, and it slides back to the low key. |
| 105 | Hold a key and push the pitch wheel all the way up, then let it go. | The note bends up by two keys' worth (a whole step), and comes back when the wheel springs back. No click. |
| 106 | Click the **Keys** track. Hold a chord and push the mod wheel up slowly, then back down. | The wobble in the pitch (vibrato) gets deeper as the wheel goes up, and is as before at the bottom. |
| 107 | Click **Bass**. Hold a low key, then a higher one. While holding both, click the **Keys** track. Let go of both keys. | Bass stops when you click Keys; no Bass note keeps sounding, and the low one doesn't come back. The keyboard now plays Keys. |
| 108 | File ▸ Import MIDI…. Pick a `.mid` file with drums and other parts (one made in another app). | If its tempo differs from the song's, a question: Return keeps the song's. Then a new track for each part, each with a sound. Space plays them in time. |
| 109 | Press ⌘Z. | All the new tracks go, in one step. |
| 110 | Drag the same `.mid` file from the Finder onto the empty space below the tracks, at bar 3. Then tell the lead what you saw, heard and felt at 103 to 109. | The new tracks start at bar 3. |

**A check by ear of the sound input's problems** (`ex`, about 10 minutes, with a USB microphone):

| # | Do this | You should hear or see |
|---|---|---|
| 111 | Click **Examples**, then **First song**. **+ Track ▸ Audio track**. Press ⌘, for Settings. Under **Sound input**, pick the USB microphone. Close Settings. Unplug the USB microphone. Press R. | A message: "(the USB microphone) isn't plugged in, so this take records from (the Mac's microphone)…". It records. The pill says "1 problem"; click it: the same words. Press R to end the take. |
| 112 | Plug the USB microphone back in. Press R, speak, press R. | It records from the USB microphone. No message. The problem is gone. |
| 113 | Press R and speak. While it records, unplug the USB microphone. | The song plays on. One message: "(the USB microphone) was unplugged, so the take ended there. It was kept up to then. The song plays on." A clip of your voice up to there. |
| 114 | Quit music2. In the Terminal type `tccutil reset Microphone ai.breadbowl.music2` (started from the Terminal, macOS asks for the Terminal: then `tccutil reset Microphone com.apple.Terminal`). Open music2 again, open the song, click the audio track, press R. When macOS asks to use the microphone, click **Don't Allow**. | The take stops by itself within a moment. A message: "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone. The take was only silence, so it wasn't kept." No clip. The song plays on. |
| 115 | Press R again. | The same "music2 can't use the microphone…" words. Nothing records. Then turn music2 (or the Terminal) back on in System Settings ▸ Privacy & Security ▸ Microphone. |
| 116 | Tell the lead what you saw and heard at 111 to 115. | |

**A check by ear of plugins from the app, and presets** (`br`, about 15 minutes, with Surge XT and Airwindows installed):

| # | Do this | You should hear or see |
|---|---|---|
| 117 | Click **Examples**, then **First song**. In the side browser, under **Effects**, open **Plugins**. | After a moment it lists the installed effect plugins, Airwindows among them. **Look again** is at its foot. |
| 118 | Drag Airwindows onto the **Bass** track. Press Space. | A message says it was added after the bass, with **Undo**. Its card shows under **Device**. The song plays on, with no click. |
| 119 | Press ⌘Z. | The plugin goes, in one step. |
| 120 | **+ Track ▸ Plugins ▸ Surge XT**. Click the new track, then **Device**. On the Surge XT card, click **Open window**. | A new track named Surge XT. Surge XT's own window opens. |
| 121 | On the Surge XT card in music2, click **Presets**. While its list loads, turn a knob in Surge XT's window. | The knob moves at once: the window never freezes. Within a second or two the list shows "Surge XT's", about 2,000 names. |
| 122 | Click one of them. Play a few notes (a MIDI keyboard, or the keyboard piano). Then press ⌘Z and play again. | A message "Loaded … into Surge XT". You hear that preset. After ⌘Z, the old sound. |
| 123 | **Presets ▸ Save as preset…**, type `My Surge`, press Return. Change a knob in Surge XT's window. Then **Presets ▸ My Surge**. | "Saved…". After the load you hear the sound you saved. `~/Music/music2/Presets` has the file. |
| 124 | **+ Track ▸ Synth pluck**. On its card, **Presets ▸ Glass**. Play notes. Press ⌘Z. | You hear the glassy pluck, then the old one after ⌘Z. |
| 125 | Tell the lead what you saw and heard at 117 to 124. | |

**A check by ear of buses** (`bus`, about 10 minutes):

| # | Do this | You should see and hear |
|---|---|---|
| 126 | Click **Examples**, then **First song**. Press ⌥3 for Mix. Click **Add bus**, then **Reverb**. | A Reverb strip after Keys, with a merge mark before its name. Every other strip has a row "+ Reverb". A message says to turn up a track's Reverb knob. |
| 127 | Press Space. On the Keys strip, click the **+** beside Reverb. | Within a moment the keys sound like they are in a room. No click. The Reverb strip's meter moves. |
| 128 | While it plays, drag that small Reverb knob on Keys up, then down, and let go. | More room as it goes up, less as it goes down. No clicks, no crackle. |
| 129 | Drag the Keys fader down to the bottom. Then right-click the Reverb knob on Keys and click **Before the fader**. | With the fader down, the keys and their room both go. After **Before the fader**, only the room comes back: a ghostly keys sound. A small dot on the knob. Press ⌘Z twice: as before. |
| 130 | Click **Add bus**, then **Group**. Under the Kick fader, click **Master** and pick **Group**. Do the same under Hats. Then drag the Group fader down and up. | Kick and Hats go quieter and louder together. Bass and Keys don't change. Kick's strip says Group under its fader. |
| 131 | On the Group strip, click the **+** beside Reverb. Then, under the Reverb fader, click **Master**. | In the list, Group is grey and says "Group already feeds Reverb, so Reverb can't go to Group." |
| 132 | Tell the lead what you saw and heard at 126 to 131. | |

**A check by ear of drawing notes** (`dn`, about 3 minutes):

| # | Do this | You should see and hear |
|---|---|---|
| 133 | Click **Examples**, then **First song**. Double-click the Bass clip, press B, then press on an empty row and drag right about a bar, up and down a little too, and let go. Then hold ⌥ and drag across a few grid steps on another row. Press ⌘Z twice. | The first drag makes one note that grows as you drag, on the row you pressed; you hear it while you hold and it stops when you let go. The ⌥-drag adds a short note in each step, each heard. Each ⌘Z takes back one drag's notes. Tell the lead what you saw and heard. |

**`m2` inside the app** (`docsite`, about 5 minutes):

| # | Do this | You should see and hear |
|---|---|---|
| 134 | Build the app as in `io` above: `(cd apps/music2-desktop && ../../web/node_modules/.bin/tauri build --bundles app)`. Then in Terminal, in the music2 folder: `target/release/bundle/macos/music2.app/Contents/MacOS/m2 docs \| head -2` | The build says `bundle-m2.sh: target/release/m2 is ready` before it bundles. `m2 docs` prints "music2's guide, built into this m2 (0.1.0)…": the app carries `m2`. |
| 135 | Open that `music2.app`. Click **New Song…**, name it "Agents test", and press **Create**. In Terminal: `grep "m2 for this song" ~/Music/music2/"Agents test"/AGENTS.md`, then `cat ~/Music/music2/"Agents test"/CLAUDE.md` | The first names the app's own `m2`: `…/music2.app/Contents/MacOS/m2`. The second says `@AGENTS.md`. The app shows neither file. |
| 136 | Tell the lead what you saw at 134 and 135. | |
