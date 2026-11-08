// Track dress2b: music2's own instruments wear their pictures (CARD-130 to CARD-144), and std's
// Limit and Master their ceiling. On first-song, with a Wavetable, a Polysynth and an FM synth added
// and a Limit after the keys: Device's chain draws the Wavetable's table (its frames, the LFO's
// reach), its filter's shape, its LFO and table B on their tabs; the Polysynth's one note, measured,
// and its 24 dB ladder; the FM synth's partials; the Kick's Drum's hit, measured; the Limit's and
// the Master's ceiling. A dot dragged on each draggable picture writes its value, stays under the
// pointer, is one undo step, and ⌘Z takes it back; while the song plays the Master's and the
// Limit's last seconds move. Each card linted and shot, to compare with the design's mockup.
//
//   node e2e/instruments.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

// ---------------------------------------------------------------- the song: three instruments more, a Limit after the keys

write(
  "tracks/wt.ts",
  `import { track } from "@m2/core";
import { wavetable } from "@m2/wavetable";

export default track("wt", { name: "Wave" }, ({ lane }) => wavetable(lane.notes("notes"), { tableA: "vowels", positionA: 0.35, lfoPosition: 0.3 }));
`,
);
write(
  "tracks/poly.ts",
  `import { track } from "@m2/core";
import { synth } from "@m2/synths";

export default track("poly", { name: "Poly" }, ({ lane }) => synth.custom(lane.notes("notes")));
`,
);
write(
  "tracks/fm.ts",
  `import { track } from "@m2/core";
import { fm } from "@m2/synths";

export default track("fm", { name: "FM" }, ({ lane }) => fm.custom(lane.notes("notes")));
`,
);
write(
  "tracks/keys.ts",
  `import { db, track } from "@m2/core";
import { pluck } from "@m2/synths";
import { limit } from "std/fx";

export default track("keys", { name: "Keys" }, ({ lane }) => limit(pluck(lane.notes("chords")), { ceiling: db(-6) }));
`,
);
// The Wavetable's pack, which first-song doesn't use yet.
write("music2.toml", read("music2.toml").replace('"@m2/synths" = "2"', '"@m2/synths" = "2"\n"@m2/wavetable" = "1"'));
write(
  "song.ts",
  read("song.ts")
    .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport wt from "./tracks/wt";\nimport poly from "./tracks/poly";\nimport fm from "./tracks/fm";')
    .replace("keys: { in: keys.out, db: db(-5), pan: -0.25 },", "keys: { in: keys.out, db: db(-5), pan: -0.25 },\n      wt: wt.out,\n      poly: poly.out,\n      fm: fm.out,"),
);

const { page, errors, api, view, box, key } = await open(1920, 1080, { device: "chain" });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const shown = (sel) => page.$eval(sel, (e) => e.scrollIntoView({ block: "nearest", inline: "center" })).catch(() => {});
/** How many points a picture's line, response or hit has. */
const points = (sel) => page.$eval(sel, (e) => (e.getAttribute("points") ?? "").split(" ").filter(Boolean).length).catch(() => 0);
const words = (pic) => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
const center = async (sel) => {
  const b = await box(sel);
  return [b.x + b.width / 2, b.y + b.height / 2];
};
/** Drag the dot `sel` by `dx`, `dy` px in eight steps; true when, held at the end, it is under the pointer. */
async function drag(sel, dx, dy) {
  await shown(sel);
  await sleep(150);
  const [x, y] = await center(sel);
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(x + (dx * i) / 8, y + (dy * i) / 8);
    await sleep(30);
  }
  const [hx, hy] = await center(sel);
  await page.mouse.up();
  return Math.abs(hx - (x + dx)) <= 3 && Math.abs(hy - (y + dy)) <= 3;
}
/** Takes back the last write, once it is `file`'s, and waits for `file` to be `was` again. */
async function undo(file, was) {
  await page.mouse.click(10, 10);
  if (read(file) !== was) await key("Meta+z");
  return until(() => read(file) === was, 8000);
}
/** A shot of the card `sel` alone, to the run's folder. */
const shot = async (sel, name) => (await page.$(sel))?.screenshot({ path: `${ROOT}/instruments-${name}.png` });
const select = async (track) => {
  await page.click(`[data-module-tab="${track}"] [data-testid=track-label]`);
  await view("graph");
};

check(await until(async () => !!(await page.$('[data-module-tab="fm"]')), 30000), "the song has its three new tracks");

// ---------------------------------------------------------------- the Wavetable (CARD-131 to CARD-135)

