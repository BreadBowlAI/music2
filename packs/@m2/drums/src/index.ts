// @m2/drums: an 808/909-style drum machine (PACK-15).
//
// One engine, the pack's own `@m2/drums:drum` kernel (built from kernels-src/), plays every drum.
// `drums.kit` maps notes to drums by their General MIDI numbers, so one notes lane plays a whole beat:
// C2 (36) kick, D2 (38) snare, D#2 (39) clap, F#2 (42) closed hat, G#2 (44) pedal hat, A#2 (46) open
// hat, toms on F2, G2, A2, B2, C3 and D3 (41 to 50), C#2 (37) rim, C#3 (49) crash, D#3 (51) ride,
// F#3 (54) tambourine, G#3 (56) cowbell, A4 and A#4 (69, 70) shakers, D#5 (75) claves. `drums.drum`
// plays one drum on every note, the one its menu picks (`drum: "snare"`): a drum to process on its
// own (a kick to duck the bass under) goes on a lane of its own, played by a Drum.
//
// A hit cuts off the drum it replaces, as on a drum machine: a kick the kick before it,
// and a closed hat an open one (the hats choke each other). Toms, rim, cowbell, ride and
// shakers ring on. Every drum reads its knobs when it is hit. No two hits are quite the
// same (each catches its noise and metal at a new place, as on an analog machine), yet
// every render of a song is.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Events, type Params, type Value } from "@m2/core";

/** The drums a Drum can play, as its menu names them. */
const DRUMS = ["kick", "snare", "clap", "hats", "tom", "rim", "cowbell", "cymbal", "shaker"] as const;
type Drum = (typeof DRUMS)[number];

/** Each drum's `model` in the kernel (-1, the kit, picks each note's drum from its General MIDI number). */
const MODEL: Record<Drum | "kit", number> = { kit: -1, kick: 0, snare: 1, clap: 2, hats: 3, tom: 4, rim: 5, cowbell: 6, cymbal: 7, shaker: 8 };
/** Whether a hit of the drum cuts off the one before it (a choke): the toms, rim, cowbell and shaker ring on. */
const CHOKES: Record<Drum, boolean> = { kick: true, snare: true, clap: true, hats: true, tom: false, rim: false, cowbell: false, cymbal: true, shaker: false };

/**
 * The drum voices for `notes`, with a patch of kernel inputs (see `@m2/drums:drum` in the pack's `m2pkg.toml`).
 * Every voice hears every hit, marked by its choke group, so the next hit of its group fades
 * it out; a drum that never chokes needs only one voice to mark them (its hits on one sample
 * then count once, told apart by their keys). Outside the voices, `hitcount` counts the hits,
 * and each hit takes its seed from that count: how many hits the lane has played, never how
 * long the engine has run, so live playback from 0 is the export.
 */
function machine(notes: Events, model: number, voices: number, chokes: boolean, patch: Record<string, Value>): Audio {
  const hits = poly(notes, (n) => kernel("std:drumhit", { pitch: n.pitch, model }), { voices: chokes ? 8 : 1 });
  const count = kernel("std:hitcount", { hits });
  return poly(notes, (n) => kernel("@m2/drums:drum", { pitch: n.pitch, velocity: n.velocity, hits, count, model, ...patch }), { voices });
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

/** What the hats play on their named notes: closed on every other. */
const HAT_NOTES = { 42: "Closed hat", 44: "Pedal hat", 46: "Open hat" };
/** The kit's notes by name, for the piano roll's rows: the General MIDI notes `@m2/drums:drum` plays (see above). */
const KIT_NOTES = {
  36: "Kick",
  37: "Rim",
  38: "Snare",
  39: "Clap",
  41: "Tom 1",
  ...HAT_NOTES,
  43: "Tom 2",
  45: "Tom 3",
  47: "Tom 4",
  48: "Tom 5",
  49: "Crash",
  50: "Tom 6",
  51: "Ride",
  54: "Tambourine",
  56: "Cowbell",
  69: "Shaker",
  70: "Shaker 2",
  75: "Claves",
};

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
      editor: {
        category: "Drums",
        description: "An 808/909 drum machine: kick, snare, clap, hats, toms and more, one per note",
        notes: KIT_NOTES,
        groups: {
          Kick: ["kickTune", "kickDecay", "punch", "kickLevel"],
          "Snare & clap": ["snareTune", "snap", "snareLevel"],
          Hats: ["hatTone", "hatDecay", "openDecay", "hatLevel"],
          Toms: ["tomTune", "tomLevel"],
          Percussion: ["percLevel", "drive"],
        },
      },
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

  /**
   * One drum on every note, the one its menu picks: kick, snare, clap, hats (G#2 pedal, A#2 open),
   * tom (chromatic: A2 is the drum's own pitch), rim, cowbell, cymbal or shaker. Its knobs move
   * whichever it plays: `tune` in semitones from the drum's own, `decay` times its own length, and
   * `punch` more or less of a kick's thump or a snare's snap. At 0, 1 and 0 it is the drum as the kit
   * plays it. Which drum is a setting: it decides which hits cut each other off, so a pick rebuilds.
   */
  drum: component(
    {
      id: "drum",
      // Not "Drum": beside "Drum kit" in the + Track list it would be hard to tell apart (taste T3).
      label: "One drum",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        tune: control.semis(0).range(-24, 24),
        decay: control.scalar(1).range(0.1, 10).log(),
        punch: control.bipolar(0).range(-1, 1),
        drive: DRIVE,
      },
      settings: { drum: setting.choice(DRUMS, "kick"), voices: setting.int(6) },
      output: port.audio(),
      editor: {
        category: "Drums",
        description: "One 808/909 drum on every note, from its menu: kick, snare, clap, hi-hats, tom, rim, cowbell, cymbal or shaker",
        groups: { Drum: ["drum", "tune", "decay", "punch", "drive"] },
      },
    },
    (notes: Events, { drum, voices, tune, decay, punch, drive }: Params): Audio =>
      machine(notes, MODEL[drum as Drum], voices, CHOKES[drum as Drum], { tune, decay, push: punch, drive }),
  ),
};
