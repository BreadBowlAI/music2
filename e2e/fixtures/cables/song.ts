import { hz, khz, song } from "@m2/core";
import { arp } from "@m2/notes";
import { lfo } from "std/control";
import { mono, osc, svf } from "std/dsp";
import { xrange } from "std/math";
import { mixer } from "std/mix";

export default song(({ lane, time }) => {
  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));
  // A note effect's values take no cable (RENDER-41): the suite drops one on its Gate.
  const keys = mono(arp(lane.notes("keys")), (n) => osc.saw(n.pitch));
  return mixer({ bass: svf.lowpass(osc.saw(hz(55)), hz(1000)), keys });
});
