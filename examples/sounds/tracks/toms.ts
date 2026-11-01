import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("toms", { name: "Toms" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "tom", voices: 16 }));
