import { ms, track } from "@m2/core";
import { follow } from "std/dsp";
import { drums } from "@m2/drums";

// The kick, on a track of its own, so the sub and the chords can duck under it alone.
export default track("kick", { name: "Kick" }, ({ lane }) => {
  const kick = drums.drum(lane.notes("hits"), { tune: 0.7, punch: 0.1 });
  // How loud the kick is right now, for ducking.
  return { out: kick, env: follow(kick, { release: ms(160) }) };
});
