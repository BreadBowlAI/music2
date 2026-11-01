import { track } from "@m2/core";
import { widen } from "@m2/effects";
import { synth } from "@m2/synths";

export default track("chords", { name: "Supersaw" }, ({ lane }) => widen(synth.supersaw(lane.notes("notes")), { width: 1.2 }));
