import { track } from "@m2/core";
import { adsr, mono, osc, vca } from "std/dsp";
import { clip } from "@m2/effects";

// A saw in short notes pushed 12 dB into the Clipper: its two cuts at four times the rate take off
// every peak over -1 dB, and its last step what still rings past it. It is 92 frames late, and so
// is the song.
export default track("mix", { name: "Mix" }, ({ lane }) => {
  const notes = mono(lane.notes("hits"), (n) => vca(osc.saw(n.pitch), adsr(n.gate, { attack: 0.002, release: 0.02 })));
  return clip(notes, { gain: 12, ceiling: -1 });
});
