import { track } from "@m2/core";
import { arp } from "@m2/notes";
import { synth } from "@m2/synths";

// The chords in "chords" are held a bar each; the Arpeggiator plays them a sixteenth at a time, up
// two octaves, a little choppy (gate 0.6), and the pluck plays its notes.
export default track("keys", { name: "Keys" }, ({ lane }) =>
  synth.pluck(arp(lane.notes("chords"), { rate: "1/16", octaves: "2", gate: 0.6 }), { decay: 0.3 }),
);
