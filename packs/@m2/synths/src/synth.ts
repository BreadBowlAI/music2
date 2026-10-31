// @m2/synths/synth: the polysynth, and the instruments voiced on it.
//
// One engine, the pack's own `@m2/synths:synth` kernel (built from kernels-src/): a unison stack of
// saws and pulses with a sub and noise, a drive, a 4-pole ladder lowpass with its own envelope, then the amp envelope.
// Each instrument here is that engine with a patch of plain numbers and a few knobs;
// `synth.custom` has every knob. A whole voice is one kernel, which costs far less than
// the same sound built from small ones, and the kernel smooths its own knobs, so none
// here is `.smooth()`.
//
// `voices: 1` plays one note at a time: a note that starts while the one before is still
// held carries on from it without starting the envelopes again (legato), sliding to its
// pitch over `glide` where the instrument has one, and when it ends while an older note is
// still held, that one sounds again. With more voices, each note gets its own and `glide`
// does nothing.
//
// `velSens` is how much velocity matters: the level follows velocity^(2 × velSens), so a
// note at velocity 0 is silent, and at 0.5 one at 0.3 is 10 dB below one at 1.
//
// Each sound's card is put in order by its groups (CARD-17): what it sounds like first, then its
// filter and its loudness over time, each with a picture to drag (CARD-16): the filter's curve over
// its cutoff and resonance, the envelope over its attack, decay, sustain and release. `shape` (a
// menu) says what the stack plays: its saws and pulses (the sound as it was made), or a square, a
// triangle or a sine alone, as loud (CARD-32).

import { component, control, kernel, poly, port, setting, signature, time, view, type Audio, type Events, type Params, type Value } from "@m2/core";
import { mono, type Voice } from "std/dsp";
import { scale } from "std/math";

/** One note at a time with `glide` when `voices` is 1, else one voice per note. */
function play(notes: Events, voices: number, glide: Value, voice: (note: Voice) => Audio): Audio {
  return voices === 1 ? mono(notes, voice, { glide }) : poly(notes, voice, { voices });
}

/** The engine for a note, with a patch of kernel inputs (see `@m2/synths:synth` in the pack's `m2pkg.toml`). The mod wheel deepens its vibrato. */
function engine(n: Voice, patch: Record<string, Value>): Audio {
  return kernel("@m2/synths:synth", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, wheel: n.modWheel, ...patch });
}

// Ranges every instrument here shares, so the same knob turns the same way everywhere.
const CUTOFF = (hz: number) => control.hz(hz).range(20, 20000).log();
const TIME = (s: number) => control.secs(s).range(0.001, 10).log();
// Logarithmic, so the useful slides (20 to 150 ms) take up the middle of the knob, not its bottom 15%.
const GLIDE = (ms: number) => control.ms(ms).range(5, 1000).log();
const VEL_SENS = (v: number) => control.frac(v).range(0, 1).label("vel sens");
const RESONANCE = (r: number) => control.frac(r).range(0, 1);
const LEVEL01 = (v: number) => control.frac(v).range(0, 1);
/** The stack's main wave, a menu: the saw is the sound as it always was. */
const SHAPE = control.choice(["saw", "square", "triangle", "sine"], "saw");
/** The pictures every sound with a filter and a whole envelope has. */
const AMP = view.envelope("attack", "decay", "sustain", "release");
const FILTER = view.filter("cutoff", "resonance");

