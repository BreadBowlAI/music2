// The Voice changer (VOX-28) and the Clipper (PACK-34, CARD-127), on first-song with a vocal track
// (an audio lane playing a sung vowel at 216 Hz) and a Loud track (a saw). The + on the vocal's
// lane lists the Voice changer under Voice, beside Tune, and picking it writes
// `voiceChanger(lane.audio("clips"), { pitch: 0, formant: 0 })` in the track's own code with the
// pack's line and lock; Pitch and Formant are bars, each drag one undo step; Presets lists Deep and
// Chipmunk, and Deep writes both down; its face draws the voice's spectrum, and while the song plays
// an agent's look at it finds the notes moved down by Deep: the loudest band coming out about 5 under
// the one going in. The + after the saw lists the Clipper under Dynamics; picking it writes
// `clip(…, { gain: 6, ceiling: -0.3 })`; its face draws its ceiling over its levels, its dot dragged
// down writes a lower ceiling, one undo step; while the saw plays, the faint line (the sound pushed
// up, its kernel's meter) stands over the ceiling and the solid one (what comes out) at it. Then the
// three new things' nodes are shot live, while the song plays, in the graph collapsed and expanded
// (to $E2E_SHOTS when set, else the run's folder): Tune's with Formant, the
// Voice changer's, and the Clipper's. Each state linted.
//
//   node e2e/voice.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { expand } from "./graphlib.mjs";

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

// ---------------------------------------------------------------- the song: Vox and Loud

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox.wav`);
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write("data/vox/clips.json", JSON.stringify({ clips: [{ id: "voxclip1", start: 0, length: 4, asset: "vox.wav", offset: 0 }] }, null, 2));
write("tracks/loud.ts", 'import { track } from "@m2/core";\nimport { osc } from "std/dsp";\n\nexport default track("loud", { name: "Loud" }, () => osc.saw(110));\n');
write(
  "song.ts",
  read("song.ts")
    .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";\nimport loud from "./tracks/loud";')
    .replace("      kick: kick.out,", "      kick: kick.out,\n      vox: vox.out,\n      loud: { in: loud.out, db: db(-12) },"),
);

const { page, errors, show, view, key, box, api } = await open(1600, 1000);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);
/** `sel` scrolled into the middle of the window, and where it is then. */
const seen = async (sel) => {
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
  await sleep(150);
  return box(sel);
};
/** Drag the bar `sel` up by `px`. */
const turn = async (sel, px) => {
  const g = await seen(`${sel} [role=slider]`);
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
  await page.mouse.down();
  for (let i = 1; i <= 10; i++) await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2 - (px * i) / 10);
  await page.mouse.up();
};
/** A picture's words, its group's name. */
const words = (pic) => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
/** What each live layer of a picture drew last (CARD-103). */
const said = (pic) => page.$eval(`${pic} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")).catch(() => []);
/** A card's shot. */
const shoot = async (card, name) => {
  const b = await seen(card);
  if (!b) return check(false, `${name}: no card to shoot`);
  await page.screenshot({ path: `${SHOTS}/voice-${name}.png`, clip: { x: Math.max(0, b.x - 4), y: Math.max(0, b.y - 4), width: Math.min(b.width + 8, 1600), height: Math.min(b.height + 8, 1000) } });
};
/** The view's id of the call labelled `label` in `module`. */
const nodeOf = async (module, label) => {
  const g = (await api("")).body;
  return g.view?.modules?.find((m) => m.key === module)?.nodes?.find((n) => n.label === label)?.id;
};
const vox = () => read("tracks/vox.ts");
const loud = () => read("tracks/loud.ts");

check(!!(await until(() => page.$('[data-module-tab="vox"]'), 20000)) && !!(await page.$('[data-module-tab="loud"]')), "the Vox and Loud tracks are there");

// ---------------------------------------------------------------- + ▸ Voice changer (VOX-28)

await show("vox");
await view("graph");
const plus = '[data-module="vox"] button[aria-label="Add an effect after clips"]';
check(!!(await until(() => page.$(plus), 15000)), "the Vox track's lane has a +");
await page.click(plus);
await page.waitForSelector('[data-effect-browser] [data-effect="voiceChanger"]', { timeout: 5000 }).catch(() => {});
const groupOf = (name) => page.$eval(`[data-effect-browser] [data-effect="${name}"]`, (e) => e.closest("[data-category]")?.dataset.category).catch(() => null);
check((await groupOf("voiceChanger")) === "Voice" && (await groupOf("tune")) === "Voice", `the Voice changer is listed under Voice, beside Tune: ${await groupOf("voiceChanger")}`);
await linted("[data-effect-browser]", "the effect browser");
await page.click('[data-effect-browser] [data-effect="voiceChanger"]');
const added = 'voiceChanger(lane.audio("clips"), { pitch: 0, formant: 0 })';
check(await until(() => vox().includes(added), 15000), `it goes around the vocal, in its track, at 0 and 0: ${vox().match(/=> .*/)?.[0]}`);
check(vox().includes('import { voiceChanger } from "@m2/vocals";') && /"@m2\/vocals" = /.test(read("music2.toml")) && read("music2.lock").includes('name = "@m2/vocals"'), "imported, and the pack listed and locked");

