#!/usr/bin/env node
// A big song to measure the views on (taste.md VIEW-10, the camera note's §8): examples/first-song's
// four sounds as 24 tracks (six each of Kick, Hats, Bass and Keys), each with 64 one-bar notes clips
// one after another, 64 bars in all, each note's velocity a little off its part's, as a played or
// humanized part's are (31 loudnesses in all). Open it in music2, then ⇧⌘P ▸ Show frame rate, and
// move, zoom and pinch the timeline while it plays.
//
//   node e2e/big-song.mjs <folder>        (a new folder, or an empty one)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [dir] = process.argv.slice(2);
if (!dir) {
  console.error("usage: node e2e/big-song.mjs <folder>   (makes a song of 24 tracks of 64 clips there)");
  process.exit(2);
}
if (fs.existsSync(dir) && fs.readdirSync(dir).length) {
  console.error(`big-song: ${dir} isn't empty; give a new folder, so nothing is written over`);
  process.exit(1);
}
const FIRST = path.join(path.dirname(fileURLToPath(import.meta.url)), "../examples/first-song");
const TRACKS = 24, CLIPS = 64, BAR = 4;

/** Each kind: its lane, its code and the notes of one bar. */
const KINDS = {
  kick: {
    name: "Kick",
    lane: "hits",
    code: (key, name) => `import { track } from "@m2/core";\nimport { drums } from "@m2/drums";\n\nexport default track("${key}", { name: "${name}" }, ({ lane }) => drums.drum(lane.notes("hits")));\n`,
    notes: () => [0, 1, 2, 3].map((b) => ({ pitch: "C1", start: b, length: 0.25, velocity: 0.8 })),
  },
  hats: {
    name: "Hats",
    lane: "hits",
    code: (key, name) => `import { track } from "@m2/core";\nimport { drums } from "@m2/drums";\n\nexport default track("${key}", { name: "${name}" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "hats", tune: 5 }));\n`,
    notes: () => [0.5, 1.5, 2.5, 3.5].map((b) => ({ pitch: "F#2", start: b, length: 0.1, velocity: 0.7 })),
  },
  bass: {
    name: "Bass",
    lane: "notes",
    code: (key, name) => `import { track } from "@m2/core";\nimport { bass } from "@m2/synths";\n\nexport default track("${key}", { name: "${name}" }, ({ lane }) => bass(lane.notes("notes")));\n`,
    notes: (bar) => [0, 0.75, 1.5, 2, 2.75, 3.5].map((b, i) => ({ pitch: ["A1", "A1", "C2", "A1", "E2", "G1"][(i + bar) % 6], start: b, length: 0.25, velocity: 0.8 })),
  },
  keys: {
    name: "Keys",
    lane: "chords",
    code: (key, name) => `import { track } from "@m2/core";\nimport { pluck } from "@m2/synths";\n\nexport default track("${key}", { name: "${name}" }, ({ lane }) => pluck(lane.notes("chords")));\n`,
    notes: (bar) =>
      [0, 1.5].flatMap((b) => (bar % 2 ? ["F3", "A3", "C4"] : ["A3", "C4", "E4"]).map((pitch) => ({ pitch, start: b, length: 0.5, velocity: 0.75 }))),
  },
};
const ORDER = ["kick", "hats", "bass", "keys"];

/** A clip id (FMT-11): 8 characters of lowercase Crockford base32, unique in its lane. */
const CROCKFORD = "0123456789abcdefghjkmnpqrstvwxyz";
/** A note's velocity `v` humanized: up to 0.15 off, in hundredths, the same each time the song is made. */
const human = (v, n) => Math.round((v + (((n * 7919) % 31) - 15) / 100) * 100) / 100;

const clipId = (n) => {
  let s = "";
  for (let i = 0; i < 6; i++, n = Math.floor(n / 32)) s = CROCKFORD[n % 32] + s;
  return `bg${s}`;
};

/** JSON as first-song's lane files write it: a space after each colon and comma. */
const json = (o) => JSON.stringify(o).replace(/":/g, '": ').replace(/,"/g, ', "');
/** A lane file: a line for each clip, and one for each of its notes. */
const lane = (clips) =>
  `{"clips": [\n${clips.map(({ notes, ...clip }) => `  ${json(clip).replace(/}$/, ', "notes": [')}\n${notes.map((n) => `    ${json(n)}`).join(",\n")}\n  ]}`).join(",\n")}\n]}\n`;

fs.mkdirSync(path.join(dir, "tracks"), { recursive: true });
const name = path.basename(path.resolve(dir));
fs.writeFileSync(path.join(dir, "music2.toml"), fs.readFileSync(path.join(FIRST, "music2.toml"), "utf8").replace(/^name = .*$/m, `name = ${JSON.stringify(name)}`));
fs.copyFileSync(path.join(FIRST, "music2.lock"), path.join(dir, "music2.lock"));
fs.mkdirSync(path.join(dir, "data"));
fs.copyFileSync(path.join(FIRST, "data/arrangement.json"), path.join(dir, "data/arrangement.json"));

const tracks = [];
for (let t = 0; t < TRACKS; t++) {
  const kind = KINDS[ORDER[t % ORDER.length]];
  const n = Math.floor(t / ORDER.length) + 1;
  const key = `${ORDER[t % ORDER.length]}-${n}`;
  const label = `${kind.name} ${n}`;
  fs.writeFileSync(path.join(dir, "tracks", `${key}.ts`), kind.code(key, label));
  const clips = Array.from({ length: CLIPS }, (_, c) => ({
    id: clipId(t * CLIPS + c + 1),
    start: c * BAR,
    length: BAR,
    notes: kind.notes(c).map((note, i) => ({ ...note, velocity: human(note.velocity, (t * CLIPS + c) * 8 + i) })),
  }));
  fs.mkdirSync(path.join(dir, "data", key));
  fs.writeFileSync(path.join(dir, "data", key, `${kind.lane}.json`), lane(clips));
  tracks.push(key);
}

const ident = (key) => key.replace(/-(\d+)$/, "$1");
fs.writeFileSync(
  path.join(dir, "song.ts"),
  `// ${TRACKS} tracks of ${CLIPS} clips each, made by e2e/big-song.mjs from examples/first-song's sounds,
// to see how smoothly the views move on a big song.

import { song, db } from "@m2/core";
import { master, mixer } from "std/mix";
${tracks.map((k) => `import ${ident(k)} from "./tracks/${k}";`).join("\n")}

export default song(() =>
  master(
    mixer({
${tracks.map((k) => `      "${k}": { in: ${ident(k)}.out, db: db(-14) },`).join("\n")}
    }),
  ),
);
`,
);
console.log(`Made ${dir}: ${TRACKS} tracks of ${CLIPS} clips each. Open it in music2, then ⇧⌘P ▸ Show frame rate.`);
