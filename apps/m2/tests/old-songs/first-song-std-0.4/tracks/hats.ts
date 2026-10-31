import { track } from "@m2/core";
import { hat } from "std/instruments";

export default track("hats", { name: "Hats" }, ({ lane }) => hat(lane.notes("hits")));
