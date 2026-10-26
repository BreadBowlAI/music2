# Working on a music2 song

**In short**
- This folder is a song made in music2, a music studio where a song is a small program.
- You help from outside, with the `m2` command. The musician sees each change you make land in the
  app, and can take it back.
- For more, run `m2 docs`: the guide, built into m2, so it matches this version. Online:
  <https://breadbowlai.github.io/music2/agents.html>

## What is here

| Path | What it is |
|---|---|
| `song.ts`, `tracks/`, `lib/` | the code: how the song sounds (instruments, effects, the mix) |
| `data/` | what plays: notes, automation (knob moves over time) and audio clips |
| `assets/` | audio files. Never overwritten |
| `music2.toml` | the song's name, format and sample rate |
| `layout.json` | the tracks' order and colours, markers and the song's key. Never heard |
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
| `m2 add-track . <instrument>` | add a track (`synth.pluck`, `drums.kit`…) |
| `m2 add-effect . <call> <effect>` | add an effect (`std:delay`) after a sound |
| `m2 clips . <gesture> <clip>…` | make, move, split, join or delete clips |
| `m2 edit . <file> --old <text> --new <text>` | change text in one file of the song |
| `m2 write . <file> -` | write a whole file, from standard input |
| `m2 listen . --bars 9-16` | measure how it sounds: loudness, peaks, bands, masking |
| `m2 focus .` | what the musician looks at now |
| `m2 point . bars:9-16 "<words>"` | show the musician where to listen |
| `m2 undo .` | take back your own newest change |
| `m2 routes`, `m2 call . <path>` | every other change the app can make |

`m2 docs commands` lists every command.

## Rules

1. Every command that writes ends with `--as <your name> --why "<a few words>"`.
2. While the app has the song open, change files only with `m2`, never directly.
3. Run `m2 check .` after every edit, and fix what it says.
4. You can't hear. `m2 listen` measures: say its numbers as numbers, then ask the musician to listen.
   `m2 point` shows them where; V in the app plays before and after your change.
5. Explain each change to the musician in plain words: they may be new to music.
6. Exit status 3 means the musician is busy with that part: wait, then try again.
7. Playing, recording, mute and solo while listening, and the view are the musician's. Ask.
