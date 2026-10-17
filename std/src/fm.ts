// std/fm: FM keys, bells and metal.
//
// One engine, the `std:fm` kernel: two pairs of sine operators, each a modulator into a
// carrier, mixed; the first modulator feeds back on itself. Each pair's brightness (how far
// its modulator bends its carrier) falls after the note starts, and rises with velocity and
// `tone`, as on a DX electric piano. The carriers and modulators are frequency ratios to the
// note: whole numbers sound harmonic, others clang like a bell. `fm.custom` has every knob;
// the others are patches of it. The kernel smooths its own knobs, so none here is
// `.smooth()`. `voices: 1` plays one note at a time, as in std/synth.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Events, type Params, type Value } from "@m2/core";
import { mono, type Voice } from "./synth";

/** The engine for a note, with a patch of kernel inputs (see `std:fm` in `m2pkg.toml`). */
function engine(n: Voice, patch: Record<string, Value>): Audio {
  return kernel("std:fm", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, ...patch });
}

/** One note at a time (legato, without a slide) when `voices` is 1, else one voice per note. */
function play(notes: Events, voices: number, voice: (note: Voice) => Audio): Audio {
  return voices === 1 ? mono(notes, voice) : poly(notes, voice, { voices });
}

// Ranges the patches share.
const RATIO = (r: number) => control.scalar(r).range(0.5, 16).log();
const AMOUNT = (i: number) => control.scalar(i).range(0, 8);
const TIME = (s: number) => control.secs(s).range(0.001, 10).log();
const VEL_SENS = (v: number) => control.frac(v).range(0, 1).label("vel sens");

