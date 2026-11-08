import { track } from "@m2/core";
import { adsr, mono, osc, vca } from "std/dsp";
import { deesser, gate, multiband } from "@m2/mixing";

// A saw in short notes, silent between them: the gate opens on each and closes between them, the
// de-esser turns its highs down, and the multiband squeezes its lows, over their threshold.
export default track("mix", { name: "Mix" }, ({ lane }) => {
  const notes = mono(lane.notes("hits"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.002, release: 0.02 })));
  return multiband(deesser(gate(notes, { threshold: -30 }), { threshold: -40, amount: 9 }), { lowThreshold: -30, lowRatio: 4 });
});
