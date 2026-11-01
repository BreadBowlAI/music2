// Node ports, port-to-port edges, cables dragged from an output circle, and the effect browser.
// node ports.mjs <scratch> [home] [project]  (defaults: <scratch>/testhome, <scratch>/proj, as run.mjs sets up)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const H = process.argv[3] ?? `${S}/testhome`;
const proj = process.argv[4] ?? `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
import { following } from "./caughtup.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "macos" };
  try { localStorage.setItem("m2.graph.panel", "560"); } catch {}
}, sess.page_token);
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers: { "x-m2-token": sess.page_token } })).json();

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 5000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const caughtUp = following(page, () => api("graph"), check);
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
// No legend on every module (taste.md DK-3): each kind's port is its own shape.
const shapes = await page.$$eval('[data-module="bass"] [data-port-dot]', (els) => {
  const by = {};
  for (const e of els) by[e.dataset.kind] = getComputedStyle(e).borderRadius + " " + getComputedStyle(e).transform;
  return by;
});
check(!(await page.$("[data-legend-kind]")) && new Set(Object.values(shapes)).size === Object.keys(shapes).length && Object.keys(shapes).length === 3, `no legend: the kinds are told by shape (${JSON.stringify(shapes)})`);

// Every edge runs from its output circle's centre to its input circle's centre, drawn as its kind is
// (since the taste pass: by weight and dash, not by colour).
// The nodes are laid out at 1 and drawn at the canvas's zoom.
const geometry = await page.$$eval('[data-module="bass"] svg path[data-from]', (paths) =>
  paths.map((p) => {
    const box = p.ownerSVGElement.getBoundingClientRect();
    const zoom = box.width / p.ownerSVGElement.width.baseVal.value;
    const n = p.getAttribute("d").match(/-?[\d.]+/g).map((x) => Number(x) * zoom);
    const at = (sel) => {
      const r = document.querySelector(sel)?.getBoundingClientRect();
      return r && { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    };
    const out = at(`[data-node-id="${p.dataset.from}"] [data-port="out"][data-port-id="${p.dataset.fromPort}"]`);
    const into = at(`[data-node-id="${p.dataset.to}"] [data-port="in"][data-port-id="${CSS.escape(p.dataset.toPort)}"]`);
    const start = { x: box.left + n[0], y: box.top + n[1] }, end = { x: box.left + n[n.length - 2], y: box.top + n[n.length - 1] };
    const d = (a, b) => (a && b ? Math.hypot(a.x - b.x, a.y - b.y) : Infinity);
    const look = [p.getAttribute("stroke"), p.getAttribute("stroke-width"), p.getAttribute("stroke-dasharray") ?? ""].join(" ");
    return { kind: p.dataset.kind, look, dOut: d(start, out), dIn: d(end, into) };
  }),
);
check(geometry.length === 8, `Bass draws 8 edges (${geometry.length})`);
check(geometry.every((g) => g.dOut < 1.5 && g.dIn < 1.5), `each edge starts on its output circle and ends on its input circle (${geometry.map((g) => `${g.dOut.toFixed(1)}/${g.dIn.toFixed(1)}`).join(" ")})`);
const looks = new Map(geometry.map((g) => [g.kind, g.look]));
check(looks.size === 3 && new Set(looks.values()).size === 3, `edges are drawn by kind (${[...looks].join(" | ")})`);
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
// Remove goes from the revision the page shows, and the undo changed the lowpass call (the knob reads
// 1000 before it too): the page shows the undo first.
await caughtUp();
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
// Fit shrinks it to the panel, but never below 75%, where its text would be too small (taste.md DV-5):
// past that it scrolls. (A chain that fits the panel's height opens at 100% and scrolls sideways.)
const fit = await page.$eval("[data-testid=graph]", (e) => ({
  fits: e.scrollWidth <= e.clientWidth + 1 && e.scrollHeight <= e.clientHeight + 1,
  tall: e.scrollHeight <= e.clientHeight + 1,
  zoom: Number(document.querySelector("[data-zoom]").dataset.zoom),
}));
check(fit.fits || fit.zoom === 0.75 || (fit.zoom === 1 && fit.tall), `fitted, the Bass graph fits the panel, or scrolls at Fit's 75% floor (zoom ${fit.zoom}, fits ${fit.fits})`);
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

