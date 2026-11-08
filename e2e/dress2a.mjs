// Track dress2a: @m2/mixing's and @m2/effects' cards wear their pictures (CARD-110 to CARD-126).
// Two tracks play a saw that hits each half beat: Mix A through the Parametric EQ, the gate, the
// transient shaper, the de-esser, the multiband and Pump; FX A through the delay, the reverb, the
// chorus, the phaser, the tremolo, drive, the bitcrusher and the widener. Device's chain draws each
// card's picture on its face; while the song plays, the live ones move (the spectra, the levels over
// time, Pump's playhead), and the meters the gate, the de-esser and the multiband send (track
// meters2, CARD-126): the gate's strip lights on the hits and goes dark between them, and its and
// the de-esser's `down` say how far they turn down; the multiband's three bars stand side by side,
// low to high, and its low band's turns down. Mix A's hits stop for a quarter beat between them, so
// its gate closes. Each draggable picture's dot is dragged: it writes its value, one undo step, which
// ⌘Z takes back byte for byte. Each card is linted and shot, to compare with the design's mockup (to
// $E2E_SHOTS when set, else the run's folder).
//
//   node e2e/dress2a.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

// ---------------------------------------------------------------- the song: two tracks of the packs' effects

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
const HITS = `const hits = vca(osc.saw(110), range(lfo(time.beats, { period: 0.5, shape: "saw" }), 1, 0));`;
// Mix A's: a quarter beat of saw, a quarter of silence.
const GAPS = `const hits = vca(osc.saw(110), range(lfo(time.beats, { period: 0.5, shape: "square" }), 1, 0));`;
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

