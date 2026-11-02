import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("ride", { name: "Ride" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "ride", voices: 16 }));
