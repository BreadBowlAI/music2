// The metronome (gaps #19; the next track's B1): its toggle after Loop in the top bar (taste.md §3.3),
// K, kept per song in `.music2/view.json` (a reload and a reopen keep it), the ⌥K sheet's row, a PC
// page's K, the song's name keeping its room at 1100 px, the lint, and on a server of the suite's
// own that plays into a file (M2_AUDIO_CAPTURE): a click on each beat while it plays, the bar's
// first the louder, and none once it is off.
//
//   node e2e/nx-click.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { HEADLESS, REPO } from "./env.mjs";
import { exe } from "./chrome.mjs";
import { lint } from "./lintlib.mjs";
import { PROJ, ROOT, check, done, open, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, box } = await open(1440, 900);
const ACCENT_SOFT = "rgb(224, 234, 251)";

const toggle = (p = page) =>
  p.$eval("[data-testid=metronome-toggle]", (e) => ({ on: e.getAttribute("aria-pressed") === "true", bg: getComputedStyle(e).backgroundColor, title: e.title })).catch(() => null);
const view = () => {
  try {
    return JSON.parse(fs.readFileSync(`${PROJ}/.music2/view.json`, "utf8"));
  } catch {
    return {};
  }
};
const local = async () => (await state()).local ?? {};
/** Press K with nothing focused, as a user does: the page's key. */
const pressK = async (p = page) => {
  await p.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await p.keyboard.press("k");
};

// ---------------------------------------------------------------- the button (taste.md §3.3)

const t0 = await toggle();
check(!!t0 && !t0.on, `the top bar has a Metronome toggle, off for a song that never had it on (${JSON.stringify(t0)})`);
check(t0?.title === "Metronome: a click on each beat (K)", `its tooltip says what it does and its key: ${t0?.title}`);
const loopBox = await box("[data-testid=loop-toggle]");
const metBox = await box("[data-testid=metronome-toggle]");
check(
  !!loopBox && !!metBox && Math.abs(metBox.x - (loopBox.x + loopBox.width + 4)) < 1 && metBox.width === 28 && metBox.height === 28,
  `it sits right after Loop, 28 px, 4 px from it (loop ${loopBox?.x}+${loopBox?.width}, metronome ${metBox?.x} ${metBox?.width}×${metBox?.height})`,
);

// ---------------------------------------------------------------- a click and K, kept per song

await page.click("[data-testid=metronome-toggle]");
await until(async () => (await toggle())?.bg === ACCENT_SOFT, 2000);
const t1 = await toggle();
check(t1?.on && t1.bg === ACCENT_SOFT, `a click turns it on, in accent-soft (${JSON.stringify(t1)})`);
check(await until(() => view().metronome === true, 3000), `the song's view.json keeps it: ${JSON.stringify(view())}`);
check((await local()).metronome === true, "the state's local has it");

await pressK();
check(await until(async () => (await toggle())?.on === false, 2000), "K turns it off");
check(await until(() => !("metronome" in view()), 3000), `off, view.json drops it: ${JSON.stringify(view())}`);
await pressK();
check(await until(async () => (await toggle())?.on === true, 2000), "K turns it on again");

// Typing in a field: K is a letter, not the metronome.
const name = await box("[data-testid=bar-song-name]");
await page.mouse.click(name.x + name.width / 2, name.y + name.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
await page.keyboard.type("k");
const typed = await page.$eval("[data-testid=song-title-input]", (e) => e.value).catch(() => "");
check(typed.endsWith("k") && (await toggle())?.on === true, `K typed into the song's name types a k and leaves the metronome on (${typed})`);
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=song-title-input]")));

// A reload, then the song closed and opened again: still on.
await until(() => view().metronome === true, 3000);
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
check((await toggle())?.on === true, "a reload keeps it on");
await api("/close", { method: "POST", body: "{}" });
const reopened = await api("/open", { method: "POST", body: JSON.stringify({ path: PROJ }) });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
check(reopened.status === 200 && reopened.body.local?.metronome === true && (await until(async () => (await toggle())?.on === true, 5000)), `the song opened again keeps it on (${reopened.status})`);

