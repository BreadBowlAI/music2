import { track } from "@m2/core";
import { pluck } from "std/instruments";

export default track("keys", { name: "Keys" }, ({ lane }) => pluck(lane.notes("chords")));
