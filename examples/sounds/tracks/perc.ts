import { db, track } from "@m2/core";
import { drums } from "@m2/drums";
import { mixer } from "std/mix";

// The drums one at a time, each a Drum on a lane of its own: a long boom at each drop, a snare roll
// into it, a clap layer, more hats, a tom fill, rim, cowbell, crash and shaker.
export default track("perc", { name: "Percussion" }, ({ lane }) =>
  mixer({
    boom: { in: drums.drum(lane.notes("boom"), { decay: 3.1, punch: -0.2 }), db: db(-4) },
    roll: drums.drum(lane.notes("roll"), { drum: "snare", punch: 0.15 }),
    clap: { in: drums.drum(lane.notes("clap"), { drum: "clap" }), db: db(-3) },
    hats: { in: drums.drum(lane.notes("hats"), { drum: "hats", decay: 0.67 }), pan: 0.35 },
    tom: { in: drums.drum(lane.notes("tom"), { drum: "tom" }), pan: -0.2 },
    rim: { in: drums.drum(lane.notes("rim"), { drum: "rim" }), db: db(-4), pan: 0.25 },
    cowbell: { in: drums.drum(lane.notes("cowbell"), { drum: "cowbell" }), db: db(-6), pan: -0.35 },
    crash: { in: drums.drum(lane.notes("crash"), { drum: "cymbal" }), db: db(-2), pan: -0.15 },
    shaker: { in: drums.drum(lane.notes("shaker"), { drum: "shaker" }), pan: 0.4 },
  }),
);
