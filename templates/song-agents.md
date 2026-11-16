# Working on a music2 song

**In short**
- This folder is a song made in music2, a music studio where a song is a small program.
- You help from outside, with the `m2` command. The musician sees each change you make land in the
  app, and can take it back.
- For more, run `m2 docs`: the guide, built into m2, so it matches this version. Online:
  <https://breadbowlai.github.io/music2/agents.html>
- This page was written when the song was made. `m2 docs agents` prints the one for the m2 you have.

## What is here

| Path | What it is |
|---|---|
| `song.ts`, `tracks/`, `lib/` | the code: how the song sounds (instruments, effects, the mix) |
| `data/` | what plays: notes, automation (knob moves over time) and audio clips; `data/arrangement.json` holds the tempo, the time signature and the song's key |
| `assets/` | audio files. Never overwritten |
| `music2.toml` | the song's name, format, sample rate, and the packs its code uses (`[dependencies]`) |
| `music2.lock` | the exact version of each pack the song uses, made by music2. Never edit it by hand |
| `layout.json` | the tracks' order and colours, and markers. Never heard |
| `.music2/` | this computer's own state. Never touch it |
| `AGENTS.md`, `CLAUDE.md` | this page, for agents. The app ignores both |

## Finding m2

m2 for this song: `music2.app/Contents/MacOS/m2` in the Mac app, `/usr/bin/m2` from the Linux `.deb`, `bin/m2` in the `.tar.gz` of m2 beside the AppImage, or `target/release/m2` in music2's source folder

If `m2` alone isn't found, use that whole path. If it doesn't run, ask the musician where music2 is.
Below, `.` is this song's folder.

## The commands you'll use most

| Command | What it does |
|---|---|
| `m2 check .` | build the song, and say what is wrong |
| `m2 controls .` | every knob, with its id |
| `m2 set . <id> <value>` | turn one knob |
| `m2 tempo . 80`, `m2 key . A minor` | set the tempo, or the song's key (`m2 key . none` takes it out) |
| `m2 search <words>` | find instruments, sounds, effects and samples, each with the id that adds it |
| `m2 add-track . <instrument>` | add a track (`synth.pluck`, `drums.drum`…) |
| `m2 add-kit . "808 kit"` | add a drum kit: a Drums bus and a Drum track for each drum (`808 kit`, `909 kit`, `Basic kit`) |
| `m2 add-effect . <call> <effect>` | add an effect (`@m2/effects:delay`) after a sound, or a note effect (`@m2/notes:arp`) before an instrument |
| `m2 clips . <gesture> <clip>…` | make, move, split, join or delete clips |
| `m2 clips . json -` | any clip gesture, as JSON: a clip's notes (`notes`), a chord on the song's key (`stamp`), a feel (`groove`), a drum's steps (`hits`)… Each, with an example: `m2 docs api`, "Clip gestures as JSON" |
| `m2 sections . duplicate 9-16 --to 25` | copy, move, empty, insert or delete whole bars on every track |
| `m2 edit . <file> --old <text> --new <text>` | change text in one file of the song |
| `m2 write . <file> -` | write a whole file, from standard input |
| `m2 listen . --bars 9-16` | measure how it sounds: loudness, peaks, bands, masking |
| `m2 render . ../mix.wav` | the song as a WAV. Write it outside this folder: a WAV in here isn't part of the song |
| `m2 undo .` | take back your own newest change, with the app open or closed |
| `m2 focus .` ¹ | what the musician looks at now |
| `m2 point . bars:9-16 "<words>"` ¹ | show the musician where to listen |
| `m2 call . <path>` ¹ | every other change the app can make (`m2 routes` lists them) |

¹ Only while the app has this song open, as `m2 watch`; else they say "music2 doesn't have this song
open". With the app closed, tell the musician in words where to listen, and use the other commands.

`m2 docs commands` lists every command; `m2 <command> --help` says how to call one. Buses and sends
are written in the code: `m2 docs code`. Example songs to read: `m2 new <folder> --example first-song`
(or `sounds`, `arp`, `sampler`, `resample`, `with-plugins`).

A note effect (`@m2/notes`: the Arpeggiator, Chord, Note echo, Lock to key) goes before an instrument and
changes its notes as they play: `synth.pluck(arp(lane.notes("chords")))`. Its notes never show in
the roll. A part the musician should see and edit is notes written into a clip (`m2 clips`).

## Rules

1. Every command that writes ends with `--as <your name> --why "<a few words>"`.
2. Change the song's files only with `m2`, never directly: then each change is an undo step, the app
   open or closed, and the app can refuse one while the musician is busy.
3. Run `m2 check .` after every edit, and fix what it says.
4. You can't hear. `m2 listen` measures: say its numbers as numbers, then ask the musician to listen.
   `m2 point` shows them where (with the app open); V in the app plays before and after your change.
5. Explain each change to the musician in plain words: they may be new to music.
6. Exit status 3 means the musician is busy with that part: wait, then try again. Exit status 2 means
   `m2` didn't take a word: it says which, and how to call the command.
7. Playing, recording, mute and solo while listening, and the view are the musician's. Ask.
