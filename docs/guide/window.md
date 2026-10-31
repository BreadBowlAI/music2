# The window

**In short**
- The top bar plays, records, loops and sets the tempo, time signature and key.
- The timeline shows each track's clips; the dock below shows the selected track as Device, Notes, Mix or Code.
- The browser on the left lists instruments and effects. Help ▸ Make Your First Song teaches the basics.

## The top bar

From left to right: the browser's toggle, back to the start, play/stop, record, loop, the metronome, where the song is (bar.beat.sixteenth, then minutes and seconds), the tempo, time signature and key, the song's name, a keyboard mark while a MIDI keyboard is plugged in, the status pill (only when something needs saying: see [Saving and undo](saving.md)), undo, redo, and **Export** (it shows how far along an export is). File → Close Song (⌘W) goes back to the start screen.

| To | Do this |
|---|---|
| Change the tempo | Drag it up or down: 1 BPM for every 4 pixels, a tenth with ⇧. You hear it as you drag, playing or not; letting go saves it, and Esc before you let go puts it back. (A song with an audio clip hears it when you let go.) Or double-click it and type `128`, `128.5` or `128 bpm` (20 to 999). |
| Hear a click on each beat | Click the **Metronome** button after Loop, or press K (Transport → Metronome). While the song plays it clicks each beat, higher on the first beat of a bar. Each song keeps its own setting; an export never has the click. |
| Change the time signature | Click it, then pick the beats in a bar (1 to 16 with the buttons, up to 64 typed) and the note a beat is (2, 4, 8 or 16). |
| Set the song's key | Click **Key** after the time signature, then pick the root (C to B) and Major or Minor; **No key** takes it away. Notes then shades the rows not in the key. It is kept in `layout.json`, and changes nothing you hear. |
| Rename the song | Double-click its name, type, then Enter. Escape cancels. The folder keeps its name. **File → Rename Song…** renames its folder too ("Also rename its folder", on at first): the song closes and opens again from there, with its undo history. Undo then takes back the name, and the folder keeps its new one. With unsaved code in the editor it says so first. |

