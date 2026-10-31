import { track } from "@m2/core";
import { synth } from "@m2/synths";

// An 808 sub: one note at a time, sliding where a note starts before the last one ends.
export default track("sub", { name: "808" }, ({ lane }) => synth.sub(lane.notes("notes")));
