// @m2/synths: music2's synths (PACK-15). Import them from here: `import { synth, fm } from "@m2/synths"`.
//
// - `synth`: the polysynth's sounds (supersaw, lead, pad, pluck, 808 sub, bass, strings) and
//   `synth.custom`, the polysynth with every knob (./synth).
// - `fm`: the FM synth's sounds (keys, bell, metal) and `fm.custom`, with every knob (./fm).
// - `bass`, `pluck`, `pad`: the first instruments, short ones built from std's blocks to read and
//   copy (./simple). The drums are @m2/drums'.
//
// The polysynth and the FM synth play the pack's own kernels (`@m2/synths:synth`, `@m2/synths:fm`,
// built from kernels-src/); the rest play std's by id. To build a voice of your own, std/dsp has
// the blocks and `mono`.

export { fm } from "./fm";
export { bass, pad, pluck } from "./simple";
export { synth } from "./synth";
