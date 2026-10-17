// 8b.5 Automation drawn in place (design-8b §7.5, taste.md TL-6, KN-2), U7: Automate on Kick's
// Decay makes a lane named after the control ("Decay", `decay`); a double-click on its empty space
// makes a clip holding the lane's value; on the curve a click adds a point and a drag moves it, ⌫
// deletes it, and Draw mode (B) paints; a click on the clip's body away from the curve picks the
// clip and adds no point; Remove automation, from the lane's header or from the input it drives in
// the Device view, puts a value back in the code and takes the lane's file away, with a toast, and
// ⌘Z brings both back; Show lane unfolds the track on the lane. Each edit is one gesture and one
// undo step. And a PC page's words.
//
//   node e2e/s8-automation.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, toasts, key, box, show } = await open(1440, 900);
const PPB = 24;
const exists = (f) => {
  try {
    read(f);
    return true;
  } catch {
    return false;
  }
};
const status = async () => (await api("/status")).body;
const lane = async () => (await state()).timeline.tracks.find((t) => t.module === "kick")?.lanes.find((l) => l.key === "decay");
const clips = async () => (await lane())?.clips ?? [];
const gestures = (from) => posts.slice(from).filter((p) => p.url === "/api/graph/clips").map((p) => JSON.parse(p.body).gesture);
const undoNow = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const menuRows = () => page.$$eval("[data-testid=clip-menu] [role=menuitem]", (els) => els.map((e) => `${e.textContent}${e.disabled ? "(off)" : ""}`));
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 5000);
const orig = read("tracks/kick.ts");

