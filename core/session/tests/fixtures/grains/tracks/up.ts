import { track } from "@m2/core";
import { adsr, mono, osc, vca } from "std/dsp";
import { grains } from "@m2/effects";

// A tune on a triangle broken into short grains an octave up, a shimmer over the tune itself.
export default track("up", { name: "Up" }, ({ lane }) => {
  const tune = mono(lane.notes("tune"), (n) => vca(osc.tri(n.pitch), adsr(n.gate, { attack: 0.005, release: 0.1 })));
  return grains(tune, { size: 0.08, density: 25, spread: 0.4, pitch: 12, mix: 0.5 });
});
