import { poly, track } from "@m2/core";
import { adsr, osc, vca } from "std/dsp";
import { flanger } from "@m2/effects";

// Chords on a saw through the Flanger, its feedback up: the ringing jet, a sweep each 2 s.
export default track("up", { name: "Up" }, ({ lane }) => {
  const chords = poly(lane.notes("chords"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.01, release: 0.2 })));
  return flanger(chords, { rate: 0.5, depth: 0.8, feedback: 0.7, mix: 0.5 });
});
