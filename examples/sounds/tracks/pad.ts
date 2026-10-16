import { track } from "@m2/core";
import { reverb } from "std/fx";
import { synth } from "std/synth";

export default track("pad", { name: "Pad" }, ({ lane }) => reverb(synth.pad(lane.notes("notes")), { size: 0.8, mix: 0.3 }));
