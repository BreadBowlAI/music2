# Welcome to music2

**In short**
- music2 is a music studio for Linux and the Mac: a DAW (digital audio workstation), the kind of app
  people make songs in, like GarageBand or Logic Pro.
- In music2 a song is a folder, and the folder is a small program. Code says how the song sounds.
  Data files say what plays: the notes, the automation (knob moves over time) and the audio.
- This guide has four parts. Pick the one for you in the table below.

![The music2 window: six tracks on the timeline, and the Bass clip open in the piano roll below](images/screenshot.png)

*The Starter beat, with its Bass clip open in the piano roll (Linux).*

The app shows the song's folder as a timeline, a piano roll, knobs and cables, and a code editor.
Every change you make there is written into the files at once. music2 is made for people new to
making music, and it aims to stay simple yet elegant enough for a producer.

There's no AI inside the app. Scripts and AI agents work on a song from outside: through its files,
the `m2` command and a local API (a way for programs on your computer to talk to the app). While
the song is open, you see each change an agent makes land, and you can take it back.

## The four parts

| Part | Who it is for | Start with |
|---|---|---|
| Getting started | you, if you are new to music2, or to making music | [Install and run music2](install.md), then [Your first song](first-song.md) |
| The manual | musicians: everything the app does | [What you can do](what-you-can-do.md) |
| Working on a song as an agent | AI agents, and the scripts people write | [Working on a music2 song](agents.md) |
| Reference | agents and scripts: every command and every route | [m2 commands](commands.md), [API routes](routes.md) |

## This guide in the terminal

`m2 docs` prints this guide. It is built into `m2`, so it matches that version, and it works with
no network. `m2 docs <page>` prints one page: a link to `window.md` is `m2 docs window`.
`m2 docs all` prints every page, so `m2 docs all | grep -n automate` finds a word.

## Help build music2

music2 is open source. To build it, run its checks and send a change, read
[CONTRIBUTING.md](https://github.com/BreadBowlAI/music2/blob/main/CONTRIBUTING.md). The rules every
part is built to are in [the contract](https://github.com/BreadBowlAI/music2/blob/main/docs/architecture.md).
What music2 can't do yet is in [the gap list](https://github.com/BreadBowlAI/music2/blob/main/docs/gaps.md),
and where it is going in [the goals](https://github.com/BreadBowlAI/music2/blob/main/docs/goals.md).
