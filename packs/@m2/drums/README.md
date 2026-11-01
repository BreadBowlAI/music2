# @m2/drums

**In short**
- music2's 808/909-style drums, as a pack that ships with music2 and installs into your store when it starts.
- `drums.drum` (Drum) plays one drum on every note, the one its menu picks: kick, snare, clap, hats, tom, rim, cowbell, cymbal or shaker, or another drum the drum machine plays, beside its family (kick 2, snare 2, claves, cymbal 2, ride, ride 2, shaker 2, tambourine) (`drums.drum(notes, { drum: "snare" })`), with its tune, decay and punch.
- Its kits, 808 kit, 909 kit and Basic kit, are what + Track's Drum kit makes: several Drum tracks that go to one Drums bus, each drum mixed on its own.
- `drums.kit` (Drum machine) plays a whole beat from one notes lane, each drum on its General MIDI note (C2 kick, D2 snare, F#2 closed hat …). No list offers it now; songs that play it play as before.

```ts
import { drums } from "@m2/drums";
```

Every drum plays the pack's own kernel, `@m2/drums:drum`, in `kernels/drums.wasm`. The guide's page
Sounds and effects lists the drums and their knobs: `m2 docs sounds` prints it.
