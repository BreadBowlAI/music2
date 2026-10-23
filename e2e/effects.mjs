import puppeteer from "puppeteer-core";
import fs from "node:fs";
const S = process.argv[2];
const keys = `${S}/proj/tracks/keys.ts`;
const original = fs.readFileSync(`${S}/keys-orig.ts`, "utf8");
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.setViewport({ width: 1600, height: 1000 });
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.page_token);
await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]");
// The panel shows one module at a time: pick the one under test.
await page.click('[data-module-tab="keys"]');
await page.waitForSelector('[data-module="keys"]');
let passed = 0, failed = 0;
const ok = (c, m) => { console.log(`${c ? "PASS" : "FAIL"}  ${m}`); c ? passed++ : failed++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const read = () => fs.readFileSync(keys, "utf8");
// The catalog writes delay's settings as knobs (its descriptor's `editor.insert`).
const delayed = 'delay(pluck(lane.notes("chords")), { time: 0.25, feedback: 0.3, mix: 0.3 })';

const plus = '[data-module="keys"] button[aria-label="Add an effect after pluck"]';
ok(!!(await page.$(plus)), "the keys' pluck offers an effect");
ok(!(await page.$('[data-module="keys"] button[aria-label^="Add an effect after"][aria-label$="notes"]')), "the notes lane doesn't");
await page.click(plus);
await page.waitForSelector('[data-module="keys"] [data-effect="delay"]');
await page.click('[data-module="keys"] [data-effect="delay"]');
await sleep(1500);
const added = read();
ok(added.includes(`({ lane }) => ${delayed}`), "pluck is wrapped in a delay");
ok(/import \{ delay \} from "std\/fx";/.test(added), "and delay is imported");
ok(added.replace(/\nimport \{ delay \} from "std\/fx";/, "").replace(delayed, "pluck(lane.notes(\"chords\"))") === original, "nothing else changed");
// In words, with its Undo; never the code diff (taste.md ST-5, ST-6). Both by their labels (DV-2).
ok((await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent))).includes("Added Delay after PluckUndo"), "a toast says what was added, with Undo");
ok(!!(await page.$('[data-module="keys"] [data-node="delay"]')), "a delay node appears in Keys");

// The delay node's remove button (since step 5 it names the effect by its descriptor's label).
await page.click('[data-module="keys"] [data-node="delay"] button[aria-label^="Remove "]');
await sleep(1500);
ok(read().includes('({ lane }) => pluck(lane.notes("chords"))'), "removing it unwraps the pluck");
ok(!(await page.$('[data-module="keys"] [data-node="delay"]')), "the delay node is gone");

await page.click('button[aria-label="Undo"]'); await sleep(1500);
ok(read() === added, "undo brings the delay back");
await page.click('button[aria-label="Undo"]'); await sleep(1500);
ok(read() === original, "undo again restores the file exactly, import and all");
ok(errors.length === 0, `no console errors ${errors.join(" | ")}`);
console.log(`\n${passed} passed, ${failed} failed`);
await browser.close();
