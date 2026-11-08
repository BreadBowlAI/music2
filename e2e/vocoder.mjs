// The Vocoder (VOX-29), on first-song with a Lead track (a saw) and a vocal track, Vox (an audio lane
// playing a sung "ah" at 216 Hz). The + after the saw lists the Vocoder under Voice, and picking it
// writes `vocoder(osc.saw(110), { clarity: 0.5, smooth: 0.05, formant: 0 })` in the Lead's own code,
// with the pack's line and lock; its face draws its picture beside Clarity, Smooth and Formant, and its
// Voice row says "No other track". Voice ▸ Vox writes `vox.out` into the call and the import of Vox,
// nothing else, one undo step ("Lead voice Vox"). Then its picture moves with the song, the voice
// singing the first second of each 3 s round of the loop: while it sings, what goes out is the saw in
// the voice's shape, far louder against what comes in about the voice's first formant than at the
// saw's own note; while it is silent, nothing goes out, though the saw comes in (each read from an
// agent's look, two looks in a row agreeing). Its card is shot live, in the patch and on Device's
// chain at Normal and Expanded (to $E2E_SHOTS when set, else the run's folder). ⌘Z takes the voice
// out, byte for byte. Each state linted.
//
//   node e2e/vocoder.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { ROOT, PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const RATE = 48000;
const SHOTS = process.env.E2E_SHOTS ?? ROOT;
fs.mkdirSync(SHOTS, { recursive: true });

/** A mono 16-bit WAV of 2 s: a sung "ah" at 216 Hz, its harmonics shaped by three hills (formants). */
function wav(path) {
  const n = 2 * RATE;
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(RATE, 24);
  b.writeUInt32LE(RATE * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  const hill = (f, at, wide) => 1 / (1 + ((f - at) / wide) ** 2);
  const weight = (f) => 0.3 + hill(f, 700, 150) + 0.7 * hill(f, 1220, 180) + 0.3 * hill(f, 2600, 250);
  const v = new Float64Array(n);
  for (let h = 1; h <= 24; h++) {
    const a = weight(216 * h) / Math.sqrt(h);
    for (let i = 0; i < n; i++) v[i] += a * Math.sin((2 * Math.PI * 216 * h * i) / RATE);
  }
  const most = v.reduce((m, x) => Math.max(m, Math.abs(x)), 0);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round((0.4 * v[i] * 32767) / most), 44 + i * 2);
  fs.writeFileSync(path, b);
}

// ---------------------------------------------------------------- the song: Vox and Lead

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox.wav`);
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
// The voice sings the first 2 beats (1 s) of each round of the loop, 6 beats (3 s): then it is silent.
write("data/vox/clips.json", JSON.stringify({ clips: [{ id: "voxclip1", start: 0, length: 2, asset: "vox.wav", offset: 0 }] }, null, 2));
write("tracks/lead.ts", 'import { track } from "@m2/core";\nimport { osc } from "std/dsp";\n\nexport default track("lead", { name: "Lead" }, () => osc.saw(110));\n');
write(
  "song.ts",
  read("song.ts")
    .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";\nimport lead from "./tracks/lead";')
    .replace("      kick: kick.out,", "      kick: kick.out,\n      vox: { in: vox.out, db: db(-12) },\n      lead: { in: lead.out, db: db(-12) },"),
);

const { page, errors, show, view, key, box, api } = await open(1600, 1000);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const revision = async () => (await api("")).body.view?.revision;
/** `sel` scrolled into the middle of the window, and where it is then. */
const seen = async (sel) => {
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
  await sleep(150);
  return box(sel);
};
/** A picture's words, its group's name. */
const words = (pic) => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
/** What each live layer of a picture drew last (CARD-103). */
const said = (pic) => page.$eval(`${pic} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")).catch(() => []);
/** A card's shot. */
const shoot = async (card, name) => {
  const b = await seen(card);
  if (!b) return check(false, `${name}: no card to shoot`);
  await page.screenshot({ path: `${SHOTS}/vocoder-${name}.png`, clip: { x: Math.max(0, b.x - 4), y: Math.max(0, b.y - 4), width: Math.min(b.width + 8, 1600), height: Math.min(b.height + 8, 1000) } });
};
/** The view's id of the call labelled `label` in `module`. */
const nodeOf = async (module, label) => {
  const g = (await api("")).body;
  return g.view?.modules?.find((m) => m.key === module)?.nodes?.find((n) => n.label === label)?.id;
};
const lead = () => read("tracks/lead.ts");

