// Audio in the app (studio, lane A).
// Part 1 (A1): every audio clip draws its file's waveform: the louder channel, mirrored about the
// middle, in the track's ink, under its name. Silence is a flat line; a reversed clip is its
// mirror; a clip's volume shows in its label and scales the drawing; one peaks request per file
// however often it redraws; a missing file says its name, as before.
//
//   node e2e/st-audio.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

/** A mono 16-bit WAV at 48 kHz of `seconds`, sample `i` from `f(i / rate)` (−1..1), written as `name` in `dir`. */
function wav(dir, name, seconds, f, rate = 48000) {
  const n = Math.round(seconds * rate);
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, f(i / rate))) * 32767), 44 + i * 2);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(`${dir}/${name}`, b);
}

const { page, errors, key } = await open(1440, 900);
const peaksAsked = [];
page.on("request", (r) => r.url().includes("/api/graph/audio/peaks") && peaksAsked.push(decodeURIComponent(r.url().replace(/^.*asset=/, ""))));

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};

// ---------------------------------------------------------------- the song: a Tone track with five clips

// At 124 BPM a beat is 0.484 s. The tone is a full-scale sine; the swell's level rises from 0 to full.
wav(`${PROJ}/assets`, "tone.wav", 2, (t) => Math.sin(2 * Math.PI * 220 * t));
wav(`${PROJ}/assets`, "silence.wav", 2, () => 0);
wav(`${PROJ}/assets`, "swell.wav", 3.8, (t) => (t / 3.8) * Math.sin(2 * Math.PI * 330 * t));
write("tracks/tone.ts", 'import { track } from "@m2/core";\n\nexport default track("tone", { name: "Tone" }, ({ lane }) => lane.audio("clips"));\n');
const clips = [
  { id: "tone", start: 0, length: 4, asset: "tone.wav" },
  { id: "quiet", start: 4, length: 4, asset: "silence.wav" },
  { id: "swell", start: 8, length: 8, asset: "swell.wav" },
  { id: "back", start: 16, length: 8, asset: "swell.wav", reverse: true },
  { id: "half", start: 24, length: 4, asset: "tone.wav", gain_db: -6 },
  { id: "gone", start: 28, length: 4, asset: "gone.wav" },
];
fs.mkdirSync(`${PROJ}/data/tone`, { recursive: true });
write("data/tone/clips.json", JSON.stringify({ clips }, null, 2));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport tone from "./tracks/tone";').replace("      kick: kick.out,", "      kick: kick.out,\n      tone: tone.out,"));

const lane = '[data-lane="tone.clips"]';
const clip = (id) => `${lane} [data-clip="${id}"]`;
const appeared = await until(() => page.$(clip("half")), 20000);
check(!!appeared, "the Tone track's clips are on the timeline");
const drawn = await until(() => page.$$eval(`${lane} [data-clip] canvas`, (cs) => cs.length === 5), 8000);
check(drawn, `each clip with a file draws a canvas: ${await page.$$eval(`${lane} [data-clip] canvas`, (cs) => cs.length)}`);
if (process.env.ST_SHOT) await page.screenshot({ path: `${process.env.ST_SHOT}/a1.png` });
await sleep(300);

/** Each canvas column of clip `id`: how many device rows hold ink, and whether the middle one does. */
const columns = (id) =>
  page.$eval(clip(id), (el) => {
    const cs = [...el.querySelectorAll("canvas")];
    const out = [];
    for (const c of cs) {
      const g = c.getContext("2d");
      const { data, width, height } = g.getImageData(0, 0, c.width, c.height);
      for (let x = 0; x < width; x++) {
        let ink = 0;
        for (let y = 0; y < height; y++) if (data[(y * width + x) * 4 + 3] > 0) ink++;
        out.push({ ink, mid: data[(Math.floor(height / 2) * width + x) * 4 + 3] > 0, height });
      }
    }
    return out;
  });

const tone = await columns("tone");
check(tone.length > 50 && tone.every((c) => c.mid), `the tone's middle row has ink all across: ${tone.length} columns`);
// The file lasts 2 s of the clip's 1.94 s: full scale fills the body, top to bottom, all across.
check(tone.length > 0 && tone.every((c) => c.ink >= c.height - 1), `full scale fills the body: the thinnest column ${Math.min(...tone.map((c) => c.ink))} of ${tone[0]?.height}`);
const ink = await page.$eval(`${clip("tone")} canvas`, (c) => {
  const d = c.getContext("2d").getImageData(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1).data;
  return `#${[d[0], d[1], d[2]].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
});
const colour = await page.$eval(clip("tone"), (el) => getComputedStyle(el).getPropertyValue("--clip-ink").trim());
check(ink === colour.toLowerCase(), `drawn in the track's ink: ${ink} (${colour})`);
const name = await page.$eval(`${clip("tone")} [data-clip-name]`, (e) => e.textContent);
check(name === "tone.wav", `its name in the strip at the top: ${name}`);

const quiet = await columns("quiet");
check(quiet.length > 50 && quiet.every((c) => c.mid && c.ink <= 2), `silence is a flat line: the thickest column ${Math.max(...quiet.map((c) => c.ink))} rows`);

const swell = (await columns("swell")).map((c) => c.ink);
const back = (await columns("back")).map((c) => c.ink);
const third = Math.floor(swell.length / 3);
const mean = (a) => a.reduce((s, v) => s + v, 0) / Math.max(1, a.length);
check(mean(swell.slice(-third)) > 2 * mean(swell.slice(0, third)) + 2, `the swell grows: ${mean(swell.slice(0, third)).toFixed(1)} → ${mean(swell.slice(-third)).toFixed(1)} rows`);
const mirrored = swell.length === back.length && swell.every((v, i) => Math.abs(v - back[back.length - 1 - i]) <= 2);
check(mirrored, `the reversed clip is its mirror (${swell.length} and ${back.length} columns): ${swell.slice(0, 4)}… / ${back.slice(-4)}`);

const half = await columns("half");
const halfMean = mean(half.map((c) => c.ink)), height = half[0]?.height ?? 0;
check(Math.abs(halfMean - height / 2) <= 2, `−6 dB draws half as tall: ${halfMean.toFixed(1)} of ${height} rows`);
const halfName = await page.$eval(`${clip("half")} [data-clip-name]`, (e) => e.textContent);
check(halfName === "tone.wav · −6 dB", `a clip's volume is in its label: ${halfName}`);

const goneWords = await page.$eval(clip("gone"), (el) => ({ text: el.textContent, canvas: !!el.querySelector("canvas"), icon: !!el.querySelector("svg") }));
check(goneWords.text === "gone.wav" && !goneWords.canvas && goneWords.icon, `a missing file keeps its icon and name: ${JSON.stringify(goneWords)}`);

// Redraws (zoom in and out, twice) ask for nothing again.
for (const k of ["Meta+Equal", "Meta+Equal", "Meta+Minus", "Meta+Minus"]) {
  await key(k);
  await sleep(150);
}
await until(() => page.$$eval(`${lane} [data-clip] canvas`, (cs) => cs.length === 5));
await sleep(500);
const asked = [...peaksAsked].sort();
check(JSON.stringify(asked) === JSON.stringify(["gone.wav", "silence.wav", "swell.wav", "tone.wav"]), `one peaks request per file: ${asked.join(", ")}`);
const toneAgain = await columns("tone");
check(toneAgain.length === tone.length && toneAgain.every((c) => c.mid), `after zooming back it draws the same: ${toneAgain.length} columns`);

await linted(lane, "the audio lane and its waveforms");
done(errors);
