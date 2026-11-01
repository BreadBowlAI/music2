// @m2/drums: an 808/909-style drum machine (PACK-15).
//
// One engine, the pack's own `@m2/drums:drum` kernel (built from kernels-src/), plays every drum.
// `drums.drum` (Drum) plays one drum on every note, the one its menu picks (`drum: "snare"`): the
// building block of a kit, several Drum tracks that go to one Drums bus (KIT-1), each mixed on its
// own. Its kits are what + Track's Drum kit makes (KIT-4). `drums.kit` (Drum machine) is the older
// way, a whole beat on one notes lane, each drum on its General MIDI note: C2 (36) kick, D2 (38)
// snare, D#2 (39) clap, F#2 (42) closed hat, G#2 (44) pedal hat, A#2 (46) open hat, toms on F2, G2,
// A2, B2, C3 and D3 (41 to 50), C#2 (37) rim, C#3 (49) crash, D#3 (51) ride, F#3 (54) tambourine,
// G#3 (56) cowbell, A4 and A#4 (69, 70) shakers, D#5 (75) claves. No list offers it now; songs
// that play it play as before.
//
// A hit cuts off the drum it replaces, as on a drum machine: a kick the kick before it,
// and a closed hat an open one (the hats choke each other). Toms, rim, cowbell, ride and
// shakers ring on. Every drum reads its knobs when it is hit. No two hits are quite the
// same (each catches its noise and metal at a new place, as on an analog machine), yet
// every render of a song is.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Choice, type Events, type Params, type Value } from "@m2/core";

/**
 * The drums the Drum can play, as its menu names them, grouped by family: each other drum the drum
 * machine plays beside the drum it is most like, named as General MIDI names it (KIT-20). A song
 * writes a sound by its name; the `one` kernel reads its place (`SOUNDS` in kernels-src), so the
 * order is for reading, and each name plays what it always played.
 */
const DRUMS = [
  "kick",
  "kick 2",
  "snare",
  "snare 2",
  "clap",
  "hats",
  "tom",
  "rim",
  "claves",
  "cowbell",
  "cymbal",
  "cymbal 2",
  "ride",
  "ride 2",
  "shaker",
  "shaker 2",
  "tambourine",
] as const;

/**
 * The drum voices for `notes`, with a patch of kernel inputs (see `@m2/drums:drum` and `one` in the
 * pack's `m2pkg.toml`). Every voice hears every hit, marked by its choke group by its General MIDI
 * pitch, so the next hit of its group fades it out (One drum's voice: any hit, when its drum
 * chokes). Outside the voices, `hitcount` counts the hits, and each hit takes its seed from that
 * count: how many hits the lane has played, never how long the engine has run, so live playback
 * from 0 is the export.
 */
