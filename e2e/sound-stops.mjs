// The gaps lane of 8a's live half (RENDER-25): a sound that crashes while the song plays goes
// silent alone, and the app says which sound, on which track, and what starts it again. On a server
// of its own, on a copy of first-song, with a copy of std whose `noise` kernel traps at once (a
// kernel with a bug: e2e/trapwasm.mjs), so only Hats' instrument uses it. Play: the toast names it
// ("The instrument on Hats stopped working…") in words that fit on screen; the song plays on, Kick
// sounding, Hats silent; it is said once, and listed among the song's problems (`sound.stopped`,
// with Hats' key: the status popover's list); Stop All Sound (⌘.) starts it again, and it stops
// again: listed again, with no second toast (the ex plan's D3).
//
//   node e2e/sound-stops.mjs <root> <home>     (the `none` kind: no project open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { exe } from "./chrome.mjs";
import { HEADLESS, REPO } from "./env.mjs";
import { trapping } from "./trapwasm.mjs";

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
const FLOOR = -60;
const SAID =
  "The instrument on Hats stopped working, so music2 made it silent. The rest of the song plays on. " +
  "To start it again, choose Transport ▸ Stop All Sound.";

// std with its `noise` kernel trapping, and first-song, each a copy of the run's own.
const std = `${ROOT}/std-noise-traps`;
fs.rmSync(std, { recursive: true, force: true });
fs.cpSync(`${REPO}/std/src`, `${std}/src`, { recursive: true });
fs.copyFileSync(`${REPO}/std/m2pkg.toml`, `${std}/m2pkg.toml`);
fs.mkdirSync(`${std}/kernels`);
const wasm = trapping(fs.readFileSync(`${REPO}/std/kernels/std.wasm`), "noise_process");
fs.writeFileSync(`${std}/kernels/std.wasm`, wasm);
let traps = false;
try {
  // std imports one host call, `m2.asset_read` (kernel ABI v1): a stand-in that reads nothing.
  new WebAssembly.Instance(new WebAssembly.Module(wasm), { m2: { asset_read: () => 0 } }).exports.noise_process(128);
} catch (e) {
  traps = String(e).includes("unreachable");
}
check(traps, "the copy of std is a module whose noise kernel traps at once");
const song = `${ROOT}/song`;
fs.rmSync(song, { recursive: true, force: true });
fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });

const file = `${ROOT}/stops.json`;
fs.rmSync(file, { force: true });
const env = { ...process.env, HOME, M2_AUDIO: "none", M2_STD: std };
delete env.CLAP_PATH;
const server = spawn(HEADLESS, [song, file, `stops${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${ROOT}/stops-server.log`, "w")] });
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
}, s.token);
await page.goto(s.url, { waitUntil: "domcontentloaded" });
check(!!(await page.waitForSelector("[data-testid=timeline] [data-track=hats]", { timeout: 20000 }).catch(() => null)), "first-song opens on the copy of std");

const api = (path) =>
  page.evaluate(
    async (token, path) => (await fetch(`/api/graph${path}`, { headers: { "x-m2-token": token } })).json(),
    s.token,
    path,
  );
const playing = async () => (await api("/status")).playing === true;
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
const meter = (track) =>
  page.$eval(`[data-testid=timeline] [data-track="${track}"] [role=meter]`, (e) => Number(e.getAttribute("aria-valuenow"))).catch(() => FLOOR);

check(!(await toasts()).some((t) => t.includes("stopped working")), "nothing is said before it plays");
await page.keyboard.press("Space");
check(await until(playing, 3000), "Space plays");
const told = await until(async () => (await toasts()).includes(SAID), 8000);
check(told, `the app says which sound stopped, on which track, and what starts it again: "${(await toasts()).join(" | ")}"`);
const first = await page.$("[data-testid=toast]");
const fits = await page
  .$$eval("[data-testid=toast]", (els) => {
    const e = els.find((x) => x.textContent?.includes("stopped working"));
    const r = e?.getBoundingClientRect();
    const words = e?.querySelector("span");
    return !!r && r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight && words.scrollWidth <= words.clientWidth;
  })
  .catch(() => false);
check(fits, "its words all show, on screen, none cut off");

// The song plays on: the kick sounds, the hats are silent.
const levels = [];
for (let i = 0; i < 12; i++) {
  levels.push([await meter("kick"), await meter("hats")]);
  await sleep(120);
}
check(await playing(), "the song plays on");
check(levels.some(([k]) => k > FLOOR) && levels.every(([, h]) => h === FLOOR), `Kick sounds and Hats is silent (${levels.map(([k, h]) => `${k}/${h}`).join(" ")})`);
// A toast said again replaces the one showing (one at a time): the first is still the one there.
check(await page.evaluate((e) => e?.isConnected && document.querySelector("[data-testid=toast]") === e, first), "it is said once");
// The status popover lists it until it plays again (RENDER-25, DIAG-4).
const problems = async () => ((await api("")).problems ?? []).filter((p) => p.code === "sound.stopped");
const listed = async () => {
  const p = await problems();
  return p.length === 1 && p[0].module === "hats" && p[0].message === SAID;
};
check(await until(listed, 4000), `it is one of the song's problems, sound.stopped, with Hats' key: ${JSON.stringify(await problems())}`);

// Stop All Sound resets every sound: the hats' instrument starts again, and stops again at once.
await page.mouse.move(700, 120);
await until(async () => !(await toasts()).includes(SAID), 6000);
check(!(await toasts()).includes(SAID), "the toast goes after a while");
await page.keyboard.down("Meta");
await page.keyboard.press(".");
await page.keyboard.up("Meta");
await sleep(2000);
check((await listed()) && !(await toasts()).includes(SAID), "after Stop All Sound (⌘.) it plays again and stops again: listed again, with no second toast");
await page.screenshot({ path: `${ROOT}/sound-stops.png` });

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
