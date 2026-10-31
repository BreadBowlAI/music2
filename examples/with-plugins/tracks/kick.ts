import { ms, track } from "@m2/core";
import { kick } from "@m2/synths";
import { follow } from "std/dsp";

export default track("kick", { name: "Kick" }, ({ lane }) => {
  const out = kick(lane.notes("hits"));
  return { out, env: follow(out, { release: ms(150) }) };
});
