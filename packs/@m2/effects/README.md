# @m2/effects

**In short**
- music2's effects that colour a sound, as a pack that ships with music2 and installs into your store when it starts.
- Echoes and space: `delay`, `reverb`. Movement: `chorus`, `phaser`, `tremolo`. Grit: `drive`, `bitcrush`. Width: `widen`.
- The effects mixing needs (`eq3`, `compress`, `limit`, `duck`) are std's, in `std/fx`.

```ts
import { delay, reverb } from "@m2/effects";
```

`chorus` and `drive` play std's kernel of their name (`std:chorus`, `std:drive`); the others play the
pack's own (`@m2/effects:delay` …), in `kernels/effects.wasm`. The guide's page Sounds and effects says
what each knob does: `m2 docs sounds` prints it.
