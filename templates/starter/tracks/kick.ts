import { ms, track } from "@m2/core";
import { follow } from "std/dsp";
import { kick } from "std/instruments";

export default track("kick", { name: "Kick" }, ({ lane }) => {
  const out = kick(lane.notes("hits"));
  // How loud the kick is right now: the bass and chords duck to it (see song.ts).
  return { out, env: follow(out, { release: ms(150) }) };
});
