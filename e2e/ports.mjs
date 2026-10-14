// Node ports, port-to-port edges, cables dragged from an output circle, and the effect browser.
// node ports.mjs <scratch> [home] [project]  (defaults: <scratch>/testhome, <scratch>/proj, as run.mjs sets up)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const H = process.argv[3] ?? `${S}/testhome`;
const proj = process.argv[4] ?? `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "macos" };
  try { localStorage.setItem("m2.graph.panel", "560"); } catch {}
}, sess.token);
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers: { "x-m2-token": sess.token } })).json();

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 5000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const read = (f) => fs.readFileSync(`${proj}/${f}`, "utf8");
const center = async (sel) => {
  const el = await page.waitForSelector(sel);
  await el.evaluate((e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await el.boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
// Click once nothing (a toast) covers it.
const press = async (sel) => {
  await page.waitForSelector(sel);
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  await until(() => page.$eval(sel, (e) => { const r = e.getBoundingClientRect(); return e.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)); }), 10000);
  await page.click(sel);
};
const clear = async () => page.$eval("[data-effect-browser] input", (e) => e.select()).then(() => page.keyboard.press("Backspace"));
const show = async (name) => {
  await page.click(`[data-module-tab="${name}"]`);
  await page.waitForSelector(`[data-module="${name}"]`);
};
const options = () => page.$$eval("[data-effect-browser] [data-effect]", (els) => els.map((e) => e.dataset.effect));
const selected = () => page.$eval("[data-effect-browser] [aria-selected=true]", (e) => e.dataset.effect).catch(() => null);

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });
const keysOriginal = read("tracks/keys.ts"), bassOriginal = read("tracks/bass.ts");

// ---------------------------------------------------------------- ports
await show("bass");
const view = (await api("graph")).view;
const bass = view.modules.find((m) => m.key === "bass");
const counts = [];
let countsMatch = true;
for (const n of bass.nodes) {
  const [ins, outs] = await page.$eval(`[data-node-id="${n.id}"]`, (e) => [e.querySelectorAll('[data-port="in"]').length, e.querySelectorAll('[data-port="out"]').length]);
  counts.push(`${n.label} ${ins}/${outs}`);
  countsMatch &&= ins === n.inputs.length && outs === n.outputs.length;
}
check(countsMatch, `each Bass node draws one circle per input and output (${counts.join(", ")})`);
const ports = async (label) => page.$eval(`[data-module="bass"] [data-node="${label}"]`, (e) => [e.querySelectorAll('[data-port="in"]').length, e.querySelectorAll('[data-port="out"]').length]);
// Since step 5 the drive the bass call leaves out is an input too, a knob at its default (PKG-3).
check(JSON.stringify(await ports("xrange")) === "[3,1]" && JSON.stringify(await ports("blend")) === "[3,1]" && JSON.stringify(await ports("bass")) === "[3,1]", "xrange and blend have 3 inputs and 1 output, bass 3 and 1");
check(JSON.stringify(await ports("notes")) === "[0,1]" && JSON.stringify(await ports("out")) === "[1,0]", "the notes lane only gives, the export only takes");
const kindsOf = (sel) => page.$$eval(sel, (els) => els.map((e) => e.dataset.kind).join(","));
check(
  (await kindsOf('[data-module="bass"] [data-node="xrange"] [data-port="in"]')) === "control,control,control" &&
    (await kindsOf('[data-module="bass"] [data-node="bass"] [data-port]')) === "notes,control,control,audio",
  "circles carry their kind: bass takes notes and two controls and gives audio",
);
const legend = await page.$$eval('[data-module="bass"] [data-legend-kind]', (els) => els.map((e) => e.dataset.legendKind));
check(JSON.stringify(legend) === '["audio","control","notes"]', `a legend names the kinds (${legend})`);

// Every edge runs from its output circle's centre to its input circle's centre, in the kind's colour.
const geometry = await page.$$eval('[data-module="bass"] svg path[data-from]', (paths) =>
  paths.map((p) => {
    const box = p.ownerSVGElement.getBoundingClientRect();
    const n = p.getAttribute("d").match(/-?[\d.]+/g).map(Number);
    const at = (sel) => {
      const r = document.querySelector(sel)?.getBoundingClientRect();
      return r && { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    };
    const out = at(`[data-node-id="${p.dataset.from}"] [data-port="out"][data-port-id="${p.dataset.fromPort}"]`);
    const into = at(`[data-node-id="${p.dataset.to}"] [data-port="in"][data-port-id="${CSS.escape(p.dataset.toPort)}"]`);
    const start = { x: box.left + n[0], y: box.top + n[1] }, end = { x: box.left + n[n.length - 2], y: box.top + n[n.length - 1] };
    const d = (a, b) => (a && b ? Math.hypot(a.x - b.x, a.y - b.y) : Infinity);
    return { kind: p.dataset.kind, stroke: p.getAttribute("stroke"), dOut: d(start, out), dIn: d(end, into) };
  }),
);
check(geometry.length === 8, `Bass draws 8 edges (${geometry.length})`);
check(geometry.every((g) => g.dOut < 1.5 && g.dIn < 1.5), `each edge starts on its output circle and ends on its input circle (${geometry.map((g) => `${g.dOut.toFixed(1)}/${g.dIn.toFixed(1)}`).join(" ")})`);
const colours = new Map(geometry.map((g) => [g.kind, g.stroke]));
check(colours.size === 3 && new Set(colours.values()).size === 3, `edges are coloured by kind (${[...colours].join(" ")})`);
const lfoOut = await page.$eval('[data-module="bass"] [data-node="lfo"] [data-port="out"]', (e) => e.getAttribute("title") ?? "");
check(lfoOut.includes("name it in code first") && lfoOut.includes("lfo(…)"), `lfo's unnamed output explains how to connect it: "${lfoOut}"`);

