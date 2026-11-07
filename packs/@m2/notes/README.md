# @m2/notes

**In short**
- music2's note effects, as a pack that ships with music2 and installs into your store when it starts.
- A note effect takes notes and gives notes. It goes **before** an instrument: keys → Arpeggiator → Pluck.
- **Arpeggiator** plays the notes you hold one at a time, in a pattern, in time with the song.
- **Chord** plays a whole chord from each note. In key, one finger plays the chords of the song's key.
- **Note echo** plays each note again, later and quieter, a few times.
- **Scale** moves every note onto the song's key, so no note is wrong.

```ts
import { arp, chord, echo, scale } from "@m2/notes";

// A held chord, played a sixteenth at a time, up two octaves, by a pluck.
pluck(arp(lane.notes("chords"), { rate: "1/16", octaves: "2" }))
```

Its sound code is its own kernels, `@m2/notes:arp`, `chord`, `echo` and `scale`, in `kernels/notes.wasm`.
The guide's page Sounds and effects says how to use them, under Note effects: `m2 docs sounds` prints it.
