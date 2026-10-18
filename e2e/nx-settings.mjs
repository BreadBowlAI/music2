// Next lane C2: the Settings window (gaps #31, plan Q14 to Q18). On a headless server of its own
// whose silent player lists two outputs (M2_AUDIO_OUTPUTS=Speakers,Headphones), on a copy of
// first-song, with the run's scratch HOME; and on the run's own server, which has no song open:
//   - ⌘, opens it (Ctrl+, on a PC page), from the start screen too; the ⌥K sheet lists it;
//   - the output list has "System default (Speakers)" first; Headphones chosen: the server's
//     `now.device`, the Now row, and no toast (X21); the buffer list with each size's ms; 256
//     chosen: `now.frames` 256 and "256 samples (5.3 ms)"; a change while playing stops play and
//     says so;
//   - the songs folder typed: kept, the New Song dialog's folder shows it; a relative folder or one
//     inside a song is refused in words; "Use the default";
//   - `.music2/settings.json` has all three; a reload and a new server keep them;
//   - Esc closes, Tab goes round, a menu's keys (↑ ↓ Return Esc); the app's menu row and its
//     folder picker (titled "Choose Where New Songs Go"); lint.
//
//   node e2e/nx-settings.mjs <root> <home>     (the `none` kind)
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
const readJson = (f) => {
  try {
    return JSON.parse(fs.readFileSync(f, "utf8"));
  } catch {
    return null;
  }
};

/** A headless server of this suite's own on `dir` (or none: "-"), whose silent player lists two outputs. */
async function serve(name, dir = "-") {
  const file = `${ROOT}/${name}.json`;
  fs.rmSync(file, { force: true });
  const env = { ...process.env, HOME, M2_AUDIO: "none", M2_AUDIO_OUTPUTS: "Speakers,Headphones", M2_MIDI: "none" };
  delete env.CLAP_PATH;
  const proc = spawn(HEADLESS, [dir, file, `${name}${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${ROOT}/${name}.log`, "w")] });
  let exited = false;
  proc.on("exit", () => (exited = true));
  servers.push(proc);
  await until(async () => fs.existsSync(file), 30000);
  await sleep(300);
  const s = JSON.parse(fs.readFileSync(file, "utf8"));
  const api = async (path, body) => {
    const r = await fetch(`${s.url}/api/${path}`, body === undefined ? { headers: { "x-m2-token": s.token } } : { method: "POST", headers: { "x-m2-token": s.token, "content-type": "application/json" }, body: JSON.stringify(body) });
    return { status: r.status, json: await r.json().catch(() => null) };
  };
  const stop = async () => {
    proc.kill("SIGTERM");
    await until(async () => exited, 5000);
    if (!exited) proc.kill("SIGKILL");
  };
  return { ...s, api, stop };
}

const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });

/**
 * A page on server `s`, in a browser context of its own: a Mac's, a PC's (`pc`, in a browser of its
 * own with a PC's agent), or the app's (`app`: its menu commands come with `window.__menu(id)`, and
 * the folder dialog answers `picked`, keeping what it was asked in `window.__asked`).
 */
async function pageOn(s, { pc = false, app = false, picked = null } = {}) {
  const b = pc ? await puppeteer.launch({ executablePath: real, args: ["--no-sandbox"] }) : browser;
  const context = pc ? b.defaultBrowserContext() : await b.createBrowserContext();
  const page = await context.newPage();
  if (pc) await page.setUserAgent(pcAgent);
  await page.setViewport({ width: 1440, height: 900 });
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.evaluateOnNewDocument(
    (token, platform, app, picked) => {
      window.__M2__ = { native: app, token, platform };
      if (!app) return;
      const callbacks = new Map();
      const menu = [];
      let next = 1;
      window.__asked = [];
      window.__TAURI_INTERNALS__ = {
        invoke: async (cmd, args) => {
          if (cmd === "plugin:event|listen" && args?.event === "menu") menu.push(args.handler);
          if (cmd === "action") window.__asked.push(args);
          return cmd === "action" && args?.id === "song_folder" ? picked : null;
        },
        transformCallback: (cb) => {
          callbacks.set(next, cb);
          return next++;
        },
        unregisterCallback: (id) => callbacks.delete(id),
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__menu = (id) => menu.forEach((h) => callbacks.get(h)?.({ event: "menu", id: 0, payload: id }));
    },
    s.token,
    pc ? "linux" : "macos",
    app,
    picked,
  );
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=start], [data-testid=timeline]", { timeout: 20000 }).catch(() => null);
  return { page, close: () => (pc ? b.close() : context.close()) };
}

