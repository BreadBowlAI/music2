// Export Audio end to end (Song in the Export dialog, Return): the button, its progress, the done toast, the WAV in <HOME>/Music/music2/Exports,
// a second export's new name, ⇧⌘E, refusals, and that it equals `m2 render`.
// node export.mjs <scratch> <home> <project> <repo>
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { execFileSync } from "node:child_process";

// M2: the `m2` CLI built with the server.
const [S, H, P, M2] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const HDR = { "x-m2-token": sess.token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: HDR } : { method: "POST", headers: { ...HDR, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 10000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 50)); } return false; };
const text = () => page.evaluate(() => document.body.innerText);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A WAV file's format, length, peak, and how quiet its last half second is. */
function readWav(file) {
  const b = fs.readFileSync(file);
  if (b.toString("ascii", 0, 4) !== "RIFF" || b.toString("ascii", 8, 12) !== "WAVE") throw new Error(`${file} isn't a WAV file`);
  let off = 12, fmt, data;
  while (off + 8 <= b.length) {
    const id = b.toString("ascii", off, off + 4), size = b.readUInt32LE(off + 4);
    if (id === "fmt ") fmt = { tag: b.readUInt16LE(off + 8), channels: b.readUInt16LE(off + 10), rate: b.readUInt32LE(off + 12), bits: b.readUInt16LE(off + 22) };
    if (id === "data") data = { start: off + 8, size };
    off += 8 + size + (size & 1);
  }
  const bytes = fmt.bits / 8, frameBytes = bytes * fmt.channels;
  const frames = data.size / frameBytes;
  const sample = (i) => { const p = data.start + i * bytes; let v = b[p] | (b[p + 1] << 8) | (b[p + 2] << 16); return v & 0x800000 ? v - 0x1000000 : v; };
  let peak = 0, tailPeak = 0;
  const tailFrom = (frames - fmt.rate / 2) * fmt.channels;
  for (let i = 0; i < frames * fmt.channels; i++) {
    const a = Math.abs(sample(i));
    peak = Math.max(peak, a);
    if (i >= tailFrom) tailPeak = Math.max(tailPeak, a);
  }
  const db = (x) => 20 * Math.log10(Math.max(x, 1e-9) / 8388607);
  return { ...fmt, frames, seconds: frames / fmt.rate, peakDb: db(peak), tailDb: db(tailPeak), riffOk: b.readUInt32LE(4) === b.length - 8 };
}

const exportsDir = `${H}/Music/music2/Exports`;
// The Export dialog (studio C3): Song is chosen and Export… has the focus, so Return exports the song.
const go = async () => {
  await page.waitForSelector("[data-testid=export-dialog]", { timeout: 5000 });
  await page.keyboard.press("Enter");
};
// The song: 32 beats at 124 bpm.
const songEnd = (32 * 60) / 124;

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });
await sleep(400);
const button = await page.$("[data-testid=export]");
check(!!button, "the top bar has an Export button");
check((await page.$eval("[data-testid=export]", (e) => e.innerText.trim())) === "Export", "labelled Export");
const idleWidth = await page.$eval("[data-testid=export]", (e) => e.getBoundingClientRect().width);
// ⇧⌘E: ⌘E is kept for Split (taste.md §5.1, TB-9).
check(/Export the song, its stems or its notes \(⇧⌘E\)/.test(await page.$eval("[data-testid=export]", (e) => e.title)), "with a tooltip naming ⇧⌘E");
check(!fs.existsSync(exportsDir), "no Exports folder before the first export");
await page.screenshot({ path: `${S}/export-bar.png`, clip: { x: 0, y: 0, width: 1440, height: 44 } });

// Watch for the progress ring: the render is quick, so record every value it shows.
await page.evaluate(() => {
  window.__progress = [];
  const seen = () => {
    const el = document.querySelector("[data-testid=export-progress]");
    if (el) window.__progress.push(Number(el.getAttribute("aria-valuenow")));
  };
  new MutationObserver(seen).observe(document.body, { subtree: true, childList: true, attributes: true });
});
const t0 = Date.now();
await page.click("[data-testid=export]");
await go();
// While it renders, the app still answers and plays.
const st = Date.now();
const status = await api("graph/status");
check(status.status === 200 && Date.now() - st < 300, `the server answers while exporting (${Date.now() - st} ms)`);
const toastRe = /Exported first-song\.wav \(0:(\d\d)\) to (.+)\./;
check(await until(async () => toastRe.test(await text())), "the done toast appears");
const took = Date.now() - t0;
const progress = await page.evaluate(() => window.__progress);
check(progress.length > 0, `the progress ring appeared (values ${[...new Set(progress)].join(", ")})`);
check(progress.every((v, i) => i === 0 || v >= progress[i - 1]), "and only went forward");
const [, secs, folder] = (await text()).match(toastRe);
check(folder === "~/Music/music2/Exports", `the toast names the folder, from the home folder: ${folder}`);
await page.screenshot({ path: `${S}/export-done.png` });
check(await until(async () => !(await page.$("[data-testid=export-progress]"))), "the ring is gone once it's done");
check((await page.$eval("[data-testid=export]", (e) => e.innerText.trim())) === "Export", "and the button says Export again");

