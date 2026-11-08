import { track } from "@m2/core";
import { lfo } from "std/control";
import { adsr, mono, noise, osc, vca } from "std/dsp";
import { bandpass, highpass } from "std/filter";
import { scale, sum, xrange } from "std/math";

// A made-up voice: a buzz at each note's pitch through two formants, its second gliding between an
// "oo" and an "ee" every 4 beats, and a breath of noise over 5 kHz, its S. Each sound starts with its
// note, so nothing of it runs while the song is stopped.
export default track("vox", { name: "Vox" }, ({ lane, time }) => {
  const second = xrange(lfo(time.beats, { period: 4 }), 800, 2300);
  return mono(lane.notes("words"), (n) => {
    const buzz = osc.saw(n.pitch);
    const vowel = sum(bandpass(buzz, 550, 5), bandpass(buzz, second, 7));
    return vca(sum(vowel, scale(highpass(noise(5), 5000), 0.2)), adsr(n.gate, { attack: 0.01, release: 0.06 }));
  });
});
