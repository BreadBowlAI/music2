// std/instruments: playable instruments, built only from std kernels.
//
// Each one takes a notes lane and returns audio. They are ordinary functions, so
// copy one into your project and change it if you want something different.

import { component, hz, ms, poly, secs, type Audio, type Events, type Value } from "@m2/core";
import { adsr, noise, osc, svf, vca } from "./dsp";
import { drive } from "./fx";
import { add, scale, xrange } from "./math";

/** A punchy bass: saw plus a sub square, through a lowpass that opens on each note. */
export const bass = component({ controls: ["1.cutoff", "1.drive"] }, function bass(notes: Events, o: { cutoff?: Value; drive?: Value; voices?: number } = {}): Audio {
  const cutoff = o.cutoff ?? hz(600);
  const v = poly(
    notes,
    (n) => {
      const body = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, -12)), 0.5));
      const env = adsr(n.gate, { attack: ms(3), decay: secs(0.3), sustain: 0.6, release: ms(80) });
      const fenv = adsr(n.gate, { attack: ms(2), decay: secs(0.18), sustain: 0.2, release: ms(80) });
      const f = svf.lowpass(body, xrange(fenv, cutoff, scale(cutoff, 4)), 1.1);
      return vca(f, scale(env, n.velocity));
    },
    { voices: o.voices ?? 4 },
  );
  return drive(v, o.drive ?? 4);
});

/** A sine kick with a fast pitch drop. */
export const kick = component({ controls: ["1.tune", "1.decay"] }, function kick(notes: Events, o: { tune?: Value; decay?: Value } = {}): Audio {
  return poly(
    notes,
    (n) => {
      const amp = adsr(n.gate, { attack: ms(1), decay: o.decay ?? secs(0.32), sustain: 0, release: ms(60) });
      const drop = adsr(n.gate, { attack: ms(1), decay: ms(45), sustain: 0, release: ms(30) });
      const f = xrange(drop, o.tune ?? hz(48), hz(170));
      return drive(vca(osc.sine(f), scale(amp, n.velocity)), 3);
    },
    { voices: 2 },
  );
});

/** Closed hi-hat: highpassed noise with a short envelope. */
export const hat = component({ controls: ["1.decay"] }, function hat(notes: Events, o: { decay?: Value } = {}): Audio {
  return poly(
    notes,
    (n) => {
      const env = adsr(n.gate, { attack: ms(1), decay: o.decay ?? ms(45), sustain: 0, release: ms(30) });
      return vca(svf.highpass(noise(7), hz(7500), 0.9), scale(env, scale(n.velocity, 0.5)));
    },
    { voices: 4 },
  );
});

/** A bright pluck for chords and arps. */
export const pluck = component({ controls: ["1.brightness"] }, function pluck(notes: Events, o: { brightness?: Value; voices?: number } = {}): Audio {
  return poly(
    notes,
    (n) => {
      const env = adsr(n.gate, { attack: ms(2), decay: secs(0.35), sustain: 0, release: secs(0.2) });
      const tone = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, 12)), 0.3));
      const f = svf.lowpass(tone, xrange(env, hz(400), o.brightness ?? hz(5000)), 0.8);
      return vca(f, scale(env, scale(n.velocity, 0.6)));
    },
    { voices: o.voices ?? 12 },
  );
});

/** A soft pad: two detuned saws, slow envelope, gentle filter. */
export const pad = component({ controls: ["1.cutoff"] }, function pad(notes: Events, o: { cutoff?: Value; voices?: number } = {}): Audio {
  return poly(
    notes,
    (n) => {
      const env = adsr(n.gate, { attack: secs(0.4), decay: secs(0.5), sustain: 0.8, release: secs(0.8) });
      const tone = add(osc.saw(add(n.pitch, -0.07)), osc.saw(add(n.pitch, 0.07)));
      return vca(svf.lowpass(tone, o.cutoff ?? hz(1800), 0.7), scale(env, scale(n.velocity, 0.35)));
    },
    { voices: o.voices ?? 12 },
  );
});
