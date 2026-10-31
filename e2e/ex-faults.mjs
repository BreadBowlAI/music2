// Track ex (one path for faults, RENDER-25, DIAG-4): a sound that crashes while the song plays is a
// problem the app shows where problems are, in the status pill's popover, naming the sound and its
// track, and it is toasted once. On a server of its own, on a copy of first-song, with a copy of std
// whose `noise` kernel traps at once (e2e/trapwasm.mjs, as sound-stops does) and hats written to
// play on it (NOISE_HATS), so only Hats' instrument uses it. Play: one toast; the pill says "1
// problem"; its popover lists it as an error (`sound.stopped`) in words that name Hats; Hats' header
// marks it and Kick's doesn't; no second toast while the song plays on.
//
//   node e2e/ex-faults.mjs <root> <home>     (the `none` kind: no project open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { exe } from "./chrome.mjs";
import { HEADLESS, REPO } from "./env.mjs";
import { NOISE_HATS, trapping } from "./trapwasm.mjs";

const [ROOT, HOME] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (f, ms = 8000) => {
  const t = Date.now();
  while (Date.now() - t < ms) {
    if (await f().catch(() => false)) return true;
    await sleep(80);
  }
  return false;
};
let pass = 0, fail = 0;
const check = (ok, what) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${what}`);
  ok ? pass++ : fail++;
};
const SAID =
  "The instrument on Hats stopped working, so music2 made it silent. The rest of the song plays on. " +
  "To start it again, choose Transport ▸ Stop All Sound.";

// std with its `noise` kernel trapping, and first-song, each a copy of the run's own.
const std = `${ROOT}/std-noise-traps`;
fs.rmSync(std, { recursive: true, force: true });
fs.cpSync(`${REPO}/std/src`, `${std}/src`, { recursive: true });
fs.copyFileSync(`${REPO}/std/m2pkg.toml`, `${std}/m2pkg.toml`);
fs.mkdirSync(`${std}/kernels`);
fs.writeFileSync(`${std}/kernels/std.wasm`, trapping(fs.readFileSync(`${REPO}/std/kernels/std.wasm`), "noise_process"));
const song = `${ROOT}/song`;
fs.rmSync(song, { recursive: true, force: true });
fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
fs.writeFileSync(`${song}/tracks/hats.ts`, NOISE_HATS);

const file = `${ROOT}/faults.json`;
fs.rmSync(file, { force: true });
const env = { ...process.env, HOME, M2_AUDIO: "none", M2_STD: std };
delete env.CLAP_PATH;
const server = spawn(HEADLESS, [song, file, `faults${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${ROOT}/faults-server.log`, "w")] });
process.on("exit", () => server.exitCode === null && server.signalCode === null && server.kill("SIGKILL"));
await until(async () => fs.existsSync(file), 30000);
await sleep(300);
const s = JSON.parse(fs.readFileSync(file, "utf8"));

const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900 });
const errors = [];
page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "macos" };
}, s.page_token);
await page.goto(s.url, { waitUntil: "domcontentloaded" });
check(!!(await page.waitForSelector("[data-testid=timeline] [data-track=hats]", { timeout: 20000 }).catch(() => null)), "first-song opens on the copy of std");

const playing = () => page.evaluate(async (token) => (await (await fetch("/api/graph/status", { headers: { "x-m2-token": token } })).json()).playing === true, s.page_token);
// Every toast element that said it, kept as it showed: a second toast is a new element.
await page.evaluate((said) => {
  window.__toasts = [];
  new MutationObserver(() => {
    for (const e of document.querySelectorAll("[data-testid=toast]")) {
      if (e.textContent?.includes(said) && !window.__toasts.includes(e)) window.__toasts.push(e);
    }
  }).observe(document.body, { childList: true, subtree: true, characterData: true });
}, SAID);
const toasted = () => page.evaluate(() => window.__toasts.length);

await page.keyboard.press("Space");
check(await until(playing, 3000), "Space plays");
check(await until(async () => (await toasted()) === 1, 8000), "the app says which sound stopped, on which track: one toast");

// The pill says there is a problem; its popover lists it, an error, naming the sound and its track.
const pill = () => page.$eval("[data-status]", (e) => ({ status: e.dataset.status, title: e.title })).catch(() => null);
check(await until(async () => (await pill())?.status.split(" ").includes("problems"), 4000), `the status pill shows the problem: ${JSON.stringify(await pill())}`);
await page.mouse.move(700, 120);
await page.click("[data-status]").catch(() => {});
const rows = () =>
  page.$$eval("[data-testid=status-popover] [data-testid=attention] [data-problem]", (els) =>
    els.map((e) => ({ code: e.dataset.problem, severity: e.dataset.severity, text: e.querySelector("[data-testid=problem-message]")?.textContent ?? "" })),
  );
check(await until(async () => (await rows()).some((r) => r.code === "sound.stopped"), 4000), `its popover lists it: ${JSON.stringify(await rows())}`);
const row = (await rows()).find((r) => r.code === "sound.stopped");
check(row?.severity === "error" && row.text === SAID, `as an error, in words that name the sound and its track: ${JSON.stringify(row)}`);
const shown = await page
  .$eval("[data-testid=status-popover]", (p) => {
    const r = p.getBoundingClientRect();
    return r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight;
  })
  .catch(() => false);
check(shown, "the popover is all on screen");
await page.screenshot({ path: `${ROOT}/ex-faults.png` });

// Its track says so too: Hats' header marks an error, Kick's nothing.
const mark = (track) => page.$eval(`[data-track="${track}"] [data-testid=track-issues]`, (e) => ({ severity: e.dataset.severity, title: e.title })).catch(() => null);
const hats = await mark("hats");
check(hats?.severity === "error" && hats.title.includes(SAID) && !(await mark("kick")), `Hats' header marks it, Kick's doesn't: ${JSON.stringify(hats)}`);

// Once: no second toast while the song plays on and the hats stay silent.
await page.keyboard.press("Escape");
await sleep(4000);
check((await playing()) && (await toasted()) === 1, `one toast in all, the song playing on (${await toasted()})`);

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
