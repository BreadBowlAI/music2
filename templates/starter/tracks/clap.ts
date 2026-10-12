import { hz, ms, poly, track } from "@m2/core";
import { adsr, noise, svf, vca } from "std/dsp";
import { reverb } from "std/fx";
import { scale } from "std/math";

// A clap built from std parts: a burst of noise through a bandpass filter, in a small room.
export default track("Clap", ({ lane }) => {
  const clap = poly(
    lane.notes("hits"),
    (n) => {
      const env = adsr(n.gate, { attack: ms(1), decay: ms(160), sustain: 0, release: ms(80) });
      return vca(svf.bandpass(noise(3), hz(1500), 1.2), scale(env, scale(n.velocity, 2)));
    },
    { voices: 2 },
  );
  return reverb(clap, { size: 0.35, damping: 0.6, mix: 0.25 });
});
