# @m2/drums

**In short**
- music2's 808/909-style drum machine, as a pack that ships with music2 and installs into your store when it starts.
- `drums.kit` plays a whole beat from one notes lane, each drum on its General MIDI note (C2 kick, D2 snare, F#2 closed hat …).
- `drums.drum` plays one drum on every note, the one its menu picks: kick, snare, clap, hats, tom, rim, cowbell, cymbal or shaker (`drums.drum(notes, { drum: "snare" })`), with its tune, decay and punch.

```ts
import { drums } from "@m2/drums";
```

Every drum plays the pack's own kernel, `@m2/drums:drum`, in `kernels/drums.wasm`. The guide's page
Sounds and effects lists the kit's notes and knobs: `m2 docs sounds` prints it.
