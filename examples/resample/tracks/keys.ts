import { track } from "@m2/core";
import { pluck } from "std/instruments";

export default track("Keys", ({ lane }) => pluck(lane.notes("chords")));
