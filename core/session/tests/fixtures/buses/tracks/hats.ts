import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("hats", { name: "Hats" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "hats" }));
