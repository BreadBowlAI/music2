import { track } from "@m2/core";
import { synth } from "std/synth";

export default track("arp", { name: "Arp" }, ({ lane }) => synth.pluck(lane.notes("notes"), { decay: 0.35 }));
