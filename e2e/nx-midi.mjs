// A MIDI keyboard (8b.10; Q3 to Q6, X37): plugged in, a toast and the top bar's mark, whose tooltip
// names it and what it plays; its keys play the selected track (the server's `live`); the window
// tells the server what plays as the selection changes (`midi.target`); the sustain pedal holds an
// Off until it lifts; unplugged while a key is held, the note stops, the mark goes and a toast says
// so; two keyboards; the Song row; a page loaded later and a PC page see it too; the lint.
//
// No keyboard can be opened here (`/dev/snd/seq` is EACCES): the suite starts a server of its own
// with `M2_MIDI=inject` and posts what a keyboard would send to `/api/graph/midi/inject`. The
// suite's usual server has that API only when run.mjs gives it `M2_MIDI=inject` too.
//
//   node e2e/nx-midi.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { HEADLESS, REPO } from "./env.mjs";
import { exe, pc as pcAgent, real } from "./chrome.mjs";
import { lint } from "./lintlib.mjs";
import { ROOT, check, done, open, sess, sleep, until } from "./fixlib.mjs";

// ---------------------------------------------------------------- the usual server

{
  const { page, browser, errors } = await open(1440, 900);
  const r = await page.evaluate(async (token) => {
    const res = await fetch("/api/graph/midi/inject", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ device: "Test Keys", plugged: true }) });
    return res.status;
  }, sess.token);
  // run.mjs may give every suite's server M2_MIDI=inject (the next track's P0), and the suite the
  // same environment: the API answers only then.
  const injects = process.env.M2_MIDI === "inject";
  check(r === (injects ? 200 : 404), `the inject API answers only with M2_MIDI=inject (M2_MIDI ${process.env.M2_MIDI ?? "unset"}: ${r})`);
  await browser.close();
  if (errors.length) console.log(`(console: ${errors.join(" | ")})`);
}

// ---------------------------------------------------------------- a server that takes injected keys

