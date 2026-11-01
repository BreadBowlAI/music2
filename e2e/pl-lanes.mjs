// Polish lane D, D3 and D4 (the polish plan §6, X17): automation on its own scale (a lane in Hz
// draws on a log scale over 20 Hz to 20 kHz, widened to its points, its lines curved as they play;
// other units as before) and hiding one automation lane.
//
//   node e2e/pl-lanes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, state, caughtUp, key } = await open(1440, 900);

const CUTOFF = "data/bass/cutoff.json";
const lane = async (k) => (await state()).timeline.tracks.find((t) => t.module === "bass")?.lanes.find((l) => l.key === k);
/** The drawing of the first clip of lane `addr` ("bass.cutoff"): its box on screen, its polyline's vertices, its points' centres. */
const drawing = (addr) =>
  page.evaluate((addr) => {
    const svg = document.querySelector(`[data-lane="${addr}"] [data-clip] svg[data-curve]`);
    if (!svg) return null;
    const r = svg.getBoundingClientRect();
    const line = svg.querySelector("polyline")?.getAttribute("points")?.trim().split(/\s+/) ?? [];
    const points = [...svg.closest("[data-clip]").querySelectorAll("[data-point]")].map((p) => {
      const b = p.getBoundingClientRect();
      return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    });
    return { top: r.top, left: r.left, width: r.width, height: r.height, vertices: line.length, points };
  }, addr);

// ---------------------------------------------------------------- D3: a lane in Hz on a log scale

// Bass's Cutoff lane (`lane.control("cutoff", "hz", hz(500))`): 30 Hz, 1 kHz half way, 15 kHz.
const cutoff = JSON.parse(read(CUTOFF));
const id = cutoff.clips[0].id;
write(CUTOFF, JSON.stringify({ unit: "hz", clips: [{ id, start: 0, length: 16, points: [[0, 30], [8, 1000], [16, 15000]] }] }));
check(await until(async () => JSON.stringify((await lane("cutoff"))?.clips[0]?.points) === "[[0,30],[8,1000],[16,15000]]"), "Bass's Cutoff lane, in Hz, has points at 30 Hz, 1 kHz and 15 kHz");
const base = read(CUTOFF);
// The page draws, and Draw mode paints on, the points as the page shows them: the outside write's, once it has it.
await caughtUp();
// Bass's automation, shown by its header's chevron.
await page.click('[data-fold="bass"]');
check(await until(async () => !!(await drawing("bass.cutoff"))), "the chevron shows Bass's automation lanes");
await sleep(300);
const d = await drawing("bass.cutoff");
const LOG = (v) => Math.log(v / 20) / Math.log(1000);
const want = d.top + (1 - LOG(1000)) * d.height;
check(Math.abs(d.points[1].y - want) <= 1.5, `1 kHz is drawn where a log scale over 20 Hz to 20 kHz puts it: ${d.points[1].y.toFixed(1)} (log ${want.toFixed(1)}, linear over the points ${(d.top + (1 - (1000 - 30) / 14970) * d.height).toFixed(1)})`);
check(Math.abs(d.points[0].y - (d.top + (1 - LOG(30)) * d.height)) <= 1.5 && Math.abs(d.points[2].y - (d.top + (1 - LOG(15000)) * d.height)) <= 1.5, "and 30 Hz and 15 kHz too");
check(d.vertices >= 33, `the line between them curves, as it plays straight in value: ${d.vertices} vertices (3 straight)`);
const drop = await drawing("bass.drop");
check(drop?.vertices === 4, `the Drop lane (a 0–1 amount) draws as before: straight, ${drop?.vertices} vertices for its 4 points`);

// A point painted at half height: about √(20 × 20000) ≈ 632 Hz.
await page.keyboard.press("b");
const x = d.left + (12 / 16) * d.width, y = d.top + d.height / 2;
await page.mouse.click(x, y);
const painted = async () => JSON.parse(read(CUTOFF)).clips[0].points.find((p) => p[0] === 12)?.[1];
check(await until(async () => (await painted()) !== undefined), "Draw mode paints a point at beat 12");
const v = await painted();
const at = 20 * 1000 ** (1 - (y - d.top) / d.height);
check(Math.abs(v - at) / at < 0.05 && Math.abs(v - 632) < 60, `at half height it writes about 632 Hz: ${v} (the log scale at that pixel: ${at.toFixed(0)}; linear would be about ${(30 + 15000) / 2})`);
check(Number(v.toPrecision(3)) === v, `rounded to 3 figures, as a person sets a frequency: ${v}`);
await page.keyboard.press("b");
await key("Meta+z");
const points = () => JSON.stringify(JSON.parse(read(CUTOFF)).clips[0].points);
check(await until(() => points() === JSON.stringify(JSON.parse(base).clips[0].points)), `⌘Z takes the point back: ${points()}`);

write(CUTOFF, JSON.stringify(cutoff, null, 0));
await until(async () => JSON.stringify((await lane("cutoff"))?.clips[0]?.points) === JSON.stringify(cutoff.clips[0].points));

// ---------------------------------------------------------------- D4: hide one automation lane

const VIEW = ".music2/view.json";
const viewHidden = () => {
  try {
    return JSON.parse(read(VIEW)).hidden ?? null;
  } catch {
    return null;
  }
};
const rowOf = (addr) => page.$(`[data-lane="${addr}"]`);
const line = () => page.$eval('[data-testid=hidden-lanes][data-module="bass"]', (e) => e.textContent).catch(() => null);
const menuRows = () => page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => e.dataset.menuItem));

