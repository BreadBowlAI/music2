import { track } from "@m2/core";
import { hat } from "std/instruments";

export default track("Hats", ({ lane }) => hat(lane.notes("hits")));
