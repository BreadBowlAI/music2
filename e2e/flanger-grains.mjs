// The Flanger (PACK-35, CARD-128) and Grains (PACK-36, CARD-129), on first-song with a Lead track (a
// saw). The + after the saw lists the Flanger under Modulation and Grains under Delay & Reverb, and
// picking the Flanger writes `flanger(osc.saw(110), { rate: 0.25, depth: 0.7, feedback: 0.5, mix: 0.5 })`
// in the Lead's own code, with the pack's import, line and lock; its face draws its sweep beside Rate,
// Depth, Feedback and Mix, and its words say the sweep. Its knobs write the call: Feedback's End and
// Home write 0.9 and −0.9, each one undo step, and its words turn hollow and say how loud it comes out.
// The + after the Flanger adds Grains: `grains(flanger(…), { size: 0.1, density: 20, spread: 0.3, pitch:
// 0, mix: 0.5 })`, its face its band beside Size, Density, Spread, Pitch and Mix (no Freeze); Pitch's End
// writes 12 and its words say the band's far edge an octave up, a grain's length further back; its
// band's edge dragged right writes a longer spread; Size's End and Density's Home write 0.5 and 1,
// long grains far apart in time. Then, the song looped, each picture's live line (from its kernel's
// meter, CARD-73) moves while the song plays and stands still while it is stopped: a state counts once
// two reads in a row show it (the first may be stale: voice2's review V1). A read is where the card's
// canvas lit its line (its pixels, by the plot's place in `data-plot`: CARD-103's `data-said` holds no
// meter playhead's place) and an agent's look at the meter. The Flanger's line is drawn at its LFO's
// phase; Grains' at `reads / 2.5 s` across, its buffer, each within 0.05 of the plot, read between two
// looks that agree (no grain started between them) after the canvas drew twice. The cards are shot
// while the song plays, in the patch and on Device's chain at Normal and Expanded: to $E2E_SHOTS when
// set, else the run's folder, as `flanger-<patch|normal|expanded>.png` and `grains-…`. ⌘Z steps back,
// byte for byte, a step at a time. Each card linted.
//
//   node e2e/flanger-grains.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const SHOTS = process.env.E2E_SHOTS ?? ROOT;
fs.mkdirSync(SHOTS, { recursive: true });

// ---------------------------------------------------------------- the song: Lead, a saw

write("tracks/lead.ts", 'import { track } from "@m2/core";\nimport { osc } from "std/dsp";\n\nexport default track("lead", { name: "Lead" }, () => osc.saw(110));\n');
write(
  "song.ts",
  read("song.ts")
    .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport lead from "./tracks/lead";')
    .replace("      kick: kick.out,", "      kick: kick.out,\n      lead: { in: lead.out, db: db(-12) },"),
);

const { page, errors, show, view, key, box, api } = await open(1600, 1000);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
/** `sel` scrolled into the middle of the window, and where it is then. */
const seen = async (sel) => {
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
  await sleep(150);
  return box(sel);
};
/** A picture's words, its group's name. */
const words = (pic) => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
/** A card's shot. */
const shoot = async (card, name) => {
  const b = await seen(card);
  if (!b) return check(false, `${name}: no card to shoot`);
  await page.screenshot({ path: `${SHOTS}/${name}.png`, clip: { x: Math.max(0, b.x - 4), y: Math.max(0, b.y - 4), width: Math.min(b.width + 8, 1600), height: Math.min(b.height + 8, 1000) } });
};
const lead = () => read("tracks/lead.ts");
const groupOf = (name) => page.$eval(`[data-effect-browser] [data-effect="${name}"]`, (e) => e.closest("[data-category]")?.dataset.category).catch(() => null);
const knobsOf = (card) => page.$$eval(`${card} [data-knob]`, (ks) => ks.map((k) => k.dataset.knob)).catch(() => []);
/** Focus the knob `slot` of `card` and press `keys`, one after another. */
const keys = async (card, slot, ...pressed) => {
  await seen(`${card} [data-knob="${slot}"]`);
  await page.$eval(`${card} [data-knob="${slot}"] svg[role=slider]`, (e) => e.focus());
  for (const k of pressed) await page.keyboard.press(k);
};

check(!!(await until(() => page.$('[data-module-tab="lead"]'), 20000)), "the Lead track is there");

// ---------------------------------------------------------------- + ▸ Flanger (PACK-35)

