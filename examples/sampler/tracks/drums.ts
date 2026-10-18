import { track } from "@m2/core";
import { sampler } from "std/sampler";

// A file for each note: C2 (36) plays kick.wav, D2 (38) snare.wav. Each plays to its end.
export default track("drums", { name: "Drums" }, ({ lane }) => sampler(lane.notes("hits"), { zones: { 36: "kick.wav", 38: "snare.wav" } }));
