import { ms, track } from "@m2/core";
import { drums } from "@m2/drums";
import { follow } from "std/dsp";

export default track("kick", { name: "Kick" }, ({ lane }) => {
  const out = drums.drum(lane.notes("hits"));
  // How loud the kick is right now: the bass and chords duck to it (see song.ts).
  return { out, env: follow(out, { release: ms(150) }) };
});