const changer = '[data-module="vox"] [data-node="voiceChanger"]';
const changerPic = `${changer} [data-picture="voice"]`;
check(!!(await until(() => page.$(`${changerPic} svg`), 15000)), "its face draws its spectrum picture");
check(
  (await words(changerPic)) ===
    "As sung: Pitch moves the notes, Formant the colour (both down for a deep voice, both up for a chipmunk). Faint: your voice as it comes in; solid: as it goes out. The spikes are its notes, the hills its colour",
  `its words at 0 and 0: ${await words(changerPic)}`,
);

const before = vox();
const pitch = `${changer} [data-knob="pitch"]`;
check(!!(await until(() => page.$(`${pitch} [role=slider]`), 5000)), "Pitch is a bar");
await until(async () => (await page.$eval(pitch, (e) => e.getAttribute("aria-disabled")).catch(() => "true")) !== "true");
await turn(pitch, 30);
check(await until(() => /\{ pitch: [1-9][0-9.]*, formant: 0 \}/.test(vox())), `dragging Pitch up moves the notes up: ${vox().match(/=> .*/)?.[0]}`);
check(await stepIs("Voice changer pitch"), `one undo step: ${await undoTitle()}`);
const formant = `${changer} [data-knob="formant"]`;
await until(async () => (await page.$eval(formant, (e) => e.getAttribute("aria-disabled")).catch(() => "true")) !== "true");
await turn(formant, -30);
check(await until(() => /formant: -[1-9][0-9.]* \}/.test(vox())), `dragging Formant down makes the colour darker: ${vox().match(/=> .*/)?.[0]}`);
check(await stepIs("Voice changer formant"), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
await key("Meta+z");
check(await until(() => vox() === before), "two ⌘Z take them back, byte for byte");

// Presets is on the expanded node's header.
check(await expand(page, `[data-testid=graph] ${changer}`), "the Voice changer's node expanded");
await seen(`${changer} [data-presets]`);
await page.click(`${changer} [data-presets]`);
check(!!(await until(() => page.$("[data-preset-group=std] [data-preset='Deep']"), 5000)) && !!(await page.$("[data-preset-group=std] [data-preset='Chipmunk']")), "Presets lists Deep and Chipmunk, music2's own");
await page.click("[data-preset-group=std] [data-preset='Deep']");
check(await until(() => vox().includes('voiceChanger(lane.audio("clips"), { pitch: -5, formant: -5 })')), `loading Deep moves both down: ${vox().match(/=> .*/)?.[0]}`);
check(await until(async () => /Undo .*Deep/.test((await undoTitle()) ?? "")), `one undo step: ${await undoTitle()}`);
check(
  await until(async () => ((await words(changerPic)) ?? "").startsWith("Down 5 semitones, colour 5 semitones darker. ")),
  `its words say what the knobs did: ${await words(changerPic)}`,
);
await linted(changer, "the Voice changer's card");

// While the voice plays, an agent's look at the Voice changer's spectrum, in and out: Deep moves the
// notes 5 semitones down, so the loudest band (a twelfth of an octave each) comes out about 5 lower.
await post("/loop", { start: 0, end: 3, on: true });
await seen(changerPic);
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
const node = await nodeOf("vox", "voiceChanger");
const g = (await api("")).body;
const look = async (at) => (await post("/look", { session: g.session, client: "agent:voice", module: "vox", node, kind: "spectrum", at, on: true })).body?.look;
/** The band a spectrum (TAP-13: a twelfth of an octave each, from 20 Hz) is loudest in. */
const loudest = (l) => {
  const bins = l?.bins ?? [];
  let k = -1;
  for (let i = 0; i < bins.length; i++) if (k < 0 || bins[i] > bins[k]) k = i;
  return k < 0 || bins[k] < -80 ? -1 : k;
};
// The first look may still hold what played before Deep (review V1): a move two looks in a row agree on.
let moved = null;
let answered = null;
let last = null;
for (let i = 0; i < 40 && moved === null; i++) {
  await sleep(150);
  const [inn, out] = [await look("in"), await look("out")];
  answered = { in: Object.keys(inn ?? {}), node };
  const [a, b] = [loudest(inn), loudest(out)];
  if (a > 0 && b > 0) {
    if (last === a - b) moved = last;
    last = a - b;
  }
}
check(moved !== null && moved >= 4 && moved <= 6, `while the voice plays through Deep, its loudest band comes out ${moved} bands (semitones) under the one going in${moved === null ? `: ${JSON.stringify(answered)}` : ""}`);
const layers = await said(changerPic);
check(layers.length === 2 && layers.every((l) => l && typeof l.v === "number"), `its picture draws the spectrum in and out as they come: ${JSON.stringify(layers)}`);
await post("/look", { session: g.session, client: "agent:voice", module: "vox", node, kind: "spectrum", at: "in", on: false });
await post("/look", { session: g.session, client: "agent:voice", module: "vox", node, kind: "spectrum", at: "out", on: false });
await page.keyboard.press("Space");

// ---------------------------------------------------------------- + ▸ Clipper (PACK-34, CARD-127)

await show("loud");
await view("graph");
const after = '[data-module="loud"] button[aria-label^="Add an effect after"]';
check(!!(await until(() => page.$(after), 15000)), "the saw has a +");
await page.click(after);
await page.waitForSelector('[data-effect-browser] [data-effect="clip"]', { timeout: 5000 }).catch(() => {});
check((await groupOf("clip")) === "Dynamics", `the Clipper is listed under Dynamics, beside Limit: ${await groupOf("clip")}`);
await page.click('[data-effect-browser] [data-effect="clip"]');
check(await until(() => loud().includes("clip(osc.saw(110), { gain: 6, ceiling: -0.3 })"), 15000), `it goes after the saw, pushing 6 dB into -0.3 dB: ${loud().match(/=> .*/)?.[0]}`);
check(loud().includes('import { clip } from "@m2/effects";') && read("music2.lock").includes('name = "@m2/effects"'), "imported, and the pack listed and locked");
const clipper = '[data-module="loud"] [data-node="clip"]';
const level = `${clipper} [data-picture="level"]`;
check(!!(await until(() => page.$(`${level} svg`), 15000)), "its face draws its ceiling over its levels");
check(
  (await words(level)) ===
    "Pushes your sound up 6 dB and cuts every peak flat at −0.3 dB. It works at 4 times the song's rate, so it adds far fewer harsh tones than a plain clipper; on very bright sounds pushed hard, a few. Faint: your sound pushed up; solid: what comes out. Drag the line to move the ceiling",
  `its words: ${await words(level)}`,
);
const kept = loud();
const dot = `${level} [data-dot="0"]`;
const d = await seen(dot);
if (d) {
  await page.mouse.move(d.x + d.width / 2, d.y + d.height / 2);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(d.x + d.width / 2, d.y + d.height / 2 + (i * 12) / 8);
    await sleep(30);
  }
  await page.mouse.up();
}
const ceilingOf = () => Number(/ceiling: (-?[0-9.]+)/.exec(loud())?.[1]);
check(await until(() => ceilingOf() < -0.3, 8000), `its ceiling's dot dragged down writes a lower ceiling: ${loud().match(/=> .*/)?.[0]}`);
check(await until(async () => ((await undoTitle()) ?? "").startsWith("Undo Clipper ceiling")), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => loud() === kept), "⌘Z takes it back, byte for byte");

