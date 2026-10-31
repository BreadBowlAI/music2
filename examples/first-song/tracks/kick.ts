import { ms, track } from "@m2/core";
import { drums } from "@m2/drums";
import { follow } from "std/dsp";

export default track("kick", { name: "Kick" }, ({ lane }) => {
  const out = drums.drum(lane.notes("hits"));
  // Exported so other tracks can duck to the kick.
  return { out, env: follow(out, { release: ms(150) }) };
});
