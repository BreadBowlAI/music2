// Small fixes (next, lane D's D4): the ruler shows markers and bar numbers both, and a tempo change
// never touches its bar's number (a); scrolled to the end of a song longer than the window, its last
// bar numbers show clear of Follow and zoom (b); a
// drum lane's note menu says rows, and its keys are rows alike (c); Shift with the wheel over a
// fader scrolls the strips, never the fader (f). Each new state linted.
//
//   node e2e/nx-nits.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, show, view, box } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const orig = { song: read("song.ts"), arrangement: read("data/arrangement.json") };
const layoutWas = fs.existsSync(`${PROJ}/layout.json`) ? read("layout.json") : null;

/** The lint over what is inside `sel`. */
const linted = async (sel, what, p = page) => {
  const bad = await p.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const overlap = (a, b) => !!a && !!b && a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

// ---------------------------------------------------------------- a: the ruler's rows (Q28)

write("layout.json", JSON.stringify({ ...(layoutWas ? JSON.parse(layoutWas) : { order: [], tracks: {} }), markers: [{ beat: 16, name: "Chorus" }] }, null, 2));
await page.waitForSelector('[data-testid=ruler] [data-marker-name="Chorus"]');
const flag = await box('[data-testid=ruler] [data-marker-name="Chorus"]');
const five = await box('[data-testid=ruler] [data-bar="5"]');
check(!!five && !overlap(flag, five), `the Chorus marker and bar 5's number both show, apart: ${JSON.stringify([flag?.y, flag?.height, five?.y, five?.height])}`);
// The numbers take no pointer, so the point at the middle of "5" is the ruler under it, never a marker.
const hit = five && (await page.evaluate((x, y) => {
  const e = document.elementFromPoint(x, y);
  return e?.closest("[data-marker]") ? `marker ${e.closest("[data-marker]").dataset.markerName}` : e?.dataset.testid ?? e?.tagName ?? null;
}, five.x + five.width / 2, five.y + five.height / 2));
check(hit === "ruler", `nothing of the marker is at the middle of "5": ${hit}`);
const ruler = await box("[data-testid=ruler]");
check(Math.round(ruler?.height ?? 0) === 36, `the ruler is 36 px: ${ruler?.height}`);
check(!!flag && !!ruler && Math.round(flag.y - ruler.y) === 0 && Math.round(flag.height) === 14, `markers keep their 14 px band at the top: ${flag && ruler ? flag.y - ruler.y : "?"}, ${flag?.height}`);

const session = (await state()).session;
const tempo = await post("/tempo", { session, at: 32, bpm: 140, add: true });
check(tempo.status === 200, `a tempo change at bar 9 (${tempo.status})`);
await page.waitForSelector('[data-testid=ruler] [data-mark="32"]');
const nine = await box('[data-testid=ruler] [data-bar="9"]');
const mark = await box('[data-testid=ruler] [data-mark="32"]');
const markText = await page.$eval('[data-testid=ruler] [data-mark="32"]', (e) => e.textContent);
check(markText === "♩140", `its mark says ♩140: ${markText}`);
check(!!nine && !!mark && mark.x - (nine.x + nine.width) >= 3.95, `"9" and "♩140" are at least 4 px apart: ${nine && mark ? Math.round((mark.x - nine.x - nine.width) * 10) / 10 : "?"}`);
check(!!nine && !!mark && Math.abs(nine.y + nine.height / 2 - (mark.y + mark.height / 2)) <= 1, `on one line: ${nine && mark ? [nine.y + nine.height / 2, mark.y + mark.height / 2] : "?"}`);
// Off a bar line, 3 px after its beat.
await post("/tempo", { session, at: 34, bpm: 150, add: true });
await page.waitForSelector('[data-testid=ruler] [data-mark="34"]');
const ppb = await page.evaluate(() => {
  const a = document.querySelector('[data-testid=ruler] [data-bar="1"]').getBoundingClientRect();
  const b = document.querySelector('[data-testid=ruler] [data-bar="5"]')?.getBoundingClientRect();
  return b ? (b.left - a.left) / 16 : null;
});
const off = await box('[data-testid=ruler] [data-mark="34"]');
const r0 = await page.$eval('[data-testid=ruler]', (e) => e.getBoundingClientRect().left);
// Its text 3 px on (its box starts 2 px before its text, for its hover).
check(!!off && !!ppb && Math.abs(off.x + 2 - (r0 + 34 * ppb + 3)) <= 1, `a change off a bar line sits 3 px after its beat: ${off && ppb ? Math.round(off.x + 2 - r0 - 34 * ppb) : "?"}`);
await linted("[data-testid=ruler]", "the ruler with a marker and changes");
write("data/arrangement.json", orig.arrangement);
await until(async () => !(await page.$('[data-testid=ruler] [data-mark="32"]')));

// ---------------------------------------------------------------- b: the end of the ruler (Q29)

// A song longer than the window (a kick clip in bar 39: its room ends with bar 40), zoomed out to
// bars of about 43 px: the timeline scrolls, and scrolled to the end, the song's last bar number
// and its spare bar's both show, clear of Follow and zoom (red without the timeline's TOOLS_W).
const kickWas = read("data/kick/hits.json");
const kicks = JSON.parse(kickWas);
kicks.clips.push({ id: "nitsfar1", start: 152, length: 4, notes: [{ pitch: "C1", start: 0, length: 0.25, velocity: 0.8 }] });
write("data/kick/hits.json", JSON.stringify(kicks));
await until(async () => (await state()).timeline.end >= 156);
const barW = () => page.evaluate(() => {
  const a = document.querySelector('[data-testid=ruler] [data-bar="1"]')?.getBoundingClientRect();
  const b = document.querySelector('[data-testid=ruler] [data-bar="2"]')?.getBoundingClientRect();
  return a && b ? b.left - a.left : 0;
});
for (let i = 0; i < 8 && (await barW()) > 50; i++) {
  await page.click("[data-testid=zoom-out]");
  await sleep(250);
}
check((await barW()) >= 35 && (await barW()) <= 50, `zoomed out to bars of ${Math.round(await barW())} px`);
const over = await page.$eval("[data-testid=timeline]", (e) => [e.scrollWidth, e.clientWidth]);
check(over[0] > over[1], `the timeline scrolls: ${over[0]} > ${over[1]}`);
await page.$eval("[data-testid=timeline]", (e) => (e.scrollLeft = e.scrollWidth));
await sleep(300);
const tools = await box("[data-testid=view-tools]");
const ends = await page.evaluate(() => ["39", "40"].map((n) => document.querySelector(`[data-testid=ruler] [data-bar="${n}"]`)?.getBoundingClientRect().right ?? null));
check(ends.every((r) => r !== null) && !!tools && ends.every((r) => r < tools.x), `scrolled to the end, bar 39's number (the song's last) and bar 40's (its spare) end left of Follow and zoom: ${JSON.stringify(ends.map((r) => (r === null ? "none" : Math.round(r))))} < ${tools ? Math.round(tools.x) : "?"}`);
await page.$eval("[data-testid=timeline]", (e) => (e.scrollLeft = 0));
write("data/kick/hits.json", kickWas);
await until(async () => (await state()).timeline.end < 156);

// ---------------------------------------------------------------- c: a drum lane (Q30)

const added = await post("/track/add", { session, component: "@m2/drums:drum-kit" });
check(added.status === 200, `a Drum kit track is added (${added.status})`);
await until(async () => (await state()).timeline.tracks.some((t) => t.module === "drum-kit"));
const s1 = await state();
const beat = [36, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 }));
await post("/lane", { session: s1.session, revision: s1.view.revision, module: "drum-kit", lane: "hits", clips: [{ id: "kitbeat1", start: 0, length: 4, notes: beat }] });
await until(async () => !!(await page.$('[data-lane="drum-kit.hits"] [data-clip]')), 5000);
const clip = await box('[data-lane="drum-kit.hits"] [data-clip]');
await page.mouse.click(clip.x + Math.min(40, clip.width / 2), clip.y + clip.height / 2, { count: 2 });
await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length > 0, 5000);
const kick = await box('[data-testid=notes] [data-note][data-pitch="36"]');
await page.mouse.click(kick.x + kick.width / 2, kick.y + kick.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=note-menu]");
const rows = await page.$$eval("[data-testid=note-menu] [role=menuitem][data-menu-item]", (els) => els.map((e) => e.dataset.menuItem + ":" + e.textContent));
check(rows.some((r) => r.startsWith("up:Up a row")) && rows.some((r) => r.startsWith("down:Down a row")), `its note menu moves a row up or down: ${rows.slice(0, 2).join(" | ")}`);
check(!rows.some((r) => r.startsWith("octave_")), `and has no octave rows: ${rows.filter((r) => r.startsWith("octave")).join(" | ") || "none"}`);
await linted("[data-testid=note-menu]", "a drum lane's note menu");
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=note-menu]")));
const fills = await page.evaluate(() => {
  document.querySelector('[data-testid=roll-keys] [data-key="61"]').scrollIntoView({ block: "center" });
  const at = (p) => {
    const row = document.querySelector(`[data-testid=roll-keys] [data-key="${p}"]`);
    const b = row.getBoundingClientRect();
    const hit = document.elementFromPoint(b.left + 4, b.top + b.height / 2);
    return { black: !!row.querySelector("[data-black]"), hit: hit === row ? "row" : hit?.dataset?.black !== undefined ? "black" : hit?.tagName, bg: getComputedStyle(hit).backgroundColor };
  };
  return { 60: at(60), 61: at(61) };
});
check(!fills[61].black && fills[61].hit === "row" && fills[61].bg === fills[60].bg, `row 61's key looks as row 60's: ${JSON.stringify(fills)}`);
const shaded = await page.$eval("[data-testid=roll-grid]", (e) => e.dataset.shaded);
check(shaded === "", `the kit's grid shades no rows: "${shaded}"`);
await linted("[data-testid=roll-keys]", "a drum lane's keys");

