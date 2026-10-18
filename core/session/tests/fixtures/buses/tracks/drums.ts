import { db, bus } from "@m2/core";
import { compress } from "std/fx";

// A group: the kick and the hats go here instead of the master (`to: "drums"` in the mixer).
export default bus("drums", { name: "Drums" }, ({ input }) => compress(input, { threshold: db(-20), ratio: 3, makeup: db(2) }));