check(!!(await until(() => page.$('[data-module-tab="vox"]'), 20000)) && !!(await page.$('[data-module-tab="lead"]')), "the Vox and Lead tracks are there");

// ---------------------------------------------------------------- + ▸ Vocoder (VOX-29)

await show("lead");
await view("graph");
const plus = '[data-module="lead"] button[aria-label^="Add an effect after"]';
check(!!(await until(() => page.$(plus), 15000)), "the saw has a +");
await page.click(plus);
await page.waitForSelector('[data-effect-browser] [data-effect="vocoder"]', { timeout: 5000 }).catch(() => {});
const groupOf = (name) => page.$eval(`[data-effect-browser] [data-effect="${name}"]`, (e) => e.closest("[data-category]")?.dataset.category).catch(() => null);
check((await groupOf("vocoder")) === "Voice" && (await groupOf("voiceChanger")) === "Voice", `the Vocoder is listed under Voice, beside the Voice changer: ${await groupOf("vocoder")}`);
await page.click('[data-effect-browser] [data-effect="vocoder"]');
const added = "vocoder(osc.saw(110), { clarity: 0.5, smooth: 0.05, formant: 0 })";
check(await until(() => lead().includes(added), 15000), `it goes after the saw, in the Lead's own code: ${lead().match(/=> .*/)?.[0]}`);
check(lead().includes('import { vocoder } from "@m2/vocals";') && /"@m2\/vocals" = /.test(read("music2.toml")) && read("music2.lock").includes('name = "@m2/vocals"'), "imported, and the pack listed and locked");
const alone = lead();

const card = '[data-module="lead"] [data-node="vocoder"]';
const pic = `${card} [data-picture="spectrum"]`;
check(!!(await until(() => page.$(`${pic} svg`), 15000)), "its face draws its picture");
const knobs = await page.$$eval(`${card} [data-knob]`, (ks) => ks.map((k) => k.dataset.knob)).catch(() => []);
check(["clarity", "smooth", "formant"].every((k) => knobs.includes(k)), `Clarity, Smooth and Formant are knobs on its face: ${knobs.join()}`);
check(
  (await words(pic)) ===
    "Faint: your synth as it comes in. Solid: as it goes out, in the shape of the voice you pick in Voice: its hills are the voice's vowels, and they move as it talks",
  `its words: ${await words(pic)}`,
);
const row = () => page.$eval(`${card} [data-listens]`, (e) => e.textContent).catch(() => null);
check(await until(async () => (await row()) === "Voice:No other track"), `its Voice row says it hears no other track yet: ${await row()}`);
await linted(card, "the Vocoder's card");

// ---------------------------------------------------------------- Voice ▸ Vox (CARD-35)

await seen(`${card} [data-testid=listens-to]`);
await page.click(`${card} [data-testid=listens-to]`).catch(() => {});
await until(async () => !!(await page.$("[data-testid=listens-menu]")), 3000);
const choices = await page.$$eval("[data-testid=listens-menu] [role=menuitem]", (r) => r.map((x) => x.textContent)).catch(() => []);
check(choices.join() === "Bass,Hats,Keys,Kick,Vox,No other track", `Voice lists the song's other tracks, then No other track: ${choices.join()}`);
await linted(`${card} [data-listens], [data-testid=listens-menu]`, "the Voice row and its menu");
const before = await revision();
await page.click('[data-testid=listens-menu] [data-track="vox"]').catch(() => {});
const voiced = alone.replace(added, "vocoder(osc.saw(110), vox.out, { clarity: 0.5, smooth: 0.05, formant: 0 })").replace('import { vocoder } from "@m2/vocals";', 'import { vocoder } from "@m2/vocals";\nimport vox from "./vox";');
check(await until(() => lead() === voiced), `picking Vox writes vox.out in the call and its import, and nothing else: ${lead()}`);
check(await until(async () => (await row()) === "Voice:Vox"), `the row says Vox: ${await row()}`);
check(await until(async () => (await undoTitle()) === "Undo Lead voice Vox (⌘Z)"), `one undo step, named by the tracks: ${await undoTitle()}`);
check((await revision()) === before + 1, "one revision");

// ---------------------------------------------------------------- its picture moves with the song

