# Samples

**In short**
- A **sample** is a short recording you drop into a song: a **one-shot** (one hit: a kick, a clap, a chord stab) or a **loop** (a few bars that repeat, played in time with the song).
- Find them with **⌘F** like any sound: type `kick`, `loop 90 bpm` or `Am`. **Space** plays the file itself. **Enter** adds it at the playhead.
- They come from music2's own small drum kit, from the packs you installed, and from your own folders, which you add once.
- Adding one copies the file into your song, with a note of where it came from, so the song keeps playing anywhere and keeps the credit.

## Where samples come from

| From | What | Shows as |
|---|---|---|
| music2 | a small drum kit: kick, snare, clap, closed and open hat, three toms, rim, cowbell, crash, ride, shaker, tambourine. music2 made them itself from its own drum machine, so they are music2's, free to use in anything | music2 |
| A pack | the samples its author listed in the pack's `samples.toml` (see [Making a pack](making-a-pack.md)) | the pack's name |
| Your folders | every WAV file in a folder you add, and in the folders inside it | Yours |

**Your folders.** In ⌘F's panel, click **Your folders**, then **Add a Folder…**, and pick a folder of samples on your computer. music2 looks through it in the background, so the panel never waits: the folder's line says "looking…", and the results fill in when it is done. The first time a big folder takes a while; after that it opens at once (music2 keeps what it measured in its cache folder). Nothing is copied until you add a sample to a song. **Remove** (the × beside a folder) takes it out of the list and stops looking through it; its files stay where they are. music2 keeps the list with your other settings, on this computer only.

| A folder that… | What happens |
|---|---|
| is your whole disk (`/`) or your home folder itself | refused: pick the folder your samples are in, so music2 doesn't look through every file you have |
| is inside one of your folders, or holds one | refused: its samples are listed already, or would be twice |
| is on a drive that is unplugged | its line says "not there"; its samples are listed as they were, and nothing is measured again when it is back |

In the app, **Add a Folder…** opens the folder picker. In a web browser (the app's page opened from a browser), it asks for the folder's whole path instead, typed in a small box: a browser can't give a page a folder's path.

**WAV only for now.** music2 reads WAV files. A folder's FLAC, MP3, AIFF or OGG files are skipped, and the folder's line says how many ("3 skipped: WAV only for now"). A WAV file music2 can't play (a kind of WAV it doesn't decode, a broken file) is left out too, and the line says how many ("2 don't read").

## What a sample row says

Each row says what the sample is, then where it is from: `Loop · 90 bpm · Am · 10.7 s · music2`.

| Word | Meaning |
|---|---|
| Loop / One-shot | a loop repeats in time with the song; a one-shot plays once |
| `90 bpm` | a loop's tempo: beats per minute |
| `Am`, `F#` | its key, if it has one: the notes it fits with (`Am` is A minor) |
| `10.7 s` | how long it is |

What music2 can measure (how long, how many channels, how loud) it reads from the file. The rest comes from the pack's own list if it has one, or is **guessed** from the file's name, its folders and its length: `dusty keys 90 Am.wav` in a `loops` folder is a loop of keys at 90 bpm in A minor; `Kicks/Kick 01.wav` is a kick one-shot. A guess can be wrong; a pack's author fixes it in the pack's `samples.toml` (`key = "none"` takes back a key guessed wrong).

| A file is a… | When |
|---|---|
| loop | its name or a folder says `loop` or `loops`; or its own name has a tempo (`Keys 90 Am`, `Fill 120 bpm`) and it lasts at least a bar at that tempo |
| one-shot | its name or a folder says `one-shot`, `hit` or `shot` (`Drum Hits/`); or nothing above says loop |

The nearest says it: a file named `Fx Loop 1` in a `One Shots` folder is a loop. A tempo on a folder alone (a construction kit's `Trap Kit 140 BPM/Kicks/Kick 1.wav`) gives a loop under it its tempo, but never makes a kick a loop. Numbers with a unit (`96k`, `24bit`, `100 Hz`, `90s`) are no tempo, and words like `I Am` are no key.

## Finding the right one

Type words as for any sound. These words are filters:

| Type, or click | Finds |
|---|---|
| `loop` or **Loops** | loops only |
| `one-shot` or **One-shots** | one-shots only |
| `kick`, `snare`, `hat`, `keys`, `vocal`… | samples of that instrument (and instruments with the word in their name) |
| `90 bpm` | samples within 2 bpm of 90 |
| `Am`, `C major`, `F#` | samples in that key, or in its **relative** key: `Am` also finds `C`, and `C` finds `Am` (they use the same notes) |
| **Fits my song** | loops near your song's tempo where the playhead is (within 8%), in your song's key or its relative. A loop with no key, such as a drum loop, fits any key. Without a key set for the song (the key in the top bar), tempo alone |

The instrument words: kick, snare, clap, hat, cymbal, tom, perc (shakers, cowbells, congas and the like), bass, keys, guitar, vocal, strings, brass, pad, lead, synth, fx, texture, foley.

## Hearing it

Move to a row with ↓ or ↑ (or click it) and press **Space**: you hear the sample's own file, not a demo. Space again, or moving, stops it. A file over 64 MB (about six minutes) has no preview: add it to the song to hear it.

## Adding it

| Do | What happens |
|---|---|
| **Enter** | adds it at the bar where the playhead is: on the selected track if it plays audio, else on a new audio track after it |
| Drag the row onto the timeline | adds it where you drop it: on an audio track, or on a new audio track below the others |
| Drag it onto a Sampler's card (in the Device view) | the Sampler plays it: a one-shot to play from notes |

Every way is **one undo step**: ⌘Z takes the clip (and a new track) back. The file stays in your song's `assets` folder, as an imported file does.

**Loops play in time at once.** A loop whose tempo music2 knows gets **Follow tempo** turned on: its 90 bpm plays at your song's 120, and a loop at your song's own tempo stays in time if you change the tempo later. Its length is its own exact length in beats, so a loop with a reverb tail at its end still lands every beat on yours. The toast says so. You can turn Follow tempo off in the clip's menu.

## Credits and notes

Adding a sample copies its file into your song's `assets` folder, and writes a small note beside it, `assets/kick.wav.json`:

```json
{"from": {"pack": "@alice/lofi-keys", "version": "1.2.0", "file": "samples/loops/dusty keys 90 Am.wav", "author": "Alice Example", "license": "CC-BY-4.0"}}
```

The note keeps who made it and its licence, which some licences ask you to credit when you share a song. A sample from your own folder notes only `{"from": {"folder": true}}`: never its path, which would show the folders on your computer to anyone you share the song with. If you add a sample from your folder, then the same file from a pack (a copy of a pack's sample kept in your folder), the pack's note replaces yours, so its credit is kept.

## From the terminal

| Command | Does |
|---|---|
| `m2 search kick --kind sample` | lists samples, with their ids |
| `m2 add-sample <song> sample:std:samples/kit/kick.wav --at 5` | adds that sample at bar 5, on a new audio track (`--track ID` puts it on that track's audio lane) |

See [m2 commands](commands.md). With the app open, `m2 add-sample` asks the app, as one undo step there. With it closed, it finds the sample as your song's own build lists it: a pack your song takes from a folder, and a pack at the version your song's lock holds. `m2 search` with the app open waits while the app is still looking through samples (just started, or a folder just added), up to 30 seconds, so a script gets what it finds.