// ---------------------------------------------------------------- the ⌥K sheet, the lint, the room

await page.keyboard.down("Alt");
await page.keyboard.press("KeyK");
await page.keyboard.up("Alt");
const row = await page
  .waitForSelector("[data-testid=shortcuts]", { timeout: 2000 })
  .then(() => page.$$eval("[data-testid=shortcuts] dl > div", (rows) => rows.map((r) => [r.querySelector("dt")?.textContent, r.querySelector("dd")?.textContent])))
  .then((rows) => rows.find(([what]) => what === "Metronome on / off"))
  .catch(() => null);
check(row?.[1] === "K", `the ⌥K sheet lists "Metronome on / off: K" (${JSON.stringify(row)})`);
await page.keyboard.press("Escape");

const bad = await page.evaluate(lint, "header");
check(bad.length === 0, `the lint finds nothing in the top bar with the metronome on${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);

// The left group is 32 px wider. At a page's smallest window (1100 px) a long name still clears it
// and Undo. In the Mac app, whose bar starts 80 px in, the name then has under 56 px there, so it
// goes (taste.md §3.8 drops it after the time), and nothing overlaps; from 1180 px it shows again.
const nameRoom = async (w, mac) => {
  await page.evaluate((mac) => (mac ? (document.documentElement.dataset.app = "mac") : delete document.documentElement.dataset.app), mac);
  await page.setViewport({ width: w, height: 700 });
  await sleep(300);
  const n = await box("[data-testid=bar-song-name]");
  if (!n) return page.evaluate(() => ({ gone: true, left: document.querySelector("[data-testid=tempo]").getBoundingClientRect().right, right: document.querySelector('button[aria-label="Undo"]').getBoundingClientRect().left }));
  await page.mouse.click(n.x + n.width / 2, n.y + n.height / 2, { count: 2 });
  await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
  await page.keyboard.type(" and a much longer name than fits");
  await page.screenshot({ path: `${ROOT}/nx-click-name-${w}${mac ? "-mac" : ""}.png`, clip: { x: 0, y: 0, width: w, height: 44 } });
  const room = await page.evaluate(() => {
    const r = (s) => {
      const b = document.querySelector(s)?.getBoundingClientRect();
      return b && { left: b.left, right: b.right };
    };
    return { field: r("[data-testid=song-title-input]"), left: r("[data-testid=tempo]").right, right: r('button[aria-label="Undo"]').left };
  });
  await page.keyboard.press("Escape");
  await until(async () => !(await page.$("[data-testid=song-title-input]")));
  return room;
};
const web = await nameRoom(1100, false);
const mac = await nameRoom(1100, true);
// Record took 32 px more (studio lane B): in the Mac app the name comes back from about 1200 px.
const mac1280 = await nameRoom(1280, true);
const clears = (r) => !!r.field && r.field.left >= r.left + 4 && r.field.right <= r.right - 4;
check(
  clears(web) && mac.gone && mac.left < mac.right && clears(mac1280),
  `the song's name keeps its room: at 1100 px its field (${web.field?.left?.toFixed(0)}–${web.field?.right?.toFixed(0)}) clears the tempo (…${web.left?.toFixed(0)}) and Undo (${web.right?.toFixed(0)}…); in the Mac app it goes at 1100 px (${JSON.stringify(mac)}) and clears them again at 1280 (${mac1280.field?.left?.toFixed(0)}–${mac1280.field?.right?.toFixed(0)})`,
);
await page.evaluate(() => delete document.documentElement.dataset.app);
await page.setViewport({ width: 1440, height: 900 });

// ---------------------------------------------------------------- a PC page

const pc = await open(1440, 900, { pc: true });
check((await toggle(pc.page))?.on === true, "a PC page shows it on too");
await pressK(pc.page);
check(await until(async () => (await toggle(pc.page))?.on === false && (await local()).metronome !== true, 3000), "K on a PC page turns it off");
await pc.browser.close();

