import { bus, ms } from "@m2/core";
import { delay } from "@m2/effects";

// A shared delay, which sends its echoes on to the reverb.
export default bus("echo", { name: "Echo" }, ({ input }) => delay(input, { time: ms(363), feedback: 0.35, mix: 1 }));
