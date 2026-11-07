import { track } from "@m2/core";
import { synth } from "@m2/synths";

export default track("pad", { name: "Pad" }, ({ lane }) => synth.pad(lane.notes("chords")));