// What an agent's look at the Vocoder, in and out, says: TAP-13's bands, a twelfth of an octave each
// from 20 Hz. The saw's note, 110 Hz, is band 29 or 30; the voice's first formant, 700 Hz, about 62.
const node = await nodeOf("lead", "vocoder");
const g = (await api("")).body;
const look = async (at) => (await post("/look", { session: g.session, client: "agent:vocoder", module: "lead", node, kind: "spectrum", at, on: true })).body?.look;
const loudest = (bins) => bins.reduce((k, b, i) => (b > bins[k] ? i : k), 0);
/** Both looks' bands, when both have them. */
const both = async () => {
  const [inn, out] = [await look("in"), await look("out")];
  return inn?.bins?.length && out?.bins?.length ? { inn: inn.bins, out: out.bins } : null;
};
/** How much louder, against what comes in, what goes out is at its most from band `lo` to `hi`. */
const gain = (l, lo, hi) => Math.max(...l.out.slice(lo, hi + 1).map((b, i) => b - l.inn[lo + i]));
const told = (l) => (l ? `in loudest at band ${loudest(l.inn)} (${l.inn[loudest(l.inn)]} dB), out loudest at band ${loudest(l.out)} (${l.out[loudest(l.out)]} dB)` : "no look");

// While the voice sings, the saw goes out in the voice's shape: about its first formant (bands 58 to
// 74, 560 Hz to 1.4 kHz) far louder against what comes in than at the saw's own note (bands 28 to
// 31). While it is silent, every band is shut: nothing goes out while the saw comes in.
const sings = (l) => Math.max(...l.out) > -60 && gain(l, 58, 74) - gain(l, 28, 31) > 10;
const silent = (l) => Math.max(...l.out) < -80 && [29, 30].includes(loudest(l.inn));
await post("/loop", { start: 0, end: 6, on: true });
await seen(pic);
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
// Each state counts once two looks in a row agree on it (the first may be stale: voice2's review V1).
const runs = { sings: 0, silent: 0 };
const last = { sings: null, silent: null };
for (let i = 0; i < 80 && (runs.sings < 2 || runs.silent < 2); i++) {
  await sleep(150);
  const l = await both();
  for (const [name, ok] of [
    ["sings", sings],
    ["silent", silent],
  ]) {
    if (runs[name] >= 2) continue;
    runs[name] = l && ok(l) ? runs[name] + 1 : 0;
    if (l && ok(l)) last[name] = l;
  }
}
check(
  runs.sings >= 2,
  `while the voice sings, the saw goes out in its shape: ${last.sings ? `${(gain(last.sings, 58, 74) - gain(last.sings, 28, 31)).toFixed(1)} dB louder about the formant than at the saw's note; ` : ""}${told(last.sings)}`,
);
check(runs.silent >= 2, `while the voice is silent, nothing goes out though the saw comes in: ${told(last.silent)}`);
// The window's own picture draws both layers as they come: two reads in a row.
let layers = [];
let drawn = 0;
for (let i = 0; i < 40 && drawn < 2; i++) {
  await sleep(150);
  layers = await said(pic);
  drawn = layers.length === 2 && layers.every((l) => l && typeof l.v === "number") ? drawn + 1 : 0;
}
check(drawn >= 2, `its picture draws the synth in and out as they come: ${JSON.stringify(layers)}`);
await post("/look", { session: g.session, client: "agent:vocoder", module: "lead", node, kind: "spectrum", at: "in", on: false });
await post("/look", { session: g.session, client: "agent:vocoder", module: "lead", node, kind: "spectrum", at: "out", on: false });

// ---------------------------------------------------------------- its card, live

await sleep(1500);
await shoot(card, "patch");
await page.click("[data-device=chain]").catch(() => {});
const chained = `[data-testid=chain] [data-module="lead"] [data-node="vocoder"]`;
check(await until(async () => !!(await page.$(chained)), 15000), "a card on Device's chain");
for (const size of ["normal", "expanded"]) {
  await page.click(`${chained} [data-size-to="${size}"]`).catch(() => {});
  await sleep(1500);
  await shoot(chained, size);
}
await linted(chained, "its chain card, Expanded");
await page.click(`${chained} [data-size-to="normal"]`).catch(() => {});
await page.keyboard.press("Space");

// ---------------------------------------------------------------- ⌘Z

await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => lead() === alone), "⌘Z takes the voice out, byte for byte");

done(errors);
