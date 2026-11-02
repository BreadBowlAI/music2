import { track } from "@m2/core";
import { widen } from "std/fx";
import { synth } from "std/synth";

export default track("chords", { name: "Supersaw" }, ({ lane }) => widen(synth.supersaw(lane.notes("notes")), { width: 1.2 }));