// ---------------------------------------------------------------- f: Shift with the wheel over a fader (Q32)

/** Shift and the wheel over Hats' fader on `p`, as `delta`: Hats' value and the strips' scroll, before and after. */
async function shiftWheel(p, delta) {
  await p.setViewport({ width: 380, height: 900 });
  await p.click('[data-module-tab="song"]').catch(() => {});
  await p.click('[data-view-tab="mix"]');
  await p.waitForSelector('[data-testid=mix] [data-strip="hats"] [data-fader]');
  await sleep(300);
  const strips = "[data-testid=mix-strips]";
  await p.$eval(strips, (e) => (e.scrollLeft = 0));
  const value = () => p.$eval('[data-testid=mix] [data-strip="hats"] [data-fader]', (e) => Number(e.dataset.value));
  const thumb = await p.$('[data-testid=mix] [data-strip="hats"] [data-testid=fader-thumb]').then((e) => e.boundingBox());
  const before = { value: await value(), left: await p.$eval(strips, (e) => e.scrollLeft), over: await p.$eval(strips, (e) => e.scrollWidth > e.clientWidth) };
  await p.mouse.move(thumb.x + thumb.width / 2, thumb.y + thumb.height / 2);
  await p.keyboard.down("Shift");
  for (let i = 0; i < 3; i++) await p.mouse.wheel(delta);
  await p.keyboard.up("Shift");
  await sleep(800);
  return { before, after: { value: await value(), left: await p.$eval(strips, (e) => e.scrollLeft) } };
}
const unwheeled = read("song.ts");
const pc = await open(1440, 900, { pc: true });
const onPc = await shiftWheel(pc.page, { deltaY: 40 });
check(onPc.before.over && onPc.after.value === onPc.before.value, `a PC page: Shift+wheel up and down leaves Hats' fader: ${onPc.before.value} → ${onPc.after.value}`);
check(onPc.after.left > onPc.before.left, `and scrolls the strips: ${onPc.before.left} → ${onPc.after.left}`);
await linted("[data-testid=mix]", "the Mix view, scrolled", pc.page);
check(pc.errors.length === 0, `no console errors on the PC page ${pc.errors.slice(0, 2).join(" | ")}`);
await pc.browser.close();
const onMac = await shiftWheel(page, { deltaX: 40 });
check(onMac.after.value === onMac.before.value && onMac.after.left > onMac.before.left, `a Mac page: Shift+wheel sideways scrolls the strips, the fader stays: ${JSON.stringify(onMac)}`);
check(read("song.ts") === unwheeled, "the wheel wrote nothing");

if (layoutWas === null) fs.rmSync(`${PROJ}/layout.json`, { force: true });
else write("layout.json", layoutWas);
done(errors);
