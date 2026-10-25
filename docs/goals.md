# Goals

**In short**
- music2 is a music studio (a DAW) where a song is a small program: code says how it sounds, data files say what plays.
- It is made for people new to making music, and it must stay simple yet elegant enough for a producer.
- The goals, non-goals and principles below are the owner's decisions. A change that goes against one needs the owner's yes first, in an issue.
- Where this page and the contract ([architecture.md](architecture.md)) disagree, the contract wins, and this page should be fixed.

## Goals

| Goal | What it means in practice |
|---|---|
| **Beginner-first, in plain words** | The app, its messages and its docs speak plainly to someone who has never made a song. A mistake in the code is said with a fix, pointing at your own line. Music words are explained in [Music basics](music-basics.md). Short sentences and tables, in docs too. |
| **A song is a program** | A song is a folder: TypeScript says how it sounds, data files say what plays ([Song as a program](song-as-program.md)). You can diff it, review it, keep it in git and share it. The app, the `m2` command line, scripts, agents, text editors and git all write the same files (contract P2). |
| **Exact export** | An export renders from the first sample, the same way every time. A song made only of std's sounds renders the same bits on every computer and every supported OS ([PLAT-4](architecture.md#platforms)). |
| **Simple yet elegant** | A tasteful, professional DAW: "Logic's clarity, Live's speed, and nothing on screen that isn't the music or a control for it". The look is in the taste guide, [taste.md](designs/step8/taste.md). |
| **Real-time safety** | The sound never glitches because of the app. The threads that make sound never allocate memory, take a lock, touch the disk or wait for a compile ([RENDER-28](architecture.md#plans-data-and-arrangement-swaps)). Edits land while the song plays, without it stopping or jumping. The timing gates measure this on a quiet machine, and the reference Mac judges them ([Acceptance gates](architecture.md#acceptance-gates)). |
| **Desktop first** | A native desktop app. Linux and macOS (Apple silicon) now, Windows next ([PLAT-1](architecture.md#platforms)). |

## Non-goals

| Not planned | Why |
|---|---|
| **AI inside the app** | The app has no AI features. Scripts and AI agents can work on a song from the outside, through its files and the local API, like any other program. Contributors may use AI agents to write code: the same rules apply to their work. |
| **VST3 and AU plugins** | music2 plays CLAP plugins only, the open plugin format (gap #33, decided 2026-10-19). |
| **Backwards compatibility before 1.0** | Until 1.0, the song format, the SDK (`@m2/core`) and std can change in ways that need old songs fixed. The app reads only the current format and carries one std. It never changes an old song in silence: the song doesn't open, or opens silent, and a message says what to change ([FMT-3](architecture.md#manifest-and-versions)). One rule in the contract still asks for more: FMT-3a wants a migrator (`m2 migrate`) with any format or SDK break after format 1. Until the owner changes one of the two, the contract wins. |

## Principles

How the work is done. [CONTRIBUTING.md](../CONTRIBUTING.md) has the full rules.

- **The contract is the rulebook.** Every rule in [architecture.md](architecture.md) has an id, such as `RENDER-8`. A change that breaks a rule changes the contract first: in the same pull request as the code, in a commit ahead of it.
- **Files are the truth.** What is on disk decides what plays. The app never moves, deletes or repairs a file on its own; it offers a fix, and you click it (contract P2, P3).
- **Fix causes, never weaken tests.** Every fix comes with a test shown failing first.
- **One full check.** `ci/check.sh` is the full check, the same script on Linux and the Mac ([PLAT-6](architecture.md#platforms)). What CI runs, you can run with the same command.
- **Small steps.** A big change starts with a design note under [docs/designs/](designs/README.md); then it lands in small, green slices.

## Roadmap at a glance

The working list is in [HANDOFF.md](../HANDOFF.md) ("What comes next") and [gaps.md](gaps.md) (what is missing, most visible first).

| When | What |
|---|---|
| **Built** | Steps 1 to 9 of the [order of work](architecture.md#order-of-work): songs as code and data, the live engine on several cores, the timeline, piano roll, Device and Mix views, the code editor, automation, buses and sends, recording notes and audio, audio clips that follow the tempo, the Sampler, CLAP plugins in their own process, export (song, stems, MIDI), the `m2` command line and the local API. Agents can work beside you while the song is open: each change one undo step, `m2 focus`, `m2 watch`, `m2 point`, Compare (V) and `m2 listen`. |
| **Next** | 1. One more full check on the reference Mac. 2. **Windows**, as its own step. 3. Six open owner decisions (HANDOFF lists them). |
| **Then** | The most visible gaps: the rest of recording notes (#24) and audio (#25), the rest of audio clips (#18) and of the Sampler (#22), meters after the fader (#20), sections (#23), scale help (#16). |
| **Later** | Shared sound packages (#39), so sounds other people make can be used. A schema for each route of the API (`m2 routes` lists them, each with an example), and MCP (#38). The rest of [gaps.md](gaps.md). |
| **Not planned** | AI features in the app, VST3 and AU (above). |
