// Audio in the app (studio, lane A).
// Part 1 (A1): every audio clip draws its file's waveform: the louder channel, mirrored about the
// middle, in the track's ink, under its name. Silence is a flat line; a reversed clip is its
// mirror; a clip's volume shows in its label and scales the drawing; one peaks request per file
// however often it redraws; a missing file says its name, as before.
// Part 2 (A2): File ▸ Import Audio… (⇧⌘I; Ctrl+Shift+I on a PC) picks WAV files and lands them on
// the selected audio track at the playhead's bar, selected, as one undo step ("Undo importing
// …"); a file that isn't a WAV is refused in Q1's words and adds nothing; `assets/` holds exactly
// the files; the lane file is canonical.
// Part 3 (A3): an audio clip moves, copies, splits, duplicates and deletes as one undo step each;
// its left edge moves where it starts in its file; its right edge stops at the file's end ("End
// of tone.wav"); a fade handle dragged in writes a fade ("Fade in 0.40 s", "Undo Tone fade in");
// Volume… −6 writes −6 dB and the waveform halves; Reverse writes it, checked, and mirrors the
// waveform; Remove fades; a reversed clip trims; each new state linted.
// Part 4 (A2, drops and + Track): + Track ▸ Audio track ("audio" finds it; its foot says the code;
// "Added Audio"; "Undo adding Audio"); WAV files dropped (as a browser drops them) below the tracks
// make a new audio track at the bar with its waveform, on Kick (notes) one right after it, on an
// audio lane they land there; the hints say so while dragging, with an outline; an MP3 says what
// music2 opens and writes nothing; so does a MIDI file, which can't be dropped yet (the review's
// R3); an instrument's drag shows no file hint; a browser's file too big to upload says so and sends
// nothing (the review's R9); each state linted.
//
//   node e2e/st-audio.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
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

const { page, errors, key, api, show, toasts, box, drag } = await open(1440, 900);
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
  }).catch(() => []);