Both fields change the tempo or time signature in effect where the playhead is. Each change is one Undo step, written in `data/arrangement.json` (the song's name in `music2.toml`). There is no Alt-click reset on the tempo: one stray click shouldn't change the song.

Play and stop work as in GarageBand: Space plays from the playhead, and Space again stops and leaves the playhead where it stopped, so the next Space plays on from there. Return (or the ⏮ button) goes back to bar 1; while playing, it plays on from there. Transport → Stop All Sound (⌘.) stops every note at once.

## Playing live

- **The keyboard piano:** ⌘K (Ctrl+K off the Mac), from anywhere, or View → Keyboard Piano, or the keyboard button in Notes. The computer's keys then play the selected track: A S D F G H J K L ; ' are the white keys, W E T Y U O P the black ones, and A is middle C at first. Z goes down an octave, X up. A card at the lower right of the timeline shows the keys and what plays. While it is on, those keys only play notes (Space, C and M work as always); typing in a field never plays. ⌘K again turns it off.
- **A MIDI keyboard:** plug one in, before or while the app runs. A message says it is connected, and a small keyboard mark shows in the top bar. Its keys play the selected track, as hard as you press them. Hold the sustain pedal and notes ring on after you let go of their keys, until you lift it; strike a key again while it rings and it starts again. Every instrument hears the pedal, and a plugin holds its own notes. If the notes the pedal holds fill all of an instrument's voices, a new note takes the place of one the pedal holds, never one whose key you are still holding down. Stop, or moving the playhead, lets the pedal up; a loop going round doesn't. After Stop, lift the pedal and press it again to use it again. When you record, each note is kept as long as it sounded, but the pedal itself isn't recorded yet. Only the keyboard's own notes are kept longer: a note from the computer's keys is recorded as long as you held its key, though the pedal holds its sound. The pitch wheel bends the track's notes, up to 2 semitones each way; the mod wheel adds vibrato to the polysynth's sounds, and plugins hear both. Unplugged, its notes stop, the wheels and the pedal go back to rest, and a message says so.
- To record what you play, press R: see [Recording](audio.md#recording).

Stop ends the notes, and echoes and reverb ring out as they would on a real instrument. Moving the playhead, looping, editing notes and adding an effect all happen while the song plays, without it starting over or jumping back. Notes already sounding where the playhead lands aren't picked up: playback starts with the next note.

## The timeline

- Each track has a row that draws its first notes (or audio) lane, and a row under it for each of its other notes lanes. Its automation lanes fold under it: the arrow by its name, or A, shows them, each with the value it plays at the playhead (see [Words you'll see](words.md)). The Song's own row is pinned at the foot. The ruler on top counts bars, following the song's time signatures.
- Tracks are listed in the order the song mixes them, until you move one. Drag a track's name up or down to move it; its colour stays its own. Both are kept in `layout.json`, which never changes the sound (see [The folder](code.md#the-folder)).
- Click a track's row to select it: the dock below shows that track. Selecting never changes which view the dock is in.
- **+ Track** (⌘T) adds a track playing one of music2's sounds (its packs `@m2/drums` and `@m2/synths`), or a pack's you installed: the drums first (the whole kit, then each drum), then the instruments, then the Sampler and the two synths with every knob, under Sound design. Typing filters the list, ↑↓ move, Enter adds; the list opens on the instrument you added last, so ⌘T then Enter adds another of the same. The new track goes below the selected one, in the first colour no other track has. It writes `tracks/<its key>.ts`, an empty notes lane in `data/`, and two lines in `song.ts`: its import, and its channel at the end of the mixer. One Undo takes it all back.
- Right-click a track's name (or click ⋯ beside it) for its menu: Rename, Change instrument…, Duplicate (⌘D), Delete (⌘⌫), Colour, its automation, and Edit in code. **Change instrument…** (also Track → Change Instrument…) opens the + Track list for that track: the one you pick replaces its instrument in its code, starting at its own settings, as one undo step. When code works out one of the old instrument's settings, it says so and leaves the code as it is. Duplicate copies the track's code, its clips, its plugins' saved settings and its mixer channel (a duck stays a duck), below it in its colour. Delete takes out its file, its import, its channel and its data. When another track's effect listens to it (the bass ducking to the kick), it asks first, and takes that effect off with it.
- Zoom with ⌘= and ⌘− (around the playhead), a pinch or ⌘-scroll (around the pointer), or the two buttons at the right end of the ruler. Z fits the selected clips, or the whole song; Z again goes back.
- Make a track taller or shorter: drag the bottom edge of its name (28 to 160 pixels), or press ⌘↓ and ⌘↑ (the selected track, or every track with none selected). A double-click on the edge puts it back. An automation lane has the same edge. **Hide this lane** in a lane's menu hides one lane; the line "1 hidden lane · Show" under the track brings it back.
- While the song plays, the view follows the playhead a page at a time. Scrolling by hand pauses it until play starts again. The Follow button at the right end of the ruler (⇧⌘F) turns it off. The zoom, the heights, Follow and hidden lanes are kept per song, in `.music2/view.json`.
- **Markers** (Intro, Verse, Chorus…): right-click the ruler, **Add marker**, or Transport → Add Marker at the playhead's bar. Type a name or pick one from the list. Click a marker to go there, drag it to move it (hold ⌘ to place it off the grid), double-click it to rename it, and right-click it to delete it, loop to the next one, or select the bars up to the next one. ⌥← and ⌥→ go to the marker before or after. They are kept in `layout.json`.
- Click the ruler to move the playhead there. Drag across the ruler to loop those beats: playback goes round them until you turn the loop off with the loop button or C (it keeps the range, also when the song is opened again). With no loop set, the loop button loops the 4 bars from the playhead. ⌘L loops the selected bars or clips (with none, the bar the playhead is in).
- Drag along the ruler's top strip, where the markers are, to select whole bars on every track, to copy, paste, empty, insert or delete them: see [Sections: copy a chorus](sections.md).
- While stopped, a click on an empty spot in a lane moves the playhead there too. While playing it doesn't, so a click to deselect never makes the song jump.
- The tempo and time signature changes are marks on the ruler. Click one to change it or remove it. Right-click the ruler to add a tempo change, or a time signature change at the start of that bar.
- While the song plays, a clip you drag (or its edge), an automation point or a note in Notes is heard where it is before you let go. Letting go saves it, as one undo step; Esc before you let go puts it back. (An audio clip is heard when you let go.)
- Double-click an empty spot in a notes lane to add a one-bar clip. Drag a clip to move it along its lane. Click a clip and press Delete to remove it. **Repeat…** in a clip's menu makes copies one after another (1 to 64), as one undo step.
- Click a notes clip to select it (Notes shows it); double-click it to open it in Notes.
- Double-click a track's name (or press ⌘R) to rename it: type, then press Enter (or click away) to save, or Escape to cancel. This changes only the name the app shows (the track's label, written in its `track()` call); its key, its data and its sound stay as they are, and Undo takes it back.
- **Mute and solo** while you listen: **M** and **S** on each track's header (and on its Mix strip), or press M or S for the selected track; ⌥-click S hears only that track. A pill at the top counts them ("1 muted · 1 solo"); click it for the list and Clear all. They change only what you hear, so Export asks first while one is on. **Mute in song** in a track's menu writes `muted: true` into its code, so the export leaves it out too: its name is struck through.
- **Meters:** a thin bar at the right of each track's name, and the Song row's, shows how loud it is. A track with a mixer channel of its own is read after its fader, so moving the fader moves its meter. A track that shares a channel with another, or isn't in the mixer, shows its own sound, before any fader. The status pill says **Heavy load** when the computer is near its limit, and **Sound dropped out** after a gap.
- **Edit clips:** drag a clip to move it (onto another track too), ⌥-drag to copy it, drag an edge to resize it. ⇧-click, ⌘-click or a drag across empty lanes picks several. ⌘E splits at the playhead, ⌘J joins, ⌘X ⌘C ⌘V cut, copy and paste at the playhead, Q quantizes, ← → nudge. Each clip has a right-click menu. The timeline's grid is in its corner (Auto, Bar to 1/64, triplets; ⌘1 to ⌘4); hold ⌘ to place a clip off the grid.
- **Draw automation** on the timeline: double-click an automation lane to make a clip, click its line to add a point, drag a point to move it, Delete removes it, and B paints. A frequency is drawn on a log scale. **Remove automation** in the lane's menu writes the value at the playhead back into the code.
- **A lane of names** (a menu's or a switch's automation) is drawn as steps, each with its name on it: each name plays until the next. Double-click the lane to make a clip (it starts with the name playing there). Click the clip and pick a name from the menu there to start it at that spot; drag a step's mark to move it; click a mark and press Delete to remove it. A name the sound no longer has is struck through, and plays the lane's default there.
- **Audio clips** show their waveform: see [Audio, recording and MIDI files](audio.md).

## The browser

Down the left (⌥⌘B shows or hides it): the instruments (Drums, Instruments, Sound design) and the effects, by group. Type to search. Enter or a double-click on an instrument adds a track with it below the selected one, and one dragged onto the space below the tracks adds it there. Enter or a double-click on an effect adds it to the end of the selected track's sound, and an effect dragged onto a track or the Device view goes at the end of that sound.

## The dock below the timeline

It shows the selected track (or the Song) in one of four views, picked with the switcher in its header or ⌥1 to ⌥4: **Device**, **Notes**, **Mix** and **Code**. The view stays what you picked while you select other tracks. Drag the line between the timeline and the dock to resize it; double-click it, or press E, to hide or show the dock.

- **Device:** the track's sound as boxes and knobs.
  - Drag a knob up or down to hear it (300 pixels take it across its whole range; hold ⇧ for a tenth of that, to fine-tune); let go to write the number into the code. ⌥-click a knob to put it back at its default (the middle of its range when its control declares no default).
  - Double-click a knob or its number to type a value: `-3`, `2k`, `120ms`, `50%`, `L20` for a pan, or a pitch such as `C4` for a frequency. Enter writes it, Escape leaves it, Tab writes it and moves to the next knob. A knob you've clicked or tabbed to also takes ↑ and ↓ (with ⇧, ten steps), Return to type and Delete to reset; the scroll wheel steps it too. A run of steps is written as one edit.
  - Numbers read the way a mixing desk writes them: "−6.0 dB", "350 Hz", "1.2 kHz", "120 ms", and pans as "C", "L 20" or "R 35".
  - Right-click a knob for **Automate** and **Reset to default**. Automate gives the knob an automation lane of its own, under its track in the timeline: the code reads the knob from the lane (`pan: lane.control("hats-pan", "bipolar", 0.3)`), and the lane plays the old number until it has points, so nothing sounds different. Undo takes the lane out again. When a knob can't be automated (it's shared by several tracks, say), the item is greyed out and says why.
  - Double-click the track's name in the dock's header to rename it, as in the timeline.
  - Select the Song, and its mixer is one small box that says how many channels it has: the levels and pans are in the Mix view (its **Open Mix** link, or ⌥3). The master's volume and ceiling stay knobs there.
  - A knob whose number is in grey is a control the code leaves out, such as a bass's `drive` when the code says only `bass(notes)`: it plays the instrument's or effect's default. Turning it writes the number into the call (`bass(notes, { drive: 9 })`), and undo takes it out again.
  - A row that says **edit in code** is a value the app can't turn into a knob (see [Editing from the app](code.md#editing-from-the-app)).
  - **Settings** sit under a card's knobs. A setting decides how a sound is built: an LFO's **Shape** (sine, tri, saw or square), how many notes an instrument plays at once (**Voices**), a Sampler's **Mode** and its file. Each kind has its own control:

    | You see | It is | To change it |
    |---|---|---|
    | a word with ▾ | a choice | click it and pick another |
    | a small switch | on or off | click it |
    | a box with a number | a number | click it, type, then Return (Escape leaves it). **Voices** and other counts take whole numbers only: `2.5` is refused, in red under the box |
    | a waveform and a file name | a recording | drop a WAV or FLAC file on it (or on the card) to play that instead |
    | a track's name with ▾ | a track | pick another track. A mixer channel's **to** lists **Master** and the song's buses |

    Each change is written into the sound's code (`lfo(time.beats, { shape: "saw" })`) as one Undo step. A setting decides what is built, so changing one rebuilds that sound: a note playing just then may stop. A grey value is the default, not written in the code yet. A setting the code works out, or a list of regions, says **edit in code**.
  - A plugin's parameter that has a few steps with names (a filter's type, say) is a menu of the plugin's own names, and one that is just off or on (a bypass) is a switch. The code still holds the number.
  - **Menus and switches that play at once.** A sound can also have a choice or an on/off that it plays while it sounds, with no rebuild: a wave's **Shape** (saw, square or sine), a **Sub** that is on or off. These sit with the knobs. Click the menu and pick a name, or click the switch: the name (`shape: "square"`) or `true` goes into the code, one undo step. They don't drag. Right-click one for **Automate**: its lane is a lane of names (see the timeline, below).
  - **Groups.** A sound can put its card in order: its knobs and pictures under titles, such as **Sound**, **Filter** and **Amp**, with **More** last for the rest. The arrow in the card's header folds it to its first group; "3 more" under it unfolds it.
  - **Pictures.** A sound can show pictures of its knobs. Each one shows values that live in the code: it never holds one of its own.

    | Picture | It shows | You can |
    |---|---|---|
    | Envelope | how loud a note is over time: it rises (attack), falls (decay) to a level it holds (sustain), and fades out when you let go (release) | drag the top corner for the attack, the middle corner for the decay and the sustain, the last corner for the release. You hear it as you drag; letting go writes the knobs, one undo step. A corner stops at the ends its knob allows. A part that follows a lane is drawn at the lane's own value and can't be dragged (its corner is hollow); its tooltip says why |
    | Filter | a low-pass filter's curve: it lets the lows through and cuts the highs above the cutoff, with a bump at the cutoff for a high q. It is a drawing of a low-pass, worked out from the two knobs, not a measurement of the sound | drag the bump: left and right for the cutoff, up and down for the q. One undo step for both |
    | Keys | which recording plays on which keys, and how hard (a box for each; harder is higher) | click to hear a key: the higher you click, the harder it plays. It plays only while audition is on (the speaker in the Notes view's header), as the piano roll's keys do |
    | Scope | the sound's wave as it plays | look. It shows only while the song plays, and only while the card is on screen |
  - Cables: drag from a named output (the dot at the right edge of a box whose result has a `const` name in the code) onto a knob. The ✕ on a connected input disconnects it.
  - An effect card's ⋯ menu has **Turn off** (the sound goes on without it; the card stays, dimmed, "Reverb · Off", and `bypass: true` goes into its code) and **Turn on**, and **Move left** and **Move right**, which swap it with the effect next to it. Each is one undo step. A Duck's card says what it listens to ("Listens to: Kick ▾"): click it to pick another track.
  - **+** on a box that makes sound opens the effect browser: 17 effects in seven groups (Filter, EQ, Dynamics, Distortion, Delay & Reverb, Modulation, Utility), as the effects themselves declare them. Type to search, then press Enter or click. The bin icon on an effect removes it.
  - Zoom with ⌘ and scroll, or pinch. The buttons at the bottom right zoom and fit the graph to the panel.
  - An instrument's card says how it plays under its name, such as "Plays one note at a time." (the rest is in its tooltip).
  - A **Sampler**'s card takes a WAV or FLAC file: drop one on it, or on its recording's row (see [Sounds and effects](sounds.md)).
  - A CLAP plugin's box has **Open window**, for the plugin's own window (see [CLAP plugins](plugins.md)).
- **Mix** (⌥3, or X): a strip for each track, in the timeline's order, and Master on the right.
  - Each strip has the track's colour and name, a pan knob, **M** and **S** (as on the track's header; a strip shared by two tracks acts on both), and a fader with its level under it. Beside the fader, two bars (left and right) show how loud the strip is, after its fader and pan: drag the fader down and they go down with it. A strip of two tracks shows them together. A small red dot over them means the strip went over the top: click it to clear it. Master's meter is the whole song.
  - **How loud the song is.** Above Master's fader, while the song plays: its loudness over the last 3 seconds, such as "−14 LUFS", and its **Peak**. LUFS measures loudness as ears hear it; streaming services play songs at about −14 LUFS, so a song much louder than that is turned down there. The peak is the loudest point of the last 3 seconds, between samples too: keep it under 0 dB, or the song can distort when it's played or streamed. It takes 3 seconds to show, and `m2 listen` measures it the same way. music2 measures it only while the Mix view shows.
  - Drag a fader to hear it; let go to write it into the code. 0 dB sits three quarters of the way up. Hold ⇧ to fine-tune; ⌥-click puts it back at 0 dB; the very bottom is silence ("−∞", written as −96 dB). Scroll up or down over a fader to move it 0.1 dB a notch (scrolling sideways moves the strips). Double-click the level to type one: `-3`, `-3 dB` or `-inf`. A fader you've clicked or tabbed to also takes ↑ and ↓ (with ⇧, 1 dB) and Return to type.
  - A level in grey is one the code leaves out: it plays at 0 dB, and the fader's first move writes it into the mixer, in the style the mixer already uses (`kick: kick.out` becomes `kick: { in: kick.out, db: db(-3) }`).
  - A pan reads "C", "L 20" or "R 35". In grey, it isn't in the code yet: the first drag writes `pan: 0` into the channel once it has moved 4 pixels, then goes on, and letting go writes the pan. Undo once puts it back at C; undo again takes it out of the code.
  - Click a strip to select its track. Right-click it to rename or recolour the track, or for **Edit in code**, which opens `song.ts` at its channel. Right-click a fader, or a pan once it's written, for **Automate** and **Reset to default** (a pan in grey has the strip's menu).
  - **Automate** on a fader gives it a lane under the Song in the timeline, "Hats volume". The fader then follows the lane: it moves with playback, shows the lane's dot, and can't be dragged. Right-click it for **Show Hats volume lane** or **Remove automation**, which writes the level at the playhead back into the code.
  - A track that doesn't reach the mixer gets a strip that says so, with **Edit in code**. So does a song that mixes its tracks its own way.
  - **Buses.** A bus is a track with no notes. It plays what other tracks send it. **Add bus** (after the strips) makes one:

    | Bus | What it is for | How a track uses it |
    |---|---|---|
    | **Reverb** | one shared room for every track | turn up the track's **Reverb** knob |
    | **Delay** | shared echoes | turn up the track's **Delay** knob |
    | **Group** | one fader for several tracks (all the drums) | set the track's output to it |

    It writes `tracks/<its key>.ts` with `bus(…)` and a channel in the mixer. A bus strip comes after the tracks, with a merge mark before its name. It has M, S and a meter, like a track.
  - **Sends.** Every strip has a row for each bus. Click **+** to send the track there, at −12 dB, after its fader. Then the row is a small knob: drag it, as a fader. Right-click it for **Before the fader** (the track's fader no longer turns it down; a dot on the knob), **Automate**, **Remove send** and **Edit in code**.
  - **Output.** Under each fader: where the strip goes, **Master** or a bus. Click it to pick. A choice that would make a loop (a bus feeding itself) is greyed, and says why.
  - Each of these is one undo step. In the timeline, a bus is a row marked **Bus**, with no lanes.
- **Code:** the selected track's TypeScript, with type checks. The list on the left shows the song's files. Right-click a track's file for **Change track ID…**: the ID is the name code and files use (`track("bass", …)`, `data/bass/`). Changing it moves both, and the track's view settings, as one undo step; the track's sounds restart. ⌘-click a name to jump to where it's defined, even inside std (std's files open read-only). ⌘S (or the Save button) saves. When the code has an error, the last working version keeps playing, and the status pill in the top bar says so; its popover offers **Show in code**. A song whose code doesn't compile when you open it still opens: in its code, at the error, with its tracks and clips greyed in the timeline and nothing to play until a save makes it compile, and then it plays.
- **Notes:** the selected clip in a piano roll; with none selected, it says how to pick one.
  - Pick a snap grid (1/4 to 1/64, triplets, or off): new notes are a grid step long. A click selects; a double-click adds a note; a drag across empty space selects with a box. In Draw mode (B, or the pencil) a click adds a note, sounding while you hold it, and dragging sets how long it is (moving up or down keeps its pitch); ⌥-drag paints a note in each grid step it crosses. Drag a note to move it, its right edge to resize it.
  - The strip at the bottom sets velocity (how hard each note is played).
  - Set the clip's length in bars in the dock's header.
  - While the clip's data file doesn't read (the status pill lists it), the piano roll is read only, with a note in the dock's header saying which file: it shows the notes the lane played before, and editing them would overwrite the file. Fix the file and it can be edited again.
  - ⌘A selects all notes, ⌘D duplicates the selection, the arrow keys move it (with ⇧, by an octave or a bar), and Delete removes it. ⌘= and ⌘− zoom the roll while it has the keyboard.
  - Right-click a note (or empty grid) for the note menu: up or down a semitone or an octave, **Quantize** (Q: onto the grid), **Legato** (each note lasts until the next starts), **Humanize** (timing and loudness a little off, so it sounds played), **Swing** (Light, Medium, Heavy: every second note a little later), Duplicate and Delete. It acts on the selected notes, or with none, on every note of the clip, as one undo step.
  - A drum kit's rows say their drum (Kick, Snare, Closed hat) instead of C2, and so do its notes. With the song's key set, the rows not in the key are shaded (never on a drum kit).

## Help

- **Help → Make Your First Song** opens a guide at the top of the browser: seven steps that tick themselves as you do them.
- Hold the mouse over any button, knob or fader: its tooltip says what it does, in plain words, with its key.
- A mistake in the code is said in plain words with a fix, pointing at your own line: "Cutoff wants a frequency (Hz), but gets a level (dB). Write it in Hz, such as hz(800)." The original message is under **Details**.
