import { hz, track } from "@m2/core";
import { bass } from "@m2/synths";

export default track("bass", { name: "Bass" }, ({ lane }) => bass(lane.notes("notes"), { cutoff: hz(420), drive: 3 }));
