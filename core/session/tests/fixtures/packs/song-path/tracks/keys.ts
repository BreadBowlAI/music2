import { track } from "@m2/core";
import { tinyCrunch, tinyKeys } from "@test/tiny";

export default track("keys", { name: "Keys" }, ({ lane }) => tinyCrunch(tinyKeys(lane.notes("notes")), 6));