// ---------------------------------------------------------------- Automate names the lane after its control
await show("kick");
const knob = '[data-module="kick"] [data-knob="decay"]';
await page.waitForSelector(knob);
await page.$eval(knob, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(150);
const k = await box(knob);
await page.mouse.click(k.x + 30, k.y + k.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu]");
await page.click('[data-testid=knob-menu] [data-menu-item="automate"]');
check(await until(() => read("tracks/kick.ts").includes('lane.control("decay"') && exists("data/kick/decay.json")), `Automate on Kick's Decay writes a lane named after the control: decay (${read("tracks/kick.ts").match(/lane\.control\([^)]*\)/)?.[0]})`);
check(await toasted("Automating decay in a new lane"), "a toast says so");
check(await until(async () => !!(await page.$('[data-lane="kick.decay"]'))), "the timeline unfolds Kick on the new lane");
const header = await page.$eval('[data-lane="kick.decay"] > div:first-child', (e) => e.innerText.split("\n")[0]);
check(header === "Decay", `its header says the control's name: ${header}`);
const automated = read("tracks/kick.ts");

// ---------------------------------------------------------------- the input it drives, in the Device view
check(await until(async () => !!(await page.$('[data-module="kick"] [data-lane-input="kick.decay"]'))), "in the Device view, Kick's Decay input follows the lane");
const input = await box('[data-module="kick"] [data-lane-input="kick.decay"]');
const inputTitle = await page.$eval('[data-module="kick"] [data-lane-input="kick.decay"]', (e) => e.title);
check(inputTitle.startsWith("Follows the Decay lane"), `its tooltip says so: ${inputTitle}`);
await page.mouse.click(input.x + 40, input.y + input.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const inputMenu = await menuRows();
check(inputMenu.join(" | ") === "Show Decay lane | Remove automation", `its menu: ${inputMenu.join(" | ")}`);
await key("Escape");

// ---------------------------------------------------------------- a double-click makes a clip holding the lane's value
const row = await box('[data-lane="kick.decay"] > div:last-child');
const L0 = await lane();
let n = posts.length;
await page.mouse.click(row.x + 5 * PPB, row.y + row.height / 2, { clickCount: 2 });
check(await until(async () => (await clips()).length === 1), "a double-click on the lane's empty space makes a clip");
const c0 = (await clips())[0];
const v0 = L0.default;
check(c0.start === 4 && c0.length === 4 && JSON.stringify(c0.points) === JSON.stringify([[0, v0], [4, v0]]), `a bar long, at the bar clicked, holding the lane's value: ${JSON.stringify(c0)}`);
check(gestures(n).map((g) => g.op).join() === "create", "one create gesture");
const id = c0.id;
const clipSel = `[data-lane="kick.decay"] [data-clip="${id}"]`;
check(await until(async () => !!(await page.$(`${clipSel}[data-picked]`))), "the new clip is picked");

/** Where value `v` at beat `b` of the clip is drawn, on screen. */
const where = async (b, v) => {
  const r = await box(`${clipSel} svg[data-curve]`);
  const { min, max } = await lane();
  const level = (v - min) / (max - min || 1);
  return { x: r.x + (b / 4) * r.width, y: r.y + (1 - level) * r.height, r };
};

// ---------------------------------------------------------------- a click on the body away from the curve picks the clip
await key("Escape");
await until(async () => !(await page.$(`${clipSel}[data-picked]`)));
const flat = await where(2, v0);
// The clip's strip above or below its drawing, whichever is farther from the curve.
const cb = await box(clipSel);
const away = flat.y - cb.y > cb.y + cb.height - flat.y ? cb.y + 2 : cb.y + cb.height - 2;
n = posts.length;
await page.mouse.click(flat.x, away);
check(await until(async () => !!(await page.$(`${clipSel}[data-picked]`))), `a click on the body ${Math.abs(away - flat.y).toFixed(0)} px from the curve picks the clip`);
await sleep(300);
check(gestures(n).length === 0 && (await clips())[0].points.length === 2, "and adds no point");

// ---------------------------------------------------------------- on the curve, a click adds a point
n = posts.length;
await page.mouse.click(flat.x, flat.y);
check(await until(async () => (await clips())[0]?.points.length === 3), "a click on the curve adds a point");
const p3 = (await clips())[0].points;
check(JSON.stringify(p3[1]) === JSON.stringify([2, v0]), `on the grid where it was clicked, on the line: ${JSON.stringify(p3)}`);
check(gestures(n).map((g) => g.op).join() === "points" && gestures(n)[0].clip.id === id, "one points gesture naming the clip");
check(await until(async () => (await page.$$eval(`${clipSel} [data-point][data-picked]`, (els) => els.map((e) => e.dataset.point))).join() === "1"), "the new point is picked");
check(!(await page.$(`${clipSel}[data-picked]`)), "and not the clip");
check(await until(async () => (await undoNow()).startsWith("Undo adding a Decay point")), `Undo names it: ${await undoNow()}`);

// ---------------------------------------------------------------- a drag moves it
const from = await where(2, v0);
const target = { x: from.x + PPB, y: from.r.y + 2 };
n = posts.length;
await page.mouse.move(from.x, from.y);
await page.mouse.down();
await page.mouse.move(target.x, target.y, { steps: 8 });
const bubble = await page.$eval("[data-testid=clip-bubble]", (e) => e.textContent).catch(() => null);
await page.mouse.up();
check(await until(async () => (await clips())[0].points[1][0] === 3 && (await clips())[0].points[1][1] > v0), `a drag moves the point a beat on and up: ${JSON.stringify((await clips())[0].points[1])}`);
check(!!bubble && / · 2\.4\.1$/.test(bubble), `while dragged, a bubble says its value and where: ${bubble}`);
check(gestures(n).length === 1 && (await clips())[0].points.length === 3, "one gesture, still three points");

// ---------------------------------------------------------------- ⌫ deletes it
n = posts.length;
await key("Backspace");
check(await until(async () => (await clips())[0]?.points.length === 2), "⌫ deletes the picked point");
check(gestures(n).map((g) => g.op).join() === "points" && (await clips()).length === 1, "as one points gesture; the clip stays");
check(await until(async () => (await undoNow()).startsWith("Undo deleting a Decay point")), `Undo names it: ${await undoNow()}`);

// ---------------------------------------------------------------- Draw mode paints
await key("b");
const top = await where(0.2, (await lane()).max);
const bottom = await where(3.9, (await lane()).min);
n = posts.length;
await page.mouse.move(top.x, top.r.y + 1);
await page.mouse.down();
await page.mouse.move(bottom.x, bottom.r.y + bottom.r.height - 1, { steps: 16 });
await page.mouse.up();
await key("b");
check(await until(async () => (await clips())[0].points.length >= 6), `B, then a drag across the clip paints a point on each line of the grid: ${(await clips())[0].points.length} points`);
const painted = (await clips())[0].points;
const { min, max } = await lane();
check(painted[0][1] > painted[painted.length - 1][1] && painted.every((p, i) => i === 0 || p[0] > painted[i - 1][0]), `from high to low, in order: ${JSON.stringify(painted)}`);
check(painted.every((p) => p[1] >= min && p[1] <= max && Number.isInteger(p[0] * 2)), "each on an eighth, within the lane's range");
check(gestures(n).length === 1, "in one gesture");
check(await until(async () => (await undoNow()).startsWith("Undo drawing Decay automation")), `Undo names it: ${await undoNow()}`);
await page.screenshot({ path: `${ROOT}/s8-automation.png`, clip: { x: 0, y: 0, width: 760, height: 400 }, captureBeyondViewport: false });

// ---------------------------------------------------------------- Remove automation, from the lane's header
// The playhead before the clip, where the lane plays its fallback: that value goes back.
const ruler = await box("[data-testid=ruler]");
await page.mouse.click(ruler.x + 2, ruler.y + ruler.height / 2);
await until(async () => (await status()).beat === 0);
const lanefile = read("data/kick/decay.json");
const rev = (await state()).view.revision;
const headerBox = await box('[data-lane="kick.decay"] > div:first-child');
await page.mouse.click(headerBox.x + 60, headerBox.y + headerBox.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const headerMenu = await menuRows();
check(headerMenu.join(" | ") === "Remove automation | Hide automationA", `the lane's header has its menu: ${headerMenu.join(" | ")}`);
await page.click('[data-testid=clip-menu] [data-menu-item="unautomate"]');
// The playhead is before the clip: the lane plays its fallback there, and that goes back.
check(await until(() => !exists("data/kick/decay.json") && !read("tracks/kick.ts").includes("lane.control")), "Remove automation takes the lane's file away and its read out of the code");
const back = read("tracks/kick.ts");
check(back.includes("decay: ms(320)") || back.includes("decay: 0.32"), `the value it played at the playhead is written in its place: ${back.match(/decay:[^,}]*/)?.[0]}`);
check(await toasted("Removed the Decay automation"), "a toast says so");
check((await state()).view.revision === rev + 1 && !(await page.$('[data-lane="kick.decay"]')), "one revision; the lane is gone from the timeline");
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === automated && read("data/kick/decay.json") === lanefile), "⌘Z brings the code and the lane file back");

// ---------------------------------------------------------------- Show lane, Remove automation from the Device view
await until(async () => !!(await page.$('[data-lane="kick.decay"]')));
await page.click('[data-fold="kick"]');
check(await until(async () => !(await page.$('[data-lane="kick.decay"]'))), "Kick's automation folded");
const input2 = await box('[data-module="kick"] [data-lane-input="kick.decay"]');
await page.mouse.click(input2.x + 40, input2.y + input2.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
await page.click('[data-testid=clip-menu] [data-menu-item="show"]');
check(await until(async () => !!(await page.$('[data-lane="kick.decay"].lit'))), "Show Decay lane unfolds Kick and lights the lane");
await page.mouse.click(input2.x + 40, input2.y + input2.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
await page.click('[data-testid=clip-menu] [data-menu-item="unautomate"]');
check(await until(() => !exists("data/kick/decay.json") && !read("tracks/kick.ts").includes("lane.control")), "Remove automation from the input does the same");
check(await until(async () => !!(await page.$('[data-module="kick"] [data-knob="decay"]'))), "and the Decay knob is back in the Device view");
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === automated && exists("data/kick/decay.json")), "⌘Z brings it back again");
void orig;

// ---------------------------------------------------------------- a PC page says Alt
const pc = await open(1440, 900, { pc: true });
await pc.page.click('[data-fold="kick"]').catch(() => {});
await pc.page.waitForSelector('[data-lane="kick.decay"] [data-clip]');
const words = await pc.page.$eval('[data-lane="kick.decay"] [data-clip]', (e) => e.title);
check(words.includes("Click the line to add a point") && words.includes("Alt-drag to copy") && !words.includes("⌥"), `a PC page's automation clip says Alt: ${words.replace(/\n/g, " | ")}`);
await pc.browser.close();

await browser.close();
done([...errors, ...pc.errors]);