const root = `${ROOT}/inject`;
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(`${root}/home/.music2`, { recursive: true });
fs.cpSync(`${REPO}/examples/first-song`, `${root}/proj`, { recursive: true });
const sessFile = `${root}/home/.music2/session.json`;
const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "inject", HOME: `${root}/home` };
const server = spawn(HEADLESS, [`${root}/proj`, sessFile, `m${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${root}/headless.log`, "w")] });
const browsers = [];
const errors = [];
try {
  if (!(await until(() => fs.existsSync(sessFile), 20_000))) {
    check(false, "the inject server started");
    done(errors);
  }
  await sleep(300);
  const s2 = JSON.parse(fs.readFileSync(sessFile, "utf8"));

  /** A page on the inject server: a Mac's, or a PC's. */
  const page = async (pc = false) => {
    const b = await puppeteer.launch({ executablePath: pc ? real : exe, args: ["--no-sandbox"] });
    browsers.push(b);
    const p = await b.newPage();
    if (pc) await p.setUserAgent(pcAgent);
    await p.setViewport({ width: 1440, height: 900 });
    p.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
    p.on("pageerror", (e) => errors.push(String(e)));
    await p.evaluateOnNewDocument((token, platform) => {
      window.__M2__ = { native: false, token, platform };
      try {
        localStorage.setItem("m2.browser.open", "false");
      } catch {}
    }, s2.token, pc ? "linux" : "macos");
    await p.goto(s2.url, { waitUntil: "domcontentloaded" });
    await p.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
    return p;
  };
  const p1 = await page();
  const api = (path, body) =>
    p1.evaluate(
      async (token, path, body) => {
        const r = await fetch(`/api${path}`, { method: body ? "POST" : "GET", headers: { "x-m2-token": token, "content-type": "application/json" }, body: body && JSON.stringify(body) });
        return { status: r.status, body: await r.json().catch(() => ({})) };
      },
      s2.token,
      path,
      body,
    );
  const inject = (body) => api("/graph/midi/inject", body);
  const key = (bytes) => inject({ device: "Test Keys", bytes });
  const live = async () => (await api("/graph/status")).body.live;
  const liveIs = (n, ms = 3000) => until(async () => (await live()) === n, ms);
  const target = async () => (await api("/graph")).body.midi?.target ?? null;
  const targetIs = async (module, lane) => {
    const t = await target();
    return t?.module === module && t?.lane === lane;
  };
  const mark = (p = p1) => p.$eval("[data-testid=midi-status]", (e) => e.title).catch(() => null);
  const toasts = (p = p1) => p.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
  const select = (m, p = p1) => p.click(`[data-module-tab="${m}"] [data-testid=track-label]`);

  check((await mark()) === null, "no MIDI mark with no keyboard");
  await select("bass");
  check(await until(() => targetIs("bass", "notes"), 3000), `Bass selected, the window tells the server: MIDI plays bass.notes (${JSON.stringify(await target())})`);

  const plugged = await inject({ device: "Test Keys", plugged: true });
  check(plugged.status === 200, `a keyboard plugged in (${plugged.status})`);
  check(await until(async () => (await toasts()).some((t) => t.includes("Test Keys is connected. It plays the selected track.")), 3000), `a toast says so: ${JSON.stringify(await toasts())}`);
  check(await until(async () => (await mark()) === "MIDI keyboard: Test Keys · plays Bass", 3000), `the top bar's mark names it and what it plays: ${await mark()}`);
  const placed = await p1.evaluate(() => {
    const m = document.querySelector("[data-testid=midi-status]").getBoundingClientRect();
    const undo = document.querySelector('button[aria-label="Undo"]').getBoundingClientRect();
    return { m: [m.width, m.height], before: m.right <= undo.left };
  });
  check(placed.m.join() === "28,28" && placed.before, `a 28 px mark before the status pill and Undo (${JSON.stringify(placed)})`);

  await key([0x90, 60, 100]);
  check(await liveIs(1), "a key down plays a note (live 1)");
  await key([0x80, 60, 0]);
  check(await liveIs(0), "its key up ends it");

  await select("keys");
  check(await until(() => targetIs("keys", "chords"), 3000), `Keys selected: MIDI plays keys.chords (${JSON.stringify(await target())})`);
  check(await until(async () => (await mark()) === "MIDI keyboard: Test Keys · plays Keys", 3000), `the mark follows: ${await mark()}`);
  await key([0x90, 64, 90]);
  check(await liveIs(1), "a key plays Keys");
  // The pedal holds the Off until it lifts.
  await key([0xb0, 64, 127]);
  await key([0x80, 64, 0]);
  await sleep(300);
  check((await live()) === 1, "with the pedal down, the key let go still sounds");
  await key([0xb0, 64, 0]);
  check(await liveIs(0), "the pedal up ends it");

  // The Song row: nothing plays, and the mark says how to make it play.
  await select("song");
  check(await until(async () => (await target()) === null && (await mark()) === "MIDI keyboard: Test Keys · Select a track with notes to play it", 3000), `the Song row: nothing plays (${await mark()})`);
  await key([0x90, 60, 100]);
  await sleep(300);
  check((await live()) === 0, "a key plays nothing then");
  await key([0x80, 60, 0]);
  await select("bass");

  // Two keyboards.
  await inject({ device: "Pads", plugged: true });
  check(await until(async () => (await mark()) === "MIDI keyboards: Test Keys and Pads · plays Bass", 3000), `two keyboards: ${await mark()}`);
  const bad = await p1.evaluate(lint, "header");
  check(bad.length === 0, `the lint finds nothing in the top bar with the mark${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
  await p1.screenshot({ path: `${ROOT}/nx-midi-bar.png`, clip: { x: 0, y: 0, width: 1440, height: 44 } });
  await inject({ device: "Pads", plugged: false });

  // A page loaded now, and a PC page, see it from the state.
  const pcPage = await page(true);
  check(await until(async () => ((await mark(pcPage)) ?? "").startsWith("MIDI keyboard: Test Keys · "), 3000), `a PC page loaded now shows the mark: ${await mark(pcPage)}`);

  // Unplugged while a key is held.
  await select("bass");
  await key([0x90, 62, 100]);
  check(await liveIs(1), "a key held");
  await inject({ device: "Test Keys", plugged: false });
  check(await liveIs(0), "unplugged: its note stops");
  check(await until(async () => (await mark()) === null && (await mark(pcPage)) === null, 3000), "the mark goes, in both windows");
  check(await until(async () => (await toasts()).some((t) => t.includes("Test Keys was unplugged. Its notes stopped.")), 3000), `a toast says so: ${JSON.stringify(await toasts())}`);
} finally {
  for (const b of browsers) await b.close().catch(() => {});
  server.kill();
}

done(errors);