export const synth = {
  /** The whole engine, every knob out: start here to design a sound of your own. */
  custom: component(
    {
      id: "synth-custom",
      label: "Polysynth",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        // Oscillators: the stack's wave and how much of it, pulse (and its width), sub sine and noise.
        shape: SHAPE,
        saw: control.frac(1).range(0, 1).label("saw level"),
        square: control.frac(0).range(0, 1).label("pulse level"),
        width: control.frac(0.5).range(0.05, 0.95).label("pulse width"),
        sub: control.frac(0).range(0, 1),
        noise: control.frac(0).range(0, 1),
        // The unison stack: how far apart its voices are, and how wide they spread.
        detune: control.frac(0.2).range(0, 1),
        spread: control.frac(0.5).range(0, 1),
        // The filter, and how far its envelope opens it, in octaves.
        cutoff: CUTOFF(8000),
        resonance: control.frac(0.1).range(0, 1),
        keytrack: control.frac(0.5).range(0, 1),
        filterEnv: control.scalar(0).range(-4, 8).label("filter env (oct)"),
        filterAttack: TIME(0.002).label("filter attack"),
        filterDecay: TIME(0.3).label("filter decay"),
        filterSustain: control.frac(0).range(0, 1).label("filter sustain"),
        // The amp envelope.
        attack: TIME(0.003),
        decay: TIME(0.5),
        sustain: control.frac(0.8).range(0, 1),
        release: TIME(0.3),
        drive: control.db(0).range(0, 24),
        // Pitch: a vibrato that fades in, a drop into each note, and with `voices: 1` a slide
        // between overlapping notes.
        vibrato: control.semis(0).range(0, 2),
        drop: control.semis(0).range(0, 48),
        dropTime: control.secs(0.04).range(0.005, 1).log().label("drop time"),
        glide: GLIDE(50).label("glide (mono)"),
        // How much velocity changes the level and the filter envelope.
        velSens: VEL_SENS(0.5),
        level: control.db(0).range(-24, 12),
      },
      // Read when a note starts: unison voices (1 to 7), the sub's octave (0, -1 or -2) and
      // the seed of the voices' start phases and noise.
      settings: { voices: setting.int(8), unison: setting.int(1), subOctave: setting.int(-1), seed: setting.number(1) },
      output: port.audio(),
      // The + Track list (PKG-7) puts the ready-made sounds first, and this, with every knob, last.
      editor: {
        category: "Sound design",
        description: "The polysynth with every knob: saws, pulses, a sub, a ladder filter, envelopes",
        views: { amp: AMP, filter: FILTER },
        groups: {
          Oscillators: ["shape", "saw", "square", "width", "sub", "subOctave", "noise", "unison", "detune", "spread"],
          Filter: ["filter", "cutoff", "resonance", "keytrack", "filterEnv", "filterAttack", "filterDecay", "filterSustain", "drive"],
          Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens", "level"],
          Pitch: ["vibrato", "drop", "dropTime", "glide"],
        },
      },
    },
    (notes: Events, p: Params): Audio =>
      play(notes, p.voices, p.glide, (n) =>
        engine(n, {
          shape: p.shape,
          saw: p.saw,
          square: p.square,
          width: p.width,
          sub: p.sub,
          sub_octave: p.subOctave,
          noise: p.noise,
          unison: p.unison,
          detune: p.detune,
          spread: p.spread,
          drop: p.drop,
          drop_time: p.dropTime,
          vibrato: p.vibrato,
          cutoff: p.cutoff,
          resonance: p.resonance,
          keytrack: p.keytrack,
          env: p.filterEnv,
          f_attack: p.filterAttack,
          f_decay: p.filterDecay,
          f_sustain: p.filterSustain,
          attack: p.attack,
          decay: p.decay,
          sustain: p.sustain,
          release: p.release,
          drive: p.drive,
          vel_amount: p.velSens,
          level: p.level,
          seed: p.seed,
        }),
      ),
  ),

  /** Bright, wide seven-saw stack, for big chords and hooks: the filter starts open, as on a JP-8000. */
  supersaw: component(
    {
      id: "synth-supersaw",
      label: "Supersaw",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        detune: control.frac(0.35).range(0, 1),
        spread: control.frac(0.85).range(0, 1),
        cutoff: CUTOFF(20000),
        resonance: RESONANCE(0.05),
        attack: TIME(0.003),
        decay: TIME(0.4),
        sustain: LEVEL01(0.85),
        release: TIME(0.35),
        velSens: VEL_SENS(0.4),
      },
      settings: { voices: setting.int(8) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "Seven detuned saws, bright and wide: big chords and hooks",
        views: { amp: AMP, filter: FILTER },
        groups: { Sound: ["shape", "detune", "spread"], Filter: ["filter", "cutoff", "resonance"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { shape, detune, spread, cutoff, resonance, attack, decay, sustain, release, velSens, voices }: Params): Audio =>
      play(notes, voices, 0, (n) =>
        engine(n, {
          shape,
          unison: 7,
          detune,
          spread,
          sub: 0.1,
          cutoff,
          resonance,
          keytrack: 0.3,
          // A little bite as each note starts, wherever the cutoff is turned down to.
          env: 0.5,
          f_decay: 0.25,
          attack,
          decay,
          sustain,
          release,
          vel_amount: velSens,
          level: -4.5,
        }),
      ),
  ),

  /** A singing lead, one note at a time: glides between overlapping notes, vibrato that fades in. */
  lead: component(
    {
      id: "synth-lead",
      label: "Lead",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        cutoff: CUTOFF(3500),
        resonance: RESONANCE(0.3),
        glide: GLIDE(60),
        vibrato: control.semis(0.15).range(0, 2),
        drive: control.db(4).range(0, 24),
        attack: TIME(0.004),
        decay: TIME(0.3),
        sustain: LEVEL01(0.9),
        release: TIME(0.18),
        velSens: VEL_SENS(0.4),
      },
      settings: { voices: setting.int(1) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "A singing mono lead that slides between overlapping notes",
        views: { amp: AMP, filter: FILTER },
        groups: { Sound: ["shape", "glide", "vibrato", "drive"], Filter: ["filter", "cutoff", "resonance"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { shape, cutoff, resonance, glide, vibrato, drive, attack, decay, sustain, release, velSens, voices }: Params): Audio =>
      play(notes, voices, glide, (n) =>
        engine(n, {
          shape,
          square: 0.5,
          width: 0.45,
          unison: 3,
          detune: 0.12,
          spread: 0.4,
          vibrato,
          cutoff,
          resonance,
          keytrack: 0.6,
          env: 2,
          f_decay: 0.25,
          f_sustain: 0.3,
          attack,
          decay,
          sustain,
          release,
          drive,
          vel_amount: velSens,
          level: 3,
        }),
      ),
  ),

  /** A big, slow, wide pad that swells open. */
  pad: component(
    {
      id: "synth-pad",
      label: "Synth pad",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        cutoff: CUTOFF(2500),
        resonance: RESONANCE(0.1),
        attack: TIME(0.6),
        decay: TIME(0.5),
        sustain: LEVEL01(0.9),
        release: TIME(1.6),
        spread: control.frac(0.9).range(0, 1),
        velSens: VEL_SENS(0.35),
      },
      // Enough that chords changing every beat never cut off a tail you can still hear.
      settings: { voices: setting.int(16) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "A big, slow, wide pad that swells open",
        views: { amp: AMP, filter: FILTER },
        groups: { Sound: ["shape", "spread"], Filter: ["filter", "cutoff", "resonance"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { shape, cutoff, resonance, attack, decay, sustain, release, spread, velSens, voices }: Params): Audio =>
      play(notes, voices, 0, (n) =>
        engine(n, {
          shape,
          square: 0.25,
          width: 0.3,
          noise: 0.03,
          unison: 5,
          detune: 0.25,
          spread,
          cutoff,
          resonance,
          keytrack: 0.3,
          // The filter opens more slowly than the sound fades in.
          env: 1.5,
          f_attack: 1.2,
          f_sustain: 0.5,
          attack,
          decay,
          sustain,
          release,
          vel_amount: velSens,
          level: -2,
        }),
      ),
  ),

  /** A wide pluck for arps and chords: a filter that snaps shut, brighter when played harder. */
  pluck: component(
    {
      id: "synth-pluck",
      label: "Synth pluck",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        // Where the filter starts on a hard hit; it closes to an eleventh of it.
        brightness: CUTOFF(4500).range(200, 20000),
        attack: TIME(0.003),
        // The filter closes in 0.6 of it.
        decay: control.secs(0.45).range(0.05, 5).log(),
        sustain: LEVEL01(0),
        release: TIME(0.25),
        spread: control.frac(0.85).range(0, 1),
        velSens: VEL_SENS(0.5),
      },
      settings: { voices: setting.int(12) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "A bright, wide pluck for arps: a filter that snaps shut",
        // No filter picture: the brightness is where the filter starts, not a cutoff that holds.
        views: { amp: AMP },
        groups: { Sound: ["shape", "brightness", "spread"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { shape, brightness, spread, attack, decay, sustain, release, velSens, voices }: Params): Audio => {
      // 3.49 octaves of envelope: from the brightness down to an eleventh of it.
      const cutoff = scale(brightness, 1 / 11.25);
      const fall = scale(decay, 0.6);
      return play(notes, voices, 0, (n) =>
        engine(n, {
          shape,
          square: 0.3,
          unison: 3,
          detune: 0.2,
          spread,
          cutoff,
          keytrack: 0.7,
          env: 3.49,
          f_decay: fall,
          attack,
          decay,
          sustain,
          release,
          vel_amount: velSens,
          level: 5.5,
        }),
      );
    },
  ),

  /** An 808 bass: a sine that drops into each note and rings out over `decay`, driven, sliding between overlapping notes. */
  sub: component(
    {
      id: "synth-sub",
      label: "808 sub",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        // How far each note drops into its pitch, in semitones.
        drop: control.semis(7).range(0, 24),
        // Warmth: enough third harmonic that the note carries on small speakers.
        drive: control.db(18).range(0, 24),
        glide: GLIDE(80),
        attack: TIME(0.003),
        // How long a held note rings, to -60 dB: short for tight hits, long for slides.
        decay: control.secs(4).range(0.1, 10).log(),
        sustain: LEVEL01(0),
        release: TIME(0.12),
        velSens: VEL_SENS(0.3),
      },
      settings: { voices: setting.int(1) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "An 808 bass: a deep sine with a punchy drop that rings out, sliding between notes",
        // A sine with no filter: no wave to pick, and no filter to draw.
        views: { amp: AMP },
        groups: { Sound: ["drop", "drive", "glide"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { drop, drive, glide, attack, decay, sustain, release, velSens, voices }: Params): Audio =>
      play(notes, voices, glide, (n) =>
        engine(n, {
          saw: 0,
          sub: 1,
          sub_octave: 0,
          drop,
          drop_time: 0.03,
          cutoff: 20000,
          keytrack: 0,
          // Fast but not instant: a retriggered note (from where its envelope is) re-punches without a tick.
          attack,
          decay,
          sustain,
          release,
          drive,
          vel_amount: velSens,
          level: 1.5,
        }),
      ),
  ),

  /** A synth bass, one note at a time: a saw and a pulse with a sine under them, and a filter that bites on each note. */
  bass: component(
    {
      id: "synth-bass",
      label: "Synth bass",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        cutoff: CUTOFF(350),
        resonance: RESONANCE(0.3),
        // How far the filter opens as each note starts, in octaves above the cutoff.
        bite: control.scalar(3).range(0, 6).label("bite (oct)"),
        // How fast it closes again.
        biteDecay: control.secs(0.2).range(0.02, 2).log().label("bite decay"),
        glide: GLIDE(40),
        attack: TIME(0.002),
        decay: TIME(0.5),
        sustain: LEVEL01(0.8),
        release: TIME(0.08),
        velSens: VEL_SENS(0.4),
      },
      settings: { voices: setting.int(1) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "A punchy mono synth bass: saw, pulse and sine, a filter that bites, glide",
        views: { amp: AMP, filter: FILTER },
        groups: {
          Sound: ["shape", "glide"],
          Filter: ["filter", "cutoff", "resonance", "bite", "biteDecay"],
          Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"],
        },
      },
    },
    (notes: Events, { shape, cutoff, resonance, bite, biteDecay, glide, attack, decay, sustain, release, velSens, voices }: Params): Audio =>
      play(notes, voices, glide, (n) =>
        engine(n, {
          shape,
          // A sine under the saw and pulse at the note itself: an octave down would be below hearing.
          saw: 1,
          square: 0.4,
          sub: 0.5,
          sub_octave: 0,
          cutoff,
          resonance,
          keytrack: 0.5,
          env: bite,
          f_attack: 0.001,
          f_decay: biteDecay,
          attack,
          decay,
          sustain,
          release,
          drive: 6,
          vel_amount: velSens,
          level: 1.5,
        }),
      ),
  ),

  /** A string machine: slow, slightly detuned saws through an ensemble chorus. */
  strings: component(
    {
      id: "synth-strings",
      label: "Strings",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        shape: SHAPE,
        cutoff: CUTOFF(4000),
        resonance: RESONANCE(0.1),
        attack: TIME(0.25),
        decay: TIME(0.5),
        sustain: LEVEL01(0.9),
        release: TIME(0.7),
        // How much of the ensemble chorus is heard.
        ensemble: control.frac(0.5).range(0, 1),
        velSens: VEL_SENS(0.35),
      },
      settings: { voices: setting.int(16) },
      output: port.audio(),
      editor: {
        category: "Instruments",
        description: "A string machine: soft saws through a lush ensemble chorus",
        views: { amp: AMP, filter: FILTER },
        groups: { Sound: ["shape", "ensemble"], Filter: ["filter", "cutoff", "resonance"], Loudness: ["amp", "attack", "decay", "sustain", "release", "velSens"] },
      },
    },
    (notes: Events, { shape, ensemble, cutoff, resonance, attack, decay, sustain, release, velSens, voices }: Params): Audio => {
      const v = play(notes, voices, 0, (n) =>
        engine(n, {
          shape,
          unison: 4,
          detune: 0.18,
          spread: 0.8,
          vibrato: 0.08,
          cutoff,
          resonance,
          keytrack: 0.5,
          attack,
          decay,
          sustain,
          release,
          vel_amount: velSens,
          level: 0,
        }),
      );
      // std's chorus kernel, as @m2/effects' chorus plays it: a pack never imports another (PACK-3).
      return kernel("std:chorus", { in: v, rate: 0.6, depth: 0.6, mix: ensemble, playing: time.playing });
    },
  ),
};
