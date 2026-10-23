// Plugin windows (HOST-7): the lead's box in examples/with-plugins opens and closes Surge XT's own
// window through `POST /api/graph/plugin/window`, and says which it is. On a headless server of
// its own, with the test plugins (CLAP_PATH, as live3b's plugin part), on a screen: on Linux an
// invisible Xvfb the suite starts itself (never your own screen; E2E_DISPLAY names another), on
// macOS the real one, where the window flashes up for a moment. Without CLAP_PATH or a screen the
// suite says SKIP, and run.mjs reports it PART.
//
//   node e2e/ph-window.mjs <root> <home>     (the `none` kind: no project open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { exe } from "./chrome.mjs";
import { HEADLESS, REPO } from "./env.mjs";

const [ROOT] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (f, ms = 10000) => {
  const t = Date.now();
  while (Date.now() - t < ms) {
    if (await f().catch(() => false)) return true;
    await sleep(100);
  }
  return false;
};
let pass = 0, fail = 0;
const check = (ok, what) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${what}`);
  ok ? pass++ : fail++;
};
const done = (code) => {
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(code);
};

const clap = process.env.CLAP_PATH;
if (!clap || !fs.existsSync(`${clap}/Surge XT.clap`)) {
  console.log("SKIP  plugin windows: CLAP_PATH must name a folder with Surge XT");
  done(0);
}

// The screen the plugin host process opens its windows on.
let xvfb = null;
const display = await (async () => {
  if (process.platform !== "linux") return null;
  if (process.env.E2E_DISPLAY) return process.env.E2E_DISPLAY;
  if (spawnSync("sh", ["-c", "command -v Xvfb"]).status !== 0) return undefined;
  const file = `${ROOT}/xvfb-display`;
  fs.rmSync(file, { force: true });
  const out = fs.openSync(file, "w");
  // Xvfb picks a free display itself and writes its number to fd 3.
  xvfb = spawn("Xvfb", ["-displayfd", "3", "-screen", "0", "1600x1000x24", "-nolisten", "tcp"], { stdio: ["ignore", "ignore", "ignore", out] });
  await until(async () => fs.readFileSync(file, "utf8").trim() !== "", 10000);
  const n = fs.readFileSync(file, "utf8").trim();
  return n ? `:${n}` : undefined;
})();
if (display === undefined) {
  console.log("SKIP  plugin windows: no screen to open them on (install Xvfb)");
  done(0);
}

const dir = `${ROOT}/proj`;
fs.rmSync(dir, { recursive: true, force: true });
fs.cpSync(`${REPO}/examples/with-plugins`, dir, { recursive: true });
fs.mkdirSync(`${ROOT}/home/.music2`, { recursive: true });
const sessFile = `${ROOT}/home/.music2/session.json`;
const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "none", HOME: `${ROOT}/home`, CLAP_PATH: clap };
if (display) env.DISPLAY = display;
const server = spawn(HEADLESS, [dir, sessFile, `w${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${ROOT}/headless.log`, "w")] });
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const consoleErrors = [];
try {
  check(await until(async () => fs.existsSync(sessFile), 20000), "the with-plugins server started");
  await sleep(300);
  const s = JSON.parse(fs.readFileSync(sessFile, "utf8"));
  const api = async (path, body) =>
    (
      await fetch(`${s.url}/api/${path}`, {
        method: body ? "POST" : "GET",
        headers: { "x-m2-token": s.page_token, "content-type": "application/json" },
        body: body ? JSON.stringify(body) : undefined,
      })
    ).json();
  const state = await api("graph");
  const lead = state.view.modules.find((m) => m.key === "lead");
  const plugins = (lead?.nodes ?? []).filter((n) => n.plugin);
  const surge = plugins.find((n) => n.title === "Surge XT");
  check(!!surge, `the lead's Surge XT box names its plugin node (${JSON.stringify(plugins.map((n) => [n.label, n.title, n.plugin]))})`);
  if (!surge) throw new Error("no Surge XT box");
  // The song plays once its plugins are ready: their instances exist then.
  await api("graph/transport", { action: "play" });
  check(await until(async () => (await api("graph")).plugins?.[surge.plugin]?.window === "closed"), "the state says its window is closed");

  const page = await browser.newPage();
  await page.setViewport({ width: 1600, height: 1000 });
  page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));
  page.on("pageerror", (e) => consoleErrors.push(String(e)));
  await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, s.page_token);
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=graph], [data-testid=mix]", { timeout: 8000 });
  if (!(await page.$("[data-testid=graph]"))) await page.keyboard.press("Escape");
  await page.click('[data-module-tab="lead"]');
  await page.waitForSelector('[data-module="lead"]');
  const button = `[data-module="lead"] [data-plugin="${surge.plugin}"] [data-plugin-window]`;
  check(await until(async () => (await page.$eval(button, (b) => b.textContent)) === "Open window"), "the lead's box offers Open window");

  await page.click(button);
  check(await until(async () => (await api("graph")).plugins[surge.plugin].window === "open"), "Open window opens Surge XT's window in the plugin host");
  check(await until(async () => (await page.$eval(button, (b) => b.textContent)) === "Close window"), "and the box says Close window");
  await sleep(1500);

  await page.click(button);
  check(await until(async () => (await api("graph")).plugins[surge.plugin].window === "closed"), "Close window closes it");
  check(await until(async () => (await page.$eval(button, (b) => b.textContent)) === "Open window"), "and the box offers Open window again");
  check(consoleErrors.length === 0, `no console errors ${consoleErrors.join(" | ")}`);
} finally {
  await browser.close();
  server.kill();
  xvfb?.kill();
}
done(fail ? 1 : 0);
