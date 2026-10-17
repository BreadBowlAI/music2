// std/drums: an 808/909-style drum machine.
//
// One engine, the `std:drum` kernel, plays every drum. `drums.kit` maps notes to drums by
// their General MIDI numbers, so one notes lane plays a whole beat: C2 (36) kick, D2 (38)
// snare, D#2 (39) clap, F#2 (42) closed hat, G#2 (44) pedal hat, A#2 (46) open hat, toms
// on F2, G2, A2, B2, C3 and D3 (41 to 50), C#2 (37) rim, C#3 (49) crash, D#3 (51) ride,
// F#3 (54) tambourine, G#3 (56) cowbell, A4 and A#4 (69, 70) shakers, D#5 (75) claves.
// Each drum is also an instrument of its own.
//
// A hit cuts off the drum it replaces, as on a drum machine: a kick the kick before it,
// and a closed hat an open one (the hats choke each other). Toms, rim, cowbell, ride and
// shakers ring on. Every drum reads its knobs when it is hit. No two hits are quite the
// same (each catches its noise and metal at a new place, as on an analog machine), yet
// every render of a song is. The kit is one sound: a drum to process on its own (a kick to
// duck the bass under) goes on a lane of its own, played by that drum alone.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Events, type Params, type Value } from "@m2/core";

/** Drums by `model`: -1 picks each note's drum from its General MIDI number. */
const MODEL = { kit: -1, kick: 0, snare: 1, clap: 2, hats: 3, tom: 4, rim: 5, cowbell: 6, cymbal: 7, shaker: 8 } as const;

/**
 * The drum voices for `notes`, with a patch of kernel inputs (see `std:drum` in `m2pkg.toml`).
 * Every voice hears every hit, marked by its choke group, so the next hit of its group fades
 * it out; a drum that never chokes needs only one voice to mark them (its hits on one sample
 * then count once, told apart by their keys). Outside the voices, `hitcount` counts the hits,
 * and each hit takes its seed from that count: how many hits the lane has played, never how
 * long the engine has run, so live playback from 0 is the export.
 */
function machine(notes: Events, model: number, voices: number, chokes: boolean, patch: Record<string, Value>): Audio {
  const hits = poly(notes, (n) => kernel("std:drumhit", { pitch: n.pitch, model }), { voices: chokes ? 8 : 1 });
  const count = kernel("std:hitcount", { hits });
  return poly(notes, (n) => kernel("std:drum", { pitch: n.pitch, velocity: n.velocity, hits, count, model, ...patch }), { voices });
}

const KICK_TUNE = control.hz(48).range(30, 120).log();
const KICK_DECAY = control.secs(0.45).range(0.05, 3).log();
const PUNCH = control.frac(0.5).range(0, 1);
const SNARE_TUNE = control.hz(190).range(100, 400).log();
const SNAP = control.frac(0.6).range(0, 1);
const HAT_DECAY = control.ms(45).range(10, 300).log();
const OPEN_DECAY = control.ms(450).range(80, 2000).log();
const HAT_TONE = control.hz(8000).range(3000, 14000).log();
const TOM_TUNE = control.hz(130).range(60, 300).log();
const DRIVE = control.db(0).range(0, 24);
const LEVEL = (what: string) => control.db(0).range(-24, 12).label(`${what} level`);

