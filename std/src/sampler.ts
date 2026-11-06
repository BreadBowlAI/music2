// std/sampler: plays audio files by note.
//
// One engine, in up to `voices` voices. Give it one file, `sample`, in the song's `assets/` folder:
// every note plays it, sped up or slowed down by how far the note is from `root` (an octave up plays
// it twice as fast). Or give it `zones`, a file for each root note: each covers from its root up to
// the next zone's root (the lowest down to note 0 too), and a note plays the zone it falls in. `zones`
// wins when both are written. Those two play through the `std:sampler` kernel.
//
// Or give it `regions` (PKG-15), the way recorded instruments are mapped: a file each, with the notes
// (`keys`), how hard (`velocity`) and in which turn (`round_robin`) it plays, whether it plays when
// the note starts or when it ends (`trigger`), a `loop` held while the note is, and its own `tune`,
// `volume`, `pan` and `offset`. Every region that matches a note plays (PKG-16), through the
// `std:regions` kernel. Give one of `sample`, `zones` and `regions`.
//
// A region in a choke group (`group`) cuts, when it starts, every region whose `off_by` is that group,
// in any voice, in 4 ms (PKG-16, rule 8): a closed hat cutting an open one, as SFZ's `group` and
// `off_by` do. A voice hears the others' starts through marks made outside the voices.
//
// A region's turn (`round_robin`) counts the notes the lane has started (`turns: "lane"`, the
// default), or each key's own (`turns: "key"`, as SFZ does): a kick and a snare that alternate on
// one lane then each take their own turns.
//
// `mode: "oneshot"` (the default) plays the whole file however short the note, as drum hits and
// chops want; `"gate"` stops with the note, fading out over `release`. `velocity` is how much a
// note's velocity changes its level: 1 (the default) follows it all the way, 0 not at all. With no
// file it plays silence: drop a WAV on its card in the app to give it one.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Events, type Params, type Region } from "@m2/core";

/** The kernel's settings for `sample` at `root`, or for `zones`: its files, and a root, a lowest and a highest note for each. */
function files(sample: string | undefined, root: number, zones: Readonly<Record<number, string>>): { assets: string[]; values: number[] } {
  const roots = Object.keys(zones)
    .map(Number)
    .sort((a, b) => a - b);
  if (roots.length > 0) {
    const values = roots.flatMap((r, i) => [r, i === 0 ? 0 : r, i + 1 < roots.length ? roots[i + 1] - 1 : 127]);
    return { assets: roots.map((r) => zones[r]), values };
  }
  return sample === undefined ? { assets: [], values: [] } : { assets: [sample], values: [root, 0, 127] };
}

/**
 * How many values a region takes in `std:regions`' layout (`std/m2pkg.toml`): 15, and 17 with its
 * choke groups' slots (PKG-16, rule 8), which `std:regions` and `std:regionmark` read.
 */
const FIELDS = 15;
const CHOKE_FIELDS = 17;

/**
 * `std:regions`' settings for `regions`: each file once, as its assets, and its values: FIELDS, then
 * each region's, its defaults filled in (PKG-15). A region that gives a root but no keys plays its
 * root alone; one that gives neither plays every note. With `slots`, the groups `off_by` names, each
 * region's mark and cut too: a group's slot, or -1.
 */
function layout(regions: readonly Region[], slots: readonly number[]): { assets: string[]; values: number[] } {
  const assets: string[] = [];
  const values = [slots.length ? CHOKE_FIELDS : FIELDS];
  const slot = (g: number | undefined) => (g === undefined ? -1 : slots.indexOf(g));
  for (const r of regions) {
    if (!assets.includes(r.file)) assets.push(r.file);
    const root = r.root ?? 60;
    const [lo, hi] = r.keys ?? (r.root === undefined ? [0, 127] : [root, root]);
    const [soft, loud] = r.velocity ?? [1, 127];
    const [position, of] = r.round_robin ?? [1, 1];
    const [start, end] = r.loop ?? [-1, -1];
    const trigger = r.trigger === "release" ? 1 : 0;
    values.push(assets.indexOf(r.file), root, lo, hi, soft, loud, position, of, trigger, start, end, r.tune ?? 0, r.volume ?? 0, r.pan ?? 0, r.offset ?? 0);
    if (slots.length) values.push(slot(r.group), slot(r.off_by));
  }
  return { assets, values };
}

/** A sampler: one WAV file on every note, pitched by the note, a file for each range of notes, or regions. */
export const sampler = component(
  {
    id: "sampler",
    label: "Sampler",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: {
      tune: control.semis(0).range(-24, 24),
      level: control.db(0).range(-24, 12),
      attack: control.secs(0).range(0, 2),
      release: control.secs(0.05).range(0, 5),
      velocity: control.frac(1).range(0, 1),
    },
    settings: {
      sample: setting.asset().optional(),
      root: setting.int(60).label("root note"),
      zones: setting.keymap(),
      regions: setting.regions(),
      mode: setting.choice(["oneshot", "gate"], "oneshot"),
      voices: setting.int(8),
      turns: setting.choice(["lane", "key"], "lane"),
    },
    output: port.audio(),
    editor: { category: "Sampler", description: "Plays a WAV file on every note, pitched by the note: drop a WAV on its card" },
  },
  (notes: Events, p: Params): Audio => {
    const mode = p.mode === "gate" ? 1 : 0;
    const patch = { tune: p.tune, level: p.level, attack: p.attack, release: p.release, mode, veltrack: p.velocity };
    const regions: readonly Region[] = p.regions;
    if (regions.length === 0) {
      const settings = files(p.sample, p.root, p.zones);
      return poly(notes, (n) => kernel("std:sampler", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, ...patch }, settings), { voices: p.voices });
    }
    const also = [p.sample !== undefined ? "sample" : "", Object.keys(p.zones).length > 0 ? "zones" : ""].filter((x) => x !== "");
    if (also.length > 0) throw new Error(`sampler(): give one of sample, zones and regions, not ${[...also, "regions"].join(" and ")}`);
    // Choke groups (PKG-16, rule 8): a slot for each group some region's `off_by` names, in order
    // (the build refuses a ninth).
    const slots = [...new Set(regions.flatMap((r) => (r.off_by === undefined ? [] : [r.off_by])))];
    const settings = layout(regions, slots);
    // A turn for each key: each note's count on its key, which the voices keep (`keyCount`). Else
    // the lane's turn: counted outside the voices, as std's drums count their hits. One voice marks
    // each sample on which notes start (as a tom, which chokes nothing), so notes that start together
    // count once, and hitcount counts the marks: never engine time, so live playback from 0 takes the
    // export's turns.
    const lane = p.turns === "key" ? undefined : kernel("std:hitcount", { hits: poly(notes, (n) => kernel("std:drumhit", { pitch: n.pitch, model: 4 }), { voices: 1 }) });
    const counted = (n: { keyCount: Audio }) => lane ?? n.keyCount;
    if (slots.length === 0) {
      return poly(notes, (n) => kernel("std:regions", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, ...patch, count: counted(n) }, settings), {
        voices: p.voices,
      });
    }
    // A voice hears the others as a Drum's do: outside the voices, one voice of regionmark a note
    // marks the groups its regions start, and every voice reads the marks. 7 voices: 3 bits a group.
    const marks = poly(notes, (n) => kernel("std:regionmark", { pitch: n.pitch, velocity: n.velocity, count: counted(n) }, settings), { voices: 7 });
    return poly(notes, (n) => kernel("std:regions", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, ...patch, count: counted(n), marks }, settings), {
      voices: p.voices,
    });
  },
);