await show("lead");
await view("graph");
const plus = '[data-module="lead"] button[aria-label^="Add an effect after"]';
check(!!(await until(() => page.$(plus), 15000)), "the saw has a +");
await page.click(plus);
await page.waitForSelector('[data-effect-browser] [data-effect="flanger"]', { timeout: 5000 }).catch(() => {});
check((await groupOf("flanger")) === "Modulation" && (await groupOf("grains")) === "Delay & Reverb", `the Flanger is listed under Modulation, Grains under Delay & Reverb: ${await groupOf("flanger")}, ${await groupOf("grains")}`);
await page.click('[data-effect-browser] [data-effect="flanger"]');
const flanged = "flanger(osc.saw(110), { rate: 0.25, depth: 0.7, feedback: 0.5, mix: 0.5 })";
check(await until(() => lead().includes(flanged), 15000), `it goes after the saw, in the Lead's own code: ${lead().match(/=> .*/)?.[0]}`);
check(
  lead().includes('import { flanger } from "@m2/effects";') && /"@m2\/effects" = /.test(read("music2.toml")) && read("music2.lock").includes('name = "@m2/effects"'),
  "imported, and the pack listed and locked",
);
const fcard = '[data-module="lead"] [data-node="flanger"]';
const sweep = `${fcard} [data-picture="sweep"]`;
check(!!(await until(() => page.$(`${sweep} svg`), 15000)), "its face draws its sweep");
const fknobs = await knobsOf(fcard);
check(["rate", "depth", "feedback", "mix"].every((k) => fknobs.includes(k)), `Rate, Depth, Feedback and Mix are knobs on its face: ${fknobs.join()}`);
const line = "The line: where they are now";
check(
  await until(async () => (await words(sweep)) === `Its notches sweep from 5 kHz down to 199 Hz and back, once each 4 s. Feedback 0.5 rings at the peaks between them. ${line}`),
  `its words say its sweep: ${await words(sweep)}`,
);
await linted(fcard, "the Flanger's card");

// Its knobs write the call (taste.md DV-15): Feedback's End, then Home.
await keys(fcard, "feedback", "End");
check(await until(() => lead().includes("flanger(osc.saw(110), { rate: 0.25, depth: 0.7, feedback: 0.9, mix: 0.5 })")), `End writes Feedback's highest, 0.9: ${lead().match(/=> .*/)?.[0]}`);
check(await until(async () => ((await undoTitle()) ?? "").startsWith("Undo Flanger feedback")), `one undo step: ${await undoTitle()}`);
await keys(fcard, "feedback", "Home");
check(await until(() => lead().includes("feedback: -0.9")), `Home writes its lowest, −0.9, the hollow sound: ${lead().match(/=> .*/)?.[0]}`);
check(await until(async () => ((await words(sweep)) ?? "").includes("Feedback −0.9 turns the copy over")), `its words turn hollow: ${await words(sweep)}`);
check(
  await until(async () => ((await words(sweep)) ?? "").endsWith(`${line}. It comes out up to 14.8 dB louder at its peaks: turn Mix or the track down`)),
  `and say how loud it comes out (PACK-35): ${await words(sweep)}`,
);

// ---------------------------------------------------------------- + ▸ Grains (PACK-36)

const after = '[data-module="lead"] button[aria-label="Add an effect after flanger"]';
check(!!(await until(() => page.$(after), 15000)), "the Flanger has a +");
await page.click(after);
await page.waitForSelector('[data-effect-browser] [data-effect="grains"]', { timeout: 5000 }).catch(() => {});
await page.click('[data-effect-browser] [data-effect="grains"]');
const grained = "grains(flanger(osc.saw(110), { rate: 0.25, depth: 0.7, feedback: -0.9, mix: 0.5 }), { size: 0.1, density: 20, spread: 0.3, pitch: 0, mix: 0.5 })";
check(await until(() => lead().includes(grained), 15000), `Grains goes after the Flanger: ${lead().match(/=> .*/)?.[0]}`);
check(lead().includes('import { flanger, grains } from "@m2/effects";'), "and is imported beside it");
const gcard = '[data-module="lead"] [data-node="grains"]';
const cloud = `${gcard} [data-picture="cloud"]`;
check(!!(await until(() => page.$(`${cloud} svg`), 15000)), "its face draws its band");
const gknobs = await knobsOf(gcard);
check(
  ["size", "density", "spread", "pitch", "mix"].every((k) => gknobs.includes(k)) && !gknobs.includes("freeze"),
  `Size, Density, Spread, Pitch and Mix are on its face, and no Freeze: ${gknobs.join()}`,
);
const tail = "The line: where the newest grain reads now. Drag the band's edge for the spread";
check(
  await until(async () => (await words(cloud)) === `Pieces of 100 ms, 20 a second, each from up to 300 ms back (the band). ${tail}`),
  `its words say its grains: ${await words(cloud)}`,
);
await linted(gcard, "Grains' card");