const head = await (await rowOf("bass.drop")).$("div");
const hb = await head.boundingBox();
await page.mouse.click(hb.x + 40, hb.y + hb.height / 2, { button: "right" });
await until(() => page.$("[data-testid=clip-menu]").then(Boolean), 2000);
const rows = await menuRows();
check(rows.join() === "unautomate,hide-lane,hide", `the Drop lane's header menu: Remove automation, Hide this lane, Hide automation (${rows})`);
await page.click('[data-testid=clip-menu] [data-menu-item="hide-lane"]').catch(() => {});
check(await until(async () => !(await rowOf("bass.drop")) && !!(await rowOf("bass.cutoff"))), "Hide this lane takes Drop's row away, and only it");
check(await until(async () => (await line()) === "1 hidden lane · Show"), `a line under Bass's lanes says so: ${await line()}`);
const lineBox = await page.$eval('[data-testid=hidden-lanes][data-module="bass"]', (e) => {
  const r = e.getBoundingClientRect();
  const cut = document.querySelector('[data-lane="bass.cutoff"]').getBoundingClientRect();
  return { h: r.height, under: Math.abs(r.top - cut.bottom) < 1 };
});
check(lineBox.h === 20 && lineBox.under, `20 px tall, right under the Cutoff lane (${JSON.stringify(lineBox)})`);
check(await until(() => JSON.stringify(viewHidden()) === '["bass.drop"]'), `kept in .music2/view.json: hidden ${JSON.stringify(viewHidden())}`);
// The taste lint (e2e/lintlib.mjs, as taste.mjs runs it) over the line.
const bad = await page.evaluate(lint, '[data-testid=hidden-lanes][data-module="bass"]');
check(bad.length === 0, `the line passes the taste lint${bad.length ? `: ${bad.join("; ")}` : ""}`);

// Folded, the line goes with the lanes (X44); unfolded (or A), Drop stays hidden.
await page.click('[data-fold="bass"]');
check(await until(async () => !(await rowOf("bass.cutoff")) && !(await line())), "folded, the lanes and the line go");
await page.click('[data-fold="bass"]');
check(await until(async () => !!(await rowOf("bass.cutoff")) && !(await rowOf("bass.drop")) && (await line()) !== null), "shown again, Drop is still hidden");

// Show brings it back.
await page.click('[data-testid=hidden-lanes][data-module="bass"] [data-testid=show-hidden]');
check(await until(async () => !!(await rowOf("bass.drop")) && !(await line())), "Show brings Drop back, and the line goes");
check(await until(() => viewHidden() === null), "view.json keeps no hidden lane");

// Asked for from the Device view: Automate on Bass's Drive knob makes a lane; hidden, "Show Drive
// lane" on the input it now drives brings it back (both say a lane is wanted: laneMade).
await page.click('[data-module-tab="bass"]');
const knob = '[data-module="bass"] [data-knob="drive"]';
await page.waitForSelector(knob, { timeout: 5000 });
await page.$eval(knob, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(150);
const kb = await page.$(knob).then((e) => e.boundingBox());
await page.mouse.click(kb.x + Math.min(30, kb.width / 2), kb.y + kb.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu]", { timeout: 3000 });
await page.click('[data-testid=knob-menu] [data-menu-item="automate"]');
check(await until(async () => !!(await rowOf("bass.drive")), 8000), "Automate on Bass's Drive makes a Drive lane, shown");
const hd = await (await (await rowOf("bass.drive")).$("div")).boundingBox();
await page.mouse.click(hd.x + 40, hd.y + hd.height / 2, { button: "right" });
await until(() => page.$("[data-testid=clip-menu]").then(Boolean), 2000);
await page.click('[data-testid=clip-menu] [data-menu-item="hide-lane"]');
check(await until(async () => !(await rowOf("bass.drive")) && (await line()) === "1 hidden lane · Show"), "hidden, the line says so");
const input = '[data-module="bass"] [data-lane-input="bass.drive"]';
await page.waitForSelector(input, { timeout: 5000 }).catch(() => null);
await page.$eval(input, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
await sleep(150);
const ib = await page.$(input).then((e) => e?.boundingBox());
if (ib) {
  await page.mouse.click(ib.x + Math.min(40, ib.width / 2), ib.y + ib.height / 2, { button: "right" });
  await until(() => page.$("[data-testid=clip-menu]").then(Boolean), 2000);
  await page.click('[data-testid=clip-menu] [data-menu-item="show"]').catch(() => {});
}
check(await until(async () => !!(await rowOf("bass.drive")) && !(await line())), "Show Drive lane, on the input Drive drives in the Device view, shows the hidden lane");
check(await until(() => viewHidden() === null), "and view.json keeps it hidden no more");

// Kept with the song: hidden, closed and opened again, still hidden.
const hb3 = await (await (await rowOf("bass.drop")).$("div")).boundingBox();
await page.mouse.click(hb3.x + 40, hb3.y + hb3.height / 2, { button: "right" });
await until(() => page.$("[data-testid=clip-menu]").then(Boolean), 2000);
await page.click('[data-testid=clip-menu] [data-menu-item="hide-lane"]');
check(await until(() => JSON.stringify(viewHidden()) === '["bass.drop"]'), "hidden again, and kept");
const proj = (await state()).dir;
await api("/close", { method: "POST", body: "{}" });
await sleep(300);
check((await api("/open", { method: "POST", body: JSON.stringify({ path: proj }) })).status === 200, "the song closes and opens again");
await page.waitForSelector('[data-fold="bass"]', { timeout: 10000 });
if (!(await rowOf("bass.cutoff"))) await page.click('[data-fold="bass"]');
check(await until(async () => !!(await rowOf("bass.cutoff")) && !(await rowOf("bass.drop")) && (await line()) === "1 hidden lane · Show"), "Drop is still hidden, the line says so");

await browser.close();
done(errors);
