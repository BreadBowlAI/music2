// Every component of std and of the packs music2 ships called once (step 5's gates): some controls
// written, some connected to a signal, some left out to play their declared defaults.
import { semis, song } from "@m2/core";
import { choose, lfo, when } from "std/control";
import { adsr, follow, gain, mono, noise, osc, pan, svf, vca } from "std/dsp";
import { bandpass, highpass, lowpass, notch } from "std/filter";
import { drums } from "@m2/drums";
import { bass, fm, pad, pluck, synth } from "@m2/synths";
import { bitcrush, chorus, delay, drive, phaser, reverb, tremolo, widen } from "@m2/effects";
import { compress, duck, eq3, limit } from "std/fx";
import { add, blend, mtof, range, scale, sum, xrange } from "std/math";
import { master, mixer } from "std/mix";
import { sampler } from "std/sampler";

export default song(({ lane, time }) => {
  const sweep = lfo(time.beats, { period: 2 });
  const wobble = xrange(sweep, 300, 3000);
  const level = range(sweep, 0.2, 0.8);
  const tone = osc.sine(mtof(semis(48)));
  const buzz = osc.saw(semis(60));
  const pulse = osc.square(110);
  const soft = osc.tri();
  const env = adsr(lane.mask("gate"), { attack: 0.01 });
  const hit = drums.drum(lane.notes("kick"));
  const side = follow(hit);
  const beat = add(hit, drums.drum(lane.notes("hats"), { drum: "hats" }));
  const tones = sum(bass(lane.notes("bass"), { cutoff: wobble }), pluck(lane.notes("chords")), pad(lane.notes("pad")));
  const filtered = notch(bandpass(highpass(lowpass(tones, 1200), 200, 0.9), wobble), 3000);
  const shaped = svf.highpass(svf.bandpass(svf.lowpass(buzz, 800), 900), 100);
  const voiced = vca(add(tone, add(pulse, add(soft, noise(7)))), env);
  const picked = choose(lane.mask("pick"), when(lane.mask("open"), blend(shaped, voiced, level)), scale(filtered, 0.5));
  const wet = widen(bitcrush(eq3(compress(tremolo(phaser(chorus(reverb(delay(drive(limit(picked, -3)), { time: 0.3 })))))))));
  const fx = duck(pan(gain(wet, -3), 0.2), side, { depth: -6 });
  const saws = sum(
    synth.supersaw(lane.notes("saws"), { cutoff: wobble, detune: 0.5 }),
    synth.pad(lane.notes("swell"), { attack: 1.2 }),
    synth.pluck(lane.notes("arp")),
    synth.strings(lane.notes("strings"), { ensemble: level }),
    synth.custom(lane.notes("custom"), { filterEnv: 2, unison: 5 }),
  );
  const lines = sum(
    synth.lead(lane.notes("lead"), { glide: 0.08 }),
    synth.sub(lane.notes("sub"), { drop: semis(12) }),
    synth.bass(lane.notes("bass2"), { bite: 4 }),
    mono(lane.notes("line"), (n) => osc.saw(n.pitch), { glide: 0.05 }),
  );
  const keys = sum(fm.keys(lane.notes("keys"), { tone: level }), fm.bell(lane.notes("bell"), { decay: 6 }), fm.custom(lane.notes("fm"), { carrierA: 2 }));
  const metal = fm.metal(lane.notes("metal"), { drop: semis(12) });
  const kit = sum(
    drums.kit(lane.notes("kit"), { kickTune: 52, hatLevel: 3 }),
    drums.drum(lane.notes("boom"), { decay: 1.2 }),
    drums.drum(lane.notes("snare"), { drum: "snare" }),
    drums.drum(lane.notes("clap"), { drum: "clap" }),
    drums.drum(lane.notes("hats2"), { drum: "hats", decay: 1.3 }),
    drums.drum(lane.notes("toms"), { drum: "tom" }),
    drums.drum(lane.notes("rim"), { drum: "rim" }),
    drums.drum(lane.notes("cowbell"), { drum: "cowbell" }),
    drums.drum(lane.notes("crash"), { drum: "cymbal" }),
    drums.drum(lane.notes("shaker"), { drum: "shaker" }),
  );
  const chops = sampler(lane.notes("chops"), { tune: 2, mode: "gate" });
  return master(mixer({ drums: beat, fx: { in: fx, db: -4 }, synth: { in: tones, pan: -0.3 }, saws, lines, keys, metal, kit, chops }));
});