const dot = `${cloud} [data-dot="0"]`;
/** Where Grains' dot is drawn: its circle's centre across, in the picture's own pixels. */
const dotAt = () => page.$eval(dot, (e) => Number(e.getAttribute("cx"))).catch(() => null);
const was = await dotAt();
await keys(gcard, "pitch", "End");
check(await until(() => lead().includes("{ size: 0.1, density: 20, spread: 0.3, pitch: 12, mix: 0.5 }")), `Pitch's End writes 12, an octave up: ${lead().match(/=> .*/)?.[0]}`);
check(
  await until(async () => ((await words(cloud)) ?? "").includes("each from up to 400 ms back and an octave higher (the band)")),
  `its words say so, and the band's far edge a grain's length further back: ${await words(cloud)}`,
);
// Its band moves back by a grain's length an octave up (PACK-36's back): drawn again, its dot has moved.
check(await until(async () => (await dotAt()) !== null && (await dotAt()) > was), `its band is drawn again, an octave up further back: its dot from ${was} to ${await dotAt()}`);
// The band's edge, its dot, dragged 60 px right: a longer spread, one undo step.
const spreadOf = () => Number(/spread: ([0-9.]+)/.exec(lead())?.[1]);
const d = await seen(dot);
if (d) {
  await page.mouse.move(d.x + d.width / 2, d.y + d.height / 2);
  await page.mouse.down();
  for (let i = 1; i <= 10; i++) {
    await page.mouse.move(d.x + d.width / 2 + i * 6, d.y + d.height / 2);
    await sleep(30);
  }
  await page.mouse.up();
}
check(await until(() => spreadOf() > 0.4, 8000), `its band's edge dragged right writes a longer spread: ${spreadOf()}`);
check(await until(async () => ((await undoTitle()) ?? "").startsWith("Undo Grains spread")), `one undo step: ${await undoTitle()}`);
// Long grains, far apart in time, so its line holds still long enough to be read against its meter.
await keys(gcard, "size", "End");
check(await until(() => /size: 0\.5, density: 20,/.test(lead())), `Size's End writes 0.5 s: ${lead().match(/=> .*/)?.[0]}`);
await keys(gcard, "density", "Home");
check(await until(() => /size: 0\.5, density: 1,/.test(lead())), `Density's Home writes 1 a second: ${lead().match(/=> .*/)?.[0]}`);
check(await until(async () => ((await words(cloud)) ?? "").startsWith("Pieces of 500 ms, 1 a second, each from up to ")), `its words say so: ${await words(cloud)}`);

// ---------------------------------------------------------------- the pictures, live (CARD-73)

const g = (await api("")).body;
const nodeOf = (label) => g.view?.modules?.find((m) => m.key === "lead")?.nodes?.find((n) => n.label === label)?.id;
/** An agent's look at the meter `name` of the call labelled `label`: what it says now. */
const look = async (label, name) => (await post("/look", { session: g.session, client: "agent:fx3", module: "lead", node: nodeOf(label), kind: "meter", at: name, on: true })).body?.look;
/**
 * Where a picture's live line is drawn, from 0 to 1 across its plot: the columns its canvas lit
 * across the middle of the plot (the line is 2 px wide at the canvas's twice-the-pixels scale), by
 * the plot's place the canvas says (`data-plot`, CARD-103's); null when it drew none.
 */
const drawn = (pic) =>
  page
    .$eval(`${pic} canvas[data-live]`, (c) => {
      const [left, span, plot] = (c.dataset.plot ?? "").split(",").map(Number);
      if (!span) return null;
      const lit = c.getContext("2d").getImageData(0, Math.round(plot), c.width, 1).data;
      const cols = [];
      for (let x = 0; x < c.width; x++) if (lit[4 * x + 3] > 0) cols.push(x);
      return cols.length ? ((cols[0] + cols[cols.length - 1]) / 2 + 0.5 - 2 * left) / (2 * span) : null;
    })
    .catch(() => null);
/** Where each picture's line is drawn now, and what an agent's looks at their meters say. */
const where = async () => ({
  sweep: await drawn(sweep),
  phase: (await look("flanger", "phase"))?.now ?? null,
  cloud: await drawn(cloud),
  reads: (await look("grains", "reads"))?.now ?? null,
});
/** Whether `holds(a, b)` holds for two pairs of reads in a row, `every` ms apart, within `ms`. */
const twice = async (holds, every, ms) => {
  let [was, runs] = [await where(), 0];
  for (const end = Date.now() + ms; Date.now() < end && runs < 2; ) {
    await sleep(every);
    const now = await where();
    runs = holds(was, now) ? runs + 1 : 0;
    was = now;
  }
  return { ok: runs >= 2, last: was };
};
const known = (a, b, k) => typeof a[k] === "number" && typeof b[k] === "number";
const moved = (a, b, k) => known(a, b, k) && Math.abs(a[k] - b[k]) > 1e-3;
const still = (a, b, k) => known(a, b, k) && Math.abs(a[k] - b[k]) < 1e-6;
const told = (w) => JSON.stringify(Object.fromEntries(Object.entries(w).map(([k, v]) => [k, typeof v === "number" ? +v.toFixed(4) : v])));

