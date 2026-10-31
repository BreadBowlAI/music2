# @m2/effects

**In short**
- music2's effects that colour a sound, as a pack that ships with music2 and installs into your store when it starts.
- Echoes and space: `delay`, `reverb`. Movement: `chorus`, `phaser`, `tremolo`. Grit: `drive`, `bitcrush`. Width: `widen`.
- The effects mixing needs (`eq3`, `compress`, `limit`, `duck`) are std's, in `std/fx`.

```ts
import { delay, reverb } from "@m2/effects";
```

Each plays std's kernel of its name (`std:reverb` …). The guide's page Sounds and effects says what each
knob does: `m2 docs sounds` prints it.
