// Polish lane D, D3 and D4 (the polish plan §6, X17): automation on its own scale (a lane in Hz
// draws on a log scale over 20 Hz to 20 kHz, widened to its points, its lines curved as they play;
// other units as before) and hiding one automation lane.
//
//   node e2e/pl-lanes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, key } = await open(1440, 900);

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

await browser.close();
done(errors);