export const drums = {
  /** The whole drum machine on one notes lane, each drum on its General MIDI note (see above). */
  kit: component(
    {
      id: "drum-kit",
      label: "Drum kit",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        kickTune: KICK_TUNE.label("kick tune"),
        kickDecay: KICK_DECAY.label("kick decay"),
        punch: PUNCH,
        snareTune: SNARE_TUNE.label("snare tune"),
        snap: SNAP,
        hatDecay: HAT_DECAY.label("hat decay"),
        openDecay: OPEN_DECAY.label("open decay"),
        hatTone: HAT_TONE.label("hat tone"),
        tomTune: TOM_TUNE.label("tom tune"),
        drive: DRIVE,
        // Each group's level against the rest, in dB: kick; snare, clap and rim; the hats;
        // the toms; cymbals and the rest of the percussion.
        kickLevel: LEVEL("kick"),
        snareLevel: LEVEL("snare"),
        hatLevel: LEVEL("hat"),
        tomLevel: LEVEL("tom"),
        percLevel: LEVEL("perc"),
      },
      settings: { voices: setting.int(16) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808/909 drum machine: kick, snare, clap, hats, toms and more, one per note" },
    },
    (notes: Events, p: Params): Audio =>
      machine(notes, MODEL.kit, p.voices, true, {
        kick_tune: p.kickTune,
        kick_decay: p.kickDecay,
        punch: p.punch,
        snare_tune: p.snareTune,
        snap: p.snap,
        hat_decay: p.hatDecay,
        open_decay: p.openDecay,
        hat_tone: p.hatTone,
        tom_tune: p.tomTune,
        drive: p.drive,
        kick_level: p.kickLevel,
        snare_level: p.snareLevel,
        hat_level: p.hatLevel,
        tom_level: p.tomLevel,
        perc_level: p.percLevel,
      }),
  ),

  /** A kick on every note, whatever its pitch: from a deep 808 boom (`punch` 0) to a hard 909 thump (1). */
  kick: component(
    {
      id: "drum-kick",
      label: "Kick drum",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tune: KICK_TUNE, decay: KICK_DECAY, punch: PUNCH, drive: DRIVE },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808/909 kick: a deep boom or a punchy thump" },
    },
    (notes: Events, { tune, decay, punch, drive, voices }: Params): Audio =>
      machine(notes, MODEL.kick, voices, true, { kick_tune: tune, kick_decay: decay, punch, drive }),
  ),

  /** A snare on every note: a tuned body and a noisy rattle, balanced by `snap`. */
  snare: component(
    {
      id: "drum-snare",
      label: "Snare",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tune: SNARE_TUNE, snap: SNAP, decay: control.secs(0.18).range(0.02, 2).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808/909 snare: a tuned body and a crisp rattle" },
    },
    (notes: Events, { tune, snap, decay, voices }: Params): Audio =>
      machine(notes, MODEL.snare, voices, true, { snare_tune: tune, snap, snare_decay: decay }),
  ),

  /** A hand clap on every note. */
  clap: component(
    {
      id: "drum-clap",
      label: "Clap",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tone: control.hz(1200).range(300, 8000).log(), decay: control.secs(0.25).range(0.03, 3).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808-style hand clap" },
    },
    (notes: Events, { tone, decay, voices }: Params): Audio => machine(notes, MODEL.clap, voices, true, { clap_tone: tone, clap_decay: decay }),
  ),

  /** Hi-hats: closed on every note, except G#2 (44) pedal and A#2 (46) open, which the next hat cuts off. */
  hats: component(
    {
      id: "drum-hats",
      label: "Hats",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { decay: HAT_DECAY, openDecay: OPEN_DECAY.label("open decay"), tone: HAT_TONE },
      settings: { voices: setting.int(6) },
      output: port.audio(),
      editor: { category: "Drums", description: "808 hi-hats: closed, pedal (G#2) and open (A#2), metallic and crisp" },
    },
    (notes: Events, { decay, openDecay, tone, voices }: Params): Audio =>
      machine(notes, MODEL.hats, voices, true, { hat_decay: decay, open_decay: openDecay, hat_tone: tone }),
  ),

  /** A tom, played chromatically: A2 (45) is at `tune`. Lower notes ring longer. */
  tom: component(
    {
      id: "drum-tom",
      label: "Tom",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tune: TOM_TUNE, decay: control.secs(0.35).range(0.03, 5).log() },
      settings: { voices: setting.int(6) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808 tom, tuned by the note: A2 plays at its tune" },
    },
    (notes: Events, { tune, decay, voices }: Params): Audio => machine(notes, MODEL.tom, voices, false, { tom_tune: tune, tom_decay: decay }),
  ),

  /** A rim shot (side stick) on every note. */
  rim: component(
    {
      id: "drum-rim",
      label: "Rim",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tone: control.hz(1700).range(300, 8000).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "A short wooden rim shot" },
    },
    (notes: Events, { tone, voices }: Params): Audio => machine(notes, MODEL.rim, voices, false, { rim_tone: tone }),
  ),

  /** The 808 cowbell on every note. */
  cowbell: component(
    {
      id: "drum-cowbell",
      label: "Cowbell",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { tune: control.hz(540).range(100, 3000).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "The 808 cowbell: two clanging squares" },
    },
    (notes: Events, { tune, voices }: Params): Audio => machine(notes, MODEL.cowbell, voices, false, { bell_tune: tune }),
  ),

  /** A crash cymbal on every note; the next crash cuts it off. */
  cymbal: component(
    {
      id: "drum-cymbal",
      label: "Cymbal",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { decay: control.secs(1.6).range(0.1, 10).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "An 808 crash cymbal: a wash of metal" },
    },
    (notes: Events, { decay, voices }: Params): Audio => machine(notes, MODEL.cymbal, voices, true, { cymbal_decay: decay }),
  ),

  /** A shaker on every note. */
  shaker: component(
    {
      id: "drum-shaker",
      label: "Shaker",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: { decay: control.ms(60).range(10, 2000).log() },
      settings: { voices: setting.int(4) },
      output: port.audio(),
      editor: { category: "Drums", description: "A crisp shaker" },
    },
    (notes: Events, { decay, voices }: Params): Audio => machine(notes, MODEL.shaker, voices, false, { shake_decay: decay }),
  ),
};
