import { track } from "@m2/core";
import { sampler } from "std/sampler";

// A made-up instrument in five short files (assets/, made by ../make-sampler-regions.mjs), all
// recorded at C4: two takes of a soft note, played in turn; a loud note; the click of a key let go;
// and a held tone under every note, which loops while the key is down.
export default track("keys", { name: "Keys" }, ({ lane }) =>
  sampler(lane.notes("notes"), {
    regions: [
      { file: "soft-1.wav", keys: [48, 72], velocity: [1, 90], round_robin: [1, 2] },
      { file: "soft-2.wav", keys: [48, 72], velocity: [1, 90], round_robin: [2, 2] },
      { file: "hard.wav", keys: [48, 72], velocity: [91, 127], pan: 0.3 },
      { file: "release.wav", keys: [48, 72], trigger: "release", volume: -12 },
      { file: "pad.wav", root: 55, keys: [48, 72], loop: [2400, 5399], volume: -6, tune: 5 },
    ],
    mode: "gate",
    release: 0.2,
  }),
);
