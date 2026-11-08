import { poly, track } from "@m2/core";
import { adsr, osc, vca } from "std/dsp";
import { grains } from "@m2/effects";

// Chords on a saw broken into long grains an octave down, from up to 1.5 s back: a cloud under them.
export default track("down", { name: "Down" }, ({ lane }) => {
  const chords = poly(lane.notes("chords"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.01, release: 0.2 })));
  return grains(chords, { size: 0.3, density: 8, spread: 1.5, pitch: -12, mix: 0.7 });
});