// ---------------------------------------------------------------- the effect browser
await show("keys");
const plus = '[data-module="keys"] button[aria-label="Add an effect after pluck"]';
await page.click(plus);
await page.waitForSelector("[data-effect-browser]");
check(await page.$eval("[data-effect-browser] input", (e) => e === document.activeElement), "the browser opens with the search box focused");
const all = await options();
const groups = await page.$$eval("[data-effect-browser] [data-category]", (els) => els.map((e) => e.dataset.category));
check(all.includes("lowpass") && all.includes("delay") && all.includes("gain"), `it lists every effect (${all.join(", ")})`);
check(groups[0] === "Filter" && groups.includes("Delay & Reverb") && groups.includes("Utility"), `grouped by category (${groups.join(", ")})`);
check((await page.$eval('[data-effect-browser] [data-effect="lowpass"]', (e) => e.textContent)).includes("Darker"), "with descriptions");
await sleep(400);
await page.screenshot({ path: `${S}/ports-browser.png` });
await page.keyboard.type("low");
await sleep(100);
const low = await options();
check(low[0] === "lowpass" && !low.includes("delay") && !low.includes("drive"), `typing "low" filters to ${low.join(", ")}`);
check((await selected()) === "lowpass", "and selects the best match");
await page.keyboard.type("pss");
await sleep(100);
check(JSON.stringify(await options()) === '["lowpass"]', `fuzzy: "lowpss" still finds lowpass (${await options()})`);
await clear();
await page.keyboard.type("echo");
check((await options())[0] === "delay", `the description matches too: "echo" finds ${await options()}`);
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$("[data-effect-browser]"))), "Escape closes it");
check(await page.$eval(plus, (e) => e === document.activeElement), "and gives the focus back to +");

