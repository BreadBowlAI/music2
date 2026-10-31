import { track } from "@m2/core";
import { synth } from "std/synth";

// The breakdown's bass: a plucky line of eighths, where the 808 rests.
export default track("bass", { name: "Bass" }, ({ lane }) => synth.bass(lane.notes("notes")));
