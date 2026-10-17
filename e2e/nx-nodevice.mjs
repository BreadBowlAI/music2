// Next lane C1: a song opens with no sound device (gaps #13, plan Q13). On a headless server of
// its own with M2_AUDIO=fail (every sound output fails to start, as on a computer with none), on a
// copy of first-song, with the run's scratch HOME:
//   - the song opens, and the status says no output plays (`no_output`, no `output` name);
//   - the status pill says "No sound output", in `warn`, with VolumeX;
//   - Play moves the playhead: the song plays silently, on the silent player's clock;
//   - the popover says why and what to do, in plain words; lint; its Settings… opens Settings (C2),
//     whose Now row says the same;
//   - a PC page says the same.
//
//   node e2e/nx-nodevice.mjs <root> <home>     (the `none` kind)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { exe, pc as pcAgent, real } from "./chrome.mjs";
import { HEADLESS, REPO } from "./env.mjs";
import { lint as lintIn } from "./lintlib.mjs";

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
const errors = [];
const servers = [];
process.on("exit", () => servers.forEach((s) => s.exitCode === null && s.signalCode === null && s.kill("SIGKILL")));

/** A headless server of this suite's own on `dir`, with no sound output that starts. */
async function serve(name, dir) {
  const file = `${ROOT}/${name}.json`;
  fs.rmSync(file, { force: true });
  const env = { ...process.env, HOME, M2_AUDIO: "fail", M2_MIDI: "none" };
  delete env.CLAP_PATH;
  const log = `${ROOT}/${name}.log`;
  const proc = spawn(HEADLESS, [dir, file, `${name}${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(log, "w")] });
  servers.push(proc);
  await until(async () => fs.existsSync(file), 30000);
  await sleep(300);
  const s = JSON.parse(fs.readFileSync(file, "utf8"));
  const get = async (path) => (await fetch(`${s.url}/api/${path}`, { headers: { "x-m2-token": s.token } })).json().catch(() => null);
  return { ...s, get, log };
}

/** A page on server `s`: a Mac's, or a PC's. */
async function pageOn(s, { pc = false } = {}) {
  const browser = await puppeteer.launch({ executablePath: pc ? real : exe, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  if (pc) await page.setUserAgent(pcAgent);
  await page.setViewport({ width: 1440, height: 900 });
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.evaluateOnNewDocument(
    (token, platform) => {
      window.__M2__ = { native: false, token, platform };
    },
    s.token,
    pc ? "linux" : "macos",
  );
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=timeline], [data-testid=start]", { timeout: 20000 }).catch(() => null);
  return { browser, page };
}

/** What the status pill says: its states, its label, and its tone's class. */
const pill = (page) =>
  page.evaluate(() => {
    const p = document.querySelector("[data-status]");
    return p ? { states: p.dataset.status, text: p.textContent ?? "", cls: p.className, icon: p.querySelector("svg")?.getAttribute("class") ?? "" } : null;
  });

const song = `${ROOT}/no-device/First Song`;
fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
const s = await serve("no-device", song);
const g = await s.get("graph");
check(!!g?.name && Array.isArray(g?.timeline?.tracks), `with no sound output, the song opens: ${g?.name ?? "nothing"} (${fs.readFileSync(s.log, "utf8").split("\n").find((l) => /couldn't|error/i.test(l)) ?? "no error"})`);
const st = await s.get("graph/status");
check(st?.no_output === true && st?.output == null, `the status says no output plays: no_output ${st?.no_output}, output ${JSON.stringify(st?.output)}`);

const { browser, page } = await pageOn(s);
const shown = await until(async () => (await pill(page))?.states.includes("no-output"));
const p = await pill(page);
check(shown && /No sound output/.test(p?.text ?? ""), `the status pill says "No sound output": ${JSON.stringify(p?.text)} (${p?.states})`);
check(/bg-warn-soft/.test(p?.cls ?? "") && /lucide-volume-x/.test(p?.icon ?? ""), `in warn, with the VolumeX mark: ${p?.icon}`);

// Space plays: the playhead moves, though nothing sounds.
await page.click("[data-testid=timeline]").catch(() => {});
await page.keyboard.press("Space");
const moved = await until(async () => ((await s.get("graph/status"))?.beat ?? 0) > 0.5, 6000);
const playing = await s.get("graph/status");
check(moved && playing?.playing === true, `Play moves the playhead: playing ${playing?.playing}, beat ${playing?.beat?.toFixed(2)}`);
await page.keyboard.press("Space");

// The popover: why, and what to do.
await page.click("[data-status]");
await page.waitForSelector("[data-testid=status-popover]", { timeout: 3000 }).catch(() => null);
const words = await page.$eval("[data-testid=status-popover]", (e) => e.textContent ?? "").catch(() => "");
check(
  words.includes("No sound output") && words.includes("No sound device was found, so the song plays silently. Plug one in: it's used from your next action."),
  `the popover says why and what to do: ${JSON.stringify(words.slice(0, 160))}`,
);
const bad = await page.evaluate(lintIn, "[data-testid=status-popover]");
check(bad.length === 0, `the popover passes the taste lint: ${bad.slice(0, 4).join(" | ")}`);
// Its Settings… opens the Settings window, which says the same.
await page.click("[data-testid=status-settings]").catch(() => {});
check(!!(await page.waitForSelector("[data-testid=settings-dialog]", { timeout: 3000 }).catch(() => null)), "the popover's Settings… opens the Settings window");
const nowRow = await page.$eval("[data-testid=settings-now]", (e) => e.textContent ?? "").catch(() => "");
check(/No sound output: the song plays silently\./.test(nowRow), `its Now row says no output plays: ${JSON.stringify(nowRow)}`);
await page.keyboard.press("Escape");
await browser.close();

// A PC's page says the same.
const pc = await pageOn(s, { pc: true });
check(await until(async () => /No sound output/.test((await pill(pc.page))?.text ?? "")), "a PC page's pill says \"No sound output\" too");
await pc.browser.close();

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
