// The std package's new effects, from the browser: reverb, compressor and chorus go after the Keys'
// pluck through the effect browser, each arriving with its settings as knobs, and the file says exactly that.
// node dsp.mjs <scratch>   (the server's HOME and project: <scratch>/testhome, <scratch>/proj, as run.mjs sets up)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const H = process.env.M2_E2E_HOME ?? `${S}/testhome`;
const proj = process.env.M2_E2E_PROJ ?? `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "macos" };
}, sess.token);
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers: { "x-m2-token": sess.token } })).json();

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f().catch(() => false)) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const keysFile = `${proj}/tracks/keys.ts`;
const read = () => fs.readFileSync(keysFile, "utf8");
const original = read();
const keysModule = async () => (await api("graph")).view.modules.find((m) => m.key === "keys");
// Click once nothing (a toast) covers it.
const press = async (sel) => {
  await page.waitForSelector(sel);
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  await until(() => page.$eval(sel, (e) => { const r = e.getBoundingClientRect(); return e.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)); }), 10000);
  await page.click(sel);
};
const options = () => page.$$eval("[data-effect-browser] [data-effect]", (els) => els.map((e) => e.dataset.effect));
const knob = (node, label) => `[data-module="keys"] [data-node="${node}"] [data-knob="${label}"]`;
const knobs = (node) =>
  page.$$eval(`[data-module="keys"] [data-node="${node}"] [data-knob]`, (els) =>
    Object.fromEntries(els.map((e) => [e.dataset.knob, { value: e.dataset.value, shown: e.querySelector(".tabular-nums")?.textContent, editable: !!e.dataset.knobId }])),
  );
/** Add `name` after the node labelled `after` by typing `query` in its browser and pressing Enter. */
const add = async (after, query, name) => {
  const revision = (await api("graph")).view.revision;
  await press(`[data-module="keys"] button[aria-label="Add an effect after ${after}"]`);
  await page.waitForSelector("[data-effect-browser] input");
  await page.keyboard.type(query);
  const first = await until(async () => (await options())[0] === name, 2000);
  check(first, `typing "${query}" offers ${name} first (${await options()})`);
  await page.keyboard.press("Enter");
  await until(async () => (await api("graph")).view.revision > revision);
  return until(async () => !!(await page.$(`[data-module="keys"] [data-node="${name}"]`)));
};

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });
await page.click('[data-module-tab="keys"]');
await page.waitForSelector('[data-module="keys"]');

// ---------------------------------------------------------------- the catalog
const catalog = (await api("graph")).effects;
const by = Object.fromEntries(catalog.map((e) => [e.name, e]));
check(
  // Since step 5 by their descriptors: a ComponentId, and what adding one writes (`insert`).
  ["reverb", "chorus", "phaser", "tremolo", "compress", "eq3", "bitcrush", "widen"].every((n) => by[n]?.module === "std/fx" && by[n].component === `std:${n}` && by[n].insert),
  "the std package offers the eight new effects, each with starting settings",
);
check(
  by.reverb.category === "Delay & Reverb" && by.compress.category === "Dynamics" && by.chorus.category === "Modulation" && by.eq3.category === "EQ" && by.bitcrush.category === "Distortion" && by.widen.category === "Utility",
  "each in its category",
);
await press('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector("[data-effect-browser]");
const groups = await page.$$eval("[data-effect-browser] [data-category]", (els) => els.map((e) => e.dataset.category));
check(JSON.stringify(groups) === JSON.stringify(["Filter", "EQ", "Dynamics", "Distortion", "Delay & Reverb", "Modulation", "Utility"]), `the browser groups them (${groups.join(", ")})`);
const groupOf = (name) => page.$eval(`[data-effect-browser] [data-effect="${name}"]`, (e) => e.closest("[data-category]")?.dataset.category ?? e.parentElement.closest("[data-category]")?.dataset.category).catch(() => null);
const listed = await Promise.all(["reverb", "chorus", "compress"].map(groupOf));
check(listed.join() === "Delay & Reverb,Modulation,Dynamics", `reverb, chorus and compress are listed under theirs (${listed})`);
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-effect-browser]")));

// ---------------------------------------------------------------- reverb after the pluck
const pluck = 'pluck(lane.notes("chords"))';
const reverbArgs = "{ size: 0.6, damping: 0.5, mix: 0.25 }";
check(await add("pluck", "reverb", "reverb"), "Enter adds a reverb node");
const reverbed = `reverb(${pluck}, ${reverbArgs})`;
check(await until(async () => read().includes(`({ lane }) => ${reverbed}`)), `keys.ts wraps the pluck: ${reverbed}`);
check(read().includes('import { reverb } from "std/fx";'), "and imports reverb from std/fx");
const r = await knobs("reverb");
check(r.size?.value === "0.6" && r.damping?.value === "0.5" && r.mix?.value === "0.25" && Object.values(r).every((k) => k.editable), `its settings are live knobs (${JSON.stringify(r)})`);

// ---------------------------------------------------------------- a compressor between the pluck and the reverb
const compArgs = "{ threshold: -18, ratio: 3, attack: 0.01, release: 0.15, makeup: 4 }";
check(await add("pluck", "comp", "compress"), "a compressor goes after the pluck too");
const compressed = `reverb(compress(${pluck}, ${compArgs}), ${reverbArgs})`;
check(await until(async () => read().includes(`({ lane }) => ${compressed}`)), `between the pluck and the reverb: ${compressed}`);
const c = await knobs("compress");
check(
  c.threshold?.shown === "-18 dB" && c.ratio?.value === "3" && c.attack?.shown === "10 ms" && c.release?.shown === "150 ms" && c.makeup?.shown === "4 dB",
  `the compressor's five settings are knobs, with units (${Object.entries(c).map(([k, v]) => `${k} ${v.shown}`).join(", ")})`,
);

