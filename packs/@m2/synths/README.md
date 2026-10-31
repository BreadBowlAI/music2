# @m2/synths

**In short**
- music2's synths, as a pack that ships with music2 and installs into your store when it starts.
- `synth`: the polysynth's sounds (supersaw, lead, pad, pluck, 808 sub, bass, strings), and `synth.custom` with every knob.
- `fm`: the FM synth's sounds (keys, bell, metal), and `fm.custom` with every knob.
- `bass`, `kick`, `hat`, `pluck`, `pad`: the first instruments, short ones built from std's blocks, to read and copy.

```ts
import { fm, synth } from "@m2/synths";
```

Every sound here plays std's kernels (`std:synth`, `std:fm`, and std/dsp's blocks). The guide's
[Sounds and effects](../../../docs/guide/sounds.md) says what each knob does.