await page.click(plus);
await page.waitForSelector("[data-effect-browser]");
await page.keyboard.type("pass");
const passes = await options();
await page.keyboard.press("ArrowDown");
const second = await selected();
await page.keyboard.press("ArrowUp");
check(second === passes[1] && (await selected()) === passes[0], `arrows move the selection (${passes.join(", ")}: ${second}, back to ${passes[0]})`);
await clear();
await page.keyboard.type("lowp");
await sleep(400);
await page.screenshot({ path: `${S}/ports-browser-search.png` });
await page.keyboard.press("Enter");
check(await until(() => read("tracks/keys.ts").includes('lowpass(pluck(lane.notes("chords")), 1000)')), "Enter puts lowpass after pluck, its cutoff written as 1000");
check(read("tracks/keys.ts").includes('import { lowpass } from "std/filter";'), "and imports it from std/filter");
const cutoff = '[data-module="keys"] [data-node="lowpass"] [data-knob="cutoff"]';
check(await until(async () => (await page.$(cutoff)) && (await page.$eval(cutoff, (e) => e.dataset.value)) === "1000"), "the lowpass node shows a cutoff knob at 1000");
const keys = (await api("graph")).view.modules.find((m) => m.key === "keys");
const id = (label) => keys.nodes.find((n) => n.label === label)?.id;
check(!!(await page.$(`[data-module="keys"] path[data-from="${id("pluck")}"][data-to="${id("lowpass")}"]`)) && !!(await page.$(`[data-module="keys"] path[data-from="${id("lowpass")}"][data-to="export:out"]`)), "edges run pluck → lowpass → out");
await sleep(400);
await page.screenshot({ path: `${S}/ports-keys.png` });
await press('[data-module="keys"] [data-node="lowpass"] button[aria-label^="Remove "]');
check(await until(() => read("tracks/keys.ts").includes('({ lane }) => pluck(lane.notes("chords"))')), "removing it unwraps pluck");
check(await until(async () => !(await page.$('[data-module="keys"] [data-node="lowpass"]'))), "and the node is gone");
await page.click('button[aria-label="Undo"]');
await until(() => read("tracks/keys.ts").includes("lowpass("));
await page.click('button[aria-label="Undo"]');
check(await until(() => read("tracks/keys.ts") === keysOriginal), "undoing both restores keys.ts exactly");

// ---------------------------------------------------------------- cables from an output circle
await show("bass");
await press('[data-module="bass"] button[aria-label="Add an effect after bass"]');
await page.waitForSelector("[data-effect-browser]");
await page.keyboard.type("lowpass");
await page.keyboard.press("Enter");
const target = '[data-module="bass"] [data-node="lowpass"] [data-knob="cutoff"]';
check(await until(async () => !!(await page.$(target))), "a lowpass after the bass has a cutoff knob");
// Both ends on screen: scroll the panel so the middle between them is centred.
const box = async (sel) => (await page.$(sel)).boundingBox();
const panel = await box("[data-testid=graph]");
const [ra, rb] = [await box('[data-outlet="wobble"]'), await box(`${target} [data-port="in"]`)];
await page.$eval("[data-testid=graph]", (e, dx, dy) => { e.scrollLeft += dx; e.scrollTop += dy; },
  (ra.x + rb.x) / 2 - (panel.x + panel.width / 2), (ra.y + rb.y) / 2 - (panel.y + panel.height / 2));
await sleep(100);
const mid = (r) => ({ x: r.x + r.width / 2, y: r.y + r.height / 2 });
const a = mid(await box('[data-outlet="wobble"]'));
const b = mid(await box(`${target} [data-port="in"]`));
await page.mouse.move(a.x, a.y);
await page.mouse.down();
await page.mouse.move((a.x + b.x) / 2, (a.y + b.y) / 2 + 40, { steps: 5 });
const lit = await page.$eval(`${target} [data-port="in"]`, (e) => ({ scale: getComputedStyle(e).scale, row: getComputedStyle(e.parentElement).backgroundColor }));
check(!!(await page.$("[data-cabling]")) && lit.scale !== "none" && lit.row !== "rgba(0, 0, 0, 0)", `while dragging, knob inputs are highlighted as targets (scale ${lit.scale}, ${lit.row})`);
await page.mouse.move(b.x, b.y, { steps: 5 });
const snapped = await page.$eval("[data-cabling] path[data-cable]", (p) => !p.getAttribute("stroke-dasharray") && p.dataset.cable !== "");
await page.screenshot({ path: `${S}/ports-cable.png` });
await page.mouse.up();
check(snapped, "over the input the cable snaps to it");
check(await until(() => /lowpass\(bass\(lane\.notes\("notes"\), \{ cutoff: blend\(cutoff, wobble, drop\) \}\), wobble\)/.test(read("tracks/bass.ts"))), "dropping on the circle wrote `wobble` as the cutoff");
check(await until(async () => !!(await page.$('[data-module="bass"] [data-node="lowpass"] [data-inlet="cutoff"]'))), "the lowpass shows cutoff ← wobble");
const bass2 = (await api("graph")).view.modules.find((m) => m.key === "bass");
const lp = bass2.nodes.find((n) => n.label === "lowpass"), xr = bass2.nodes.find((n) => n.label === "xrange");
const wire = bass2.port_edges.find((e) => e.from === xr.id && e.to === lp.id);
check(!!wire && wire.to_port.startsWith("inlet:") && !!(await page.$(`[data-module="bass"] path[data-from="${xr.id}"][data-to="${lp.id}"]`)), "an edge runs from wobble's output to the lowpass's cutoff input");
// The inlet's ✕ disconnects it in a tracks/ file too, writing the cutoff's declared default.
const wired = read("tracks/bass.ts");
await press('[data-module="bass"] [data-node="lowpass"] button[aria-label="Disconnect wobble from cutoff"]');
check(await until(() => read("tracks/bass.ts") === wired.replace("drop) }), wobble)", "drop) }), hz(1000))")), "✕ disconnects wobble, writing hz(1000) in its place");
check(await until(async () => !!(await page.$(target)) && (await page.$eval(target, (e) => e.dataset.value)) === "1000"), "and the cutoff knob is back at 1000");
await press('button[aria-label="Undo"]');
check(await until(() => read("tracks/bass.ts") === wired), "undo puts the cable back");
await press('button[aria-label="Undo"]');
check(await until(async () => !!(await page.$(target)) && (await page.$eval(target, (e) => e.dataset.value)) === "1000"), "undoing the connection brings the cutoff knob back");
await press('[data-module="bass"] [data-node="lowpass"] button[aria-label^="Remove "]');
check(await until(() => read("tracks/bass.ts").includes('return bass(lane.notes("notes"), { cutoff: blend(cutoff, wobble, drop) });')), "removing the lowpass leaves the bass as it was");
for (let i = 0; i < 2; i++) {
  const rev = (await api("graph")).view.revision;
  await page.click('button[aria-label="Undo"]');
  await until(async () => (await api("graph")).view.revision > rev);
}
check(await until(() => read("tracks/bass.ts") === bassOriginal), "undoing the removal and the add restores bass.ts exactly");