const SHOTS = process.env.E2E_SHOTS ?? ROOT;
fs.mkdirSync(SHOTS, { recursive: true });
const { page, errors, api, view, box, key } = await open(1920, 1080, { device: "chain" });
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const shown = async (sel) => {
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "nearest", inline: "center" })).catch(() => {});
  await sleep(150);
};
/** How many points a picture's line has. */
const points = (sel) => page.$eval(sel, (e) => (e.getAttribute("points") ?? "").split(" ").filter(Boolean).length).catch(() => 0);
/** A picture's words, its group's name. */
const words = (pic) => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
/** What each live layer of a picture drew last (CARD-103). */
const said = (pic) => page.$eval(`${pic} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")).catch(() => []);
/** A card's shot, to compare with the mockup. */
const shoot = async (card, name) => {
  await shown(card);
  const b = await box(card);
  await page.screenshot({ path: `${SHOTS}/dress2a-${name}.png`, clip: { x: Math.max(0, b.x), y: Math.max(0, b.y), width: Math.min(b.width, 1920), height: Math.min(b.height, 1080) } });
};

/**
 * Drags dot `dot` of picture `pic` by (`dx`, `dy`) px in steps, then lets go: the file `file` is
 * written by the release with what `wrote` finds, one undo step named for `step`, and ⌘Z takes it
 * back byte for byte.
 */
const drag = async (pic, dot, dx, dy, file, wrote, step, what) => {
  const before = read(file);
  const sel = `${pic} [data-dot="${dot}"]`;
  await shown(sel);
  const d = await box(sel);
  const [x, y] = [d.x + d.width / 2, d.y + d.height / 2];
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(x + (i * dx) / 8, y + (i * dy) / 8);
    await sleep(30);
  }
  await page.mouse.up();
  check(await until(() => read(file) !== before && wrote(read(file)), 8000), `${what}: ${read(file).match(/=> \{?[^\n]*\n?.*return.*|=> .*/)?.[0]?.slice(0, 160) ?? ""}`);
  check(await until(async () => ((await undoTitle()) ?? "").startsWith(`Undo ${step}`)), `one undo step, ${step}: ${await undoTitle()}`);
  await page.mouse.click(10, 10);
  await key("Meta+z");
  check(await until(() => read(file) === before, 8000), "⌘Z takes it back, byte for byte");
};
/** A number written in `code` after `name: `. */
const num = (code, name) => Number(new RegExp(`${name}: (-?[0-9.]+)`).exec(code)?.[1]);

check(await until(async () => !!(await page.$('[data-module-tab="mixa"]')) && !!(await page.$('[data-module-tab="fxa"]')), 30000), "the song has its Mix A and FX A tracks");
await page.click('[data-module-tab="mixa"] [data-testid=track-label]');
await view("graph");
const mixa = '[data-testid=chain] [data-module="mixa"]';
check(await until(async () => !!(await page.$(`${mixa} [data-node="pump"]`)), 20000), "Mix A's chain has its six cards");

// ---------------------------------------------------------------- the Parametric EQ (CARD-111)

const eq = `${mixa} [data-node="eq"]`;
const eqPic = `${eq} [data-face] [data-picture="curve"]`;
await shown(eq);
check(await until(async () => (await points(`${eqPic} polyline[data-response]`)) === 256, 20000), `the EQ's face draws its curve, measured from its own sound: ${await points(`${eqPic} polyline[data-response]`)} points`);
const eqMarks = await page.$$eval(`${eqPic} [data-dot]`, (els) => els.map((e) => e.parentElement?.querySelector("text")?.textContent?.trim() ?? ""));
check(JSON.stringify(eqMarks) === JSON.stringify(["1", "2", "3", "4", "5", "6", "7", "8"]), `a dot for each band, numbered: ${JSON.stringify(eqMarks)}`);
check((await words(eqPic)) === "6 bands on: band 4 bell −3 dB at 800 Hz", `its words: ${await words(eqPic)}`);
await linted(eq, "the EQ's card");
await shoot(eq, "eq");
await drag(eqPic, 3, 0, -30, "tracks/mixa.ts", (c) => num(c, "gain4") > -3, "Parametric EQ", "band 4's dot dragged up raises its gain");

// ---------------------------------------------------------------- the gate (CARD-112)

const gate = `${mixa} [data-node="gate"]`;
const gatePic = `${gate} [data-face] [data-picture="levels"]`;
await shown(gate);
check(await until(async () => (await points(`${gatePic} polyline[data-line="threshold"]`)) === 2, 20000), "the gate's face draws where it opens, a line");
check((await words(gatePic)) === "Opens when the sound reaches −30 dB; below that it turns it down to −80 dB. The strip at the foot lights while it is open. Drag the line", `its words: ${await words(gatePic)}`);
const gateDown = `${gate} [data-face] [data-picture="down"]`;
check(
  await until(async () => (await words(gateDown)) === "How far it turns the sound down: none while it is open, up to 80 dB once it closes", 20000),
  `beside it, how far it turns down: ${await words(gateDown)}`,
);
await linted(gate, "the gate's card");
await shoot(gate, "gate");
await drag(gatePic, 0, 0, 20, "tracks/mixa.ts", (c) => num(c, "threshold") < -30, "Gate threshold", "its dot dragged down lowers the threshold");

// ---------------------------------------------------------------- the transient shaper (CARD-113)

const tr = `${mixa} [data-node="transient"]`;
const trPic = `${tr} [data-face] [data-picture="hit"]`;
await shown(tr);
check(await until(async () => (await points(`${trPic} polyline[data-line="after"]`)) === 400, 20000), "the transient shaper's face draws one hit, after it, a point each millisecond");
check((await words(trPic)) === "One hit, before (faint) and after: its start up 6 dB, its tail down 3 dB", `its words: ${await words(trPic)}`);
await linted(tr, "the transient shaper's card");
await shoot(tr, "transient");
await drag(trPic, 0, 0, -20, "tracks/mixa.ts", (c) => num(c, "attack") > 6, "Transient shaper attack", "the start's dot dragged up raises the attack");

// ---------------------------------------------------------------- the de-esser (CARD-114)

const de = `${mixa} [data-node="deesser"]`;
const dePic = `${de} [data-face] [data-picture="band"]`;
await shown(de);
check(await until(async () => (await page.$$(`${dePic} [data-dot]`)).length === 1, 20000), "the de-esser's face draws its band and one dot");
check(/^Listens from 3\.71 kHz to 9\.71 kHz: when that band's peaks go over −40 dB/.test((await words(dePic)) ?? ""), `its words: ${await words(dePic)}`);
const deDown = `${de} [data-face] [data-picture="down"]`;
check(
  await until(async () => (await words(deDown)) === "How far it turns the band down at 6 kHz: up to 6 dB, while the band's peaks are over −40 dB", 20000),
  `beside it, how far it turns its band down: ${await words(deDown)}`,
);
await linted(de, "the de-esser's card");
await shoot(de, "deesser");
await drag(dePic, 0, 30, 0, "tracks/mixa.ts", (c) => num(c, "freq") > 6000, "De-esser", "its dot dragged right raises the frequency");

// ---------------------------------------------------------------- the multiband (CARD-115)

const mb = `${mixa} [data-node="multiband"]`;
const mbPic = `${mb} [data-face] [data-picture="split"]`;
await shown(mb);
check(await until(async () => (await points(`${mbPic} polyline[data-response][data-line="split"]`)) === 256, 20000), "the multiband's face draws its split, measured from its sound under its thresholds");
const opens = await page.$$eval(`${mbPic} [data-dot]`, (els) => els.map((e) => e.dataset.opens ?? ""));
check(JSON.stringify(opens) === JSON.stringify(["", "", "Low", "Mid", "High"]), `a dot on each crossover, and one for each band's gain that shows its tab: ${JSON.stringify(opens)}`);
await linted(mb, "the multiband's card");
await shoot(mb, "multiband");
await drag(mbPic, 3, 0, -20, "tracks/mixa.ts", (c) => num(c, "midGain") > -2, "Multiband compressor", "the mid's dot dragged up raises its gain");

// ---------------------------------------------------------------- Pump (CARD-116)

const pump = `${mixa} [data-node="pump"]`;
const pumpPic = `${pump} [data-face] [data-picture="dip"]`;
await shown(pump);
check(await until(async () => (await points(`${pumpPic} polyline[data-line="dip"]`)) > 150, 20000), "Pump's face draws its dip over a bar, from its own sum");
check((await page.$eval(`${pumpPic} svg`, (e) => e.dataset.xSpan).catch(() => "")) === "0,4", "over 4 beats");
check(/^Dips 8 dB every 1\/4 note/.test((await words(pumpPic)) ?? ""), `its words: ${await words(pumpPic)}`);
const rate = await page.$eval(`${pump} [data-testid=setting-menu]`, (e) => e.textContent?.trim()).catch(() => null);
check(/1\/4/.test(rate ?? ""), `its rate is on its face, its menu: ${rate}`);
await linted(pump, "Pump's card");
await shoot(pump, "pump");
await drag(pumpPic, 0, 0, 20, "tracks/mixa.ts", (c) => num(c, "depth") < -8, "Pump depth", "the foot dragged down deepens the dip");

// ---------------------------------------------------------------- while the song plays, the live layers move

await post("/loop", { start: 0, end: 8, on: true });
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
const moving = async (pic, layer, what) => {
  const seen = new Set();
  for (let i = 0; i < 40 && seen.size < 3; i++) {
    await sleep(150);
    const l = (await said(pic))[layer];
    if (l && l.v > -90) seen.add(JSON.stringify(l));
  }
  check(seen.size >= 3, `while the song plays, ${what} moves: ${seen.size} reads`);
};
await shown(eqPic);
await moving(eqPic, 1, "the EQ's spectrum, out");
await shown(gatePic);
await moving(gatePic, 1, "the gate's level coming out, over time");
await shown(dePic);
await moving(dePic, 0, "the de-esser's spectrum, in");
await shown(pumpPic);
await moving(pumpPic, 1, "Pump's playhead");
// The meters (CARD-126): what each live layer said, read again and again while the song plays.
const reads = async (pic, layer, n = 60) => {
  const seen = [];
  for (let i = 0; i < n; i++) {
    await sleep(100);
    const l = (await said(pic))[layer];
    if (l) seen.push(l);
  }
  return seen;
};
await shown(gatePic);
const strip = await reads(gatePic, 3);
const lit = strip.map((l) => l.v);
check(
  lit.includes(1) && lit.includes(0) && strip.every((l) => l.y > 0.9),
  `the gate's strip, along its foot, lights on the hits and goes dark between them (its meter open, a switch): ${[...new Set(lit)]} at ${strip[0]?.y}`,
);
await shown(gateDown);
const turned = (await reads(gateDown, 1, 40)).map((l) => l.v);
check(turned.some((v) => v === 0) && turned.some((v) => v > 20), `its down says nothing turned down while it is open, and more once it closes: ${[...new Set(turned)].slice(0, 6)}`);
await shown(deDown);
const deTurned = (await reads(deDown, 1, 40)).map((l) => l.v);
check(deTurned.some((v) => v > 0) && deTurned.every((v) => v >= 0 && v <= 6), `the de-esser's down says how far it turns its band down, up to 6 dB: ${[...new Set(deTurned)].slice(0, 6)}`);
await shown(mbPic);
const bars = await reads(mbPic, 2, 40);
const xs = [2, 3, 4].map((k) => said(mbPic).then((l) => l[k]?.x));
const [lowX, midX, highX] = await Promise.all(xs);
check(lowX < midX && midX < highX && highX > 0.9, `the multiband's bars stand side by side, low to high, at its right: ${[lowX, midX, highX]}`);
check(bars.some((l) => l.v > 0), `the low band's bar turns down: ${[...new Set(bars.map((l) => l.v))].slice(0, 6)}`);
await shoot(gate, "gate-playing");
await shoot(de, "deesser-playing");
await shoot(mb, "multiband-playing");
await shoot(eq, "eq-playing");
await shoot(pump, "pump-playing");
await page.keyboard.press("Space");

// ---------------------------------------------------------------- FX A: the effects

await page.click('[data-module-tab="fxa"] [data-testid=track-label]');
const fxa = '[data-testid=chain] [data-module="fxa"]';
check(await until(async () => !!(await page.$(`${fxa} [data-node="widen"]`)), 20000), "FX A's chain has its eight cards");

const dl = `${fxa} [data-node="delay"]`;
const dlPic = `${dl} [data-face] [data-picture="echoes"]`;
await shown(dl);
check(await until(async () => (await page.$$(`${dlPic} rect`)).length >= 7, 20000), `the delay's face draws its click and its echoes as bars: ${(await page.$$(`${dlPic} rect`)).length}`);
check(/^An echo every 250 ms, each 7\.96 dB quieter/.test((await words(dlPic)) ?? ""), `its words: ${await words(dlPic)}`);
await linted(dl, "the delay's card");
await shoot(dl, "delay");
await drag(dlPic, 0, 30, 0, "tracks/fxa.ts", (c) => num(c, "time") > 0.25, "Delay time", "the first echo dragged right makes the time longer");

const rv = `${fxa} [data-node="reverb"]`;
const rvPic = `${rv} [data-face] [data-picture="tail"]`;
await shown(rv);
check(await until(async () => (await points(`${rvPic} polyline[data-line="tail"]`)) === 48, 20000), "the reverb's face draws its tail, from its own sums");
check(/^Its tail fades 30 dB in /.test((await words(rvPic)) ?? ""), `its words: ${await words(rvPic)}`);
await linted(rv, "the reverb's card");
await shoot(rv, "reverb");
await drag(rvPic, 0, 30, 0, "tracks/fxa.ts", (c) => num(c, "size") > 0.6, "Reverb size", "its dot dragged right makes the room bigger");

for (const [node, line, n, said0] of [
  ["chorus", "one", 241, /^Two copies of the sound, 8 to 11 ms late/],
  ["phaser", "mid", 241, /^Three notches sweep round 640 Hz/],
  ["tremolo", "level", 241, /^The level dips to 0\.5 and back, 5 times a second/],
]) {
  const card = `${fxa} [data-node="${node}"]`;
  const pic = `${card} [data-face] [data-picture="motion"]`;
  await shown(card);
  check(await until(async () => (await points(`${pic} polyline[data-line="${line}"]`)) === n, 20000), `the ${node}'s face draws a second of its motion`);
  check(said0.test((await words(pic)) ?? ""), `its words: ${await words(pic)}`);
  await linted(card, `the ${node}'s card`);
  await shoot(card, node);
}

const dr = `${fxa} [data-node="drive"]`;
const drPic = `${dr} [data-face] [data-picture="curve"]`;
await shown(dr);
check(await until(async () => (await points(`${drPic} polyline[data-line="curve"]`)) === 201, 20000), "drive's face draws its curve, from its own sum");
await linted(dr, "drive's card");
await shoot(dr, "drive");
await drag(drPic, 0, 0, -30, "tracks/fxa.ts", (c) => num(c, "amount") > 9, "Drive amount", "its dot dragged up drives harder");

const bc = `${fxa} [data-node="bitcrush"]`;
const bcPic = `${bc} [data-face] [data-picture="crush"]`;
await shown(bc);
check(await until(async () => (await points(`${bcPic} polyline[data-line="crushed"]`)) === 96, 20000), "the bitcrusher's face draws a sine, crushed, a step each sample");
check((await words(bcPic)) === "A 1 kHz sine, rounded to 6 bits and held 12 thousand times a second", `its words: ${await words(bcPic)}`);
await linted(bc, "the bitcrusher's card");
await shoot(bc, "bitcrush");

const wd = `${fxa} [data-node="widen"]`;
const wdPic = `${wd} [data-face] [data-picture="field"]`;
await shown(wd);
check(await until(async () => (await points(`${wdPic} polyline[data-line="field"]`)) === 3, 20000), "the widener's face draws its field");
check((await points(`${wdPic} polyline[data-line="made"]`)) === 3, "and, past width 1, the fainter fan of the width it makes up for a mono sound");
await linted(wd, "the widener's card");
await shoot(wd, "widen");
await drag(wdPic, 0, 20, 0, "tracks/fxa.ts", (c) => num(c, "width") > 1.5, "Widen width", "its edge dragged right widens it");

// The whole chains, to compare with the mockup.
await page.screenshot({ path: `${SHOTS}/dress2a-fxa.png` });
await page.click('[data-module-tab="mixa"] [data-testid=track-label]');
await sleep(500);
await page.screenshot({ path: `${SHOTS}/dress2a-mixa.png` });

done(errors);
