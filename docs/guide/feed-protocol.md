# The feed protocol, feed/1

**In short**
- `feed/1` is the set of messages an agent uses to play a track live: four acts (**open**, **put**, **status**, **close**) and one event, the feed's state.
- The app speaks it over HTTP (`/api/feed/…`, [Playing live](live.md)). A web page that plays a song with `@music2/web` will speak it through its own relay. The messages are the same; only the way they travel differs.
- This page is for anyone who writes a player or an agent: each body, each answer field with its meaning, the timing rules, the limits, and each refusal's exact words.
- A fixture checks it: `crates/server/tests/fixtures/feed-protocol.json` lists the acts and what each answer must hold, and a test runs it against the app's routes. A change to a body, an answer or a refusal's words is a change to `feed/1`, said here.

## The words

| Word | Meaning |
|---|---|
| Feed | one track's notes, written live, named by its track (`bass`) |
| Act | one message to the player, with its body; the player answers it |
| Beat | song beats: beat 0 is the song's start. Every time in `feed/1` is in song beats |
| Playhead | where the song is playing, in beats |
| Lock | the first beat a put can still change. Before it, notes are on their way to the sound |
| Written to | the end of what the feed holds, from the playhead on |
| Buffer | written to minus the playhead: how much is written ahead |
| Low mark | a buffer size in bars. Under it, the player says `low` |
| Envelope | what the app adds around a message: `session`, which open song it is for. It isn't part of `feed/1` |
| Relay | a page's own code that passes messages between its agent and the player |

## The acts

| Act | Body | Answer | In the app |
|---|---|---|---|
| `open` | `{"track": "bass", "lane"?: "notes", "low_bars"?: 4, "as"?: "Claude"}` | the feed's state, with `"why": "open"` | `POST /api/feed/open` |
| `put` | `{"feed": "bass", "from": 64, "to": 80, "notes": [ … ], "as"?: "Claude"}` | the put's answer (below) | `POST /api/feed/put` |
| `status` | `{}` | `{"feeds": [ … ]}`: every open feed's state | `GET /api/feed` |
| `close` | `{"feed": "bass", "as"?: "Claude"}` | the feed's state, with `"why": "closed"` | `POST /api/feed/close` |

A field with `?` may be left out.

| Body field | Meaning |
|---|---|
| `track` | the track to open a feed on: its key, as `m2 check` lists it. At most one feed a track |
| `lane` | which notes lane, when the track has more than one. Left out: its only one |
| `low_bars` | the low mark, in bars: above 0, at most 64. Left out: 4 |
| `as` | the agent's name, shown on the track's Live mark: the name of whoever opened it or put last. Send it on every put. Left out, the app uses the name in the request's `x-m2-as` header, else `agent` |
| `feed` | the feed, named by its track |
| `from`, `to` | the beats the put writes: from `from` up to, not including, `to`. `from` is 0 or more, `to` is after it |
| `notes` | the notes from `from` to `to`: a list, empty for a rest. Left out: empty |

Each note is a lane file's note, with `start` in **song beats**:

| Note field | Meaning |
|---|---|
| `pitch` | 0 to 127 (60 is middle C), or a name: `"A1"`, `"C#4"` |
| `start` | the beat it starts on, inside `[from, to)` |
| `length` | in beats, above 0 |
| `velocity` | how hard, 0 to 1. Left out: 0.8 |

In the app, each POST body also carries `session`, the open song's number from `GET /api/graph`. That is the app's envelope. A page's song is its own session, so a page's relay takes the body without it.

`feed/1` has no play, stop or seek. Those are the musician's ([AGENT-11](https://github.com/BreadBowlAI/music2/blob/main/docs/architecture.md#agents)). Keeping what played (`POST /api/feed/keep`, `m2 feed keep`) is the app's, not `feed/1`'s: it writes into the song as one undo step, and a page has no song files.

## The answers

### The feed's state

