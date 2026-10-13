// Every std component called once (step 5's gates): some controls written, some connected to a
// signal, some left out to play their declared defaults.
import { semis, song } from "@m2/core";
import { choose, lfo, when } from "std/control";
import { adsr, follow, gain, noise, osc, pan, svf, vca } from "std/dsp";
import { bandpass, highpass, lowpass, notch } from "std/filter";
import { bitcrush, chorus, compress, delay, drive, duck, eq3, limit, phaser, reverb, tremolo, widen } from "std/fx";
import { bass, hat, kick, pad, pluck } from "std/instruments";
import { add, blend, mtof, range, scale, sum, xrange } from "std/math";
import { master, mixer } from "std/mix";

export default song(({ lane, time }) => {
  const sweep = lfo(time.beats, { period: 2 });
  const wobble = xrange(sweep, 300, 3000);
  const level = range(sweep, 0.2, 0.8);
  const tone = osc.sine(mtof(semis(48)));
  const buzz = osc.saw(semis(60));
  const pulse = osc.square(110);
  const soft = osc.tri();
  const env = adsr(lane.mask("gate"), { attack: 0.01 });
  const hit = kick(lane.notes("kick"));
  const side = follow(hit);
  const drums = add(hit, hat(lane.notes("hats")));
  const synth = sum(bass(lane.notes("bass"), { cutoff: wobble }), pluck(lane.notes("chords")), pad(lane.notes("pad")));
  const filtered = notch(bandpass(highpass(lowpass(synth, 1200), 200, 0.9), wobble), 3000);
  const shaped = svf.highpass(svf.bandpass(svf.lowpass(buzz, 800), 900), 100);
  const voiced = vca(add(tone, add(pulse, add(soft, noise(7)))), env);
  const picked = choose(lane.mask("pick"), when(lane.mask("open"), blend(shaped, voiced, level)), scale(filtered, 0.5));
  const wet = widen(bitcrush(eq3(compress(tremolo(phaser(chorus(reverb(delay(drive(limit(picked, -3)), { time: 0.3 })))))))));
  const fx = duck(pan(gain(wet, -3), 0.2), side, { depth: -6 });
  return master(mixer({ drums, fx: { in: fx, db: -4 }, synth: { in: synth, pan: -0.3 } }));
});
