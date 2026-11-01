import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("snare", { name: "Snare" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "snare", voices: 16 }));
