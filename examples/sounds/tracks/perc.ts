import { db, ms, secs, track } from "@m2/core";
import { drums } from "@m2/drums";
import { mixer } from "std/mix";

// The drums one at a time, each on a lane of its own: a long boom at each drop, a snare roll
// into it, a clap layer, more hats, a tom fill, rim, cowbell, crash and shaker.
export default track("perc", { name: "Percussion" }, ({ lane }) =>
  mixer({
    boom: { in: drums.kick(lane.notes("boom"), { decay: secs(1.4), punch: 0.3 }), db: db(-4) },
    roll: drums.snare(lane.notes("roll"), { snap: 0.75 }),
    clap: { in: drums.clap(lane.notes("clap")), db: db(-3) },
    hats: { in: drums.hats(lane.notes("hats"), { decay: ms(30) }), pan: 0.35 },
    tom: { in: drums.tom(lane.notes("tom")), pan: -0.2 },
    rim: { in: drums.rim(lane.notes("rim")), db: db(-4), pan: 0.25 },
    cowbell: { in: drums.cowbell(lane.notes("cowbell")), db: db(-6), pan: -0.35 },
    crash: { in: drums.cymbal(lane.notes("crash")), db: db(-2), pan: -0.15 },
    shaker: { in: drums.shaker(lane.notes("shaker")), pan: 0.4 },
  }),
);
