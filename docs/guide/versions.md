# Versions: try ideas in one clip

**In short**
- A notes clip can hold several **versions** of its notes: one bass line that is busy, one that is sparse, one that walks up.
- One version **plays**. Its notes are the clip's notes: what you hear, see in the piano roll, and export. The others **wait** in the clip, and are never heard.
- You pick which one plays, put a star on the ones you like, and **keep** one when you have chosen. Each of these is one Undo step.
- Today you do this with `m2 versions` in a terminal, and an agent proposes ideas the same way. A panel in the app for flipping between versions while the loop plays is not built yet.

An idea you ask for no longer has to replace the last one. Ask an agent for three bass lines for the chorus, and you get three versions in the bass clip. Nothing you hear changes until you choose.

## The words

| Word | Meaning |
|---|---|
| Version | one set of notes for the clip, with a short name (`busy`) |
| Plays | the version whose notes are the clip's notes now. Exactly one plays |
| Waits | a version kept in the clip that isn't playing. It costs nothing: it is never heard or exported |
| Star | a mark on a version you like, so you (and an agent) can find it again |
| Keep | remove every version but the one that plays. The clip is then a plain clip again |
| `original` | the name the clip's own notes get when the first versions are added |

## Look, pick and keep

A clip is named as `m2 clips` names it: `track/lane:id` (`bass/notes:k3f9q2zt`), or `track/lane@place` for the clip that starts there (`bass/notes@9` starts at bar 9). `.` is the song's folder.

| Do | Command |
|---|---|
| See which clips have versions | `m2 versions . list` |
| See one clip's versions | `m2 versions . list bass/notes:k3f9q2zt` (`--json` shows their notes too) |
| Add a version, which waits | `m2 versions . add bass/notes:k3f9q2zt busy notes.json` (or `-`: the notes from standard input) |
| Make a version the one that plays | `m2 versions . play bass/notes:k3f9q2zt busy` |
| Star a version, or take the star off | `m2 versions . star bass/notes:k3f9q2zt busy`, `unstar` |
| Give a version another name | `m2 versions . rename bass/notes:k3f9q2zt busy "busy 2"` |
| Remove a version that waits | `m2 versions . remove bass/notes:k3f9q2zt sparse` |
| Keep the one that plays, and remove the others | `m2 versions . keep bass/notes:k3f9q2zt` |

The list shows each version, the one that plays, a `*` for a star, and who added it:

```text
bass/notes:k3f9q2zt: 3 versions, `original` plays
    original  plays  48 notes
  * busy      waits  64 notes  by Claude
    sparse    waits  12 notes  by Claude
```

The notes of a version are a list, as a lane's file writes them. Times are beats from the clip's start:

```json
[{"pitch": "A1", "start": 0, "length": 0.5}, {"pitch": "C2", "start": 1.5, "length": 0.5}]
```

With the app open, each command lands in the app as one Undo step, as every `m2` command does ([How your changes reach the song](changes.md)). ⌘Z takes it back.

## What happens to versions when you edit

| You | The versions |
|---|---|
| Edit notes in the piano roll, quantize, add a chord, record into the clip | Only the version that plays changes. The ones that wait stay as they are |
| Move, copy, duplicate or paste the clip, or copy its section | They go with the clip. A copy has them too |
| Drag the clip's right edge | They stay as they are |
| Drag the clip's left edge | Every version's notes stay where they would sound |
| Split or join the clip, or cut it with a section edit | Refused while versions wait: "Keep one version first" |
| Delete the clip | They go with it. Undo brings them back |

## For agents: proposing ideas

Asked for ideas for a part ("three bass lines for the chorus"), don't write over the clip's notes. Add each idea as a version: it waits in the clip, never heard, until the musician picks it.

1. Find the clip: `m2 focus .` says what the musician has selected, with the app open; else ask, or read the lane's file in `data/`.
2. Read what fits there: `m2 key .`, the tempo, the other tracks' notes over the same beats, and `m2 versions . list <clip> --json`: the versions already there, their notes, which one plays, and which have a star.
3. Add each idea with a short name that says what it is (`busy`, `sparse`, `walks up`): `m2 versions . add <clip> <name> -`, its notes on standard input. Several in one undo step: `m2 clips . json -` with the `versions` gesture ([Clip gestures as JSON](api.md#clip-gestures-as-json)).
4. Leave `play` and `keep` to the musician, unless they ask you: which version plays is their choice. Tell them what each version tries, and `m2 point` at the clip.
5. Asked for more, list again first: build on the starred ones and the one that plays, which the musician may have edited.

## Rules for names

A name is 1 to 40 characters, with no space at its start or end. Two versions of one clip can't have the same name.

## Not there yet

- A panel in the app to flip between versions while the loop plays.
- Versions that go together on several tracks (a bass line and a drum part).
- Versions of automation and of audio clips.
