import { hz, track } from "@m2/core";
import { pluck } from "std/instruments";
import { delay, reverb, widen } from "std/fx";

// The tune: a bright pluck with echoes three 16ths apart (0.388 s at 116 bpm).
export default track("hook", { name: "Hook" }, ({ lane }) => {
  const hook = pluck(lane.notes("notes"), { brightness: hz(4200) });
  const echoes = delay(hook, { time: 0.388, feedback: 0.3, mix: 0.25 });
  return reverb(widen(echoes, { width: 1.3 }), { size: 0.6, mix: 0.2 });
});
