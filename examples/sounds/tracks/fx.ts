import { hz, secs, track } from "@m2/core";
import { fm } from "std/fm";
import { sum } from "std/math";
import { synth } from "std/synth";

// A riser from the raw synth engine (noise and a saw into a singing filter that the `cutoff`
// lane sweeps open), and a metallic FM hit at each drop.
export default track("fx", { name: "FX" }, ({ lane }) => {
  const riser = synth.custom(lane.notes("riser"), {
    noise: 0.8,
    saw: 0.4,
    cutoff: lane.control("cutoff", "hz", hz(300)),
    resonance: 0.65,
    keytrack: 0,
    attack: secs(3),
    release: secs(0.4),
    unison: 3,
    detune: 0.3,
  });
  const stab = fm.metal(lane.notes("stabs"), { decay: secs(1.2), drop: 12 });
  return sum(riser, stab);
});