function machine(notes: Events, voice: "@m2/drums:drum" | "@m2/drums:one", voices: number, patch: Record<string, Value | Choice>): Audio {
  const hits = poly(notes, (n) => kernel("std:drumhit", { pitch: n.pitch, model: -1 }), { voices: 8 });
  const count = kernel("std:hitcount", { hits });
  return poly(notes, (n) => kernel(voice, { pitch: n.pitch, velocity: n.velocity, hits, count, ...patch }), { voices });
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
  /**
   * The whole drum machine on one notes lane, each drum on its General MIDI note (see above). No
   * list offers it (KIT-2): a kit of Drum tracks does what it did, each drum on a track of its own.
   */
  kit: component(
    {
      id: "drum-kit",
      label: "Drum machine",
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
        unlisted: true,
        description: "An 808/909 drum machine: kick, snare, clap, hats, toms and more, one per note",
        notes: KIT_NOTES,
        groups: {
          Kick: ["kickTune", "kickDecay", "punch", "kickLevel"],
          "Snare & clap": ["snareTune", "snap", "snareLevel"],
          Hats: ["hatTone", "hatDecay", "openDecay", "hatLevel"],
          Toms: ["tomTune", "tomLevel"],
          Percussion: ["percLevel"],
          "Whole kit": ["drive"],
        },
      },
    },
    (notes: Events, p: Params): Audio =>
      machine(notes, "@m2/drums:drum", p.voices, {
        model: -1,
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
   * tom (chromatic: A2 is the drum's own pitch), rim, cowbell, cymbal (a crash) or shaker, or another
   * drum the drum machine plays (kick 2, snare 2, claves, cymbal 2, ride, ride 2, shaker 2, tambourine:
   * KIT-20), as the drum machine plays it at its note. The menu is a
   * control: a pick plays from the next hit, and a lane can automate it. Its knobs move whichever it
   * plays: `tune` in semitones from the drum's own, `decay` times its own length, and `punch` more or
   * less of a kick's thump or a snare's snap, or on the others of the hit's first 15 ms. At 0, 1
   * and 0 it is the drum as the drum machine plays it, and at its single's voices (4, or 6 for the
   * hats and the tom) the drum each removed single played (CARD-33). Its kits are several of it,
   * each on a track of its own, going to one Drums bus (KIT-4).
   */
  drum: component(
    {
      id: "drum",
      // The one building block (KIT-2): + Track lists the kits apart, as "Drum kit (several tracks)".
      label: "Drum",
      signature: signature<(notes: Events) => Audio>(),
      inputs: { notes: port.notes() },
      controls: {
        drum: control.choice(DRUMS, "kick").label("sound"),
        tune: control.semis(0).range(-24, 24),
        decay: control.scalar(1).range(0.1, 10).log().label("decay ×"),
        punch: control.bipolar(0).range(-1, 1).label("punch ±"),
        drive: DRIVE,
      },
      settings: { voices: setting.int(6) },
      output: port.audio(),
      editor: {
        category: "Drums",
        description: "One 808/909 drum on every note, from its menu: kick, snare, clap, hi-hats, tom, rim, cowbell, cymbal, shaker, ride, tambourine, claves and more",
        groups: { Drum: ["drum", "tune", "decay", "punch", "drive"] },
        // What + Track's Drum kit makes (KIT-4): each track a Drum, going to one Drums bus. The hats
        // are one Hats track, closed, pedal and open by the note, so a closed hat cuts an open one.
        // The 808's kick is a long boom (punch -0.5), its snare more tone than snap, its toms and
        // cymbal ring long; the 909's kick thumps (punch 0.5), its snare snaps, its hats and cymbal
        // are brighter and its toms shorter. Each kit's bus fader (db) keeps its beat under -1 dBTP,
        // about as loud as the drum machine's was: the 808's loud fill keeps it quieter.
        kits: {
          "808 kit": {
            db: -4,
            tracks: [
              { name: "Kick", drum: "kick", punch: -0.5, decay: 1.6 },
              { name: "Snare", drum: "snare", punch: -0.3 },
              { name: "Clap", drum: "clap" },
              { name: "Hats", drum: "hats" },
              { name: "Toms", drum: "tom", decay: 1.5 },
              { name: "Rim", drum: "rim" },
              { name: "Cowbell", drum: "cowbell" },
              { name: "Cymbal", drum: "cymbal", decay: 1.5 },
            ],
          },
          "909 kit": {
            db: -3.8,
            tracks: [
              { name: "Kick", drum: "kick", punch: 0.5 },
              { name: "Snare", drum: "snare", punch: 0.3 },
              { name: "Clap", drum: "clap" },
              { name: "Hats", drum: "hats", tune: 2 },
              { name: "Toms", drum: "tom", decay: 0.8 },
              { name: "Rim", drum: "rim" },
              { name: "Cymbal", drum: "cymbal", tune: 2 },
            ],
          },
          "Basic kit": {
            db: -2,
            tracks: [
              { name: "Kick", drum: "kick" },
              { name: "Snare", drum: "snare" },
              { name: "Hats", drum: "hats" },
            ],
          },
        },
      },
    },
    (notes: Events, { drum, voices, tune, decay, punch, drive }: Params): Audio =>
      machine(notes, "@m2/drums:one", voices, { drum, tune, decay, push: punch, drive }),
  ),
};