const DIALOG = "[data-testid=settings-dialog]";
const shown = (page) => page.$(DIALOG).then((e) => !!e);
const text = (page, sel) => page.$eval(sel, (e) => (e.tagName === "INPUT" ? e.value : (e.textContent ?? "")).trim()).catch(() => null);
const key = async (page, combo) => {
  const keys = combo.split("+");
  for (const k of keys.slice(0, -1)) await page.keyboard.down(k);
  await page.keyboard.press(keys.at(-1));
  for (const k of keys.slice(0, -1).reverse()) await page.keyboard.up(k);
};
/** The rows of the menu `testid` opens, as their words. */
async function menuRows(page, testid) {
  await page.click(`[data-testid=${testid}]`);
  await page.waitForSelector(`[data-testid=${testid}-menu]`, { timeout: 3000 }).catch(() => null);
  return page.$$eval(`[data-testid=${testid}-menu] [role=menuitemradio]`, (els) => els.map((e) => e.textContent.trim()));
}
/** Pick the row `choice` of the menu `testid` opens (opening it first). */
async function pick(page, testid, choice) {
  if (!(await page.$(`[data-testid=${testid}-menu]`))) await page.click(`[data-testid=${testid}]`);
  await page.waitForSelector(`[data-testid=${testid}-menu] [data-choice="${choice}"]`, { timeout: 3000 });
  await page.click(`[data-testid=${testid}-menu] [data-choice="${choice}"]`);
  await until(async () => !(await page.$(`[data-testid=${testid}-menu]`)), 2000);
}
const lint = (page, sel) => page.evaluate(lintIn, sel);
const toasts = (page) => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));

// ------------------------------------------------------------ the start screen: the run's server

{
  const sess = JSON.parse(fs.readFileSync(`${HOME}/.music2/session.json`, "utf8"));
  const { page, close } = await pageOn({ url: sess.url, token: sess.token });
  check(!!(await page.$("[data-testid=start]")), "the run's server shows the start screen");
  check(!(await shown(page)), "no Settings window at first");
  await key(page, "Meta+Comma");
  check(await until(() => shown(page), 3000), "⌘, opens the Settings window on the start screen");
  check(await until(async () => /starts when a song opens/.test((await text(page, "[data-testid=settings-now]")) ?? "")), `with no song open, nothing plays yet: ${await text(page, "[data-testid=settings-now]")}`);
  await page.keyboard.press("Escape");
  check(await until(async () => !(await shown(page)), 2000), "Esc closes it");
  await close();
}

// ------------------------------------------------------------ a song open, two outputs

