import { bus } from "@m2/core";
import { reverb } from "@m2/effects";

// A shared reverb: the channels send it a little of their sound.
export default bus("verb", { name: "Reverb" }, ({ input }) => reverb(input, { size: 0.7, mix: 1 }));