const first = `${exportsDir}/first-song.wav`;
check(fs.existsSync(first), `the WAV is at ${first} (exported in ${took} ms)`);
const w = readWav(first);
check(w.riffOk && (w.tag === 1 || w.tag === 0xfffe) && w.channels === 2 && w.rate === 48000 && w.bits === 24, `a valid 48 kHz 24-bit stereo WAV (tag ${w.tag})`);
check(w.seconds >= songEnd + 0.5 && w.seconds <= songEnd + 10, `${w.seconds.toFixed(3)} s: the song's ${songEnd.toFixed(3)} s and a tail of ${(w.seconds - songEnd).toFixed(3)} s`);
check(Number(secs) === Math.round(w.seconds), `the toast's length (0:${secs}) is the file's`);
check(w.peakDb > -6 && w.peakDb <= 0, `it isn't silent: peak ${w.peakDb.toFixed(1)} dBFS`);
check(w.tailDb <= -89, `its last half second is silence (${w.tailDb.toFixed(1)} dBFS)`);
check(!fs.readdirSync(exportsDir).some((f) => f.endsWith(".part")), "no half-written file is left");

// The CLI renders the same project to the same bytes.
const cli = `${S}/export-cli.wav`;
execFileSync(M2, ["render", P, cli]);
check(Buffer.compare(fs.readFileSync(cli), fs.readFileSync(first)) === 0, "`m2 render` writes the same file, byte for byte");

// ⇧⌘E exports again, to a new name.

await page.keyboard.down("Meta"); await page.keyboard.down("Shift"); await page.keyboard.press("e"); await page.keyboard.up("Shift"); await page.keyboard.up("Meta");
await go();
check(await until(async () => (await text()).includes("Exported first-song 2.wav")), "⇧⌘E exports again, as first-song 2.wav");
const second = `${exportsDir}/first-song 2.wav`;
check(fs.existsSync(second) && Buffer.compare(fs.readFileSync(second), fs.readFileSync(first)) === 0, "the same audio, and the first file is untouched");

// Two at once: one starts, the other is refused and says why.
await until(async () => !(await page.$("[data-testid=export-progress]")));
const [a, b] = await Promise.all([api("graph/export", {}), api("graph/export", {})]);
const ok = [a, b].filter((r) => r.status === 200), refused = [a, b].filter((r) => r.status === 400);
check(ok.length === 1 && refused.length === 1, `one of two exports at once starts (${a.status}, ${b.status})`);
check(/already being exported/.test(refused[0]?.json?.error ?? ""), `the other is refused: ${refused[0]?.json?.error}`);
check(ok[0]?.json?.path === `${exportsDir}/first-song 3.wav`, `and the one that starts says where: ${ok[0]?.json?.path}`);
check(await until(() => fs.existsSync(`${exportsDir}/first-song 3.wav`)), "it's written");

// A file the user picked (the desktop app's save dialog), and plain errors.
await until(async () => !(await page.$("[data-testid=export-progress]")));
const picked = await api("graph/export", { path: `${H}/My Mix` });
check(picked.status === 200 && picked.json.path === `${H}/My Mix.wav`, `a picked path gets .wav: ${picked.json?.path}`);
check(await until(async () => (await text()).includes("Exported My Mix.wav")), "and its toast names it");
const bad = await api("graph/export", { path: "relative.wav" });
check(bad.status === 400 && /full path/.test(bad.json.error), `a relative path is refused: ${bad.json?.error}`);
const missing = await api("graph/export", { path: "/no/such/folder/x.wav" });
check(missing.status === 400 && /no folder/.test(missing.json.error), `a missing folder is refused: ${missing.json?.error}`);

// A long song, to see the ring mid-way: its kick clip moved 400 beats on.
const long = `${S}/export-long`;
fs.rmSync(long, { recursive: true, force: true });
fs.cpSync(P, long, { recursive: true });
fs.writeFileSync(`${long}/data/kick/hits.json`, fs.readFileSync(`${P}/data/kick/hits.json`, "utf8").replace('"start": 0,', '"start": 400,'));
check((await api("graph/open", { path: long })).status === 200, "a longer song opens");
await until(async () => (await text()).includes("first-song"));
await sleep(500);
await page.click("[data-testid=export]");
await go();
check(await until(async () => Number(await page.$eval("[data-testid=export-progress]", (e) => e.getAttribute("aria-valuenow")).catch(() => 0)) >= 30, 20000), "the ring fills as it goes");
await page.screenshot({ path: `${S}/export-progress.png` });
await page.screenshot({ path: `${S}/export-progress-bar.png`, clip: { x: 900, y: 0, width: 540, height: 64 } });
const label = await page.$eval("[data-testid=export]", (e) => e.innerText.replace(/\s+/g, " ").trim());
// The ring in the icon's place, the percentage in the label's: the same width, so Undo beside it never moves (taste.md §3.3).
const busyWidth = await page.$eval("[data-testid=export]", (e) => e.getBoundingClientRect().width);
check(/^\d+%$/.test(label) && Math.abs(busyWidth - idleWidth) < 0.5, `the button says how far, in its own width: "${label}" (${busyWidth} vs ${idleWidth}px)`);
check(await page.$eval("[data-testid=export]", (e) => e.getAttribute("aria-busy") === "true"), "and is busy");
await page.keyboard.down("Meta"); await page.keyboard.down("Shift"); await page.keyboard.press("e"); await page.keyboard.up("Shift"); await page.keyboard.up("Meta");
check(await until(async () => (await text()).includes("The song is already being exported."), 3000), "⇧⌘E meanwhile says it's already exporting");
check(await until(async () => (await text()).includes("Exported first-song 4.wav"), 30000), "the long one finishes");
const lw = readWav(`${exportsDir}/first-song 4.wav`);
check(lw.seconds > (432 * 60) / 124 && lw.seconds <= (432 * 60) / 124 + 10, `${lw.seconds.toFixed(2)} s long`);

check(errors.length === 0, `no console errors${errors.length ? `: ${errors.join(" | ")}` : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
