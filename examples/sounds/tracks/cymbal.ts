import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("cymbal", { name: "Cymbal" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "cymbal", voices: 16 }));
