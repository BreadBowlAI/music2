// Track dress1: std's own cards wear their pictures (CARD-90 to CARD-99), on the Starter beat. Its
// hook is played through std's eq3 and lowpass: Device's chain draws the lowpass's curve and eq3's,
// each measured from the sound itself, with their dots. Dragging eq3's middle dot up writes its mid
// gain, one undo step, which ⌘Z takes back. The pluck's envelope, std's picture now (CARD-96), drags
// by its attack corner, and ⌘Z takes it back. The mix's compressor draws its transfer curve and how
// far it turns down; while the song plays, the level riding its curve and its meter move. Each card
// linted; the chains are shot to compare with the design's mockup.
//
//   node e2e/dress.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

// ---------------------------------------------------------------- the Starter beat, in place of first-song

const STARTER = path.join(import.meta.dirname, "../templates/starter");
// first-song's own files the Starter beat hasn't (its tracks, their lanes' data) go after the copy.
const under = (dir, at = "") => fs.readdirSync(path.join(dir, at), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? under(dir, path.join(at, e.name)) : [path.join(at, e.name)]));
const before = ["tracks", "data"].flatMap((d) => (fs.existsSync(path.join(PROJ, d)) ? under(PROJ, d) : []));
fs.cpSync(STARTER, PROJ, { recursive: true, force: true });
fs.rmSync(path.join(PROJ, "template.toml"), { force: true });
for (const f of before) if (!fs.existsSync(path.join(STARTER, f))) fs.rmSync(path.join(PROJ, f));
// The hook through std's eq3 and lowpass, its pluck the one whose envelope is drawn.
write(
  "tracks/hook.ts",
  `import { hz, track } from "@m2/core";
import { synth } from "@m2/synths";
import { delay, reverb, widen } from "@m2/effects";
import { lowpass } from "std/filter";
import { eq3 } from "std/fx";

// The tune: a bright pluck, shaped by an EQ and a filter, with echoes three 16ths apart.
export default track("hook", { name: "Hook" }, ({ lane }) => {
  const hook = lowpass(eq3(synth.pluck(lane.notes("notes"), { brightness: hz(4200) }), { mid: 3 }), { cutoff: hz(6000), q: 1.2 });
  const echoes = delay(hook, { time: 0.388, feedback: 0.3, mix: 0.25 });
  return reverb(widen(echoes, { width: 1.3 }), { size: 0.6, mix: 0.2 });
});
`,
);

// A track whose LFO, of 3 beats, sets how loud its saw is: its card is the patch's.
write(
  "tracks/wobble.ts",
  `import { time, track } from "@m2/core";
import { lfo } from "std/control";
import { osc, vca } from "std/dsp";

export default track("wobble", { name: "Wobble" }, () => vca(osc.saw(110), lfo(time.beats, { period: 3, shape: "saw" })));
`,
);
write(
  "song.ts",
  read("song.ts").replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport wobble from "./tracks/wobble";').replace("hook: { in: hook.out, db: db(4) },", "hook: { in: hook.out, db: db(4) },\n        wobble: { in: wobble.out, db: db(-24) },"),
);

const { page, errors, api, view, box, key } = await open(1920, 1080, { device: "chain" });
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const shown = (sel) => page.$eval(sel, (e) => e.scrollIntoView({ block: "nearest", inline: "center" })).catch(() => {});
/** How many points a picture's line or response has. */
const points = (sel) => page.$eval(sel, (e) => (e.getAttribute("points") ?? "").split(" ").filter(Boolean).length).catch(() => 0);

check(await until(async () => !!(await page.$('[data-module-tab="hook"]')) && !(await page.$('[data-module-tab="keys"]')), 30000), "the Starter beat is open, its hook a track");
await page.click('[data-module-tab="hook"] [data-testid=track-label]');
await view("graph");
const hookChain = '[data-testid=chain] [data-module="hook"]';
check(await until(async () => !!(await page.$(`${hookChain} [data-node="lowpass"]`)), 20000), "the hook's chain has the lowpass and eq3 cards");

// ---------------------------------------------------------------- the lowpass's curve (CARD-91)