errors.push(...pc.errors);
await heard();

done(errors);

/** A server of the suite's own that plays into a file: an empty song, so what it plays is the click. */
async function heard() {
  const root = `${ROOT}/capture`;
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(`${root}/home/.music2`, { recursive: true });
  fs.cpSync(`${REPO}/templates/empty`, `${root}/proj`, { recursive: true });
  const capture = `${root}/played.f32`;
  const sessFile = `${root}/home/.music2/session.json`;
  const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "none", M2_AUDIO_CAPTURE: capture, HOME: `${root}/home` };
  const server = spawn(HEADLESS, [`${root}/proj`, sessFile, `c${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${root}/headless.log`, "w")] });
  const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
  try {
    if (!(await until(() => fs.existsSync(sessFile), 20_000))) return check(false, "the capture server started");
    await sleep(300);
    const s2 = JSON.parse(fs.readFileSync(sessFile, "utf8"));
    const p2 = await browser.newPage();
    await p2.setViewport({ width: 1440, height: 900 });
    await p2.evaluateOnNewDocument((token) => {
      window.__M2__ = { native: false, token, platform: "macos" };
    }, s2.page_token);
    await p2.goto(s2.url, { waitUntil: "domcontentloaded" });
    await p2.waitForSelector("[data-testid=metronome-toggle]", { timeout: 10000 });
    const transport = (action) =>
      p2.evaluate(
        (token, action) => fetch("/api/graph/transport", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ action }) }),
        s2.page_token,
        action,
      );
    await until(() => fs.existsSync(capture), 5000);
    const frames = () => Math.floor(fs.statSync(capture).size / 8);
    const left = (a, b) => {
      const buf = fs.readFileSync(capture);
      const x = new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.length / 4));
      const out = [];
      for (let i = a; i < Math.min(b, x.length / 2); i++) out.push(x[2 * i]);
      return out;
    };
    /** Where blips start (after 100 ms of quiet), and each one's peak over 30 ms. */
    const blips = (x) => {
      const found = [];
      let quiet = 4800;
      for (let i = 0; i < x.length; i++) {
        if (Math.abs(x[i]) > 0.01 && quiet >= 4800) {
          let peak = 0;
          for (let j = i; j < Math.min(i + 1440, x.length); j++) peak = Math.max(peak, Math.abs(x[j]));
          found.push({ at: i, peak });
        }
        quiet = Math.abs(x[i]) < 1e-4 ? quiet + 1 : 0;
      }
      return found;
    };
    // On, playing from the start for 2.3 s at 120 bpm: beats at 0, 0.5, 1, 1.5 and 2 s.
    await p2.click("[data-testid=metronome-toggle]");
    await sleep(400);
    const f0 = frames();
    await transport("play");
    await sleep(2300);
    await transport("stop");
    await sleep(200);
    const on = blips(left(f0, frames()));
    const gaps = on.slice(1).map((b, i) => b.at - on[i].at);
    // 4/4: beats 0 and 4 start bars.
    const accent = (b, i) => (i % 4 === 0 ? b.peak > 0.2 : b.peak > 0.1 && b.peak < 0.2);
    check(
      on.length >= 4 && gaps.every((g) => g === 24000) && on.every(accent),
      `on and playing, a click on each beat, exactly 0.5 s apart, each bar's first the louder (${on.map((b) => `${b.at}:${b.peak.toFixed(3)}`).join(" ")})`,
    );
    // Off: nothing.
    await p2.click("[data-testid=metronome-toggle]");
    await sleep(400);
    const f1 = frames();
    await transport("play");
    await sleep(1300);
    await transport("stop");
    await sleep(200);
    const off = left(f1, frames());
    const loudest = off.reduce((m, v) => Math.max(m, Math.abs(v)), 0);
    check(off.length > 48000 && loudest < 1e-3, `off, nothing is heard while it plays (peak ${loudest.toExponential(1)} over ${off.length} frames)`);
  } finally {
    await browser.close();
    server.kill();
  }
}
