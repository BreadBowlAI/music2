// @m2/synths: music2's synths (PACK-15). Import them from here: `import { synth, fm } from "@m2/synths"`.
//
// - `synth`: the polysynth's sounds (supersaw, lead, pad, pluck, 808 sub, bass, strings) and
//   `synth.custom`, the polysynth with every knob (./synth).
// - `fm`: the FM synth's sounds (keys, bell, metal) and `fm.custom`, with every knob (./fm).
// - `bass`, `kick`, `hat`, `pluck`, `pad`: the first instruments, short ones built from std's
//   blocks to read and copy (./simple).
//
// Each plays std's kernels by id. To build a voice of your own, std/dsp has the blocks and `mono`.

export { fm } from "./fm";
export { bass, hat, kick, pad, pluck } from "./simple";
export { synth } from "./synth";