// ---------------------------------------------------------------- zoom
// Fitted, the whole graph is in the panel; the zoom bar steps it, Fit brings it back.
await show("bass");
const fitted = await page.$eval("[data-testid=graph]", (e) => e.scrollWidth <= e.clientWidth + 1 && e.scrollHeight <= e.clientHeight + 1);
check(fitted, "fitted, the Bass graph fits the panel without scrolling");
const nodeWidth = () => page.$eval('[data-module="bass"] [data-node="bass"]', (e) => e.getBoundingClientRect().width);
const w0 = await nodeWidth();
await page.click('button[aria-label="Zoom in"]');
check(await until(async () => Math.abs((await nodeWidth()) / w0 - 1.2) < 0.02), `zoom in draws the nodes 1.2× larger (${w0.toFixed(0)} → ${(await nodeWidth()).toFixed(0)}px)`);
await page.click('button[aria-label="Zoom out"]');
await page.click('button[aria-label="Zoom out"]');
check(await until(async () => Math.abs((await nodeWidth()) / w0 - 1 / 1.2) < 0.02), "zoom out draws them smaller");
// Zoomed out, a dragged cable still ends on the circle under the pointer.
const before = read("tracks/bass.ts");
const from = mid(await box('[data-outlet="wobble"]'));
const period = '[data-module="bass"] [data-node="lfo"] [data-knob="period"] [data-port="in"]';
const to = mid(await box(period));
await page.mouse.move(from.x, from.y);
await page.mouse.down();
await page.mouse.move(to.x, to.y, { steps: 6 });
const gap = await page.$eval("[data-cabling] path[data-cable]", (p, sel) => {
  const n = p.getAttribute("d").match(/-?[\d.]+/g).map(Number);
  const end = new DOMPoint(n[n.length - 2], n[n.length - 1]).matrixTransform(p.getScreenCTM());
  const r = document.querySelector(sel).getBoundingClientRect();
  return Math.hypot(end.x - (r.left + r.width / 2), end.y - (r.top + r.height / 2));
}, period);
check(gap < 1.5, `zoomed out, the cable snaps onto the input's circle (${gap.toFixed(2)}px off)`);
await page.mouse.move(from.x + 30, from.y - 200, { steps: 4 });
await page.mouse.up();
await sleep(500);
check(read("tracks/bass.ts") === before, "let go away from any input, nothing is written");
await page.click('button[aria-label="Fit to panel"]');
check(await until(async () => Math.abs((await nodeWidth()) - w0) < 0.5), "Fit goes back to the fitted size");

check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
