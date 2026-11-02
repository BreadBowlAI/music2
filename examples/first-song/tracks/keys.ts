import { track } from "@m2/core";
import { pluck } from "@m2/synths";

export default track("keys", { name: "Keys" }, ({ lane }) => pluck(lane.notes("chords")));