const song = `${ROOT}/settings/First Song`;
fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
const s = await serve("settings-1", song);
const mac = await pageOn(s);
const page = mac.page;
await page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
await key(page, "Meta+Comma");
check(await until(() => shown(page), 3000), "⌘, opens it with a song open");
check(
  await until(async () => (await page.evaluate(() => document.activeElement?.dataset?.testid)) === "settings-done"),
  `Done has the focus: ${await page.evaluate(() => document.activeElement?.dataset?.testid)}`,
);
await until(async () => /Speakers/.test((await text(page, "[data-testid=settings-output]")) ?? ""));
const outs = await menuRows(page, "settings-output");
check(JSON.stringify(outs) === JSON.stringify(["System default (Speakers)", "Speakers", "Headphones"]), `the outputs, the default first: ${JSON.stringify(outs)}`);
const menuBad = await lint(page, "[data-testid=settings-output-menu]");
check(menuBad.length === 0, `the open menu passes the taste lint: ${menuBad.slice(0, 4).join(" | ")}`);
await pick(page, "settings-output", "Headphones");
const after = (await s.api("settings")).json;
check(after?.output?.device === "Headphones" && after?.now?.device === "Headphones", `Headphones chosen: the server plays there (${after?.now?.device}), and keeps it (${after?.output?.device})`);
check(
  JSON.stringify(after?.outputs?.map((o) => [o.id, o.name])) === JSON.stringify([["Speakers", "Speakers"], ["Headphones", "Headphones"]]) && after?.output?.name === "Headphones",
  `each output has an id, which the choice keeps, with its name (NX-S1): ${JSON.stringify(after?.output)}`,
);
check(await until(async () => /^Headphones · 48 kHz/.test((await text(page, "[data-testid=settings-now]")) ?? "")), `the Now row says so: ${await text(page, "[data-testid=settings-now]")}`);
check((await text(page, "[data-testid=settings-output]")) === "Headphones", `the output button says Headphones: ${await text(page, "[data-testid=settings-output]")}`);
check((await toasts(page)).length === 0, `no toast: a direct change (X21): ${JSON.stringify(await toasts(page))}`);

const bufs = await menuRows(page, "settings-buffer");
const wantBufs = ["System default", "64 samples (1.3 ms)", "128 samples (2.7 ms)", "256 samples (5.3 ms)", "512 samples (10.7 ms)", "1024 samples (21.3 ms)", "2048 samples (42.7 ms)"];
check(JSON.stringify(bufs) === JSON.stringify(wantBufs), `the buffer sizes, each with its time: ${JSON.stringify(bufs)}`);
await pick(page, "settings-buffer", "256");
check(await until(async () => (await s.api("settings")).json?.now?.frames === 256), `256 chosen: the player's buffer is 256 (${(await s.api("settings")).json?.now?.frames})`);
check(await until(async () => /256 samples \(5\.3 ms\)/.test((await text(page, "[data-testid=settings-now]")) ?? "")), `the Now row says "256 samples (5.3 ms)": ${await text(page, "[data-testid=settings-now]")}`);
check(!/Playback stopped/.test((await text(page, "[data-testid=settings-now]")) ?? ""), "stopped, nothing to say about play");

// A change while playing makes the new player where the song was, stopped, and says so.
await s.api("graph/transport", { action: "play" });
const played = await until(async () => ((await s.api("graph/status")).json?.beat ?? 0) > 0.5, 5000);
await pick(page, "settings-buffer", "512");
const stopped = async () => {
  const st = (await s.api("graph/status")).json;
  return st?.playing === false && st?.beat > 0.5;
};
check(played && (await until(stopped, 3000)), `while playing: play stopped where it was (beat ${(await s.api("graph/status")).json?.beat?.toFixed(2)})`);
check(await until(async () => /Playback stopped\. Press Play to go on\./.test((await text(page, "[data-testid=settings-now]")) ?? "")), `the window says "Playback stopped. Press Play to go on.": ${await text(page, "[data-testid=settings-now]")}`);

// The songs folder, typed (a browser page has a field): kept, and shown by New Song.
const songs = `${ROOT}/settings/My Songs`;
const field = "[data-testid=settings-songs]";
await page.click(field, { clickCount: 3 });
await page.keyboard.type("Songs");
await page.keyboard.press("Enter");
check(await until(async () => /Type a full path/.test((await text(page, "[data-testid=settings-songs-error]")) ?? "")), `a relative folder is refused in words: ${await text(page, "[data-testid=settings-songs-error]")}`);
await page.click(field, { clickCount: 3 });
await page.keyboard.type(`${song}/tracks`);
await page.keyboard.press("Enter");
check(
  await until(async () => (await text(page, "[data-testid=settings-songs-error]")) === "That folder is inside the song First Song. Pick a folder outside any song."),
  `a folder inside a song is refused in words: ${await text(page, "[data-testid=settings-songs-error]")}`,
);
await page.click(field, { clickCount: 3 });
await page.keyboard.type(songs);
await page.keyboard.press("Enter");
check(await until(async () => (await s.api("settings")).json?.songs === songs), `the folder typed is where new songs go: ${(await s.api("settings")).json?.songs}`);
check(await until(async () => !!(await page.$("[data-testid=settings-songs-default]"))), "\"Use the default\" shows once a folder is chosen");
const bad = await lint(page, DIALOG);
check(bad.length === 0, `the Settings window passes the taste lint: ${bad.slice(0, 4).join(" | ")}`);

