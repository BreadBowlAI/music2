import { track } from "@m2/core";
import { reverb } from "std/fx";
import { fm } from "std/fm";

export default track("bell", { name: "Bell" }, ({ lane }) => reverb(fm.bell(lane.notes("notes"), { tone: 0.6 }), { size: 0.7, mix: 0.3 }));