await post("/loop", { start: 0, end: 8, on: true });
await seen(sweep);
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
let m = await twice((a, b) => moved(a, b, "sweep") && moved(a, b, "phase"), 400, 15000);
check(m.ok, `while the song plays, the Flanger's line moves with its sweep, and so does an agent's look at its meter: ${told(m.last)}`);
// Drawn where its meter says its sweep is: the read of each a moment apart, its sweep a quarter of a cycle a second.
check(known(m.last, m.last, "sweep") && known(m.last, m.last, "phase") && Math.abs(m.last.sweep - m.last.phase) < 0.05, `the Flanger's line is drawn at its LFO's phase: ${told(m.last)}`);
m = await twice((a, b) => moved(a, b, "cloud") && moved(a, b, "reads"), 400, 15000);
check(m.ok, `while the song plays, Grains' line jumps from grain to grain, and so does an agent's look at its meter: ${told(m.last)}`);
// Drawn where its meter says its newest grain reads, `reads / 2.5 s` across: read between two looks
// that agree (no grain started between them: an octave up a grain's place drifts 0.1 s in the time),
// once the canvas has drawn twice since the first, twice in a row.
const drew = () => page.$eval(`${cloud} canvas[data-live]`, (c) => Number(c.dataset.reads ?? 0)).catch(() => 0);
const against = [];
for (const end = Date.now() + 20000; Date.now() < end && against.length < 2; ) {
  const a = (await look("grains", "reads"))?.now;
  const r0 = await drew();
  await until(async () => (await drew()) >= r0 + 2, 2000);
  const c = await drawn(cloud);
  const b = (await look("grains", "reads"))?.now;
  if (!(a > 0 && b > 0 && Math.abs(a - b) < 0.1 && typeof c === "number")) continue;
  const off = Math.abs(c - (a + b) / 2 / 2.5);
  if (off < 0.05) against.push(+off.toFixed(4));
  else against.length = 0;
}
check(against.length === 2, `Grains' line is drawn at reads / 2.5 s across, within 0.05 (off by ${against.join(", ")})`);
// Shot while the song plays: in the patch, then on Device's chain at Normal and Expanded.
await shoot(`[data-testid=graph] ${fcard}`, "flanger-patch");
await shoot(`[data-testid=graph] ${gcard}`, "grains-patch");
await page.click("[data-device=chain]").catch(() => {});
const chained = (c) => `[data-testid=chain] ${c}`;
check(await until(async () => !!(await page.$(chained(fcard))) && !!(await page.$(chained(gcard))), 15000), "both are cards on Device's chain");
for (const size of ["normal", "expanded"]) {
  for (const [c, name] of [
    [fcard, "flanger"],
    [gcard, "grains"],
  ]) {
    await page.click(`${chained(c)} [data-size-to="${size}"]`).catch(() => {});
    await sleep(1200);
    await shoot(chained(c), `${name}-${size}`);
  }
}
await linted(chained(fcard), "the Flanger's chain card, Expanded");
await linted(chained(gcard), "Grains' chain card, Expanded");
for (const c of [fcard, gcard]) await page.click(`${chained(c)} [data-size-to="normal"]`).catch(() => {});
await page.click("[data-device=patch]").catch(() => {});
await until(async () => !!(await page.$(`[data-testid=graph] ${gcard}`)), 15000);
await seen(sweep);

// Stopped: both lines stand still, the Flanger's where its sweep waits, Grains' at now once its last
// grains have played out.
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
m = await twice((a, b) => ["sweep", "phase", "cloud", "reads"].every((k) => still(a, b, k)), 600, 15000);
check(m.ok, `stopped, both pictures' lines stand still, and so do an agent's looks: ${told(m.last)}`);
check(m.last.reads === 0 && Math.abs(m.last.cloud) < 0.01, `stopped, no grain plays: Grains' line rests at now: ${told(m.last)}`);
for (const [label, name] of [
  ["flanger", "phase"],
  ["grains", "reads"],
]) {
  await post("/look", { session: g.session, client: "agent:fx3", module: "lead", node: nodeOf(label), kind: "meter", at: name, on: false });
}

// ---------------------------------------------------------------- ⌘Z

await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => /size: 0\.5, density: 20,/.test(lead())), `⌘Z takes Density back: ${lead().match(/=> .*/)?.[0]}`);
await key("Meta+z");
check(await until(() => /size: 0\.1, density: 20,/.test(lead())), `⌘Z takes Size back: ${lead().match(/=> .*/)?.[0]}`);
await key("Meta+z");
check(await until(() => lead().includes("spread: 0.3")), "⌘Z takes the spread back");

done(errors);
