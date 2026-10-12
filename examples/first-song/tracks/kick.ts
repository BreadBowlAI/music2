import { ms, track } from "@m2/core";
import { follow } from "std/dsp";
import { kick } from "std/instruments";

export default track("Kick", ({ lane }) => {
  const out = kick(lane.notes("hits"));
  // Exported so other tracks can duck to the kick.
  return { out, env: follow(out, { release: ms(150) }) };
});