await seen(level);
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
const over = ([pushed, out]) => pushed && out && pushed.v > -0.3 + 3 && out.v <= -0.3 + 0.05 && out.v > -0.3 - 1;
let lines = [];
for (let i = 0; i < 60 && !over(lines); i++) {
  await sleep(100);
  lines = await said(level);
}
check(over(lines), `while the saw plays, the sound pushed into the cut stands over the ceiling and what comes out at it: ${JSON.stringify(lines.map((l) => l && l.v))}`);
await linted(clipper, "the Clipper's card");

// ---------------------------------------------------------------- the three nodes, live

// Tune after the Voice changer, its Formant up 3, so its node shows Formant beside Glide and Amount.
write("tracks/vox.ts", vox().replace(/=> voiceChanger\((.*)\)\);/, '=> tune(voiceChanger($1), { glide: 0.1, amount: 1, formant: 3 }));').replace('import { voiceChanger } from "@m2/vocals";', 'import { tune, voiceChanger } from "@m2/vocals";'));
await show("vox");
await view("graph");
check(await until(async () => !!(await page.$('[data-module="vox"] [data-node="tune"] [data-knob="formant"]')), 20000), "Tune's card has Formant beside Glide and Amount");
const shots = [
  ["vox", "tune", "tune"],
  ["vox", "voiceChanger", "voice-changer"],
  ["loud", "clip", "clipper"],
];
// In the graph, while the song plays: each node collapsed, then expanded.
await page.click("[data-testid=collapse-all]").catch(() => {});
for (const [module, node, name] of shots) {
  await show(module);
  await view("graph");
  const at = `[data-testid=graph] [data-module="${module}"] [data-node="${node}"]`;
  await until(async () => (await page.$eval(at, (e) => e.dataset.size).catch(() => null)) === "collapsed", 15000);
  await sleep(1500);
  await shoot(at, `${name}-collapsed`);
  await expand(page, at);
  await sleep(1500);
  await shoot(at, `${name}-expanded`);
  await linted(at, `${name}'s node, expanded`);
}
await page.keyboard.press("Space");

done(errors);