const tone = await columns("tone");
check(tone.length > 50 && tone.every((c) => c.mid), `the tone's middle row has ink all across: ${tone.length} columns`);
// The file lasts 2 s of the clip's 1.94 s: full scale fills the body, top to bottom, all across.
check(tone.length > 0 && tone.every((c) => c.ink >= c.height - 1), `full scale fills the body: the thinnest column ${Math.min(...tone.map((c) => c.ink))} of ${tone[0]?.height}`);
// Each read answers null where the page has nothing to read (the page before A1), so the rest runs.
const ink = await page.$eval(`${clip("tone")} canvas`, (c) => {
  const d = c.getContext("2d").getImageData(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1).data;
  return `#${[d[0], d[1], d[2]].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}).catch(() => null);
const colour = await page.$eval(clip("tone"), (el) => getComputedStyle(el).getPropertyValue("--clip-ink").trim());
check(!!ink && ink === colour.toLowerCase(), `drawn in the track's ink: ${ink} (${colour})`);
const name = await page.$eval(`${clip("tone")} [data-clip-name]`, (e) => e.textContent).catch(() => null);
check(name === "tone.wav", `its name in the strip at the top: ${name}`);

const quiet = await columns("quiet");
check(quiet.length > 50 && quiet.every((c) => c.mid && c.ink <= 2), `silence is a flat line: the thickest column ${Math.max(...quiet.map((c) => c.ink))} rows`);

const swell = (await columns("swell")).map((c) => c.ink);
const back = (await columns("back")).map((c) => c.ink);
const third = Math.floor(swell.length / 3);
const mean = (a) => a.reduce((s, v) => s + v, 0) / Math.max(1, a.length);
check(mean(swell.slice(-third)) > 2 * mean(swell.slice(0, third)) + 2, `the swell grows: ${mean(swell.slice(0, third)).toFixed(1)} → ${mean(swell.slice(-third)).toFixed(1)} rows`);
const mirrored = swell.length > 0 && swell.length === back.length && swell.every((v, i) => Math.abs(v - back[back.length - 1 - i]) <= 2);
check(mirrored, `the reversed clip is its mirror (${swell.length} and ${back.length} columns): ${swell.slice(0, 4)}… / ${back.slice(-4)}`);

const half = await columns("half");
const halfMean = mean(half.map((c) => c.ink)), height = half[0]?.height ?? 0;
check(height > 0 && Math.abs(halfMean - height / 2) <= 2, `−6 dB draws half as tall: ${halfMean.toFixed(1)} of ${height} rows`);
const halfName = await page.$eval(`${clip("half")} [data-clip-name]`, (e) => e.textContent).catch(() => null);
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
check(toneAgain.length > 0 && toneAgain.length === tone.length && toneAgain.every((c) => c.mid), `after zooming back it draws the same: ${toneAgain.length} columns`);

await linted(lane, "the audio lane and its waveforms");

// ---------------------------------------------------------------- part 2: File ▸ Import Audio…

const files = `${ROOT}/files`;
wav(files, "Lead Vox.WAV", 1.5, (t) => 0.5 * Math.sin(2 * Math.PI * 440 * t));
wav(files, "hit.wav", 0.3, (t) => Math.exp(-t * 20) * Math.sin(2 * Math.PI * 80 * t));
fs.writeFileSync(`${files}/song.mp3`, Buffer.from("ID3\x04\x00\x00\x00\x00\x00\x00 not a wav"));
const assetsWere = fs.readdirSync(`${PROJ}/assets`).sort();
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const laneClips = () => JSON.parse(read("data/tone/clips.json")).clips;

/** ⇧⌘I (or `combo`), then pick `paths` in the file chooser. */
async function importVia(combo, paths) {
  const [chooser] = await Promise.all([page.waitForFileChooser({ timeout: 5000 }).catch(() => null), key(combo)]);
  check(!!chooser?.isMultiple(), `${combo} opens a chooser that takes several files`);
  await chooser?.accept(paths);
}

// The playhead in bar 5 (beat 17): files land on bar 5's line, beat 16.
await show("tone");
const session = (await api("")).body.session;
await api("/seek", { method: "POST", body: JSON.stringify({ beat: 17 }) });
await until(async () => Math.abs(((await api("/status")).body.beat ?? 0) - 17) < 0.01);
await sleep(300);
const before = laneClips().length;
await importVia("Meta+Shift+KeyI", [`${files}/Lead Vox.WAV`, `${files}/hit.wav`]);
check(await until(() => laneClips().length === before + 2, 8000), `⇧⌘I brought two files onto Tone: ${laneClips().length - before}`);
const landed = laneClips().filter((c) => c.asset === "Lead Vox.wav" || c.asset === "hit.wav");
const beats = (s) => (s * 124) / 60;
const close = (a, b) => Math.abs(a - b) < 1e-6;
check(
  landed.length === 2 && landed[0].asset === "Lead Vox.wav" && landed[0].start === 16 && close(landed[0].length, beats(1.5)) && landed[1].asset === "hit.wav" && close(landed[1].start, 16 + beats(1.5)) && close(landed[1].length, beats(0.3)),
  `at bar 5's line, one after another, each the file's length at 124 BPM: ${JSON.stringify(landed.map((c) => [c.asset, c.start, c.length]))}`,
);
check(landed.length === 2 && landed.every((c) => c.offset === 0 && c.gain_db === 0 && c.hash?.length === 16 && c.id), `each a whole clip with its pin and an id: ${JSON.stringify(landed[0])}`);
const picked = await until(() => page.$$eval(`${lane} [data-clip][data-picked]`, (els) => els.map((e) => e.dataset.clip)).then((ids) => ids.length === 2 && landed.every((c) => ids.includes(c.id))));
check(picked, `the new clips are selected: ${await page.$$eval(`${lane} [data-clip][data-picked]`, (els) => els.map((e) => e.dataset.clip))}`);
check(await until(async () => (await undoTitle()) === "Undo importing 2 files (⌘Z)"), `one undo step, named: ${await undoTitle()}`);
const assetsNow = fs.readdirSync(`${PROJ}/assets`).sort();
check(JSON.stringify(assetsNow) === JSON.stringify([...assetsWere, "Lead Vox.wav", "hit.wav"].sort()), `assets/ holds exactly the files: ${assetsNow}`);
check(fs.existsSync(`${PROJ}/assets/Lead Vox.wav`) && fs.readFileSync(`${PROJ}/assets/Lead Vox.wav`).equals(fs.readFileSync(`${files}/Lead Vox.WAV`)), "a copy, byte for byte");
const text = read("data/tone/clips.json");
check(text.startsWith('{"clips": [\n  {"id": ') && text.endsWith("]}\n"), `the lane file is canonical: ${text.slice(0, 40)}…`);
check(await until(() => page.$$eval(`${lane} [data-clip] canvas`, (cs) => cs.length >= 7)), "their waveforms draw");
await page.$eval(`${lane} [data-clip="${landed[0]?.id}"]`, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
if (process.env.ST_SHOT) await page.screenshot({ path: `${process.env.ST_SHOT}/a2-menu.png` });
await linted(lane, "the lane after an import");

// ⌘Z takes both back; the files stay (music2 never deletes an asset).
await key("Meta+KeyZ");
check(await until(() => laneClips().length === before), `one ⌘Z takes the import back: ${laneClips().length - before} left`);
check(fs.existsSync(`${PROJ}/assets/hit.wav`), "the files stay in assets/");

// Not a WAV: refused in words, nothing added.
const revision = (await api("")).body.view.revision;
await importVia("Meta+Shift+KeyI", [`${files}/song.mp3`]);
check(await until(async () => (await toasts()).some((t) => t.includes("song.mp3 isn't a WAV file. music2 reads WAV files. Save it as a WAV first: most audio apps can (Export, then WAV)."))), `an MP3 says Q1's words: ${await toasts()}`);
check(laneClips().length === before && (await api("")).body.view.revision === revision && !fs.existsSync(`${PROJ}/assets/song.mp3`), "and adds nothing");

// A PC page: Ctrl+Shift+I.
const pc = await open(1440, 900, { pc: true });
pc.page.on("pageerror", (e) => errors.push(String(e)));
await pc.show("tone");
const [pcChooser] = await Promise.all([pc.page.waitForFileChooser({ timeout: 5000 }).catch(() => null), pc.key("Control+Shift+KeyI")]);
await pcChooser?.accept([`${files}/hit.wav`]);
check(await until(() => laneClips().some((c) => c.asset === "hit.wav")), "a PC page's Ctrl+Shift+I imports too");
await pc.browser.close();

// ---------------------------------------------------------------- part 3: editing audio clips

await page.keyboard.press("Escape");
write("data/tone/clips.json", JSON.stringify({ clips: [
  { id: "p3", start: 32, length: 3, asset: "tone.wav" },
  { id: "sw", start: 36, length: 6, asset: "swell.wav" },
] }, null, 2));
const p3 = clip("p3"), sw = clip("sw");
check(await until(() => page.$(`${p3} canvas`).then((x) => !!x) && page.$(`${sw} canvas`).then((x) => !!x), 8000), "two clips for part 3");
await page.$eval(p3, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(300);
const one = (id) => laneClips().find((c) => c.id === id);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);
const bubble = () => page.$eval("[data-testid=clip-bubble], [data-testid=fade-bubble]", (e) => e.textContent).catch(() => null);
const ppb = 24;

// The right edge past the file's end stops there: tone.wav is 2 s, 4.13 beats at 124 BPM.
let b = await box(p3);
await page.mouse.move(b.x + b.width - 2, b.y + b.height / 2 + 6);
await page.mouse.down();
await page.mouse.move(b.x + b.width + 3 * ppb, b.y + b.height / 2 + 6, { steps: 8 });
const endWords = await bubble();
await page.mouse.up();
check(endWords === "End of tone.wav", `the bubble says where the file ends: ${endWords}`);
check(await until(() => close(one("p3").length, beats(2))), `the right edge stops at the file's end: ${one("p3").length}`);
check(await stepIs("resizing a Tone clip"), `one undo step: ${await undoTitle()}`);

// The left edge in by a beat: the clip starts a beat later in its file.
b = await box(p3);
await page.mouse.move(b.x + 2, b.y + b.height / 2 + 6);
await page.mouse.down();
await page.mouse.move(b.x + 2 + ppb, b.y + b.height / 2 + 6, { steps: 8 });
await page.mouse.up();
check(await until(() => one("p3").start === 33 && close(one("p3").offset, 60 / 124)), `a left trim moves where it starts in the file: ${JSON.stringify(one("p3"))}`);

// A fade handle dragged in: a fade in, in seconds.
b = await box(p3);
await page.click(p3, { offset: { x: b.width / 2, y: b.height - 6 } });
const handle = (await box(`${p3} [data-fade=in]`)) ?? { x: b.x + 1, y: b.y + 1, width: 0, height: 0 };
check(Math.round(handle.width) === 7 && Math.round(handle.height) === 7, `the selected clip shows its 7 px fade handles: ${handle.width}`);
await page.mouse.move(handle.x + 3, handle.y + 3);
await page.mouse.down();
await page.mouse.move(handle.x + 3 + 20, handle.y + 3, { steps: 6 });
const fadeWords = await bubble();
await page.mouse.up();
check(/^Fade in 0\.\d\d s$/.test(fadeWords ?? ""), `the bubble says the fade: ${fadeWords}`);
check(await until(() => one("p3").fade_in > 0.3 && one("p3").fade_in < 0.6), `the fade is written in seconds: ${one("p3").fade_in}`);
check(await stepIs("Tone fade in"), `named "Tone fade in": ${await undoTitle()}`);
await linted(p3, "a clip with its fade handles");

/** Open the clip menu on `sel`, then pick row `id`. */
async function menu(sel, id) {
  const r = await box(sel);
  await page.mouse.click(r.x + r.width / 2, r.y + r.height - 6, { button: "right" });
  await page.waitForSelector(`[data-testid=clip-menu] [data-menu-item=${id}]`, { timeout: 3000 }).catch(() => null);
  return page.$(`[data-testid=clip-menu] [data-menu-item=${id}]`);
}
const rows = async () => {
  await menu(p3, "volume");
  const r = await page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => [e.dataset.menuItem, e.getAttribute("aria-checked"), e.disabled]));
  await linted("[data-testid=clip-menu]", "the audio clip's menu");
  await page.keyboard.press("Escape");
  return r;
};
const listed = await rows();
const at = (id) => listed.findIndex((r) => r[0] === id);
check(at("volume") === at("split") + 1 && at("reverse") === at("volume") + 1 && at("follow") === at("reverse") + 1 && at("unfade") === at("follow") + 1, `Volume…, Reverse, Follow tempo and Remove fades come after Split: ${listed.map((r) => r[0])}`);
check(listed[at("reverse")]?.[1] === "false" && listed[at("unfade")]?.[2] === false, `Reverse unchecked, Remove fades on: ${JSON.stringify(listed.slice(at("volume"), at("unfade") + 1))}`);

// Volume… −6: the label says it and the waveform halves.
const full = mean((await columns("p3")).map((c) => c.ink)) || 1;
await (await menu(p3, "volume"))?.click();
await page.waitForSelector("[data-testid=volume-popover] input", { timeout: 3000 }).catch(() => null);
const focused = await page.evaluate(() => document.activeElement?.dataset.testid);
check(focused === "clip-volume", `the field has the focus: ${focused}`);
await linted("[data-testid=volume-popover]", "the Clip volume popover");
await page.keyboard.press("ArrowDown");
await page.keyboard.press("ArrowDown");
const stepped = await page.$eval("[data-testid=clip-volume]", (e) => e.value).catch(() => null);
check(stepped === "−1.0", `↓ steps 0.5 dB: ${stepped}`);
await page.$eval("[data-testid=clip-volume]", (e) => e.select()).catch(() => {});
await page.keyboard.type("-6");
await page.keyboard.press("Enter");
check(await until(() => one("p3").gain_db === -6), `Volume… −6 writes gain_db −6: ${one("p3").gain_db}`);
check(await stepIs("Tone clip volume"), `named "Tone clip volume": ${await undoTitle()}`);
check(await until(async () => (await page.$eval(`${p3} [data-clip-name]`, (e) => e.textContent).catch(() => "")) === "tone.wav · −6 dB"), "its label says −6 dB");
let halved = 0;
await until(async () => {
  halved = mean((await columns("p3")).map((c) => c.ink));
  return Math.abs(halved - full / 2) <= 2;
});
check(Math.abs(halved - full / 2) <= 2, `and its waveform is half as tall: ${full.toFixed(1)} → ${halved.toFixed(1)}`);

// Remove fades.
await (await menu(p3, "unfade"))?.click();
check(await until(() => one("p3").fade_in === 0 && one("p3").fade_out === 0), "Remove fades takes them out");
check(await stepIs("removing the Tone clip's fades"), `named so: ${await undoTitle()}`);

// Reverse: written, checked, and the waveform mirrored.
const ahead = (await columns("sw")).map((c) => c.ink);
await (await menu(sw, "reverse"))?.click();
check(await until(() => one("sw").reverse === true), "Reverse writes reverse: true");
check(await stepIs("reversing the Tone clip"), `named so: ${await undoTitle()}`);
let behind = [];
await until(async () => {
  behind = (await columns("sw")).map((c) => c.ink);
  return behind.length === ahead.length && ahead.every((v, i) => Math.abs(v - behind[behind.length - 1 - i]) <= 2);
});
check(ahead.length > 0 && behind.length === ahead.length && ahead.every((v, i) => Math.abs(v - behind[behind.length - 1 - i]) <= 2), "its waveform is mirrored");
const checkedNow = await (await menu(sw, "reverse"))?.evaluate((e) => e.getAttribute("aria-checked"));
await page.keyboard.press("Escape");
check(checkedNow === "true", `the menu's Reverse is checked: ${checkedNow}`);
// A reversed clip's right edge in by a beat: it starts later in its file.
b = await box(sw);
await page.mouse.move(b.x + b.width - 2, b.y + b.height / 2 + 6);
await page.mouse.down();
await page.mouse.move(b.x + b.width - 2 - ppb, b.y + b.height / 2 + 6, { steps: 8 });
await page.mouse.up();
check(await until(() => one("sw").length === 5 && close(one("sw").offset, 60 / 124)), `a reversed clip trims from its right edge: ${JSON.stringify(one("sw"))}`);

// Move, copy, split, duplicate and delete: one undo step each.
b = await box(p3);
await drag({ x: b.x + b.width / 2, y: b.y + b.height - 6 }, { x: b.x + b.width / 2 + 4 * ppb, y: b.y + b.height - 6 });
check(await until(() => one("p3").start === 37), `a drag moves it: ${one("p3").start}`);
check(await stepIs("moving a Tone clip"), `"moving a Tone clip": ${await undoTitle()}`);
await key("Meta+KeyD");
check(await until(() => laneClips().filter((c) => c.asset === "tone.wav" && c.gain_db === -6).length === 2), "⌘D duplicates it");
check(await stepIs("duplicating a Tone clip"), `"duplicating a Tone clip": ${await undoTitle()}`);
await key("Backspace");
check(await until(() => laneClips().filter((c) => c.asset === "tone.wav" && c.gain_db === -6).length === 1), "⌫ deletes the copy");
check(await stepIs("deleting a Tone clip"), `"deleting a Tone clip": ${await undoTitle()}`);
await linted(lane, "the lane after its edits");
if (process.env.ST_SHOT) await page.screenshot({ path: `${process.env.ST_SHOT}/a3.png` });

// ---------------------------------------------------------------- part 4: + Track ▸ Audio track, and drops

const tracksNow = async () => (await api("")).body.timeline.tracks.filter((t) => t.track).map((t) => t.module);
await page.keyboard.press("Escape");
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list] input");
await page.keyboard.type("audio");
const row = await page.$$eval("[data-testid=instrument-list] [role=group]", (gs) => gs.map((g) => [g.getAttribute("aria-label"), [...g.querySelectorAll("[role=option]")].map((o) => o.textContent)]));
check(JSON.stringify(row) === JSON.stringify([["Audio", ["Audio trackPlays recordings and loops: drop a WAV file on it"]]]), `typing "audio" finds the Audio group: ${JSON.stringify(row)}`);
const foot = await page.$eval("[data-testid=instrument-code]", (e) => e.textContent);
check(foot === 'tracks/audio.ts · lane.audio("clips")', `its foot says the code: ${foot}`);
await linted("[data-testid=instrument-list]", "+ Track's list with Audio");
await page.keyboard.press("Enter");
check(await until(async () => (await tracksNow()).includes("audio")), `+ Track ▸ Audio track adds one: ${await tracksNow()}`);
check(fs.existsSync(`${PROJ}/tracks/audio.ts`) && read("tracks/audio.ts").includes('lane.audio("clips")') && JSON.parse(read("data/audio/clips.json")).clips.length === 0, "its file plays an empty clips lane");
check(await until(async () => (await toasts()).some((t) => t.startsWith("Added Audio"))), `a toast says so: ${await toasts()}`);
check(await stepIs("adding Audio"), `named "adding Audio": ${await undoTitle()}`);