// ---------------------------------------------------------------- Fit with scrollbars that take room
// A Mac with a mouse (or "Show scroll bars: Always") has scrollbars that take room from the panel, as
// this browser has without --hide-scrollbars. Fit must not depend on them (DV-5): one fitted to the
// room the scrollbars leave shrinks the graph when its scrollbar shows, then grows it when the
// scrollbar goes, every frame: the graph flickers until the panel's size or the zoom changes. Bass at
// 100% is wider than a 1440 px panel, so a scrollbar shows under it; at 75% it isn't.
const bars = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"], ignoreDefaultArgs: ["--hide-scrollbars"] });
const tab = await bars.newPage();
await tab.setViewport({ width: 1440, height: 900 });
tab.on("console", (m) => m.type() === "error" && errors.push(m.text()));
tab.on("pageerror", (e) => errors.push(String(e)));
await tab.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "macos" };
  try {
    localStorage.setItem("m2.browser.open", "false");
    localStorage.setItem("m2.graph.panel", "300");
  } catch {}
}, sess.page_token);
await tab.goto(sess.url, { waitUntil: "domcontentloaded" });
await tab.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });
await tab.click('[data-module-tab="bass"]');
await tab.waitForSelector('[data-module="bass"]');
const barRoom = () =>
  tab.evaluate(() => {
    const d = document.createElement("div");
    d.style.cssText = "position:absolute;overflow:scroll;width:50px;height:50px";
    document.body.append(d);
    const room = d.offsetWidth - d.clientWidth;
    d.remove();
    return room;
  });
// Where scrollbars float over the page (a Mac with a trackpad), the page gets ones that take room, as with a mouse.
if (!(await barRoom())) {
  await tab.addStyleTag({ content: "* { scrollbar-width: auto !important; scrollbar-color: auto !important } ::-webkit-scrollbar { width: 12px; height: 12px }" });
}
const bar = await barRoom();
// Bass's size at 100% (its nodes' box, before the zoom scales it) and the panel's width.
const at1 = await tab.$eval('[data-module="bass"] [data-canvas] > div > div', (e) => ({
  w: parseFloat(e.style.width),
  h: parseFloat(e.style.height),
  panel: document.querySelector("[data-testid=graph]").offsetWidth,
}));
// The panel's padding and the module's, each side (NodeGraph's CHROME_W, CHROME_H).
const PADS = 64;
check(
  bar > 0 && at1.w + PADS > at1.panel && at1.w * 0.75 + PADS < at1.panel - bar,
  `with scrollbars that take ${bar}px, Bass at 100% is wider than the ${at1.panel}px panel, and at 75% narrower`,
);
// The panel's heights where Bass at 100% fits it only with no scrollbar under it, and a few either side.
const lo = Math.floor(at1.h + PADS) - 2, hi = Math.ceil(at1.h + PADS + bar) + 2;
// Waits until the panel's size and the zoom hold still for half a second (or 4 s pass), and says what they went through.
const settle = () =>
  tab.evaluate(
    () =>
      new Promise((done) => {
        const el = document.querySelector("[data-testid=graph]");
        const states = [];
        const t0 = performance.now();
        let since = t0;
        const frame = () => {
          const now = performance.now();
          const s = `${el.clientWidth}×${el.clientHeight} at ${document.querySelector("[data-zoom]").dataset.zoom}`;
          if (s !== states.at(-1)) (states.push(s), (since = now));
          if (now - since >= 500) return done({ still: true, states });
          if (now - t0 >= 4000) return done({ still: false, states });
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      }),
  );
const split = mid(await (await tab.$("[data-testid=split]")).boundingBox());
const panelH = () => tab.$eval("[data-testid=graph]", (e) => e.offsetHeight);
const h0 = await panelH();
await tab.mouse.move(split.x, split.y);
await tab.mouse.down();
const shaky = [], zooms = [];
for (let h = lo; h <= hi; h++) {
  await tab.mouse.move(split.x, split.y - (h - h0));
  await until(async () => (await panelH()) === h);
  const { still, states } = await settle();
  const got = await panelH();
  if (!still || got !== h) shaky.push(`${h}px${got !== h ? ` (is ${got})` : ""}: ${states.length} states, ${states.slice(-2).join(", ")}`);
  zooms.push(Number(await tab.$eval("[data-zoom]", (e) => e.dataset.zoom)));
}
await tab.mouse.up();
check(shaky.length === 0, `fitted, Bass holds still at every panel height from ${lo} to ${hi}px${shaky.length ? `; not at ${shaky.join("; ")}` : ""}`);
check(zooms.every((z, i) => i === 0 || z >= zooms[i - 1]), `and its zoom never drops as the panel grows (${zooms.join(" ")})`);
await bars.close();

check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