const lp = `${hookChain} [data-node="lowpass"]`;
await shown(lp);
const lpPic = `${lp} [data-picture="curve"]`;
check(await until(async () => (await points(`${lpPic} polyline[data-response]`)) === 256, 20000), `the lowpass's face draws its curve, measured from its own sound (256 points): ${await points(`${lpPic} polyline[data-response]`)}`);
check((await page.$$(`${lpPic} [data-dot]`)).length === 1, "one dot: its cutoff and its q");
const lpWords = await page.$eval(`${lpPic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
check(/^Cuts the highs above 6 kHz, q 1\.2/.test(lpWords ?? ""), `its words: ${lpWords}`);
await linted(lp, "the lowpass's card");

// ---------------------------------------------------------------- eq3's dots (CARD-92)

const eq = `${hookChain} [data-node="eq3"]`;
await shown(eq);
const eqPic = `${eq} [data-picture="curve"]`;
check(await until(async () => (await points(`${eqPic} polyline[data-response]`)) === 256, 20000), "eq3's face draws its curve, measured");
const marks = await page.$$eval(`${eqPic} [data-dot]`, (els) => els.map((e) => e.parentElement?.textContent?.trim() ?? ""));
check(marks.length === 3, `a dot for each band, numbered: ${JSON.stringify(marks)}`);
await linted(eq, "eq3's card");
await page.screenshot({ path: `${ROOT}/dress-hook.png` });
const hookCode = read("tracks/hook.ts");
const mid = await box(`${eqPic} [data-dot="1"]`);
const [mx, my] = [mid.x + mid.width / 2, mid.y + mid.height / 2];
const drawn = () => page.$eval(eqPic, (e) => Number(e.dataset.drawn ?? 0));
const drawn0 = await drawn();
const curve0 = await page.$eval(`${eqPic} polyline[data-response]`, (e) => e.getAttribute("points"));
await page.mouse.move(mx, my);
await page.mouse.down();
for (let i = 1; i <= 8; i++) {
  await page.mouse.move(mx, my - i * 3);
  await sleep(30);
}
check(
  await until(async () => (await drawn()) > drawn0 && (await page.$eval(`${eqPic} polyline[data-response]`, (e) => e.getAttribute("points"))) !== curve0, 8000),
  "while the dot is held, the measured curve follows it",
);
await page.mouse.up();
check(await until(() => read("tracks/hook.ts") !== hookCode, 8000), "the release writes the code");
const midDb = Number(/mid: ([-0-9.]+)/.exec(read("tracks/hook.ts"))?.[1]);
check(midDb > 3, `dragging the middle dot up raised the mid gain from where it was: ${midDb} dB`);
check(await until(async () => /^Undo EQ3 mid/.test((await undoTitle()) ?? "")), `one undo step, named by what moved: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/hook.ts") === hookCode, 8000), "⌘Z takes it back, byte for byte");

// ---------------------------------------------------------------- the pluck's envelope (CARD-96)

const pluck = `${hookChain} [data-node="synth.pluck"]`;
await shown(pluck);
if (await page.$(`${pluck} [data-tab="Loudness"]`)) await page.click(`${pluck} [data-tab="Loudness"]`);
const amp = `${pluck} [data-picture="amp"]`;
check(await until(async () => (await page.$$(`${amp} [data-dot]`)).length === 3, 20000), "the pluck's envelope is std's picture: its three corners");
check(await until(async () => (await points(`${amp} polyline[data-line="envelope"]`)) > 100, 8000), "its shape is drawn from std's sums");
await linted(pluck, "the pluck's card");
const attackBefore = Number(/attack: ([0-9.]+)/.exec(read("tracks/hook.ts"))?.[1] ?? 0);
const corner = await box(`${amp} [data-dot="0"]`);
const [cx, cy] = [corner.x + corner.width / 2, corner.y + corner.height / 2];
const span0 = await page.$eval(`${amp} svg`, (e) => e.dataset.xSpan);
/** The envelope's peak, in px across the page: where its line is highest. */
const peakX = () =>
  page.$eval(`${amp} svg`, (svg) => {
    const pts = (svg.querySelector('polyline[data-line="envelope"]')?.getAttribute("points") ?? "").split(" ").map((p) => p.split(",").map(Number));
    const top = pts.reduce((b, p) => (p[1] < b[1] ? p : b), [0, Infinity]);
    const r = svg.getBoundingClientRect();
    return r.left + (top[0] / Number(svg.dataset.size.split("x")[0])) * r.width;
  });
const ampDrawn = () => page.$eval(amp, (e) => Number(e.dataset.drawn ?? 0));
const ampDrawn0 = await ampDrawn();
await page.mouse.move(cx, cy);
await page.mouse.down();
for (let i = 1; i <= 10; i++) {
  await page.mouse.move(cx + i * 4, cy);
  await sleep(30);
}
check(await until(async () => (await ampDrawn()) > ampDrawn0 && Math.abs((await peakX()) - (cx + 40)) <= 4, 8000), `while held, the curve's peak stays under the pointer (CARD-102): ${Math.round(await peakX())} px against ${Math.round(cx + 40)}`);
check((await page.$eval(`${amp} svg`, (e) => e.dataset.xSpan)) === span0, `and the picture's span is the one at the press: ${span0}`);
await page.mouse.up();
check(await until(() => /attack: [0-9.]+/.test(read("tracks/hook.ts")), 8000), `dragging the attack corner right writes the attack: ${/attack: [0-9.]+/.exec(read("tracks/hook.ts"))?.[0]}`);
const attack = Number(/attack: ([0-9.]+)/.exec(read("tracks/hook.ts"))?.[1]);
check(attack > attackBefore, `a longer attack: ${attackBefore || "its default"} → ${attack}`);
check(await until(async () => /^Undo Synth pluck attack/.test((await undoTitle()) ?? "")), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/hook.ts") === hookCode, 8000), "⌘Z takes the corner back");
// The decay's corner, dragged up: a higher sustain.
await until(async () => !!(await page.$(`${amp} [data-dot="1"]`)), 8000);
await shown(`${amp} [data-dot="1"]`);
await sleep(200);
const dk = await box(`${amp} [data-dot="1"]`);
await page.mouse.move(dk.x + dk.width / 2, dk.y + dk.height / 2);
await page.mouse.down();
for (let i = 1; i <= 8; i++) {
  await page.mouse.move(dk.x + dk.width / 2, dk.y + dk.height / 2 - i * 2);
  await sleep(30);
}
await page.mouse.up();
check(await until(() => /sustain: [0-9.]+/.test(read("tracks/hook.ts")), 8000), `dragging the decay's corner up writes the sustain: ${/sustain: [0-9.]+/.exec(read("tracks/hook.ts"))?.[0]}`);
await page.mouse.click(10, 10);
// Only a write of its own is taken back: an ⌘Z with none would take back the Starter beat's files.
if (read("tracks/hook.ts") !== hookCode) await key("Meta+z");
check(await until(() => read("tracks/hook.ts") === hookCode, 8000), "⌘Z takes it back");

// ---------------------------------------------------------------- the mix's compressor (CARD-93)

await page.click('[data-module-tab="song"]');
const comp = '[data-testid=chain] [data-module="song"] [data-node="compress"]';
check(await until(async () => !!(await page.$(comp)), 20000), "the mix's chain has its compressor");
await shown(comp);
const transfer = `${comp} [data-picture="transfer"]`;
const history = `${comp} [data-picture="history"]`;
check(await until(async () => (await points(`${transfer} polyline[data-line="transfer"]`)) === 121, 20000), "its face draws its transfer curve, from its own sum");
check((await page.$$(`${transfer} [data-dot]`)).length === 2, "two dots: the threshold's corner and the ratio's, on the line");
const tWords = await page.$eval(`${transfer} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
check(tWords === "Above −12 dB the level rises only 1 dB for each 2, then up 3 dB. Drag the corner for the threshold, the line for the ratio", `its words: ${tWords}`);
check(!!(await page.$(history)), "and its last seconds, beside it");
await linted(comp, "the compressor's card");

// The ratio's dot, dragged across, writes the ratio; its wheel steps it.
const songCode = read("song.ts");
const rd = await box(`${transfer} [data-dot="1"]`);
await page.mouse.move(rd.x + rd.width / 2, rd.y + rd.height / 2);
await page.mouse.down();
for (let i = 1; i <= 8; i++) {
  await page.mouse.move(rd.x + rd.width / 2 + i * 4, rd.y + rd.height / 2);
  await sleep(30);
}
await page.mouse.up();
const ratio = () => Number(/ratio: ([0-9.]+)/.exec(read("song.ts"))?.[1]);
check(await until(() => ratio() > 2, 8000), `dragging the line's dot right raises the ratio: ${ratio()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("song.ts") === songCode, 8000), "⌘Z takes it back");
const rd2 = await box(`${transfer} [data-dot="1"]`);
await page.mouse.move(rd2.x + rd2.width / 2, rd2.y + rd2.height / 2);
await page.mouse.wheel({ deltaY: -130 });
check(await until(() => ratio() > 2, 8000), `its wheel steps the ratio up: ${ratio()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("song.ts") === songCode, 8000), "⌘Z takes that back too");

// While the song plays: the level dot rides the curve, the meter turns down, and the history's
// layers sit where its axis puts their values (CARD-103).
const said = (sel) => page.$eval(`${sel} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")).catch(() => []);
const KNEE = 6;
const out = (l) => {
  const over = l + 12, slope = 1 - 1 / 2;
  const down = over <= -KNEE / 2 ? 0 : over < KNEE / 2 ? (slope * (over + KNEE / 2) ** 2) / (2 * KNEE) : slope * over;
  return l - down + 3;
};
const clamp = (v) => Math.min(1, Math.max(0, v));
await post("/loop", { start: 0, end: 8, on: true });
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
let rides = false, turned = 0, placed = true, seenHistory = false;
for (let i = 0; i < 60 && !(rides && turned > 0 && seenHistory); i++) {
  await sleep(150);
  const [dot] = await said(transfer);
  if (dot && dot.v > -60) rides = Math.abs(dot.y - clamp(1 - (out(dot.v) + 60) / 72)) < 0.01 && Math.abs(dot.x - clamp((dot.v + 60) / 60)) < 0.01;
  const h = await said(history);
  for (const layer of h.slice(0, 3)) {
    if (!layer) continue;
    seenHistory = true;
    if (Math.abs(layer.y - clamp(1 - (layer.v + 60) / 84)) > 0.005) placed = false;
  }
  if (h[3]) turned = Math.max(turned, h[3].v);
}
check(rides, "while the song plays, the level going in rides the curve: its dot is on the line at its level");
check(turned > 0, `and the compressor turns it down, by its own meter: up to ${turned.toFixed(2)} dB`);
check(seenHistory && placed, "the history's layers (in, out, how far) sit where its dB axis puts their values");
await sleep(500);
await page.screenshot({ path: `${ROOT}/dress-mix.png` });

// ---------------------------------------------------------------- the LFO's playhead (CARD-98, CARD-104)

await page.click('[data-module-tab="wobble"] [data-testid=track-label]');
if (await page.$('[data-device="patch"]')) await page.click('[data-device="patch"]');
const lfoCard = '[data-testid=graph] [data-module="wobble"] [data-node="lfo"]';
check(await until(async () => !!(await page.$(lfoCard)), 15000), "the Wobble's LFO has its card in the patch");
await shown(`${lfoCard} [data-picture="wave"]`);
const wave = `${lfoCard} [data-picture="wave"]`;
check(await until(async () => (await page.$eval(`${wave} svg`, (e) => e.dataset.xSpan).catch(() => "")) === "0,6", 15000), `an LFO of 3 beats is drawn over two whole cycles, 6 beats: ${await page.$eval(`${wave} svg`, (e) => e.dataset.xSpan).catch(() => "")}`);
let round = 0, ok = true;
for (let i = 0; i < 30 && round < 5; i++) {
  await sleep(120);
  const [ph] = await said(wave);
  if (!ph || ph.v <= 0) continue;
  round += 1;
  if (Math.abs(ph.x - (ph.v % 6) / 6) > 0.01) ok = false;
}
check(round >= 5 && ok, `its playhead is the song's beat gone round its 6 beats, where the LFO is (${round} reads)`);
await page.screenshot({ path: `${ROOT}/dress-lfo.png` });
await page.keyboard.press("Space");

done(errors);
