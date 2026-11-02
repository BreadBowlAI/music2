import { track } from "@m2/core";
import { reverb } from "@m2/effects";
import { synth } from "@m2/synths";

export default track("pad", { name: "Pad" }, ({ lane }) => reverb(synth.pad(lane.notes("notes")), { size: 0.8, mix: 0.3 }));
