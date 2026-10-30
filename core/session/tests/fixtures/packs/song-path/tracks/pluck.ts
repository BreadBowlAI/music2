import { track } from "@m2/core";
import { tinySampler } from "@test/tiny";

export default track("pluck", { name: "Pluck" }, ({ lane }) => tinySampler(lane.notes("notes")));