await select("wt");
const wt = '[data-testid=chain] [data-module="wt"] [data-node="wavetable"]';
check(await until(async () => !!(await page.$(wt)), 20000), "Wave's chain has the Wavetable's card");
await shown(wt);
const table = `${wt} [data-picture="wavesA"]`;
check(await until(async () => (await points(`${table} polyline[data-line="frame"]`)) === 65, 20000), "its face draws table A: the frame playing, 65 points");
// Eight frames behind it, and the two at the ends of the LFO's reach, with the line between them.
check((await page.$$(`${table} polyline:not([data-line])`)).length === 10 && !!(await page.$(`${table} polyline[data-line="reach"]`)), "eight frames behind it, and the LFO's reach");
check(/^Table A, vowels, at position 0\.35: drag across to move through it\. The LFO moves it from 0\.05 to 0\.65$/.test((await words(table)) ?? ""), `its words: ${await words(table)}`);
const tabs = await page.$$eval(`${wt} [role=tab]`, (els) => els.map((e) => e.dataset.tab));
check(JSON.stringify(tabs) === '["Osc A","Osc B","Filter","Env","LFO","More","All"]', `its tabs: ${JSON.stringify(tabs)}`);
await linted(wt, "the Wavetable's card");
await shot(wt, "wavetable");
const wtCode = read("tracks/wt.ts");
check(await drag(`${table} [data-dot="0"]`, 40, 0), "the position's dot, held, stays under the pointer");
const position = () => Number(/positionA: ([0-9.]+)/.exec(read("tracks/wt.ts"))?.[1]);
check(await until(() => position() > 0.36, 8000), `dragged right, it writes a later position A: ${position()}`);
check(await until(async () => /^Undo Wavetable position A/.test((await undoTitle()) ?? "")), `one undo step: ${await undoTitle()}`);
check(await undo("tracks/wt.ts", wtCode), "⌘Z takes it back");
await page.click(`${wt} [data-tab="Filter"]`);
const curve = `${wt} [data-picture="curve"]`;
check(await until(async () => (await points(`${curve} polyline[data-line="curve"]`)) === 241, 15000), "its Filter tab draws its own shape's curve");
check(/^Cuts the highs above 8 kHz, q 0\.7/.test((await words(curve)) ?? ""), `its words: ${await words(curve)}`);
await page.click(`${wt} [data-tab="LFO"]`);
check(await until(async () => (await points(`${wt} [data-picture="lfo"] polyline[data-line="shape"]`)) > 30, 15000), "its LFO tab draws the LFO's sine");
await page.click(`${wt} [data-tab="Osc B"]`);
check(await until(async () => (await points(`${wt} [data-picture="wavesB"] polyline[data-line="frame"]`)) === 65, 15000), "its Osc B tab draws table B");

// ---------------------------------------------------------------- the Polysynth (CARD-136, CARD-138)

await select("poly");
const poly = '[data-testid=chain] [data-module="poly"] [data-node="synth.custom"]';
check(await until(async () => !!(await page.$(poly)), 20000), "Poly's chain has the Polysynth's card");
await shown(poly);
const note = `${poly} [data-picture="note"]`;
check(await until(async () => (await points(`${note} polygon[data-hit]`)) === 512, 20000), "its face draws one note, measured from its own sound");
check(!!(await page.$(`${note} rect[data-play="60"]`)), "a place over it that plays middle C");
check(await until(async () => (await page.$$(`${poly} [data-picture="amp"] [data-dot]`)).length === 3, 15000), "beside its envelope, three corners");
await linted(poly, "the Polysynth's card");
await shot(poly, "polysynth");
await page.click(`${poly} [data-tab="Filter"]`);
const ladder = `${poly} [data-picture="filter"]`;
check(await until(async () => (await points(`${ladder} polyline[data-line="curve"]`)) === 241, 15000), "its Filter tab draws its ladder");
check(/24 dB an octave/.test((await words(ladder)) ?? ""), `its words: ${await words(ladder)}`);
const polyCode = read("tracks/poly.ts");
check(await drag(`${ladder} [data-dot="0"]`, -30, 0), "the cutoff's dot, held, stays under the pointer");
check(await until(() => /cutoff: /.test(read("tracks/poly.ts")), 8000), `dragged left, it writes the cutoff: ${/cutoff: [^,}]+/.exec(read("tracks/poly.ts"))?.[0]}`);
check(await undo("tracks/poly.ts", polyCode), "⌘Z takes it back");
await shot(poly, "ladder");

// ---------------------------------------------------------------- the FM synth (CARD-140)

await select("fm");
const fmc = '[data-testid=chain] [data-module="fm"] [data-node="fm.custom"]';
check(await until(async () => !!(await page.$(fmc)), 20000), "FM's chain has the FM synth's card");
await shown(fmc);
const parts = `${fmc} [data-picture="partials"]`;
check(await until(async () => (await page.$$(`${parts} g > rect`)).length > 5, 15000), "its face draws its partials as bars");
check((await page.$$(`${parts} [data-dot]`)).length === 2, "a dot for each pair");
await linted(fmc, "the FM synth's card");
await shot(fmc, "fm");
const fmCode = read("tracks/fm.ts");
check(await drag(`${parts} [data-dot="0"]`, 30, 0), "pair A's dot, held, stays under the pointer");
const modulator = () => Number(/modulatorA: ([0-9.]+)/.exec(read("tracks/fm.ts"))?.[1]);
check(await until(() => modulator() > 1, 8000), `dragged right, it writes a higher modulator A: ${modulator()}`);
check(await undo("tracks/fm.ts", fmCode), "⌘Z takes it back");

