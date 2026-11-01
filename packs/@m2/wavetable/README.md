# @m2/wavetable

**In short**
- music2's wavetable synth, as a pack that ships with music2 and installs into your store when it starts.
- A wavetable is a set of waveforms; `position` slides between them. Two oscillators each play one: five are built in (basic, pulse, vowels, harmonics, digital), or drop a WAV of your own on the card.
- A filter, two envelopes, an LFO, unison and glide. Four presets: Slow pad, Round bass, Sharp lead and Short pluck.

```ts
import { wavetable } from "@m2/wavetable";
```

Its sound code is its own kernel, `@m2/wavetable:voice`, in `kernels/wavetable.wasm`. The guide's
page Sounds and effects says what each knob does: `m2 docs sounds` prints it.
