// @m2/synths/simple: the first instruments, built only from std's blocks.
//
// Each one takes a notes lane and returns audio. They are ordinary components, so copy one into
// your project and change it if you want something different: short enough to read, they show
// how a sound is made from std/dsp. The polysynth's and the drum machine's sound fuller, so these
// have no `editor.category`: the lists leave them out (PKG-7), and code plays them as before.

import { component, control, hz, kernel, ms, poly, port, secs, setting, signature, type Audio, type Events, type Params, type Value } from "@m2/core";
import { adsr, noise, osc, svf, vca } from "std/dsp";
import { add, scale, xrange } from "std/math";

/** Soft saturation, `amount` dB of drive: std's drive kernel, as @m2/effects' drive plays it (a pack never imports another, PACK-3). */
function drive(x: Audio, amount: Value): Audio {
  return kernel("std:drive", { in: x, amount });
}

/** A punchy bass: saw plus a sub square, through a lowpass that opens on each note. */
export const bass = component(
  {
    id: "bass",
    label: "Bass",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    // Both reach kernels that read them every sample (the filter, the drive), so they ramp (PKG-14).
    controls: {
      cutoff: control.hz(600).range(20, 20000).log().smooth(),
      drive: control.db(4).range(0, 36).smooth(),
    },
    settings: { voices: setting.int(4) },
    output: port.audio(),
    editor: { description: "A simple saw bass built from the blocks in std/dsp; synth.bass and synth.sub sound fuller" },
  },
  // `p.drive`, not a destructured `drive`, which would hide the `drive` effect.
  function bass(notes: Events, p: Params): Audio {
    const v = poly(
      notes,
      (n) => {
        const body = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, -12)), 0.5));
        const env = adsr(n.gate, { attack: ms(3), decay: secs(0.3), sustain: 0.6, release: ms(80) });
        const fenv = adsr(n.gate, { attack: ms(2), decay: secs(0.18), sustain: 0.2, release: ms(80) });
        // The cutoff, opened up to 4× on each note.
        const f = svf.lowpass(body, scale(p.cutoff, xrange(fenv, 1, 4)), 1.1);
        return vca(f, scale(env, n.velocity));
      },
      { voices: p.voices },
    );
    return drive(v, p.drive);
  },
);

/** A sine kick with a fast pitch drop. */
export const kick = component(
  {
    id: "kick",
    label: "Kick",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: {
      tune: control.hz(48).range(20, 200).log(),
      decay: control.secs(0.32).range(0.01, 2).log(),
    },
    output: port.audio(),
    editor: { description: "A simple sine kick built from the blocks in std/dsp; drums.kick is the drum machine's" },
  },
  function kick(notes: Events, { tune, decay }: Params): Audio {
    return poly(
      notes,
      (n) => {
        const amp = adsr(n.gate, { attack: ms(1), decay, sustain: 0, release: ms(60) });
        const drop = adsr(n.gate, { attack: ms(1), decay: ms(45), sustain: 0, release: ms(30) });
        const f = xrange(drop, tune, hz(170));
        return drive(vca(osc.sine(f), scale(amp, n.velocity)), 3);
      },
      { voices: 2 },
    );
  },
);

/** Closed hi-hat: highpassed noise with a short envelope. */
export const hat = component(
  {
    id: "hat",
    label: "Hat",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { decay: control.ms(45).range(5, 1000).log() },
    output: port.audio(),
    editor: { description: "A simple noise hat built from the blocks in std/dsp; drums.hats is the drum machine's" },
  },
  function hat(notes: Events, { decay }: Params): Audio {
    return poly(
      notes,
      (n) => {
        const env = adsr(n.gate, { attack: ms(1), decay, sustain: 0, release: ms(30) });
        return vca(svf.highpass(noise(7), hz(7500), 0.9), scale(env, scale(n.velocity, 0.5)));
      },
      { voices: 4 },
    );
  },
);

/** A bright pluck for chords and arps. */
export const pluck = component(
  {
    id: "pluck",
    label: "Pluck",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    // It sets the filter's cutoff, which the kernel reads every sample, so it ramps (PKG-14).
    controls: { brightness: control.hz(5000).range(200, 20000).log().smooth() },
    settings: { voices: setting.int(12) },
    output: port.audio(),
    editor: { description: "A simple pluck built from the blocks in std/dsp; synth.pluck is wider and brighter" },
  },
  function pluck(notes: Events, { brightness, voices }: Params): Audio {
    return poly(
      notes,
      (n) => {
        const env = adsr(n.gate, { attack: ms(2), decay: secs(0.35), sustain: 0, release: secs(0.2) });
        const tone = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, 12)), 0.3));
        const f = svf.lowpass(tone, xrange(env, hz(400), brightness), 0.8);
        return vca(f, scale(env, scale(n.velocity, 0.6)));
      },
      { voices },
    );
  },
);

/** A soft pad: two detuned saws, slow envelope, gentle filter. */
export const pad = component(
  {
    id: "pad",
    label: "Pad",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { cutoff: control.hz(1800).range(20, 20000).log().smooth() },
    settings: { voices: setting.int(12) },
    output: port.audio(),
    editor: { description: "A simple two-saw pad built from the blocks in std/dsp; synth.pad is bigger" },
  },
  function pad(notes: Events, { cutoff, voices }: Params): Audio {
    return poly(
      notes,
      (n) => {
        const env = adsr(n.gate, { attack: secs(0.4), decay: secs(0.5), sustain: 0.8, release: secs(0.8) });
        const tone = add(osc.saw(add(n.pitch, -0.07)), osc.saw(add(n.pitch, 0.07)));
        return vca(svf.lowpass(tone, cutoff, 0.7), scale(env, scale(n.velocity, 0.35)));
      },
      { voices },
    );
  },
);