// ---------------------------------------------------------------- the Drum (CARD-141)

await select("kick");
const drum = '[data-testid=chain] [data-module="kick"] [data-node="drums.drum"]';
check(await until(async () => !!(await page.$(drum)), 20000), "Kick's chain has the Drum's card");
await shown(drum);
const hit = `${drum} [data-picture="hit"]`;
check(await until(async () => (await points(`${hit} polygon[data-hit]`)) === 512, 20000), "its face draws its hit, measured from its own sound");
check(/^Kick, one hit as it sounds: 600 ms\. Click it to hear it/.test((await words(hit)) ?? ""), `its words: ${await words(hit)}`);
const playTitle = await page.$eval(`${hit} rect[data-play="45"] title`, (e) => e.textContent).catch(() => null);
check(/Audition is off|Click to hear it/.test(playTitle ?? ""), `a place over it that plays it, saying how: ${playTitle}`);
const kickBefore = read("tracks/kick.ts");
await page.click(`${hit} rect[data-play="45"]`);
await sleep(300);
check(read("tracks/kick.ts") === kickBefore, "clicking it plays (or says why not) and writes nothing");
await linted(drum, "the Drum's card");
await shot(drum, "drum");
const kickCode = read("tracks/kick.ts");
const hit0 = await page.$eval(`${hit} polygon[data-hit]`, (e) => e.getAttribute("points"));
check(await drag(`${hit} [data-dot="0"]`, 0, -20), "the punch's dot, held, stays under the pointer");
const punch = () => Number(/punch: ([-0-9.]+)/.exec(read("tracks/kick.ts"))?.[1]);
check(await until(() => punch() > 0, 8000), `dragged up, it writes more punch: ${punch()}`);
check(await until(async () => (await page.$eval(`${hit} polygon[data-hit]`, (e) => e.getAttribute("points"))) !== hit0, 15000), "and the hit is measured again");
check(await until(async () => /^Undo Drum punch/.test((await undoTitle()) ?? "")), `one undo step: ${await undoTitle()}`);
check(await undo("tracks/kick.ts", kickCode), "⌘Z takes it back");

// ---------------------------------------------------------------- the Limit and the Master (CARD-143, CARD-144)

await select("keys");
const lim = '[data-testid=chain] [data-module="keys"] [data-node="limit"]';
check(await until(async () => !!(await page.$(`${lim} [data-picture="level"] [data-dot]`)), 20000), "Keys' Limit draws its ceiling, with a dot");
check((await words(`${lim} [data-picture="level"]`)) === "Turns peaks down so they stay under −6 dB. Drag the line to move the ceiling", `its words: ${await words(`${lim} [data-picture="level"]`)}`);
const keysCode = read("tracks/keys.ts");
check(await drag(`${lim} [data-picture="level"] [data-dot="0"]`, 0, 15), "the ceiling's dot, held, stays under the pointer");
const ceiling = () => Number(/ceiling: db\(([-0-9.]+)\)|ceiling: ([-0-9.]+)/.exec(read("tracks/keys.ts"))?.slice(1).find((x) => x !== undefined));
check(await until(() => ceiling() < -6, 8000), `dragged down, it writes a lower ceiling: ${ceiling()}`);
check(await undo("tracks/keys.ts", keysCode), "⌘Z takes it back");

await page.click('[data-module-tab="song"]');
const master = '[data-testid=chain] [data-module="song"] [data-node="master"]';
const ml = `${master} [data-picture="level"]`;
check(await until(async () => !!(await page.$(`${ml} [data-dot]`)), 20000), "the Master draws its ceiling, with a dot");
await shown(master);
await linted(master, "the Master's card");
// While the song plays, the Master's last seconds move: what comes out of it, read again and again.
const said = (sel) => page.$eval(`${sel} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")).catch(() => []);
const reads = (sel) => page.$eval(`${sel} canvas[data-live]`, (e) => Number(e.dataset.reads ?? 0)).catch(() => 0);
const reads0 = await reads(ml);
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
let loud = false;
for (let i = 0; i < 60 && !loud; i++) {
  await sleep(150);
  const h = await said(ml);
  loud = !!h[1] && h[1].v > -48;
}
// Read again and again: more than 5 reads since Play, waited for, since the first loud read may be
// one of the first few (with the meters' tick late, 4 reads had come then).
const moved = loud && (await until(async () => (await reads(ml)) > reads0 + 5));
check(moved, `while the song plays, its last seconds move: what comes out, at ${(await said(ml))[1]?.v} dB, read ${(await reads(ml)) - reads0} times since Play`);
await sleep(400);
await shot(master, "master");
await page.keyboard.press("Space");

done(errors);
