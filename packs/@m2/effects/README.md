# @m2/effects

**In short**
- music2's effects that colour a sound, as a pack that ships with music2 and installs into your store when it starts.
- Echoes and space: `delay`, `reverb`, and `grains`, which breaks a sound into tiny pieces to smear it, stretch it or freeze it. Movement: `chorus`, `flanger` (the jet's whoosh: a copy of the sound sweeping against it), `phaser`, `tremolo`. Grit: `drive`, `bitcrush`. Width: `widen`.
- Louder without going over: `clip`, the Clipper, which cuts every peak flat at its ceiling. It works at four times the song's rate, so it adds far fewer harsh tones than a plain clipper; on very bright sounds pushed hard, a few. It makes the song 92 samples (2 ms) later, and every other track waits for it.
- The effects mixing needs (`eq3`, `compress`, `limit`, `duck`) are std's, in `std/fx`.

```ts
import { delay, reverb } from "@m2/effects";
```

`chorus` and `drive` play std's kernel of their name (`std:chorus`, `std:drive`); the others play the
pack's own (`@m2/effects:delay` …), in `kernels/effects.wasm`. The guide's page Sounds and effects says
what each knob does: `m2 docs sounds` prints it.
