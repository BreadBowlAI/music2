import { track } from "@m2/core";
import { adsr, mono, osc, vca } from "std/dsp";
import { flanger } from "@m2/effects";

// A bass line on a saw through the Flanger, its feedback down: the hollow sound, a sweep each 0.8 s.
export default track("down", { name: "Down" }, ({ lane }) => {
  const line = mono(lane.notes("line"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.005, release: 0.05 })));
  return flanger(line, { rate: 1.25, depth: 0.6, feedback: -0.6, mix: 0.5 });
});