`open` and `close` answer it, `status` lists it, and the event is it ([below](#the-event)).

```json
{"t": "feed", "feed": "bass", "track": "bass", "lane": "notes", "writer": "Claude", "why": "low",
 "playing": true, "playhead": 51.9, "lock": 52.0, "written_to": 64,
 "buffer": {"beats": 12.1, "bars": 3.0, "seconds": 5.9},
 "low_bars": 4, "low_mark": {"beats": 16, "bars": 4},
 "notes": 9, "empties": 0, "late": 0, "heard": true}
```

| Field | Meaning |
|---|---|
| `t` | always `"feed"` |
| `feed` | the feed's name: its track |
| `track` | the track, the same as `feed` |
| `lane` | the notes lane it plays |
| `writer` | the agent that opened it or wrote to it last; `null` when the musician opened it and no agent wrote yet |
| `why` | why it is said: `open`, `closed`, `low`, `empty` or `jump`. Absent from `status` and from the state said every quarter second |
| `playing` | whether the song is playing |
| `playhead` | where it plays, in beats |
| `lock` | the first beat a put can still change |
| `written_to` | the end of the written stretch the playhead is in; the playhead itself when it is in none |
| `buffer.beats`, `buffer.bars`, `buffer.seconds` | how much is written ahead of the playhead. Inside a loop, what is written from the loop's start counts too, up to one time round |
| `low_bars` | the low mark the feed was opened with, in bars |
| `low_mark.beats`, `low_mark.bars` | the low mark the buffer is judged by: `low_bars`, but never more than the loop's length while the loop is on, so a loop all written is never low |
| `notes` | how many notes the feed holds |
| `empties` | how many times the playhead reached written to while playing |
| `late` | how many notes, in all, came after the lock and were left out |
| `heard` | whether its notes reach the sound now |
| `silent` | only when `heard` is false because the notes are too dense for the tempo: why, in words (below) |

In the app each state also carries `session`, the envelope.

### The put's answer

```json
{"ok": true, "taken": 1, "late": 1, "lock": 1.11, "written_to": 16, "playhead": 1.0, "heard": true}
```

| Field | Meaning |
|---|---|
| `ok` | always `true`: a put that fails is a refusal |
| `taken` | how many of its notes the feed took |
| `late` | how many it left out, as they start before the lock (or would replace a note that is sounding) |
| `lock` | the lock when the put landed |
| `written_to` | written to, after the put: write the next bars from here |
| `playhead` | the playhead when the put landed |
| `heard` | whether the feed's notes reach the sound now |

## The event

Each feed's state is also told as an event. In the app it comes on the socket, `/api/ws` (`m2 watch`); a page's relay passes each one on.

| `why` | When |
|---|---|
| `open` | the feed opened |
| `closed` | the feed closed |
| `low` | while playing, the buffer dropped under `low_mark`: once, until it is above it again |
| `empty` | while playing, the playhead reached written to: once, counted in `empties` |
| `jump` | the musician stopped, sought, or the loop went round |
| none | the state, at most every quarter second while playing |

The player looks at its feeds every 50 ms, so `low`, `empty` and `jump` come within about 50 ms of when they happen.

## The timing rules

| Rule | What it means |
|---|---|
| The lock | while playing, the lock is the playhead plus a margin: two of the sound card's buffers, but at least 50 ms. Stopped, it is the playhead |
| Before the lock | a put's note that starts before the lock is left out and counted in `late`. A note the feed holds there stays. A note already sounding is never cut |
| Inside a loop | the lock goes round the loop's end: notes just after the loop's start are late too. Notes before the playhead in the loop are written for the next time round |
| Past the lock | a put replaces every note the feed holds that starts in `[from, to)` past the lock |
| Stopped | a put is taken whole |
| How far ahead | `to` may be at most 64 bars past the playhead |
| How many | a feed holds at most 4,096 notes, plays at most 128 at once, and at most 64 starting within 128 samples (about 3 ms) |
| Behind | notes that ended more than 16 bars behind the playhead leave the feed, 4 bars at a time, except inside a loop that is on |
| Nothing half done | a refused put, open or close changes nothing |

A faster tempo can squeeze a feed's notes together past what it can play. Then it plays nothing, says `"heard": false` and `silent`, until a put makes it fit.

## The loop an agent runs

1. `open` the feed, with a low mark longer than your answer time (at 120 BPM a bar is 2 seconds).
2. Wait for the musician to press Play.
3. Wait for an event with `why` `low` (or `empty`).
4. `put` the next bars from `written_to`.
5. Go to 3. After a `jump`, read the playhead again before you write.
6. `close` it when you are done.

On a loop, write inside it: once the loop is all written, `low` doesn't come. Rewrite bars inside the loop ahead of the playhead, and wait for `jump` to hear each time round.

## Refusals

A refusal is answered `{"error": "<words>"}`, in the app with the status 400. Nothing changed. The words are these; `<…>` is filled in.

| Act | When | The words |
|---|---|---|
| `open` | the track is `song` (or a part shown with no track header) | `<track> isn't a track: a feed plays a track's notes, which shows Live on its header.` |
| `open` | no such track | ``The song has no track `<track>`: `m2 check` lists its tracks and their lanes.`` |
| `open` | it has no notes lane | `<track> plays no notes lane: a feed plays a track's notes.` |
| `open` | `lane` isn't one of its notes lanes | ``<track> has no notes lane `<lane>`: it has <lanes>.`` |
| `open` | it has several and no `lane` | ``<track> has more than one notes lane: say which, as `lane` (<lanes>).`` |
| `open` | a feed is open on it | `<track> already has a feed open: close it first, or write to it.` |
| `open` | `low_bars` is 0 or less, or more than 64 | `The low mark is a number of bars above 0, at most 64: not <low_bars>.` |
| `put` | no feed is open on it | ``<feed> has no feed open: open one first (`m2 feed open`).`` |
| `put` | `from` below 0, or `to` not after it | ``A put's `from` and `to` are beats, `from` 0 or more and `to` after it: not <from> to <to>.`` |
| `put` | `to` more than 64 bars ahead | ``A feed holds at most 64 bars past the playhead (beat <playhead, 2 decimals>): `to` is beat <to>, <bars, 1 decimal> bars past it. A feed is a buffer, not a song: write the next few bars, and more as they play.`` |
| `put` | a note starts outside `[from, to)` | `Each note starts inside the put's range, from beat <from> to before <to>: a note starts at <start>.` |
| `put` | a note's length is 0 or less | `A note's length is above 0 beats: the note at <start> is <length> long.` |
| `put` | a pitch outside 0 to 127 | `A note's pitch is 0 to 127: the note at <start> is <pitch>.` |
| `put` | a velocity outside 0 to 1 | `A note's velocity is 0 to 1: the note at <start> is <velocity>.` |
| `put` | the feed would hold more than 4,096 notes | `A feed holds at most 4,096 notes: this put would make it <count>. Write fewer, or let the bars behind play out first.` |
| `put` | too many at once | `That's more notes at once than a feed plays: at most 128 sounding together, and 64 starting within 128 samples. Write fewer.` |
| `close` | no feed is open on it | `<feed> has no feed open.` |
| any | the body doesn't read: a field missing, of the wrong type, or a pitch with no name | `The body doesn't read: <what>.`, such as ``The body doesn't read: missing field `to` at line 1 column 47.`` (`<what>` is the app's JSON reader's; a relay may say it in its own words) |
| any | no song is open | `no project is open` |

The app's envelope adds one: a `session` that isn't the open song's is refused with `the open song changed: read the state again (session <session> isn't open)`.

When a feed is too dense for the tempo, `silent` says: `too many notes at once at this tempo: a feed plays at most 128 sounding together and 64 starting within 128 samples. Write them again, fewer.`

Numbers in the words are written as the app writes them: `4` not `4.0`, `0.5`, `-1`.
