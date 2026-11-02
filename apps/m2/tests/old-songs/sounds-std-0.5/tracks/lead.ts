import { ms, secs, track } from "@m2/core";
import { delay } from "std/fx";
import { synth } from "std/synth";

// The hook: overlapping notes glide into each other. Dotted-eighth echoes at 140 bpm.
export default track("lead", { name: "Lead" }, ({ lane }) =>
  delay(synth.lead(lane.notes("hook"), { glide: ms(70) }), { time: secs(0.321), feedback: 0.3, mix: 0.2 }),
);
