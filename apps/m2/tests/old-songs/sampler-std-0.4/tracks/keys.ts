import { track } from "@m2/core";
import { sampler } from "std/sampler";

// One plucked C4, pitched to every note. "gate" stops it when the note ends, over 0.15 s.
export default track("keys", { name: "Keys" }, ({ lane }) => sampler(lane.notes("notes"), { sample: "pluck.wav", mode: "gate", release: 0.15 }));
