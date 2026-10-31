# @m2/drums

**In short**
- music2's 808/909-style drum machine, as a pack that ships with music2 and installs into your store when it starts.
- `drums.kit` plays a whole beat from one notes lane, each drum on its General MIDI note (C2 kick, D2 snare, F#2 closed hat …).
- Each drum is also an instrument of its own: `drums.kick`, `drums.snare`, `drums.clap`, `drums.hats`, `drums.tom`, `drums.rim`, `drums.cowbell`, `drums.cymbal`, `drums.shaker`.

```ts
import { drums } from "@m2/drums";
```

Every drum plays std's `std:drum` kernel. The guide's [Sounds and effects](../../../docs/guide/sounds.md)
lists the kit's notes and knobs.
