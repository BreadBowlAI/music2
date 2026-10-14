// Step 6's write path, as its review found it wanting, in the app: the watcher takes outside edits
// in after a file that was there for one poll only (WRITE-19); a Tx naming a file the disk spells
// another way is refused, writing nothing (WRITE-3a); a Tx that doesn't read is answered 422 with
// its diagnostics, as every refusal is (the step 6 design's 1.2); a renamed track's folder that
// holds a file that isn't data is offered file by file, naming that file, never a "Move all" that
// can't apply (DIAG-1, ID-17).
//
//   node e2e/s6-fixes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import crypto from "node:crypto";
import fs from "node:fs";
import { exe } from "./chrome.mjs";

const [S, , proj = `${S}/proj`] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const consoleErrors = [];
page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
page.on("pageerror", (e) => consoleErrors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const headers = { "x-m2-token": sess.token, "content-type": "application/json" };
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers })).json();
const post = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, { method: "POST", headers, body: JSON.stringify(body) });
  return { status: r.status, body: await r.json().catch(() => null) };
};

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const read = (f) => (fs.existsSync(`${proj}/${f}`) ? fs.readFileSync(`${proj}/${f}`, "utf8") : null);
const write = (f, text) => fs.writeFileSync(`${proj}/${f}`, text);
const sha = (text) => `sha256:${crypto.createHash("sha256").update(text).digest("hex")}`;
const orig = { song: read("song.ts"), kick: read("tracks/kick.ts"), hits: read("data/kick/hits.json") };
const graph = () => api("graph");
const revision = async () => (await graph()).view.revision;
const key = async (k, mods = []) => {
  for (const m of mods) await page.keyboard.down(m);
  await page.keyboard.press(k);
  for (const m of mods.reverse()) await page.keyboard.up(m);
};

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });
await page.click('[data-module-tab="song"]');
await page.waitForSelector('[data-module="song"] [data-knob="hats.pan"]');
const pan = () => page.$eval('[data-module="song"] [data-knob="hats.pan"]', (e) => e.dataset.value).catch(() => null);

// ---------------------------------------------------------------- the watcher (WRITE-19)

// A script's temporary file, made and gone within one poll, ten times; a lane file moved away and
// back as quickly. Neither may hold the watcher's batch open for good.
for (let i = 0; i < 10; i++) {
  write("tracks/scratch.ts", "export const x = 1;\n");
  await sleep(20);
  fs.rmSync(`${proj}/tracks/scratch.ts`);
  await sleep(60);
}
fs.renameSync(`${proj}/data/hats/hits.json`, `${proj}/data/hats/x.json`);
await sleep(30);
fs.renameSync(`${proj}/data/hats/x.json`, `${proj}/data/hats/hits.json`);
await sleep(400);
const rev0 = await revision();
write("song.ts", orig.song.replace("pan: 0.3", "pan: 0.5"));
check(await until(async () => (await pan()) === "0.5"), "after files that were there for one poll, an outside edit is taken in");
check((await revision()) === rev0 + 1, "as one revision");
write("data/kick/hits.json", orig.hits.replace('"start": 1,', '"start": 1.5,'));
check(await until(async () => (await revision()) === rev0 + 2), "and so is the next, a lane file");
write("song.ts", orig.song);
write("data/kick/hits.json", orig.hits);
check(await until(async () => (await pan()) === "0.3" && (await revision()) > rev0 + 2), "and the ones after: both put back");

// ---------------------------------------------------------------- a Tx's paths (WRITE-3a, 1.2)

const at = orig.song.indexOf("pan: 0.3") + 5;
const edit = (file) => ({ op: "text_edit", file, edits: [{ start: at, end: at + 3, expected: "0.3", text: "0.9" }] });
const tx = (file, extra = {}) => ({ id: "tx-e2e", base: { [file]: sha(orig.song) }, ops: [edit(file)], expect: "identical", ...extra });
const folds = fs.existsSync(`${proj}/SONG.TS`);
const rev1 = await revision();
for (const route of ["tx/check", "tx/commit"]) {
  const aliased = await post(route, { tx: tx("Song.ts") });
  const code = aliased.body?.rejected?.diagnostics?.[0]?.code;
  check(folds ? aliased.status === 422 && code === "tx.path" : aliased.status === 409, `${route}: song.ts spelled Song.ts is refused (${aliased.status} ${code ?? JSON.stringify(aliased.body)})`);
  const extra = await post(route, { tx: tx("song.ts", { extra: 1 }) });
  check(extra.status === 422 && extra.body?.rejected?.diagnostics?.[0]?.code === "tx.invalid", `${route}: a Tx with a field a Tx doesn't have is rejected in JSON, 422 tx.invalid (${extra.status})`);
}
check(read("song.ts") === orig.song && (await revision()) === rev1, "nothing written, no revision");

// ---------------------------------------------------------------- a folder holding a stray file (ID-17)

write("data/kick/notes.txt", "hits from the old take\n");
write("tracks/kick.ts", orig.kick.replace('track("kick"', 'track("kicks"'));
const row = '[data-testid=attention] [data-problem="data.unclaimed"][data-src="data/kick"]';
check(await until(async () => !!(await page.$(row))), "a track key renamed by hand: Needs attention names its folder");
const offered = await page.$eval(row, (e) => ({
  message: e.querySelector("[data-testid=problem-message]").textContent,
  likely: e.querySelector("[data-likely]")?.textContent ?? null,
  moves: [...e.querySelectorAll("[data-fix=relink-pick] option[data-to]")].map((o) => [o.textContent, o.dataset.to]),
}));
check(offered.likely === null && !offered.moves.some(([t]) => t.startsWith("Move all")), `no "Move all" is offered, since it couldn't apply (${JSON.stringify(offered.moves)})`);
check(offered.message.includes("data/kick/notes.txt isn't data"), `the problem names the file that keeps the folder (${offered.message})`);
await page.$eval(row, (e) => e.scrollIntoView({ block: "center" }));
await page.screenshot({ path: `${S}/s6-fixes-stray.png` });
const to = offered.moves.findIndex(([, d]) => d === "data/kicks/hits.json");
check(to >= 0, "its lane file is offered on its own");
const revF = await revision();
await page.select(`${row} [data-fix=relink-pick]`, String(to));
check(await until(() => read("data/kicks/hits.json") === orig.hits && read("data/kick/hits.json") === null), "moving it moves that file alone");
check(read("data/kick/notes.txt") !== null && (await until(async () => (await revision()) > revF)) && (await sleep(300), (await revision()) === revF + 1), "the stray file stays, one revision");
await page.mouse.click(800, 850);
await key("z", ["Meta"]);
check(await until(() => read("data/kick/hits.json") === orig.hits && read("data/kicks/hits.json") === null), "⌘Z moves it back");
fs.rmSync(`${proj}/data/kick/notes.txt`);
write("tracks/kick.ts", orig.kick);
check(await until(async () => !(await page.$(row))), "the key put back: the folder needs no attention");

check(read("song.ts") === orig.song && read("tracks/kick.ts") === orig.kick, "every code file is as it was");
check(consoleErrors.length === 0, `no console errors ${consoleErrors.length ? JSON.stringify(consoleErrors) : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
