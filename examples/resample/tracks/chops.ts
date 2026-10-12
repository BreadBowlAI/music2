import { track } from "@m2/core";

// Audio clips from the timeline: see data/chops/clips.json.
export default track("chops", { name: "Chops" }, ({ lane }) => lane.audio("clips"));
