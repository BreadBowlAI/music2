// Track dress2a's gate strip (CARD-73, CARD-126), apart from dress2a.mjs: on dress2a's song, while it
// plays, the gate's strip along the foot of its levels picture (its meter `open`, a switch, drawn
// from the most since the read before) lights on the hits and goes dark between them, read again
// and again, and lights once for each opening its `down` meter's last 3 s show (one more or less at
// the window's edges, the two pictures' reads not quite at one moment).
// That needs reads shorter than the gate's closed spells: the meters read every 33 ms, a sleep, so
// it is tagged `sleeps` (left out of GitHub's Mac job, whose sleeps run late: every 120 ms there,
// most reads hold an opening and a closing).
//
//   node e2e/dress2a-strip.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, sleep, until } from "./fixlib.mjs";
import { dress2aSong } from "./dress2a-song.mjs";
import { expand } from "./graphlib.mjs";

dress2aSong();

const { page, errors, api, view } = await open(1920, 1080);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const mixa = '[data-testid=graph] [data-module="mixa"]';
const gate = `${mixa} [data-node="gate"]`;
const gatePic = `${gate} [data-views] [data-picture="levels"]`;
const gateDown = `${gate} [data-views] [data-picture="down"]`;

check(await until(async () => !!(await page.$('[data-module-tab="mixa"]')), 30000), "the song has its Mix A track");
await page.click('[data-module-tab="mixa"] [data-testid=track-label]');
await view("graph");
check(await until(async () => !!(await page.$(gate)), 20000) && (await expand(page, `${mixa} [data-node]`)), "Mix A's gate is in the graph, its nodes expanded");
check(
  await until(async () => !!(await page.$(`${gatePic} canvas[data-live]`)) && !!(await page.$(`${gateDown} canvas[data-live]`)), 20000),
  "Mix A's gate draws its levels and how far it turns down, live",
);
await page.$eval(gate, (e) => e.scrollIntoView({ block: "nearest", inline: "center" })).catch(() => {});
await sleep(150);

await post("/loop", { start: 0, end: 8, on: true });
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
/** The `down` meter's last 3 s (its layer 0) and the strip's (the levels picture's layer 3), read at one moment. */
const histories = () =>
  page.evaluate(
    (a, b) => [a, b].map((sel, k) => JSON.parse(document.querySelector(`${sel} canvas[data-live]`)?.dataset.said ?? "[]")[k === 0 ? 0 : 3]?.h ?? []),
    gateDown,
    gatePic,
  );
// Their last 3 s all heard: the song past 3.5 s (a history may start full, at the floor).
await until(async () => (await api("/status")).body.seconds >= 3.5, 20000);
await until(async () => (await histories()).every((h) => h.length === 90), 20000);
const runs = (h, on) => {
  const out = [];
  h.forEach((v, j) => {
    if (!on(v)) return;
    const last = out.at(-1);
    if (last && last[1] === j - 1) last[1] = j;
    else out.push([j, j]);
  });
  return out;
};
// What the strip drew last, read 60 times 100 ms apart: lit on the hits, dark between them.
const lit = [];
for (let i = 0; i < 60; i++) {
  await sleep(100);
  const l = await page.$eval(`${gatePic} canvas[data-live]`, (e) => JSON.parse(e.dataset.said ?? "[]")[3]).catch(() => null);
  if (l) lit.push(l.v);
}
check(lit.includes(1) && lit.includes(0), `the gate's strip lights on the hits and goes dark between them (its meter open, a switch): ${[...new Set(lit)]}`);
const [downH, stripH] = await histories();
const openings = runs(downH, (v) => v < 0.5).length;
const lighted = runs(stripH, (v) => v >= 0.5).length;
check(stripH.length === 90 && lighted >= 6 && Math.abs(lighted - openings) <= 1, `its strip lights once for each opening: ${lighted} lit, ${openings} openings`);
await page.keyboard.press("Space");

done(errors);
