// The song of track dress2a's suites (dress2a.mjs, dress2a-strip.mjs): two tracks of the packs'
// effects. Mix A plays a saw that hits each half beat and stops, through every mixing tool; FX A
// the same hits through every effect that colours a sound.
import fs from "node:fs";
import path from "node:path";
import { read, write } from "./fixlib.mjs";

/** Write the song into the open project: its packs listed and locked, its tempo, its two tracks. */
export function dress2aSong() {
  const versions = fs.readFileSync(path.join(import.meta.dirname, "../packs/versions.toml"), "utf8");
  // A pack's lock at the version music2 comes with, its m2pkg.toml's (the record keeps older ones too).
  const locked = (name) => {
    const pkg = fs.readFileSync(path.join(import.meta.dirname, `../packs/${name}/m2pkg.toml`), "utf8");
    const [, version] = /^version = "([0-9.]+)"$/m.exec(pkg);
    const [, hash] = new RegExp(`^"${name} ${version.replaceAll(".", "\\.")}" = "([^"]+)"$`, "m").exec(versions);
    return `\n[[pack]]\nname = "${name}"\nversion = "${version}"\nhash = "${hash}"\n`;
  };
  write("music2.toml", `${read("music2.toml")}"@m2/effects" = "1"\n"@m2/mixing" = "1"\n`);
  write("music2.lock", read("music2.lock") + locked("@m2/effects") + locked("@m2/mixing"));
  // At 125 bpm a half beat is 11,520 frames (90 blocks, 27 cycles of a 112.5 Hz saw), so each of Mix A's
  // hits is the one before, sample for sample, and so is each of its gate's closings: the check that
  // draws them as deep as each other measures the drawing alone (at 124 bpm and 110 Hz each began at
  // another place in the saw and the block grid, and their depths differed by up to 1.15 dB).
  write("data/arrangement.json", read("data/arrangement.json").replace('"bpm": 124', '"bpm": 125'));
  const HITS = `const hits = vca(osc.saw(110), range(lfo(time.beats, { period: 0.5, shape: "saw" }), 1, 0));`;
  // Mix A's: a quarter beat of saw, a quarter of silence.
  const GAPS = `const hits = vca(osc.saw(112.5), range(lfo(time.beats, { period: 0.5, shape: "square" }), 1, 0));`;
  write(
    "tracks/mixa.ts",
    `import { time, track } from "@m2/core";
import { lfo, range } from "std/control";
import { osc, vca } from "std/dsp";
import { deesser, eq, gate, multiband, pump, transient } from "@m2/mixing";

// A saw that hits each half beat and stops, through every mixing tool.
export default track("mixa", { name: "Mix A" }, () => {
  ${GAPS}
  return pump(multiband(deesser(transient(gate(eq(hits, { gain4: -3, freq4: 800 }), { threshold: -30 }), { attack: 6, sustain: -3 }), { threshold: -40 }), { midGain: -2 }), { depth: -8, shape: 0.5 });
});
`,
  );
  write(
    "tracks/fxa.ts",
    `import { time, track } from "@m2/core";
import { lfo, range } from "std/control";
import { osc, vca } from "std/dsp";
import { bitcrush, chorus, delay, drive, phaser, reverb, tremolo, widen } from "@m2/effects";

// The same hits, through every effect that colours a sound.
export default track("fxa", { name: "FX A" }, () => {
  ${HITS}
  return widen(bitcrush(drive(tremolo(phaser(chorus(reverb(delay(hits, { time: 0.25, feedback: 0.4, mix: 0.3 }), { size: 0.6, mix: 0.2 })))), { amount: 9 }), { bits: 6 }), { width: 1.5 });
});
`,
  );
  write(
    "song.ts",
    read("song.ts")
      .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport mixa from "./tracks/mixa";\nimport fxa from "./tracks/fxa";')
      .replace("keys: { in: keys.out, db: db(-5), pan: -0.25 },", "keys: { in: keys.out, db: db(-5), pan: -0.25 },\n      mixa: { in: mixa.out, db: db(-12) },\n      fxa: { in: fxa.out, db: db(-12) },"),
  );
}
