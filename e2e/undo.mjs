import puppeteer from "puppeteer-core";
import fs from "node:fs";
const S = process.argv[2];
const song = `${S}/cables/song.ts`;
const original = fs.readFileSync(`${S}/cables-orig.ts`, "utf8");
const wired = original.replace("svf.lowpass(osc.saw(hz(55)), hz(1000))", "svf.lowpass(osc.saw(hz(55)), wobble)");
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.setViewport({ width: 1600, height: 1000 });
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]");
// The panel shows one module at a time: pick the one under test.
await page.click('[data-module-tab="song"]');
await page.waitForSelector('[data-module="song"]');
let passed = 0, failed = 0;
const ok = (c, m) => { console.log(`${c ? "PASS" : "FAIL"}  ${m}`); c ? passed++ : failed++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const disabled = (label) => page.$eval(`button[aria-label="${label}"]`, (b) => b.disabled);
const center = async (sel) => { const b = await (await page.waitForSelector(sel)).boundingBox(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 }; };

ok(await disabled("Undo") && await disabled("Redo"), "nothing to undo or redo at first");
const a = await center('[data-outlet="wobble"]'), b = await center('[data-knob="cutoff"] svg');
await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 8 }); await page.mouse.up();
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === wired, "connected");
ok(!(await disabled("Undo")), "undo is available");

const named = await page.$eval('button[aria-label="Undo"]', (b) => b.title);
await page.click('button[aria-label="Undo"]');
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === original, "the Undo button put the value back");
// In words, in Undo's tooltip; the cable's undo shows on the canvas, so no toast, and never a file or a diff (taste.md ST-5, ST-6).
ok(named === "Undo connecting wobble (⌘Z)" && !(await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent))).some((t) => t.includes(".ts")), `Undo's tooltip said what it would undo (${named}), and no toast names a file`);
ok(!!(await page.$('[data-knob="cutoff"]')) && await disabled("Undo") && !(await disabled("Redo")), "the knob is back; only redo is available");

await page.keyboard.down("Meta"); await page.keyboard.down("Shift"); await page.keyboard.press("z");
await page.keyboard.up("Shift"); await page.keyboard.up("Meta");
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === wired, "⇧⌘Z redid the cable");
ok(!!(await page.$('[data-inlet="cutoff"]')), "the canvas shows it connected again");

await page.keyboard.down("Meta"); await page.keyboard.press("z"); await page.keyboard.up("Meta");
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === original, "⌘Z undid it again");
ok(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${passed} passed, ${failed} failed`);
await browser.close();
