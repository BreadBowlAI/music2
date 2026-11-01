import { track } from "@m2/core";
import { synth } from "std/synth";

export default track("strings", { name: "Strings" }, ({ lane }) => synth.strings(lane.notes("notes"), { ensemble: 0.6 }));
