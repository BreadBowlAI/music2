# New songs, the songs folder and export

**In short**
- New songs go in `~/Music/music2`, each in its own folder. Nothing is ever replaced.
- Save As makes a copy; Settings picks the sound output, the input and where songs go.
- Export writes the song as a WAV file, each track as stems, or the notes as MIDI.

- **New Song…** (the start screen, or File → New Song…, ⌘N, even while a song is open) asks for a name, a tempo (the template's until you type one; Tab moves to it) and how the song starts:
  - **Starter beat**: six tracks (kick, clap, hats, bass, chords and a hook), 8 bars at 116 BPM, ready to play and change.
  - **Empty**: no tracks yet, so it plays silence. Its timeline offers **Add a track**. `song.ts` also has a drum track ready, switched off: delete the `//` in front of its two lines and save, and a Drums lane appears.
- New songs go in the **songs folder**, `~/Music/music2`, each in a folder named after the song. If the name is taken, the new one gets a number ("My song 2"): nothing is ever replaced. `Exports` there is taken by exports, so a song called "Exports" becomes "Exports 2". In the app, **Choose Folder…** in the New Song dialog saves it somewhere else.
- The start screen lists the songs you opened last, newest first, with their tempo and when you opened them; so does File → Open Recent. A click opens one. A song whose folder was moved or deleted says so, with × to take it off the list. The song that was open when you quit opens again at the next launch (not after Close Song, ⌘W).
- **Save As…** (File → Save As…, ⇧⌘S) saves a copy of the song under a new name and opens the copy; the song itself stays as it was. The copy has the song's files, but not its `.music2/` (mutes, view settings) or its undo history: the copy starts with none. **Choose Folder…** puts it somewhere else (not inside the song's own folder).
- **Settings** (⌘, on a Mac, Ctrl+, elsewhere; music2 → Settings… on a Mac, File → Settings… elsewhere) chooses the sound output and its buffer size, the sound input and **Hear yourself while recording**, which MIDI keyboards play (untick one and it plays and records nothing), shows what plays now, and sets where new songs go (and their exports). A folder reached through a link is shown at its real place, as the songs made in it are. They are kept for this computer in `~/.music2/settings.json`, and apply at once: a change of output stops playback where it was. A song opens with no sound device too: it plays silently, and the status pill says **No sound output**. Plug one in, and within a few seconds the song moves to it and stops.
- **Examples** on the start screen lists the examples. Picking one copies it into the songs folder and opens the copy. An example that needs plugins you don't have (With plugins) says which, and nothing is copied.
- **Export** (the button in the top bar, or File → Export…, ⇧⌘E) asks what to make:
  - **Song**: the whole song as one WAV file (below).
  - **Stems**: each track as its own WAV file, before its volume and pan, in a new folder. Not yet for a song that plays plugins.
  - **MIDI**: the notes, as a MIDI file other music apps open, with a fitting General MIDI sound for each part and drums on channel 10. A notes clip's menu has **Export as MIDI…** too. It saves anywhere but in the song's own folder, which keeps the song's files only.
- **Song** writes a WAV file: 24-bit, stereo, at the song's sample rate (48 kHz unless its `music2.toml` says otherwise). It starts at bar 1 and goes to the end of the last clip, then on until the sound has died away (silent for half a second, at most 10 seconds more). A song longer than 3 hours (most likely a clip far past the rest) isn't exported: the message says in which bar the last clip ends.
  - The app asks where to save it, starting at `~/Music/music2/Exports/<song>.wav`. Opened in a browser, it saves there straight away, adding " 2", " 3"… rather than replacing a file, even one that appeared while it was exporting.
  - The button shows how far along it is, and you can keep playing and editing meanwhile. It exports the song as it was when you clicked. If you close the song or open another, the export carries on, and a message in the corner shows how far along it is.
  - A song with plugins pauses while it exports, as a tape would: the export borrows the plugins that play, so they don't need a second copy. The top bar says **Paused for export**. Afterwards play goes on where it paused, and a note held across the pause starts again. Edits made meanwhile are heard once the export is done.
  - If any part breaks while it exports (a sound, a plugin that crashes or stays stuck for 30 seconds, a file that can't be read, a stretch), the export fails and names the part and its track. No file is written with a part silent. Playback goes on.
  - Quitting while a song is being exported asks first: quitting stops the export, and no file is saved.
  - The file is written under a hidden name and renamed when it's complete. A hidden file left by an export that was cut off (the app was killed) is removed by the next export into that folder.
  - When it's done, a message gives the file's name and length, with **Show in Finder** (**Show in Folder** on Linux; in a browser, it says which folder).
  - `m2 render` writes the same file.
