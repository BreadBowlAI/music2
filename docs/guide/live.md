# Playing live: an agent plays a track

**In short**
- An agent can play a track **live**: it writes the track's notes a few bars **ahead of the playhead**, and music2 plays them as the song plays, the way a video player buffers ahead.
- That stream of notes is a **feed**. music2 plays it, says when it runs low, and refuses notes that come too late. The agent decides what to play: music2 has no AI in it.
- The track shows a **Live** mark with the agent's name. Mute, solo and the fader work on it as on any track. Nothing sounds until you press Play.
- A feed is never written to the song, unless you **keep what played**: then it is one change, which ⌘Z takes back.

## The words

| Word | Meaning |
|---|---|
| Feed | one track's notes, written live by an agent. It is named by its track (`bass`) |
| Playhead | where the song is playing, in beats |
| Lock | the first beat a write can still change: the playhead plus about 50 ms. Notes before it are on their way to the sound and are final |
| Written to | the end of what the feed holds, from the playhead on |
| Buffer | written to minus the playhead: how much is written ahead, in beats, bars and seconds |
| Low mark | a buffer size in bars (4 unless the agent says). Under it, music2 says `low` |
| Empty | the playhead reached written to: the track rests until more comes |

## For the musician

| Do | How |
|---|---|
| Let an agent play a track | the track's **⋯** menu: **Let an agent play this track**. Then ask your agent to play it |
| See who plays it | the **Live** mark on the track's header: "Live · Claude". "Live" alone: no agent has written yet |
| Hear it | press Play, as always. The feed plays with the track's own clips, through its sound and effects |
| Silence it | M on the track, or solo another, or the fader. Or close it |
| Keep a good take | click the Live mark: **Keep what played**. It goes into the song as a new clip (or as a version of the clip under it, when that clip has versions). One change: ⌘Z takes it back |
| Stop it | click the Live mark: **Close**. Its notes stop at once. What played can still be kept until the song closes |

Play, Stop, the loop and seeking stay yours: an agent can't press them. A feed over a loop plays the loop again each time round, so an agent can change what comes next time while you listen.

## For an agent: the loop

Every form needs the app open with the song: a feed is playback. `.` is the song's folder.

```bash
m2 feed . open bass --low 4 --as Claude                  # the musician sees "Live · Claude"
# (the musician presses Play)
m2 feed . wait bass --for low --timeout 60               # prints the feed's state as JSON when it runs low
m2 feed . put bass 64 80 next.json --as Claude           # the notes from beat 64 to 80
m2 feed . wait bass --for low --timeout 60
cat more.json | m2 feed . put bass 80 96 - --as Claude  # notes from standard input
m2 feed . keep bass --as Claude                          # only when the musician asks for it
m2 feed . close bass --as Claude
```

| Command | What it does |
|---|---|
| `m2 feed . open <track> [--low <bars>]` | open a feed on the track's notes lane |
| `m2 feed . put <track> <from> <to> <notes.json \| ->` | the notes from beat `<from>` to beat `<to>` are these. It replaces what the feed held there past the lock. Prints the answer as JSON: `taken`, `late`, `lock`, `written_to`, `playhead` |
| `m2 feed . status [--json]` | every open feed: the playhead, written to, the buffer, empties, late notes |
| `m2 feed . wait [<track>] [--for low\|empty\|jump] --timeout <seconds>` | wait for the buffer to run low (`low` takes `empty` too), to run out, or for a jump (Stop, a seek, the loop going round). Prints the event, exits 0. At once when the feed is already low. The time limit is required, at most 86400 seconds (a day); past it, it exits 1 |
| `m2 feed . keep <track> [--into clip\|version]` | write what played into the song, one undo step: a new clip, or a version of the clip under it |
| `m2 feed . close <track>` | end the feed: its notes stop at once |

A note is a lane file's note, but its `start` is in **song beats**, not beats from a clip's start:

```json
[{"pitch": "A1", "start": 64, "length": 0.5, "velocity": 0.9},
 {"pitch": "E2", "start": 65, "length": 0.5}]
```

Each note starts inside `[from, to)`. A put that writes nothing (`[]`) makes those beats a rest. A feed holds at most 4,096 notes, and plays at most 128 at once and 64 starting in the same 128 samples (about 3 ms): a put past either is refused, and changes nothing.

### What a put does

| Case | What happens |
|---|---|
| `from` is at or after the lock | taken whole |
| `from` is before the lock | the part from the lock on is taken. Notes that start before the lock are left out and counted in `late`. A note already sounding is never cut |
| `to` is more than 64 bars past the playhead | refused: a feed is a buffer, not a song. Write a few bars, and more as they play |
| The song isn't playing | taken whole: the feed waits for Play |

Notes that ended more than 16 bars behind the playhead leave the feed (outside the loop), so a long jam stays small. They are still in what played, for Keep.

### Plan for the delay

You are slower than the music. A bar at 120 BPM is 2 seconds; a model may take 5 to 20 seconds to answer.

| Rule | Why |
|---|---|
| Write ahead: keep the buffer above the low mark | a write that arrives after its beat is late, and left out |
| Start from `written_to` | the event and every answer say it: write the next bars from there |
| Rewrite only past the lock | before it, notes are final. Rewriting later bars is fine: it is the same act as writing ahead |
| Set the low mark to your answer time | at 120 BPM, 4 bars is 8 seconds. If you take 15 seconds, open with `--low 8` |
| Re-plan after `jump` | the musician stopped, sought or the loop went round: the playhead moved |
| When `wait` exits 1, wait again | its time limit passed with nothing to write: the buffer is full enough |
| On a loop, write inside it | the low mark is at most the loop's length, so once the loop is all written `wait --for low` waits. `written_to` is past the loop's end then, where nothing plays until the loop is off: rewrite bars inside the loop ahead of the playhead instead, and `wait --for jump` to hear each time round |

## Events

Each feed's state is an event on the app's socket (`m2 watch`, `GET /api/ws`), and `GET /api/feed` lists every open feed the same way:

```json
{"t": "feed", "feed": "bass", "why": "low", "playing": true, "playhead": 51.9, "lock": 52.0,
 "written_to": 64, "buffer": {"beats": 12.1, "bars": 3.0, "seconds": 5.9},
 "low_bars": 4, "low_mark": {"beats": 16, "bars": 4}, "empties": 0, "late": 0, "writer": "Claude"}
```

| `why` | When |
|---|---|
| `low` | the buffer dropped under `low_mark` while playing: once, until it is above it again. `low_mark` is `low_bars`, but never more than the loop's length while the loop is on |
| `empty` | the playhead reached written to while playing: once, and counted in `empties` |
| `jump` | Stop, a seek, or the loop going round |
| `open`, `closed` | the feed opened or closed |
| none | the state, at most every quarter second while playing |

The routes, open to agents: `POST /api/feed/open`, `POST /api/feed/put`, `GET /api/feed`, `POST /api/feed/keep` and `POST /api/feed/close` ([API routes](routes.md)).
