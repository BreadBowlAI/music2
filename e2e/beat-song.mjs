#!/usr/bin/env node
// A beat song to measure the views on (taste.md VIEW-10, the zoom note's §1), like the owner's "Beat
// Pack": 10 tracks (eight drums, a bass and keys) over 32 bars, each drum's one-bar clips full of
// hits, the bass's cutoff automation shown as curve clips: 312 clips, nearly all in view at the
// timeline's first zoom, as in a short song. Each note's velocity is a little off, the same each
// time the song is made. `zoom-bench` measures a pinch on it; open it in music2 to try one by hand.
//
//   node e2e/beat-song.mjs <folder>        (a new folder, or an empty one)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [dir] = process.argv.slice(2);
if (!dir) {
  console.error("usage: node e2e/beat-song.mjs <folder>   (makes a beat song of 10 tracks there)");
  process.exit(2);
}
if (fs.existsSync(dir) && fs.readdirSync(dir).length) {
  console.error(`beat-song: ${dir} isn't empty; give a new folder, so nothing is written over`);
  process.exit(1);
}
const FIRST = path.join(path.dirname(fileURLToPath(import.meta.url)), "../examples/first-song");
const BARS = 32;

/** JSON as first-song's lane files write it: a space after each colon and comma. */
const json = (o) => JSON.stringify(o).replace(/":/g, '": ').replace(/,"/g, ', "');
/** A clip id (FMT-11): 8 characters of lowercase Crockford base32, unique in its lane. */
const CROCKFORD = "0123456789abcdefghjkmnpqrstvwxyz";
let ids = 0;
const clipId = () => {
  let n = ++ids;
  let s = "";
  for (let i = 0; i < 6; i++, n = Math.floor(n / 32)) s = CROCKFORD[n % 32] + s;
  return `bp${s}`;
};
/** A note's velocity: 0.6 to 0.9, the same each time the song is made. */
const vel = (n) => Math.round((0.6 + ((n * 7919) % 31) / 100) * 100) / 100;
const range = (n, f) => Array.from({ length: n }, (_, i) => f(i));

/** A drum track: `hits(bar)` gives the beats it hits in a bar, one clip a bar. */
const drum = (key, name, drumName, pitch, hits) => ({
  key,
  lanes: { hits: range(BARS, (b) => ({ start: b * 4, length: 4, notes: hits(b).map((t, i) => ({ pitch, start: t, length: 0.1, velocity: vel(b * 16 + i) })) })) },
  code: `import { track } from "@m2/core";\nimport { drums } from "@m2/drums";\n\nexport default track("${key}", { name: "${name}" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "${drumName}" }));\n`,
});
const tracks = [
  drum("kick", "Kick", "kick", "C1", (b) => (b % 4 === 3 ? [0, 1, 2, 2.5, 3] : [0, 1, 2, 3])),
  drum("snare", "Snare", "snare", "D1", () => [1, 3]),
  drum("clap", "Clap", "clap", "D#1", (b) => (b % 2 ? [1, 3, 3.75] : [1, 3])),
  drum("hats", "Hats", "hats", "F#1", () => range(16, (i) => i / 4)),
  drum("open", "Open Hat", "open hat", "A#1", () => [0.5, 1.5, 2.5, 3.5]),
  drum("tom", "Tom", "tom", "A1", (b) => (b % 4 === 3 ? [2, 2.25, 2.5, 2.75, 3, 3.5] : [3.5])),
  drum("rim", "Rim", "rim", "C#1", () => [0.75, 1.75, 2.25, 3.25]),
  drum("shaker", "Shaker", "shaker", "A#2", () => range(8, (i) => i / 2 + 0.25)),
  {
    key: "bass",
    lanes: {
      notes: range(BARS, (b) => ({ start: b * 4, length: 4, notes: [0, 0.75, 1.5, 2, 2.75, 3.5].map((t, i) => ({ pitch: ["A1", "A1", "C2", "A1", "E2", "G1"][(i + b) % 6], start: t, length: 0.25, velocity: vel(b * 6 + i) })) })),
      cutoff: range(BARS / 4, (k) => ({ start: k * 16, length: 16, points: range(9, (i) => [i * 2, 300 + ((i * 977 + k * 131) % 1700)]) })),
    },
    code: `import { hz, track } from "@m2/core";\nimport { bass } from "@m2/synths";\n\nexport default track("bass", { name: "Bass" }, ({ lane }) => bass(lane.notes("notes"), { cutoff: lane.control("cutoff", "hz", hz(500)) }));\n`,
  },
  {
    key: "keys",
    lanes: { chords: range(BARS / 2, (b) => ({ start: b * 8, length: 8, notes: [0, 1.5, 4, 5.5].flatMap((t, j) => (b % 2 ? ["F3", "A3", "C4"] : ["A3", "C4", "E4"]).map((p) => ({ pitch: p, start: t, length: 0.5, velocity: vel(b * 4 + j) }))) })) },
    code: `import { track } from "@m2/core";\nimport { pluck } from "@m2/synths";\n\nexport default track("keys", { name: "Keys" }, ({ lane }) => pluck(lane.notes("chords")));\n`,
  },
];

fs.mkdirSync(path.join(dir, "tracks"), { recursive: true });
const name = path.basename(path.resolve(dir));
fs.writeFileSync(path.join(dir, "music2.toml"), fs.readFileSync(path.join(FIRST, "music2.toml"), "utf8").replace(/^name = .*$/m, `name = ${JSON.stringify(name)}`));
fs.copyFileSync(path.join(FIRST, "music2.lock"), path.join(dir, "music2.lock"));
fs.mkdirSync(path.join(dir, "data"));
fs.copyFileSync(path.join(FIRST, "data/arrangement.json"), path.join(dir, "data/arrangement.json"));
let clips = 0;
let notes = 0;
for (const t of tracks) {
  fs.writeFileSync(path.join(dir, "tracks", `${t.key}.ts`), t.code);
  fs.mkdirSync(path.join(dir, "data", t.key));
  for (const [lane, list] of Object.entries(t.lanes)) {
    // A lane file: a line for each clip, and one for each of its notes; a curve's points on its line.
    const body = list
      .map(({ notes: ns, points, ...c }) => {
        clips++;
        const head = json({ id: clipId(), ...c });
        if (points) return `  ${head.replace(/}$/, `, "points": ${JSON.stringify(points)}}`)}`;
        notes += ns.length;
        return `  ${head.replace(/}$/, ', "notes": [')}\n${ns.map((n) => `    ${json(n)}`).join(",\n")}\n  ]}`;
      })
      .join(",\n");
    const unit = list[0].points ? '"unit": "hz", ' : "";
    fs.writeFileSync(path.join(dir, "data", t.key, `${lane}.json`), `{${unit}"clips": [\n${body}\n]}\n`);
  }
}
fs.writeFileSync(
  path.join(dir, "song.ts"),
  `// A beat of ${tracks.length} tracks over ${BARS} bars, made by e2e/beat-song.mjs, like the owner's Beat Pack:
// to see how smoothly the views move and zoom on it.

import { song, db } from "@m2/core";
import { master, mixer } from "std/mix";
${tracks.map((t) => `import ${t.key} from "./tracks/${t.key}";`).join("\n")}

export default song(() =>
  master(
    mixer({
${tracks.map((t) => `      ${t.key}: { in: ${t.key}.out, db: db(-12) },`).join("\n")}
    }),
  ),
);
`,
);
console.log(`Made ${dir}: ${tracks.length} tracks, ${clips} clips, ${notes} notes. Open it in music2, then ⇧⌘P ▸ Show frame rate.`);
