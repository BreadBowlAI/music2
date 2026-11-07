import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("kick", { name: "Kick" }, ({ lane }) => drums.drum(lane.notes("hits")));
