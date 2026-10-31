// @m2/wavetable: music2's wavetable synth (PACK-27). `import { wavetable } from "@m2/wavetable"`.
//
// A wavetable is a set of waveforms, its frames, one after another; `position` slides between them,
// and as it moves the sound changes shape. Two oscillators, A and B, each play a table: one of five
// built in (basic, pulse, vowels, harmonics, digital), or your own, a WAV dropped on `fileA` or
// `fileB` (frames of 2048 samples one after another, as other wavetable synths write them; a file
// shorter than that is one cycle). A file, when there is one, is that oscillator's table, in place
// of the menu's. Each oscillator has its own pitch (in semitones) and level, and `unison` stacks up
// to 8 copies of both, `detune` semitones apart at the edges and `spread` across the stereo field.
// Then a filter (its shape a choice: lowpass, bandpass, highpass or notch), whose cutoff follows
// the key (`keytrack`), its own envelope (`filterEnv` octaves at its peak) and the LFO, and the amp
// envelope. The LFO, a sine from each note's start, moves both positions and the cutoff.
//
// A whole voice is one kernel, the pack's own (`@m2/wavetable:voice`): its tables are band-limited,
// so a high note doesn't fold back into a low hum, and are made once for all the voices of a sound.
// `voices: 1` plays one note at a time, sliding over `glide` between notes that overlap (std/dsp's
// `mono`); with more, each note gets its own voice and `glide` does nothing.

import { component, control, kernel, poly, port, setting, signature, view, type Audio, type Events, type Params } from "@m2/core";
import { mono, type Voice } from "std/dsp";

/** The built-in tables, in the order the kernel numbers them. */
const TABLES = ["basic", "pulse", "vowels", "harmonics", "digital"] as const;

/** A time, from a millisecond to 10 seconds, on a log scale so the short ones get room. */
const TIME = (s: number) => control.secs(s).range(0.001, 10).log();

export const wavetable = component(
  {
    id: "wavetable",
    label: "Wavetable",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: {
      // Where each oscillator is in its table (0 its first frame, 1 its last), its pitch and level.
      positionA: control.frac(0).range(0, 1).label("position A"),
      pitchA: control.semis(0).range(-24, 24).label("pitch A"),
      levelA: control.frac(1).range(0, 1).label("level A"),
      positionB: control.frac(0).range(0, 1).label("position B"),
      pitchB: control.semis(0).range(-24, 24).label("pitch B"),
      levelB: control.frac(0).range(0, 1).label("level B"),
      // The filter, and how far its envelope opens it, in octaves.
      filter: control.choice(["lowpass", "bandpass", "highpass", "notch"], "lowpass"),
      cutoff: control.hz(8000).range(20, 20000).log(),
      q: control.scalar(0.7).range(0.5, 12).log(),
      keytrack: control.frac(0).range(0, 1),
      filterEnv: control.scalar(0).range(-4, 8).label("filter env (oct)"),
      filterAttack: TIME(0.005).label("filter attack"),
      filterDecay: TIME(0.3).label("filter decay"),
      filterSustain: control.frac(0).range(0, 1).label("filter sustain"),
      filterRelease: TIME(0.3).label("filter release"),
      // The amp envelope.
      attack: TIME(0.005),
      decay: TIME(0.5),
      sustain: control.frac(0.8).range(0, 1),
      release: TIME(0.3),
      // The LFO: how fast, how far it moves both positions (each way), and the cutoff (octaves each way).
      lfoRate: control.hz(2).range(0.05, 20).log().label("lfo rate"),
      lfoPosition: control.frac(0).range(0, 1).label("lfo to position"),
      lfoCutoff: control.scalar(0).range(0, 4).label("lfo to cutoff (oct)"),
      // Unison: how far apart its outer voices are, and how wide they spread.
      detune: control.semis(0.1).range(0, 1),
      spread: control.frac(0.5).range(0, 1),
      // Logarithmic, so the useful slides (20 to 150 ms) take up the middle of the knob.
      glide: control.ms(50).range(5, 1000).log().label("glide (mono)"),
      velSens: control.frac(0.5).range(0, 1).label("vel sens"),
      level: control.db(0).range(-24, 12),
    },
    // What each oscillator plays, the unison voices (1 to 8) and the notes at once (1: one at a time).
    settings: {
      tableA: setting.choice(TABLES, "basic"),
      fileA: setting.asset().optional(),
      tableB: setting.choice(TABLES, "basic"),
      fileB: setting.asset().optional(),
      unison: setting.int(1),
      voices: setting.int(8),
    },
    output: port.audio(),
    editor: {
      category: "Instruments",
      description: "Two oscillators that slide through tables of waveforms: from soft to bright, vowels to digital",
      views: {
        amp: view.envelope("attack", "decay", "sustain", "release"),
        curve: view.filter("cutoff", "q"),
        filterEnvelope: view.envelope("filterAttack", "filterDecay", "filterSustain", "filterRelease"),
      },
      groups: {
        Oscillators: ["tableA", "fileA", "positionA", "pitchA", "levelA", "tableB", "fileB", "positionB", "pitchB", "levelB"],
        Filter: ["filter", "curve", "cutoff", "q", "keytrack", "filterEnv"],
        Envelopes: ["amp", "attack", "decay", "sustain", "release", "filterEnvelope", "filterAttack", "filterDecay", "filterSustain", "filterRelease"],
        LFO: ["lfoRate", "lfoPosition", "lfoCutoff"],
        Unison: ["unison", "detune", "spread"],
      },
    },
  },
  (notes: Events, p: Params): Audio => {
    // The files, each once, and which is each oscillator's (-1: none, so its menu's table plays).
    const files = [...new Set([p.fileA, p.fileB].filter((f): f is string => f !== undefined))];
    const file = (f: string | undefined) => (f === undefined ? -1 : files.indexOf(f));
    const values = [TABLES.indexOf(p.tableA), TABLES.indexOf(p.tableB), file(p.fileA), file(p.fileB), p.unison];
    const voice = (n: Voice): Audio =>
      kernel(
        "@m2/wavetable:voice",
        {
          pitch: n.pitch,
          gate: n.gate,
          velocity: n.velocity,
          a_position: p.positionA,
          a_pitch: p.pitchA,
          a_level: p.levelA,
          b_position: p.positionB,
          b_pitch: p.pitchB,
          b_level: p.levelB,
          detune: p.detune,
          spread: p.spread,
          filter: p.filter,
          cutoff: p.cutoff,
          q: p.q,
          keytrack: p.keytrack,
          env: p.filterEnv,
          f_attack: p.filterAttack,
          f_decay: p.filterDecay,
          f_sustain: p.filterSustain,
          f_release: p.filterRelease,
          attack: p.attack,
          decay: p.decay,
          sustain: p.sustain,
          release: p.release,
          lfo_rate: p.lfoRate,
          lfo_position: p.lfoPosition,
          lfo_cutoff: p.lfoCutoff,
          vel_amount: p.velSens,
          level: p.level,
        },
        { assets: files, values },
      );
    return p.voices === 1 ? mono(notes, voice, { glide: p.glide }) : poly(notes, voice, { voices: p.voices });
  },
);