// ---------------------------------------------------------------- chorus after the compressor
const chorusArgs = "{ rate: 0.8, depth: 0.5, mix: 0.5 }";
check(await add("compress", "chorus", "chorus"), "a chorus goes after the compressor");
const chained = `reverb(chorus(compress(${pluck}, ${compArgs}), ${chorusArgs}), ${reverbArgs})`;
check(await until(async () => read().includes(`({ lane }) => ${chained}`)), `the chain reads pluck → compress → chorus → reverb: ${chained}`);
const imports = read().split("\n").filter((l) => l.includes('from "std/fx"'));
check(imports.length >= 1 && ["reverb", "compress", "chorus"].every((n) => imports.some((l) => new RegExp(`\\b${n}\\b`).test(l))), `all three are imported from std/fx (${imports.join(" / ")})`);
const without = read().split("\n").filter((l) => !l.includes('from "std/fx"')).join("\n");
check(without === original.split("\n").filter((l) => !l.includes('from "std/fx"')).join("\n").replace(`({ lane }) => ${pluck}`, `({ lane }) => ${chained}`), "nothing else in keys.ts changed");
const ch = await knobs("chorus");
check(ch.rate?.shown === "0.8 Hz" && ch.depth?.value === "0.5" && ch.mix?.value === "0.5", `the chorus's rate, depth and mix are knobs (${Object.entries(ch).map(([k, v]) => `${k} ${v.shown}`).join(", ")})`);
const keys = await keysModule();
const rate = keys.nodes.find((n) => n.label === "chorus").knobs.find((k) => k.label === "rate");
// Its range is the one the chorus declares (PKG-8), not a guess from the unit.
check(rate.unit === "hz" && rate.range?.[0] <= 0.1 && rate.range?.[1] === 20 && rate.log, `the rate knob sweeps a rate, not the audio range (${rate.range?.join("..")} Hz)`);

// The graph is the chain, and the song still compiles and plays it.
const id = (label) => keys.nodes.find((n) => n.label === label)?.id;
const edge = (a, b) => keys.port_edges.some((e) => e.from === id(a) && e.to === (b === "out" ? "export:out" : id(b)));
check(edge("pluck", "compress") && edge("compress", "chorus") && edge("chorus", "reverb") && edge("reverb", "out"), "edges run pluck → compress → chorus → reverb → out");
check(
  !!(await page.$(`[data-module="keys"] path[data-from="${id("compress")}"][data-to="${id("chorus")}"]`)) && !!(await page.$(`[data-module="keys"] path[data-from="${id("reverb")}"][data-to="export:out"]`)),
  "and are drawn",
);
const state = await api("graph");
check(state.view.draft_errors.length === 0 && state.diagnostics.length === 0, "the song compiles");

// ---------------------------------------------------------------- a knob turns the reverb's mix
const mixKnob = knob("reverb", "mix");
await page.$eval(`${mixKnob} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const kb = await (await page.$(`${mixKnob} svg`)).boundingBox();
const kx = kb.x + kb.width / 2, ky = kb.y + kb.height / 2;
await page.mouse.move(kx, ky);
await page.mouse.down();
for (let i = 1; i <= 8; i++) await page.mouse.move(kx, ky - i * 4);
await page.mouse.up();
// 32px of 160 on a 0..1 range: 0.25 → 0.45.
const turned = await until(async () => read().includes("{ size: 0.6, damping: 0.5, mix: 0.45 }"));
check(turned, `dragging the reverb's mix knob up writes mix: 0.45${turned ? "" : ` (${read().match(/\{ size[^}]*\}/)?.[0]}; shown ${await page.$eval(mixKnob, (e) => e.dataset.value)})`}`);

// ---------------------------------------------------------------- undo takes it all back
for (let i = 0; i < 4; i++) {
  const rev = (await api("graph")).view.revision;
  await press('button[aria-label="Undo"]');
  await until(async () => (await api("graph")).view.revision > rev);
}
check(await until(async () => read() === original), "four undos restore keys.ts exactly, imports and all");
check(await until(async () => !(await page.$('[data-module="keys"] [data-node="reverb"]'))), "and the effect nodes are gone");

// ---------------------------------------------------------------- a widener on a track the mixer pans
// Widened is stereo, and the song pans Keys: std's pan balances a stereo channel, so it goes in.
check(await add("pluck", "widen", "widen"), "a widener goes after the pluck, though the mixer pans Keys");
check(await until(async () => read().includes(`({ lane }) => widen(${pluck}, { width: 1.5 })`)), "keys.ts reads widen(pluck(…), { width: 1.5 })");
check((await knobs("widen")).width?.value === "1.5", "with a width knob at 1.5");
check((await api("graph")).view.draft_errors.length === 0, "and the song compiles");
const rev = (await api("graph")).view.revision;
await press('button[aria-label="Undo"]');
await until(async () => (await api("graph")).view.revision > rev);
check(await until(async () => read() === original), "undo takes it out again");

check(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
