import { hz, khz, song } from "@m2/core";
import { lfo } from "std/control";
import { osc, svf } from "std/dsp";
import { xrange } from "std/math";

export default song(({ time }) => {
  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));
  return svf.lowpass(osc.saw(hz(55)), hz(1000));
});