/** Drag files over (`x`, `y`) as a browser does, then (unless `hold`) drop them there; the hint said while over. */
async function dropFiles(x, y, files, hold = false) {
  return page.evaluate(
    async (x, y, files, hold) => {
      const dt = new DataTransfer();
      for (const f of files) {
        const file = new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: f.type });
        // A file as big as a long recording, without making one.
        if (f.size) Object.defineProperty(file, "size", { value: f.size });
        dt.items.add(file);
      }
      const at = document.elementFromPoint(x, y) ?? document.body;
      const fire = (type) => at.dispatchEvent(new DragEvent(type, { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
      fire("dragenter");
      fire("dragover");
      // The hint draws on the next frame.
      await new Promise((r) => setTimeout(r, 120));
      const hint = document.querySelector("[data-testid=drop-hint]")?.textContent ?? null;
      const outline = document.querySelector("[data-testid=drop-outline]")?.dataset.kind ?? null;
      if (!hold) fire("drop");
      return { hint, outline };
    },
    x,
    y,
    files.map((f) => ({ name: f.name, type: f.type ?? "audio/wav", size: f.size, b64: fs.readFileSync(f.path).toString("base64") })),
    hold,
  );
}
await sleep(150);
await page.evaluate(() => document.querySelector("[data-testid=timeline]").scrollTo({ left: 0 }));
await sleep(200);
const ruler = await box("[data-testid=ruler]");
const xAt = (beat) => ruler.x + beat * ppb + 6;
const yBelow = async () => {
  const rows = await page.$$eval("[data-track]", (els) => els.filter((e) => e.dataset.track !== "song").map((e) => e.getBoundingClientRect().bottom));
  return Math.max(...rows) + 20;
};
wav(files, "drum loop.wav", 1, (t) => Math.sin(2 * Math.PI * 110 * t) * Math.exp(-((t * 4) % 1) * 6));
// Below the tracks at bar 3: a new audio track, named after the file.
const below = await dropFiles(xAt(9), await yBelow(), [{ name: "drum loop.wav", path: `${files}/drum loop.wav` }]);
check(below.hint === "New audio track · bar 3" && below.outline === "below", `the hint below the tracks: ${JSON.stringify(below)}`);
check(await until(async () => (await tracksNow()).includes("drum-loop")), `a drop below the tracks makes "Drum loop": ${await tracksNow()}`);
const dl = () => JSON.parse(read("data/drum-loop/clips.json")).clips;
check(dl().length === 1 && dl()[0].start === 8 && dl()[0].asset === "drum loop.wav", `its clip at bar 3: ${JSON.stringify(dl())}`);
check(await until(() => page.$$eval('[data-lane="drum-loop.clips"] [data-clip] canvas', (cs) => cs.length === 1)), "with its waveform");
check(await stepIs("importing drum loop.wav"), `named "importing drum loop.wav": ${await undoTitle()}`);
// On Kick (notes) at bar 2: a new audio track right after Kick.
const kick = await box('[data-lane="kick.hits"]');
const onKick = await dropFiles(xAt(5), kick.y + kick.height / 2, [{ name: "hit.wav", path: `${files}/hit.wav` }]);
check(onKick.hint === "New audio track · bar 2" && onKick.outline === "track", `the hint on Kick: ${JSON.stringify(onKick)}`);
check(await until(async () => {
  const t = await tracksNow();
  return t[t.indexOf("kick") + 1] === "hit";
}), `a drop on Kick makes an audio track right after it: ${await tracksNow()}`);
// Two files onto the Audio track's lane at bar 2: onto it, one after another.
const audioRow = await box('[data-lane="audio.clips"]');
const onLane = await dropFiles(xAt(6), audioRow.y + audioRow.height / 2, [{ name: "hit.wav", path: `${files}/hit.wav` }, { name: "Lead Vox.WAV", path: `${files}/Lead Vox.WAV` }], true);
check(onLane.hint === "Audio · bar 2" && onLane.outline === "lane", `the hint on the audio lane: ${JSON.stringify(onLane)}`);
await linted("[data-testid=drop-hint]", "the drop hint");
if (process.env.ST_SHOT) await page.screenshot({ path: `${process.env.ST_SHOT}/a4-hint.png` });
await dropFiles(xAt(6), audioRow.y + audioRow.height / 2, [{ name: "hit.wav", path: `${files}/hit.wav` }, { name: "Lead Vox.WAV", path: `${files}/Lead Vox.WAV` }]);
const audioClips = () => JSON.parse(read("data/audio/clips.json")).clips;
check(await until(() => audioClips().length === 2), `two files onto the lane: ${audioClips().length}`);
check(audioClips()[0].start === 4 && close(audioClips()[1].start, 4 + beats(0.3)), `one after another from bar 2: ${JSON.stringify(audioClips().map((c) => c.start))}`);
check(!(await page.$("[data-testid=drop-hint]")), "the hint is gone after the drop");
// Not a WAV: says what music2 opens, writes nothing.
const rev = (await api("")).body.view.revision;
const mp3 = await dropFiles(xAt(9), await yBelow(), [{ name: "song.mp3", path: `${files}/song.mp3`, type: "audio/mpeg" }]);
check(mp3.hint === "music2 opens WAV files" && mp3.outline === null, `an MP3's hint: ${JSON.stringify(mp3)}`);
await sleep(400);
check((await api("")).body.view.revision === rev && !fs.existsSync(`${PROJ}/assets/song.mp3`), "and nothing is written");
// A MIDI file: no drop takes it yet, and the words never say one does.
const mid = await dropFiles(xAt(9), await yBelow(), [{ name: "riff.mid", path: `${files}/song.mp3`, type: "audio/midi" }]);
check(mid.hint === "music2 opens WAV files" && mid.outline === null, `a MIDI file's hint: ${JSON.stringify(mid)}`);
check(await until(async () => (await toasts()).includes("music2 opens WAV files.")), `its drop's toast says the same: ${await toasts()}`);
// An instrument dragged from the browser is no file: no hint.
const instrument = await page.evaluate(async (x, y) => {
  const dt = new DataTransfer();
  dt.setData("application/x-m2-instrument", "std:synth-pluck");
  const at = document.elementFromPoint(x, y);
  at.dispatchEvent(new DragEvent("dragenter", { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
  at.dispatchEvent(new DragEvent("dragover", { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
  await new Promise((r) => setTimeout(r, 120));
  const hint = !!document.querySelector("[data-testid=drop-hint]");
  at.dispatchEvent(new DragEvent("dragleave", { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
  return hint;
}, xAt(9), await yBelow());
check(!instrument, "an instrument's drag shows no file hint");
// Too big to upload from a browser: said in words, and nothing is sent.
const sent = [];
const watch = (r) => r.url().includes("/api/graph/audio/import") && sent.push(r.url());
page.on("request", watch);
const revBig = (await api("")).body.view.revision;
await dropFiles(xAt(9), await yBelow(), [{ name: "long.wav", path: `${files}/hit.wav`, size: 2 ** 30 + 1 }]);
check(
  await until(async () => (await toasts()).includes("Nothing was added to the song: long.wav is too big to upload from a browser (over 1 GB).")),
  `a file over 1 GB says so: ${await toasts()}`,
);
await sleep(300);
check(sent.length === 0 && (await api("")).body.view.revision === revBig, `and nothing is sent or written (${sent.length})`);
page.off("request", watch);
await linted(lane, "the timeline after the drops");
done(errors);
