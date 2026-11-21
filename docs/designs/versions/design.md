# Versions and the Playground

**In short**
- A notes clip can hold several **versions** of its notes. One plays; the others wait.
- The **Playground** is a panel for one clip: its bars loop, and you flip through its versions, star the ones you like, and keep one.
- Anyone can add a version: you, or an agent (Claude) through `m2 versions`. The app has no AI in it and calls none ([AGENT-9](../../architecture.md#agents)).
- The engine never learns of versions: it plays a clip's `notes`, as today.
- Status: accepted by the owner, 2026-11-21 ("go ahead and implement it… keep the core minimum and elegant"). Built in two tracks: `versions` (the format, the gesture, `m2 versions`, the docs), then `playground` (the panel).

## Why

A musician wants to try ideas for a few bars: "give me five bass lines, funkier". Today an agent can only replace the notes, one idea at a time, and each idea destroys the last. The musician can't compare.

Good tools solve this with alternatives that sit beside what plays: Logic's take folders, Ableton's take lanes. music2 already has it for audio (a take for each time round a loop, MON-11). Versions bring the same idea to notes, and make it the one way an agent proposes.

## The one new thing: a clip's versions

A notes clip gains one optional field, `versions`:

```json
{"id": "k3f9q2zt", "start": 32, "length": 16,
 "notes": [{"pitch": "A1", "start": 0, "length": 0.5}],
 "versions": [
   {"name": "original", "notes": [{"pitch": "E1", "start": 0, "length": 1}]},
   {"name": "walks up"},
   {"name": "busy", "star": true, "by": "Claude", "notes": [{"pitch": "A1", "start": 0, "length": 0.25}]}
 ]}
```

| Rule | Why |
|---|---|
| `notes` is what plays, as today | The engine, the roll, export and `m2 listen` change nothing |
| `versions` lists every version, in the order they were added | ← and → have a fixed order |
| Exactly one entry has no `notes`: it is the one that plays, and its notes are the clip's | The playing notes are written once, so two copies can never differ |
| Names are unique in a clip, 1 to 40 characters | A version is named by its name; no new kind of id |
| `star` (optional, `true`) marks one the musician likes | It tells an agent what to build on |
| `by` (optional) is who added it: an agent's name; none for the musician | The panel can say "Claude" beside it |
| A clip with no `versions` is today's clip, byte for byte | No conversion, and nothing else has to know |
| A list of one entry is never written: the field goes | One way to write each state |

A version's notes are a clip's notes: the same fields, times from the clip's start.

Only notes clips have versions. Automation and audio clips don't (audio has takes). Later, if wanted, the same field fits an automation clip's `points`.

A file with `versions` that breaks a rule (two names the same, none or two that play) is a lane file that doesn't read, said in words, as any other field that is wrong.

### The format's number

`versions` is a new optional field, and lane files are read strictly, so a music2 from before this refuses a song that has one. The builder follows the contract's rule for a new field (the format's number, or what the manifest says it needs), as `render` and `sung` did on audio clips.

## One gesture

One clip gesture, `versions`, on one clip, with one of these acts:

| Act | What it does |
|---|---|
| `add` | Adds a version with a name and notes. It waits; what plays doesn't change. On a clip with no versions yet, what plays is first named `original` |
| `play` | Makes the named version the one that plays: its notes become the clip's, and the notes that played go back under their name |
| `star` | Sets or clears a version's star |
| `rename` | Changes a version's name |
| `remove` | Removes a waiting version. The one that plays can't be removed: play another first |
| `keep` | Removes every version but the one that plays, and the field with them |

Each is one undo step, through the same path as every clip gesture (`POST /api/graph/clips`, `m2 clips . json -`), so AGENT-1 to AGENT-5 hold with nothing new: the app shows it land, the musician can undo it, and it is refused while the musician holds that lane.

**Choosing a version is a write**, one undo step, as choosing an audio take is. An earlier sketch made it "listening only", like solo. That needs a second way to play data that isn't the song's, and it loses the choice when the app closes. A write is the smaller design, and ⌘Z going back to the version heard before is what a musician expects (Logic and Ableton do the same).

`add` takes several versions in one step, so five ideas from an agent are one undo step and one notice.

### What the other gestures do with versions

| Gesture | Versions |
|---|---|
| Move, copy, duplicate, resize, sections | Go with the clip, untouched. A copy has them too |
| Split | Refused while the clip has versions that wait: "Keep one version first". Cutting six sets of notes at once would surprise |
| Join | The same |
| The piano roll's edits | Change what plays, and leave the waiting ones as they are |
| Recording into the clip (overdub) | The same |
| Delete | The clip goes, its versions with it; Undo brings them back |

Split and Join can be loosened later, if musicians ask.

## `m2 versions`

```
m2 versions <song>                              the clips that have versions, and each one's list
m2 versions <song> <clip> list                  one clip's versions: name, who, star, plays, how many notes
m2 versions <song> <clip> add <name> -          add one; its notes as JSON from standard input
m2 versions <song> <clip> play|star|unstar|remove <name>
m2 versions <song> <clip> rename <name> <new name>
m2 versions <song> <clip> keep
```

`<clip>` is `<track>/<lane>/<id>`, as the other commands name a clip. `list --json` prints the notes too. Several versions in one step: `m2 clips . json -` with the gesture.

## The Playground (track `playground`)

A panel in the dock for one notes clip. It is a view, like Notes: no new state in the song.

| Do | How |
|---|---|
| Open it | Select a notes clip, then **Open Playground** (the clip's menu, or ⌥P). With bars selected on a track instead, the clip is first cut at their edges (one undo step), so the Playground has a clip of just those bars |
| What opens | The loop is set to the clip's bars and turned on. The panel lists the versions; the roll above shows the one that plays, the others drawn faintly behind it |
| Flip | ← and →, or a click. The change is heard at once, as any edit |
| Star | S, or the star on a row |
| Make your own | **Duplicate** copies the one that plays under a new name; edit it in the roll |
| Rename, remove | On each row |
| Keep | **Keep this one** removes the others, and closes the Playground |
| Close | Esc. The versions stay with the clip. The loop goes back to what it was |

A clip with versions shows a small mark with their count on the timeline, so they are never hidden.

The page tells the app which clip the Playground is open on, in the focus record (AGENT-6): `playground: {track, lane, id, from, to}`. It is attention, not song: never written to a file.

## How an agent works with it

Nothing in the app is for agents alone. An agent uses what any tool would:

1. `m2 focus .` says where the musician is: `playground` when it is open, else the selected clips and the loop.
2. It reads the song for what fits there: `m2 key`, the tempo, the other tracks' notes over those beats (the files in `data/`), and `m2 versions . <clip> list --json` for what is there already and which are starred.
3. It adds versions, each with a short name that says what it is ("busy", "sparse", "walks up"), in one step.
4. It never plays, flips or keeps: the transport and the choice are the musician's (AGENT-11). It may `m2 point` at the clip and say what each version tries.
5. Asked for more, it reads the list again: the stars, what plays now, and any version the musician edited, and builds from those.

The song's `AGENTS.md` (`templates/song-agents.md`) and the guide get a short section saying this.

## What this leaves out

- Versions across several tracks at once (a bass and a drum part that go together). Later: a set that names one version on each clip.
- Versions of automation and of audio.
- Flipping on the loop's next round, not at once.
- A text box in the app for what the musician wants: the musician talks to the agent in the agent's own window.

## Tests (each first, and seen to fail)

- The format: a clip with versions reads and writes back byte for byte; each broken rule is refused in words; a clip without `versions` is unchanged.
- The gesture: every act, each one undo step; `play` there and back gives the first bytes; `keep` leaves today's clip; Split and Join refuse; a move, a copy and the roll's write keep the versions.
- The engine: a render of the song with waiting versions equals the render without them, bit for bit.
- An agent: `m2 versions add` with the app open lands as one step with the agent's name, and is refused (status 3) while the musician holds the lane.
- The panel (browser suite): open, flip, star, duplicate, keep, close; the focus record names the clip.
