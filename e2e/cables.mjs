import puppeteer from "puppeteer-core";
import fs from "node:fs";
const S = process.argv[2];
const song = `${S}/cables/song.ts`;
const original = fs.readFileSync(`${S}/cables-orig.ts`, "utf8");
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
const center = async (sel) => { const b = await (await page.waitForSelector(sel)).boundingBox(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 }; };
async function drag(fromSel, toSel) {
  const a = await center(fromSel), b = await center(toSel);
  await page.mouse.move(a.x, a.y); await page.mouse.down();
  // First into the gap right of the outlet, where no knob is (the midpoint can cross another node's knob).
  await page.mouse.move(a.x + 30, a.y + 8, { steps: 4 });
  const dashed = await page.$('[data-cabling] path[stroke-dasharray]');
  await page.mouse.move(b.x, b.y, { steps: 4 }); await page.mouse.up();
  return !!dashed;
}
const html = () => page.content();

ok(!!(await page.$('[data-outlet="wobble"]')), "wobble has an output handle");
const drew = await drag('[data-outlet="wobble"]', '[data-knob="cutoff"] svg');
ok(drew, "a cable follows the pointer while dragging");
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === original.replace("svf.lowpass(osc.saw(hz(55)), hz(1000))", "svf.lowpass(osc.saw(hz(55)), wobble)"), "dropping on cutoff wrote `wobble` there, nothing else");
// The input reads "cutoff ← wobble" now: that says it, so a cable makes no toast (taste.md §2.14).
ok(!!(await page.$('[data-inlet="cutoff"]')) && (await page.$$("[data-testid=toast]")).length === 0, "the canvas shows the edit, with no toast");
ok(!!(await page.$('[data-inlet="cutoff"]')), "the filter shows `cutoff ← wobble`");
ok(!(await page.$('[data-knob="cutoff"]')), "and no cutoff knob any more");

await page.click('[data-inlet="cutoff"] button');
await sleep(1200);
ok(fs.readFileSync(song, "utf8") === original, "disconnecting wrote the declared default back: the file is as it started");
ok(!!(await page.$('[data-knob="cutoff"]')), "the cutoff knob is back");

const errorsBeforeRefusal = [...errors];
await drag('[data-outlet="wobble"]', '[data-knob="period"] svg');
await sleep(1200);
ok((await html()).includes("declared after this value"), "connecting into its own declaration is refused with a reason");
ok(fs.readFileSync(song, "utf8") === original, "and the file is unchanged");
ok(errorsBeforeRefusal.length === 0, `no console errors before the refusal ${errorsBeforeRefusal.join(" | ")}`);
ok(errors.length === errorsBeforeRefusal.length + 1 && errors.at(-1).includes("400"), "the refusal is the only error: its 400 response");
console.log(`\n${passed} passed, ${failed} failed`);
await browser.close();