// Tab goes round the window, never behind it; a menu's own keys.
await page.focus("[data-testid=settings-done]");
const order = [];
for (let i = 0; i < 8; i++) {
  await page.keyboard.press("Tab");
  order.push(await page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.tagName));
}
// The sound input and Hear yourself (io) come after the buffer.
const wantOrder = ["settings-output", "settings-buffer", "settings-input", "settings-hear", "settings-songs", "settings-songs-default", "settings-done", "settings-output"];
check(JSON.stringify(order) === JSON.stringify(wantOrder), `Tab goes round: ${order.join(" → ")}`);
await page.keyboard.press("Enter");
await page.waitForSelector("[data-testid=settings-output-menu]", { timeout: 2000 }).catch(() => null);
const focused = await page.evaluate(() => document.activeElement?.dataset?.choice);
await page.keyboard.press("ArrowUp");
const up = await page.evaluate(() => document.activeElement?.dataset?.choice);
await page.keyboard.press("Escape");
await sleep(150);
check(
  focused === "Headphones" && up === "Speakers" && !(await page.$("[data-testid=settings-output-menu]")) && (await shown(page)),
  `Return opens the menu on the chosen row (${focused}), ↑ moves (${up}), Esc closes the menu only`,
);
await page.keyboard.press("Escape");
check(await until(async () => !(await shown(page)), 2000), "Esc again closes the window");

// New Song's folder is the one chosen.
await key(page, "Meta+KeyN");
await page.waitForSelector("[data-testid=song-folder]", { timeout: 5000 }).catch(() => null);
check(await until(async () => (await text(page, "[data-testid=song-folder]")) === songs), `the New Song dialog saves in it: ${await text(page, "[data-testid=song-folder]")}`);
await page.keyboard.press("Escape");

// Kept in the HOME's settings file, and by a reload.
const kept = readJson(`${HOME}/.music2/settings.json`);
check(kept?.output?.device === "Headphones" && kept?.output?.name === "Headphones" && kept?.output?.frames === 512 && kept?.songs === songs, `.music2/settings.json has all three: ${JSON.stringify(kept)}`);
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
await key(page, "Meta+Comma");
await until(() => shown(page), 3000);
await until(async () => /Headphones/.test((await text(page, "[data-testid=settings-output]")) ?? ""));
const reloaded = [await text(page, "[data-testid=settings-output]"), await text(page, "[data-testid=settings-buffer]"), await text(page, "[data-testid=settings-songs]")];
check(JSON.stringify(reloaded) === JSON.stringify(["Headphones", "512 samples (10.7 ms)", songs]), `a reload shows them: ${JSON.stringify(reloaded)}`);
await page.click("[data-testid=settings-songs-default]");
check(await until(async () => (await s.api("settings")).json?.songs_chosen === false), "\"Use the default\" goes back to the default folder");
// A folder typed, then Done at once: a refusal keeps the window open and says why; a folder taken
// closes it (R4).
await page.click(field, { clickCount: 3 });
await page.keyboard.type("Songs");
await page.click("[data-testid=settings-done]");
check(
  (await until(async () => /Type a full path/.test((await text(page, "[data-testid=settings-songs-error]")) ?? ""))) && (await shown(page)),
  `a relative folder typed, then Done: the window stays open and says why: ${await text(page, "[data-testid=settings-songs-error]")}`,
);
await page.click(field, { clickCount: 3 });
await page.keyboard.type(songs);
await page.click("[data-testid=settings-done]");
check((await until(async () => !(await shown(page)), 3000)) && (await s.api("settings")).json?.songs === songs, "a full path typed, then Done: it is where new songs go, and Done closes the window");
await s.api("settings", { songs: null });

