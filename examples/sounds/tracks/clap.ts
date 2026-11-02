import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("clap", { name: "Clap" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "clap", voices: 16 }));
