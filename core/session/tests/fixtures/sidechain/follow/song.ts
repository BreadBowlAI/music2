// Gate 8's second ducking fixture: the bass ducks to `follow(kick.out)` written here, in the song,
// whose input is a sidechain input. Muting the kick must leave the bass ducked.

import { db, ms, song, track } from "@m2/core";
import { drums } from "@m2/drums";
import { follow, osc } from "std/dsp";
import { duck } from "std/fx";
import { mixer } from "std/mix";

const kick = track("kick", { name: "Kick" }, ({ lane }) => drums.drum(lane.notes("hits")));
const bass = track("bass", { name: "Bass" }, () => osc.saw(55));

export default song(() =>
  mixer({ kick: kick.out, bass: duck(bass.out, follow(kick.out, { release: ms(150) }), { depth: db(-12) }) }),
);