// While an export runs, the output and buffer lists are greyed and the window says why; once it
// ends they come back, the window still open (R5). A song of 300 bars, so the export takes a while.
const kickFile = `${song}/data/kick/hits.json`;
const kickWas = fs.readFileSync(kickFile, "utf8");
const long = JSON.parse(kickWas);
long.clips.push({ id: "setfar01", start: 1196, length: 4, notes: [{ pitch: "C1", start: 0, length: 0.25, velocity: 0.8 }] });
fs.writeFileSync(kickFile, JSON.stringify(long));
await until(async () => ((await s.api("graph")).json?.timeline?.end ?? 0) >= 1200, 10000);
const exported = await s.api("graph/export", {});
await key(page, "Meta+Comma");
await until(() => shown(page), 3000);
const listsOff = () => page.$$eval("[data-testid=settings-output], [data-testid=settings-buffer]", (els) => els.map((e) => e.disabled));
const waitSaid = async () => /Wait for the export to finish/.test((await text(page, DIALOG)) ?? "");
const during = (await until(async () => (await listsOff()).every(Boolean) && (await waitSaid()), 3000)) && ((await s.api("settings")).json?.exporting === true);
check(exported.status === 200 && during, `while an export runs, the output and buffer can't change, and the window says why (${exported.status}, ${JSON.stringify(await listsOff())})`);
const ended = await until(async () => (await s.api("settings")).json?.exporting === false, 120000);
const back = await until(async () => !(await listsOff()).some(Boolean) && !(await waitSaid()), 3000);
check(ended && back && (await shown(page)), `once it ends, they can change again, the window still open: ${JSON.stringify(await listsOff())}`);
await page.keyboard.press("Escape");
fs.writeFileSync(kickFile, kickWas);
await until(async () => ((await s.api("graph")).json?.timeline?.end ?? 0) < 1200, 10000);
await mac.close();
await s.stop();

// A new launch: the server reads the file before anything plays.
const s2 = await serve("settings-2", song);
const again = (await s2.api("settings")).json;
check(again?.now?.device === "Headphones" && again?.output?.frames === 512, `a new server plays on Headphones at 512 at once: ${JSON.stringify(again?.now)}`);

// The app: Settings… from the menu, and its folder picker, titled for what it picks.
const app = await pageOn(s2, { app: true, picked: songs });
await app.page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
await app.page.evaluate(() => window.__menu("settings"));
check(await until(() => shown(app.page), 3000), "the app's Settings… menu row opens it");
await until(async () => !!(await app.page.$("[data-testid=settings-songs]")));
const changeButton = await app.page.evaluateHandle((d) => [...document.querySelectorAll(`${d} button`)].find((b) => b.textContent.trim() === "Change…"), DIALOG);
await changeButton.click();
const asked = await until(async () => (await s2.api("settings")).json?.songs === songs);
const args = await app.page.evaluate(() => window.__asked.find((a) => a.id === "song_folder"));
check(asked && JSON.parse(args?.arg ?? "{}").title === "Choose Where New Songs Go", `Change… asks for a folder titled "Choose Where New Songs Go" and uses it: ${args?.arg}`);
await app.close();

// A PC's page: Ctrl+, and the ⌥K sheet's words.
const pc = await pageOn(s2, { pc: true });
await pc.page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
await key(pc.page, "Control+Comma");
check(await until(() => shown(pc.page), 3000), "Ctrl+, opens it on a PC page");
await pc.page.keyboard.press("Escape");
await key(pc.page, "Alt+KeyK");
await pc.page.waitForSelector("[data-testid=shortcuts]", { timeout: 3000 }).catch(() => null);
const sheetRows = Object.fromEntries(await pc.page.$$eval("[data-testid=shortcuts] dl > div", (els) => els.map((e) => [e.querySelector("dt").textContent, e.querySelector("dd").textContent])));
check(sheetRows.Settings === "Ctrl+,", `the ⌥K sheet lists Settings: ${sheetRows.Settings}`);
await pc.close();
await s2.stop();

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
