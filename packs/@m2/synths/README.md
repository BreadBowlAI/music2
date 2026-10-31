# @m2/synths

**In short**
- music2's synths, as a pack that ships with music2 and installs into your store when it starts.
- `synth`: the polysynth's sounds (supersaw, lead, pad, pluck, 808 sub, bass, strings), and `synth.custom` with every knob.
- `fm`: the FM synth's sounds (keys, bell, metal), and `fm.custom` with every knob.
- `bass`, `pluck`, `pad`: the first instruments, short ones built from std's blocks, to read and copy. The drums are `@m2/drums`'.

```ts
import { fm, synth } from "@m2/synths";
```

The polysynth and the FM synth play the pack's own kernels, `@m2/synths:synth` and `@m2/synths:fm`, in
`kernels/synths.wasm`; the first instruments play std/dsp's blocks. The guide's page Sounds and effects
says what each knob does: `m2 docs sounds` prints it.