export const fm = {
  /** The whole FM engine, every knob out: start here for a sound of your own. */
  custom: component(
    {
      id: "fm-custom",
      label: "FM synth",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        // Pair A: the carrier's and the modulator's frequency ratios to the note, how far the
        // modulator bends the carrier as the note starts (the FM index), and how long that
        // brightness takes to mellow.
        carrierA: RATIO(1).label("carrier A"),
        modulatorA: RATIO(1).label("modulator A"),
        amountA: AMOUNT(1.5).label("amount A"),
        brightDecayA: TIME(1).label("bright decay A"),
        // Pair B, the same, and how much of it is heard.
        carrierB: RATIO(1).label("carrier B"),
        modulatorB: control.scalar(14).range(0.5, 32).log().label("modulator B"),
        amountB: AMOUNT(0.8).label("amount B"),
        brightDecayB: TIME(0.1).label("bright decay B"),
        mixB: control.frac(0.35).range(0, 1).label("mix B"),
        // Modulator A's feedback (toward a saw), pair B's detune and the brightness.
        feedback: control.frac(0).range(0, 1),
        detune: control.scalar(0).range(0, 20).label("detune (cents)"),
        tone: control.frac(0.5).range(0, 1),
        attack: TIME(0.002),
        decay: control.secs(3).range(0.01, 30).log(),
        sustain: control.frac(0).range(0, 1),
        release: TIME(0.4),
        // A drop into each note, for zaps and metal hits.
        drop: control.semis(0).range(0, 48),
        dropTime: control.secs(0.04).range(0.005, 1).log().label("drop time"),
        // How much velocity changes the level and the brightness.
        velSens: VEL_SENS(0.7),
        level: control.db(0).range(-24, 12),
      },
      settings: { voices: setting.int(12) },
      output: port.audio(),
      // The + Track list (PKG-7) puts the ready-made sounds first, and this, with every knob, last.
      editor: { category: "Sound design", description: "The FM synth with every knob: two operator pairs, feedback, envelopes" },
    },
    (notes: Events, p: Params): Audio =>
      play(notes, p.voices, (n) =>
        engine(n, {
          ratio_a: p.carrierA,
          mod_a: p.modulatorA,
          index_a: p.amountA,
          fall_a: p.brightDecayA,
          ratio_b: p.carrierB,
          mod_b: p.modulatorB,
          index_b: p.amountB,
          fall_b: p.brightDecayB,
          mix_b: p.mixB,
          feedback: p.feedback,
          detune: p.detune,
          tone: p.tone,
          attack: p.attack,
          decay: p.decay,
          sustain: p.sustain,
          release: p.release,
          drop: p.drop,
          drop_time: p.dropTime,
          vel_amount: p.velSens,
          level: p.level,
        }),
      ),
  ),

  /** An electric piano: a soft, round body and a bright tine that barks when played hard. */
  keys: component(
    {
      id: "fm-keys",
      label: "FM keys",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        tone: control.frac(0.5).range(0, 1),
        // How much of the tine (the bell-like attack) is heard.
        tine: control.frac(0.35).range(0, 1),
        decay: control.secs(3.5).range(0.2, 12).log(),
        release: TIME(0.35),
        // Wide by default: an electric piano lives on its dynamics.
        velSens: VEL_SENS(0.8),
      },
      settings: { voices: setting.int(12) },
      output: port.audio(),
      editor: { category: "Instruments", description: "An electric piano: a warm body and a tine that barks when played hard" },
    },
    (notes: Events, { tone, tine, decay, release, velSens, voices }: Params): Audio =>
      play(notes, voices, (n) =>
        engine(n, {
          // The body: 1:1, mellowing over a second.
          index_a: 1.8,
          fall_a: 1.2,
          // The tine: a fourteenth harmonic, gone in a moment.
          mod_b: 14,
          index_b: 0.9,
          fall_b: 0.07,
          mix_b: tine,
          detune: 3,
          tone,
          decay,
          release,
          vel_amount: velSens,
        }),
      ),
  ),

  /** A glassy bell: inharmonic, bright and long. */
  bell: component(
    {
      id: "fm-bell",
      label: "FM bell",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        tone: control.frac(0.5).range(0, 1),
        decay: control.secs(4).range(0.3, 15).log(),
        velSens: VEL_SENS(0.6),
      },
      settings: { voices: setting.int(12) },
      output: port.audio(),
      editor: { category: "Instruments", description: "A glassy bell: bright, shimmering and long" },
    },
    (notes: Events, { tone, decay, velSens, voices }: Params): Audio =>
      play(notes, voices, (n) =>
        engine(n, {
          // A modulator at 3.5× gives the bell its inharmonic partials.
          mod_a: 3.5,
          index_a: 2,
          fall_a: 2.5,
          // A shimmer an octave and a fifth up.
          ratio_b: 3,
          mod_b: 1,
          index_b: 0.6,
          fall_b: 1.5,
          mix_b: 0.45,
          tone,
          decay,
          sustain: 0,
          release: 0.6,
          vel_amount: velSens,
        }),
      ),
  ),

  /** Metallic FM percussion: a short, clangy, inharmonic hit that zaps down into its note. */
  metal: component(
    {
      id: "fm-metal",
      label: "FM metal",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        tone: control.frac(0.5).range(0, 1),
        decay: control.secs(0.5).range(0.05, 4).log(),
        // Modulator A on itself: from a clang to a grind.
        feedback: control.frac(0.35).range(0, 1),
        // How far each hit drops into its note, in semitones: a zap.
        drop: control.semis(7).range(0, 36),
        velSens: VEL_SENS(0.5),
      },
      settings: { voices: setting.int(8) },
      output: port.audio(),
      // Percussion, so it plays from a drum lane and is listed with the drums.
      editor: { category: "Drums", description: "Metallic FM percussion: a clangy, inharmonic hit with a zap" },
    },
    (notes: Events, { tone, decay, feedback, drop, velSens, voices }: Params): Audio =>
      play(notes, voices, (n) =>
        engine(n, {
          // Ratios that share no harmonics, so the partials clang.
          mod_a: 3.17,
          index_a: 4,
          fall_a: 0.3,
          ratio_b: 2,
          mod_b: 7.13,
          index_b: 2.5,
          fall_b: 0.12,
          mix_b: 0.4,
          feedback,
          tone,
          drop,
          drop_time: 0.02,
          attack: 0.0005,
          // A hit rings for its decay however short its note: the release goes at the same rate.
          decay,
          sustain: 0,
          release: decay,
          vel_amount: velSens,
        }),
      ),
  ),
};
