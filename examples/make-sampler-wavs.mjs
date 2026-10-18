// Makes the three WAV files of examples/sampler/assets, so their bytes come from this file:
//   node examples/make-sampler-wavs.mjs
// kick.wav: 48 kHz mono, a sine that falls from 150 Hz to 50 Hz and dies away (0.35 s).
// snare.wav: 48 kHz stereo, a short tone and noise, its own noise on each side (0.25 s).
// pluck.wav: 44.1 kHz mono, a plucked C4 (261.63 Hz) with its harmonics (1 s): its own rate, so
//   the sampler plays it at the song's rate too.
// 16-bit PCM. The noise is a fixed sequence, so every run writes the same bytes; the script prints
// each file's SHA-256 to compare.

import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const out = join(dirname(fileURLToPath(import.meta.url)), "sampler", "assets");

/** A 16-bit PCM WAV of `channels` (arrays of floats in -1..1, as long) at `rate`. */
function wav(rate, channels) {
  const frames = channels[0].length;
  const n = channels.length;
  const data = Buffer.alloc(frames * n * 2);
  for (let i = 0; i < frames; i++) {
    for (let c = 0; c < n; c++) {
      const v = Math.max(-1, Math.min(1, channels[c][i]));
      data.writeInt16LE(Math.round(v * 32767), (i * n + c) * 2);
    }
  }
  const head = Buffer.alloc(44);
  head.write("RIFF", 0);
  head.writeUInt32LE(36 + data.length, 4);
  head.write("WAVE", 8);
  head.write("fmt ", 12);
  head.writeUInt32LE(16, 16);
  head.writeUInt16LE(1, 20);
  head.writeUInt16LE(n, 22);
  head.writeUInt32LE(rate, 24);
  head.writeUInt32LE(rate * n * 2, 28);
  head.writeUInt16LE(n * 2, 32);
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

const kick = (() => {
  const rate = 48000;
  const len = Math.round(0.35 * rate);
  const x = new Array(len);
  let phase = 0;
  for (let i = 0; i < len; i++) {
    const t = i / rate;
    const hz = 50 + 100 * Math.exp(-t * 30);
    phase += hz / rate;
    const fade = i < 24 ? i / 24 : 1;
    x[i] = 0.9 * Math.sin(2 * Math.PI * phase) * Math.exp(-t * 9) * fade * (1 - i / len);
  }
  return wav(rate, [x]);
})();

const snare = (() => {
  const rate = 48000;
  const len = Math.round(0.25 * rate);
  const sides = [noise(1), noise(2)].map((rand) => {
    const x = new Array(len);
    let lp = 0;
    for (let i = 0; i < len; i++) {
      const t = i / rate;
      lp += 0.6 * (rand() - lp);
      const body = 0.5 * Math.sin(2 * Math.PI * 185 * t) * Math.exp(-t * 30);
      x[i] = (body + 0.5 * lp * Math.exp(-t * 18)) * (1 - i / len);
    }
    return x;
  });
  return wav(rate, sides);
})();

const pluck = (() => {
  const rate = 44100;
  const len = rate;
  const x = new Array(len);
  const f = 261.6256;
  for (let i = 0; i < len; i++) {
    const t = i / rate;
    let v = 0;
    for (let h = 1; h <= 6; h++) v += (Math.sin(2 * Math.PI * f * h * t) / h) * Math.exp(-t * (2 + 3 * h));
    const fade = i < 44 ? i / 44 : 1;
    x[i] = 0.5 * v * fade * (1 - i / len);
  }
  return wav(rate, [x]);
})();

for (const [name, bytes] of [
  ["kick.wav", kick],
  ["snare.wav", snare],
  ["pluck.wav", pluck],
]) {
  writeFileSync(join(out, name), bytes);
  console.log(`${createHash("sha256").update(bytes).digest("hex")}  ${name} (${bytes.length} bytes)`);
}
