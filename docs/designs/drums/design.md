# Drums, the FL way: one drum per track, a kit is a group

| | |
|---|---|
| **Status** | Accepted. The owner asked for this note on 2026-10-31 ("maybe I should use FL Studio because it's like RISC"; "yes" to a design note) and said "Yes" to it, D1 to D4 as recommended, on 2026-11-01. Rules: KIT-1 to KIT-n in the contract, written by the tracks of §7. |
| **Written** | 2026-11-01, from `main` at 3eefd85 (wave 3 landed: One drum, `@m2/drums` 2.0.0) |
| **Replaces** | The two drum instruments of today: **Drum kit** (a drum machine on one track) and **One drum** (one drum on one track) |

**In short**
- **One building block: Drum.** One drum sound on one track, with its own fader, effects and notes. Today's "One
  drum", renamed. Its menu picks the sound (Kick, Snare, Hats…).
- **A kit is a group, not an instrument.** + Track ▸ **Drum kit** makes several Drum tracks (Kick, Snare, Hats, …)
  that all go to one **Drums** bus, so the kit has one fader too. Nothing new under it: tracks and a bus, which music2
  already has.
- **One beat grid for the group.** A new view shows every Drum track of a group as a row of steps, like FL Studio's
  step sequencer. Clicking a step writes a note in that track's own lane. You write the beat in one place and mix each
  drum on its own.
- **Choking stays simple.** The hats are one Drum with closed, pedal and open notes, so an open hat is cut by a
  closed one inside that drum, as today. Choking across tracks waits until someone needs it.
- **The one-track drum machine goes**, once the grid exists. `m2 upgrade` turns a Drum kit track into a group.

## 1. Why

Today music2 has two drum instruments that overlap (the owner, 2026-10-31: "why are there duplicate drums?"):

| | Drum kit | One drum |
|---|---|---|
| What | A drum machine: a different drum on each note | One drum: every note plays it |
| Writing a beat | One grid for every drum | One lane per drum, no shared grid |
| Mixing a drum alone | Not possible: one fader for the kit | Yes |

Each is good at one of the two things a beat needs, so a musician uses both, and wonders which to pick. The big DAWs
answer this in two ways:

| DAW | Building block | Kit | Beat |
|---|---|---|---|
| Ableton, Logic | A kit device with pads inside | The device | Pads inside the device; a pad can be pulled out to its own track |
| FL Studio | One sound per channel | Just channels side by side | The step sequencer: one row per channel |

FL's way is the "RISC" one: one small building block, used for everything, with no special case for drums. music2 is
built the same way (a track is a function, a bus is a track), so FL's way fits it with almost nothing new.

## 2. The model

| Thing | What it is in music2 | New? |
|---|---|---|
| **Drum** | A track playing `drums.drum(lane.notes("hits"), { drum: "snare" })` | Renamed from One drum |
| **Drum kit** | A set of Drum tracks whose output is one bus, and a name for the set | The set is new; tracks and buses aren't |
| **Beat grid** | A view of a kit: one row per Drum track, one column per step | New view |
| **Kit preset** | Which drums a new kit makes, with their sounds and knobs | New, small |

- **A group is the song's own fact, in code.** The kit's bus is a `bus("drums", …)` like any other; each Drum track's
  output is that bus. The beat grid shows "the tracks whose output is this bus and that play a Drum". No new file
  kind, no hidden link: delete the bus and the tracks are plain tracks again.
- **Each row is one track's own lane.** A step is a note in that track's notes lane, at the track's own pitch (any
  note plays its drum). The grid writes the lane through the same clip gestures the piano roll uses: one Tx, one undo
  step. A track's lane in the timeline and its row in the grid are the same notes.
- **Steps have velocity** (a bar under each row, as the piano roll has), and the grid's step length is its snap
  (1/16 by default). Notes off the grid show where they are and aren't moved.

## 3. Using it

