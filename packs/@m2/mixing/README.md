# @m2/mixing

**In short**
- music2's mixing tools, as a pack that ships with music2 and installs into your store when it starts.
- `eq`: an eight-band parametric EQ. Each band is a low cut, low shelf, bell, high shelf or high cut, with its own frequency, gain, Q and on/off.
- `gate`: silences a track between its sounds, keyed by the sound itself or by another track (`gate(pad.out, kick.out)`).
- `transient`: makes each hit's start punchier or softer (`attack`), and its tail longer or shorter (`sustain`).
- `deesser`: tames the hiss of S sounds in a voice, turning down only the band around its frequency.
- `multiband`: a compressor for the lows, mids and highs, each on its own.
- `pump`: makes the sound dip on every beat and swell back, in time with the song, with no other track to listen to.

```ts
import { deesser, eq, gate, multiband, pump, transient } from "@m2/mixing";
```

The first five play the pack's own kernel (`@m2/mixing:eq` …), in `kernels/mixing.wasm`. `pump` is
TypeScript over std's blocks (an LFO of the song's beats, and `duck`). The four effects every mix
starts with (`eq3`, `compress`, `limit`, `duck`) are std's, in `std/fx`. The guide's page Sounds and
effects says what each knob does: `m2 docs sounds` prints it.
