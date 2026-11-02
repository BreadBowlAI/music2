// Gate 8's first ducking fixture: the bass ducks to the kick's envelope, `kick.env`, a control
// export the kick track makes itself. Muting the kick must leave the bass ducked.

import { db, ms, song, track } from "@m2/core";
import { drums } from "@m2/drums";
import { follow, osc } from "std/dsp";
import { duck } from "std/fx";
import { mixer } from "std/mix";

const kick = track("kick", { name: "Kick" }, ({ lane }) => {
  const out = drums.drum(lane.notes("hits"));
  return { out, env: follow(out, { release: ms(150) }) };
});
const bass = track("bass", { name: "Bass" }, () => osc.saw(55));

export default song(() => mixer({ kick: kick.out, bass: duck(bass.out, kick.env, { depth: db(-12) }) }));