| You do | What happens |
|---|---|
| + Track ▸ **Drum kit** ▸ "808" | Eight Drum tracks (Kick, Snare, Clap, Closed hat… ) and a **Drums** bus, as one undo step |
| Click a kit's bus, or a Drum track, then **Beat** (a tab beside Notes) | The beat grid for that kit |
| Click a step | A hit on that row's track; click again to remove it |
| Put reverb on the snare only | The Snare track's effects, as for any track |
| Turn the whole kit down | The Drums bus's fader |
| + Drum (in the grid) | One more Drum track in the kit |
| Play pads on a MIDI pad controller | Each pad's note goes to the kit's track for that drum, by General MIDI's drum notes: a Drum answers to its sound's notes (a Hats track to the closed, pedal and open hat) |
| Import a MIDI file with drums | One kit, one track per drum used |

## 4. Choking (one drum cutting another off)

- **Inside one Drum** it works today: the hats are one Drum with closed, pedal and open notes, and a closed hat cuts
  the open one. A kit preset keeps them as one Hats track.
- **Across tracks** (a cut group, as FL has) needs a Drum to hear another track's hits. Kernels can read note events
  since track `events` (ABI-8), so a Drum could take a second events input fed by the other tracks' lanes. It waits
  until a real case needs it; this note doesn't build it.

## 5. What goes

- The **Drum kit** instrument (the drum machine on one track, `drums.kit`) goes from + Track once the grid lands. Its
  kernel stays (One drum plays through it), so nothing about the sound changes.
- **`m2 upgrade`** turns a song's Drum kit track into a kit group: one Drum track per drum its lane uses, each with the
  notes of that drum, the kit's knobs carried to each Drum, and a Drums bus with the kit's old fader and effects. One
  undo step. Same sound, except that each drum now has its own voices.
- *Amended by track `kit-upgrade` (2026-11-01):* it is **Make it a kit**, the musician's own edit on a drum machine
  track, not part of `m2 upgrade`, which names the track and writes nothing for it: each hit's noise is seeded from its
  count on its own lane, so a lane of several drums can't keep its bits as a kit (KIT-22), and an upgrade keeps a
  song's sound exactly. The drum machine stays, unlisted, until 1.0 (KIT-24).

## 6. Decisions for the owner

| # | Question | Recommendation |
|---|---|---|
| D1 | Rename One drum to **Drum** | Yes: with the kit gone from + Track, "Drum" no longer sits next to "Drum kit" as an instrument (the kit becomes a group you add, worded "Drum kit (several tracks)") |
| D2 | Remove the one-track drum machine once the grid exists | Yes, with `m2 upgrade` converting old songs |
| D3 | Cut groups across tracks now or later | Later: the hats choke inside their own Drum |
| D4 | The beat grid as its own tab (Beat) or a mode of the piano roll | Its own tab, shown when a kit is selected: a grid of tracks is a different thing from a grid of pitches |

## 7. Plan

Each step is a track with a builder, a reviewer and a fix round, as today's tracks were. `kits` and `beat` run side
by side; `kit-upgrade` starts when both have landed.

| Track | Builds |
|---|---|
| `kits` | Rename to Drum; kit presets declared by the pack (`@m2/core` 2.2); + Track ▸ Drum kit makes a group (tracks + bus) as one undo step; one table from a Drum's sound to its General MIDI notes; a MIDI file's drums import as a kit |
| `beat` | Which tracks make a kit (KIT-1, in the song's view); the beat grid: rows of a kit's tracks, steps, velocity, snap, notes off the grid, + Drum, one Tx per gesture, a browser suite |
| `kit-upgrade` | `m2 upgrade` converts a Drum kit track into a group; pads on a MIDI controller play a kit's tracks; the one-track drum machine leaves + Track |

### Decided by the lead (2026-11-01)

| What | Decision | Why |
|---|---|---|
| Which note a Drum answers to | Its sound's General MIDI notes, worked out, never stored. Two Drums with the same sound: the first in the song's order answers | Nothing new to save or keep in step; a kit's tracks are already the facts |
| Where kit presets live | In the pack that has the Drum (`@m2/drums`), as typed data its descriptor carries | The owner's rule: most things in packs |
| The rules' prefix | KIT- | A new section of the contract, "Drum kits" |

## 8. Alternatives

| Instead | Why not |
|---|---|
| Keep both instruments | Two ways to do one thing, and the question "which do I pick?" every time |
| A kit device with pads that can be pulled out (Ableton, Logic) | A second, bigger building block with its own rules, where music2 already has tracks and buses |
| Per-drum outputs from the one-track drum machine | One instrument feeding many channels is a new kind of thing in the graph; separate tracks need nothing new |
