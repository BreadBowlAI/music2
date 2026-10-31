import { ms, track } from "@m2/core";
import { hat } from "@m2/synths";

export default track("hats", { name: "Hats" }, ({ lane }) => hat(lane.notes("hits"), { decay: ms(40) }));
