import { track } from "@m2/core";

// Audio clips from the timeline: see timeline/chops.clips.json.
export default track("Chops", ({ lane }) => lane.audio("clips"));
