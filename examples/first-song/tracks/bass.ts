import { hz, khz, track } from "@m2/core";
import { blend, lfo } from "std/control";
import { bass } from "std/instruments";
import { xrange } from "std/math";

export default track("Bass", ({ lane, time }) => {
  const cutoff = lane.control("cutoff", "hz", hz(500)); // automation, drawn on the timeline
  const drop = lane.mask("drop"); // 1 where the wobble should take over

  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(149), khz(0.778));
  return bass(lane.notes("notes"), { cutoff: blend(cutoff, wobble, drop) });
});
