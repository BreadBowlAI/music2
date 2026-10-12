import { hz, track } from "@m2/core";
import { pad } from "std/instruments";
import { chorus, reverb, widen } from "std/fx";

// A soft pad, spread wide, thickened by a chorus and placed in a big room.
export default track("chords", { name: "Chords" }, ({ lane }) => {
  const chords = pad(lane.notes("notes"), { cutoff: hz(1600) });
  const wide = chorus(widen(chords, { width: 1.6 }), { rate: 0.6, depth: 0.4, mix: 0.4 });
  return reverb(wide, { size: 0.75, damping: 0.5, mix: 0.3 });
});
