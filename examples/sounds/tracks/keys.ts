import { track } from "@m2/core";
import { chorus } from "std/fx";
import { fm } from "std/fm";

export default track("keys", { name: "Keys" }, ({ lane }) => chorus(fm.keys(lane.notes("chords")), { rate: 0.5, depth: 0.4, mix: 0.35 }));
