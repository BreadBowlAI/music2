// Makes the five WAV files of sampler-regions/assets, a small made-up instrument, so their bytes
// come from this file:
//   node core/session/tests/fixtures/make-sampler-regions.mjs
// All 24 kHz mono 16-bit PCM, recorded (made) at C4:
// soft-1.wav, soft-2.wav: two takes of a soft note (0.12 s): a C4 with its second harmonic, each
//   with its own fixed noise.
// hard.wav: the loud note (0.12 s): more harmonics, brighter.
// release.wav: the key let go (0.04 s): a short click of noise.
// pad.wav: a held tone (0.25 s) at 200 Hz, 120 frames a cycle, so frames 2400 to 5399 (25 cycles)
//   loop without a seam.
// The noise is a fixed sequence, so every run writes the same bytes; the script prints each file's
// SHA-256 to compare.

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const out = join(dirname(fileURLToPath(import.meta.url)), "sampler-regions", "assets");
const rate = 24000;

/** A 16-bit PCM WAV of one channel (floats in -1..1) at `rate`. */
function wav(x) {
  const data = Buffer.alloc(x.length * 2);
  x.forEach((v, i) => data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, v)) * 32767), i * 2));
  const head = Buffer.alloc(44);
  head.write("RIFF", 0);
  head.writeUInt32LE(36 + data.length, 4);
  head.write("WAVE", 8);
  head.write("fmt ", 12);
  head.writeUInt32LE(16, 16);
  head.writeUInt16LE(1, 20);
  head.writeUInt16LE(1, 22);
  head.writeUInt32LE(rate, 24);
  head.writeUInt32LE(rate * 2, 28);
  head.writeUInt16LE(2, 32);
  head.writeUInt16LE(16, 34);
  head.write("data", 36);
  head.writeUInt32LE(data.length, 40);
  return Buffer.concat([head, data]);
}

/** White noise in -1..1 from a fixed seed (a 32-bit LCG). */
function noise(seed) {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 2147483648 - 1;
  };
}

/** `seconds` of a note at C4 with `harmonics`, a little noise from `seed`, fading in and out. */
function note(seconds, harmonics, seed, level) {
  const len = Math.round(seconds * rate);
  const rand = noise(seed);
  const f = 261.6256;
  return Array.from({ length: len }, (_, i) => {
    const t = i / rate;
    let v = 0;
    for (let h = 1; h <= harmonics; h++) v += Math.sin(2 * Math.PI * f * h * t) / h;
    const fade = Math.min(1, i / 24) * (1 - i / len);
    return level * (v + 0.05 * rand()) * Math.exp(-t * 12) * fade;
  });
}

const click = (() => {
  const len = Math.round(0.04 * rate);
  const rand = noise(9);
  return Array.from({ length: len }, (_, i) => 0.4 * rand() * Math.exp(-i / 120) * (1 - i / len));
})();

const pad = Array.from({ length: Math.round(0.25 * rate) }, (_, i) => {
  const p = (2 * Math.PI * i) / 120;
  return 0.3 * (Math.sin(p) + 0.3 * Math.sin(2 * p)) * Math.min(1, i / 240);
});

for (const [name, x] of [
  ["soft-1.wav", note(0.12, 2, 1, 0.4)],
  ["soft-2.wav", note(0.12, 2, 2, 0.4)],
  ["hard.wav", note(0.12, 6, 3, 0.7)],
  ["release.wav", click],
  ["pad.wav", pad],
]) {
  const bytes = wav(x);
  writeFileSync(join(out, name), bytes);
  console.log(`${createHash("sha256").update(bytes).digest("hex")}  ${name} (${bytes.length} bytes)`);
}
