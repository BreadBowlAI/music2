// Polish lane C: songs. On the run's headless server (no song open, a scratch HOME) and on servers
// of its own (E2E_HEADLESS, the same HOME), each on a copy of first-song:
//   C1  view settings kept per song (Q16): the roll's grid, the timeline's snap and the audition
//       toggle are as left after the server stops and another starts (another port, another
//       browser context: nothing in the browser's storage), and `.music2/view.json` holds them; a
//       new song starts with the defaults.
//   C2  recent songs (Q17, taste.md SS-2): songs made with New Song are listed newest first on the
//       start screen with their tempo and "opened today"; a click opens one, Return opens the
//       newest; a folder that is gone says "Moved or deleted" and its × forgets it; Close Song
//       keeps the newest from opening at the next launch; `~/.music2/recent.json` holds the list.
//
//   node e2e/pl-projects.mjs <root> <home>     (the `none` kind: no project open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { exe } from "./chrome.mjs";
import { HEADLESS, REPO } from "./env.mjs";

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
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const readJson = (f) => {
  try {
    return JSON.parse(fs.readFileSync(f, "utf8"));
  } catch {
    return null;
  }
};

const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const errors = [];
const servers = [];

/** A headless server of this suite's own on `dir` (or none: "-"), with the run's HOME. */
async function serve(name, dir = "-") {
  const file = `${ROOT}/${name}.json`;
  fs.rmSync(file, { force: true });
  const env = { ...process.env, HOME, M2_AUDIO: "none" };
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

/** A page on server `s`, in a browser context of its own: nothing in its storage. */
async function pageOn(s, { pc = false } = {}) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
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
  if (!(await page.waitForSelector("[data-testid=start], [data-testid=timeline]", { timeout: 20000 }).catch(() => null))) {
    console.log(`the page on ${s.url} shows: ${(await page.evaluate(() => document.documentElement.outerHTML)).slice(0, 600)} ${errors.join(" | ")}`);
    await page.screenshot({ path: `${ROOT}/page-${Date.now()}.png` });
  }
  return page;
}

/** The taste lint (e2e/taste.mjs) over what is inside `sel`: font sizes, radii, gradients, backdrop filters, mono text. */
const lint = (page, sel) =>
  page.evaluate((sel) => {
    const bad = [];
    for (const root of document.querySelectorAll(sel))
      for (const e of [root, ...root.querySelectorAll("*")]) {
        if (!e.getClientRects().length) continue;
        const s = getComputedStyle(e);
        const texty = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
        if (texty && e.namespaceURI === "http://www.w3.org/1999/xhtml" && ![11, 12, 13, 15].includes(parseFloat(s.fontSize))) bad.push(`font-size ${s.fontSize}`);
        if (texty && /mono|menlo|courier/i.test(s.fontFamily) && !e.closest("[data-code]")) bad.push("mono font");
        for (const c of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"]) {
          const r = parseFloat(s[c]);
          if (![0, 2, 4, 8].includes(r) && r < 9999) bad.push(`radius ${s[c]}`);
        }
        if (/gradient/.test(s.backgroundImage)) bad.push("gradient");
        if (s.backdropFilter && s.backdropFilter !== "none") bad.push("backdrop-filter");
      }
    return [...new Set(bad)];
  }, sel);

/** What the grid chips and the audition toggle show. */
const shown = (page) =>
  page.evaluate(() => {
    const at = (id) => document.querySelector(`[data-testid=${id}]`);
    return {
      roll: at("grid")?.dataset.grid ?? null,
      snap: at("song-snap")?.getAttribute("aria-pressed") ?? null,
      audition: at("audition")?.getAttribute("aria-pressed") ?? null,
    };
  });

/** Open Bass's first clip in Notes. */
async function openBass(page) {
  await page.waitForSelector('[data-lane="bass.notes"] [data-clip]', { timeout: 10000 });
  const c = await (await page.$('[data-lane="bass.notes"] [data-clip]')).boundingBox();
  await page.mouse.click(c.x + 20, c.y + c.height / 2, { clickCount: 2 });
  return !!(await page.waitForSelector("[data-testid=roll-grid]", { timeout: 5000 }).catch(() => null));
}

// ---------------------------------------------------------------- C1: view settings kept per song

{
  const song = `${ROOT}/kept/First Song`;
  fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
  const view = () => readJson(`${song}/.music2/view.json`);
  const s1 = await serve("kept-1", song);
  const p1 = await pageOn(s1);
  check(await openBass(p1), "server 1: Bass's clip opens in Notes");
  check(same(await shown(p1), { roll: "1/16", snap: "true", audition: "true" }), `at first the roll's grid is 1/16, snap is on, audition is on: ${JSON.stringify(await shown(p1))}`);
  await p1.click("[data-testid=grid]");
  await p1.waitForSelector("[role=menu][aria-label=Grid]");
  await p1.click('[role=menu][aria-label=Grid] [data-grid="1/8"]');
  await p1.click("[data-testid=song-snap]");
  await p1.click("[data-testid=audition]");
  check(same(await shown(p1), { roll: "1/8", snap: "false", audition: "false" }), `set: the roll's grid 1/8, the timeline's snap off, audition off: ${JSON.stringify(await shown(p1))}`);
  const kept = (v) => v?.grid?.notes?.pick === 0.5 && v?.grid?.song?.on === false && v?.audition === false;
  check(await until(async () => kept(view()), 5000), `.music2/view.json holds them: ${JSON.stringify(view())}`);
  await p1.browserContext().close();
  await s1.stop();

  const s2 = await serve("kept-2", song);
  check(s2.url !== s1.url, `server 2 is on another port (${s1.url.split(":").at(-1)} → ${s2.url.split(":").at(-1)}), so the page's storage is another`);
  const p2 = await pageOn(s2);
  check(await openBass(p2), "server 2: Bass's clip opens in Notes");
  const after = await shown(p2);
  check(after.roll === "1/8", `the roll's grid is 1/8 after a new launch: ${after.roll}`);
  check(after.snap === "false", `the timeline's snap is still off: ${after.snap}`);
  check(after.audition === "false", `audition is still off: ${after.audition}`);

  // Another song: its own settings, the defaults. Then back: this one's again.
  const other = `${ROOT}/kept/Other Song`;
  fs.cpSync(`${REPO}/examples/first-song`, other, { recursive: true });
  await s2.api("graph/open", { path: other });
  await until(async () => (await s2.api("graph")).json?.dir === other);
  check(await openBass(p2), "another song opens in the same window");
  check(same(await shown(p2), { roll: "1/16", snap: "true", audition: "true" }), `a new song starts with the defaults: ${JSON.stringify(await shown(p2))}`);
  await s2.api("graph/open", { path: song });
  await until(async () => (await s2.api("graph")).json?.dir === song);
  check(await openBass(p2), "back to the first song");
  check(same(await shown(p2), { roll: "1/8", snap: "false", audition: "false" }), `its settings come back with it: ${JSON.stringify(await shown(p2))}`);
  check(!fs.existsSync(`${other}/.music2/view.json`) || !readJson(`${other}/.music2/view.json`)?.grid, "and nothing was kept for the other song");
  await p2.browserContext().close();
  await s2.stop();
}

// ---------------------------------------------------------------- C2: recent songs

// The run's own server: no song open, songs made in HOME's ~/Music/music2.
const sess = JSON.parse(fs.readFileSync(`${HOME}/.music2/session.json`, "utf8"));
const run = { ...sess, api: null };
run.api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: { "x-m2-token": sess.token } } : { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
const recentFile = `${HOME}/.music2/recent.json`;
const songs = `${HOME}/Music/music2`;
const openName = async () => (await run.api("graph")).json?.name ?? null;
/** The start screen's recent rows: name and the line under it, top to bottom. */
const rows = (page) =>
  page.$$eval("[data-testid=recent] [data-recent]", (els) => els.map((e) => [...e.querySelectorAll("span")].map((s) => s.textContent).join(" | ")));
{
  // Only what this part makes (C1's servers kept the HOME's list too).
  fs.rmSync(recentFile, { force: true });
  for (const [name, tempo] of [["Alpha", 120], ["Beta", 96], ["Gamma", undefined]]) await run.api("project/new", { name, tempo });
  await run.api("graph/close", {});
  const page = await pageOn(run);
  const listed = await page.waitForSelector("[data-testid=recent] [data-recent]", { timeout: 5000 }).catch(() => null);
  check(!!listed, "the start screen lists recent songs");
  check(
    same(await rows(page), ["Gamma | 116 BPM · opened today", "Beta | 96 BPM · opened today", "Alpha | 120 BPM · opened today"]),
    `newest first, each with its tempo and when it was opened: ${JSON.stringify(await rows(page))}`,
  );
  const title = await page.$eval("[data-testid=recent] [data-recent]", (e) => e.title);
  check(title === "~/Music/music2/Gamma", `its folder is in the tooltip: ${title}`);
  const box = await (await page.$("[data-testid=recent]")).boundingBox();
  const row = await (await page.$("[data-testid=recent] [data-recent]")).boundingBox();
  check(Math.round(box.width) === 560 && Math.round(row.height) === 44, `a 560 px column of 44 px rows: ${Math.round(box.width)}, ${Math.round(row.height)}`);
  const bad = await lint(page, "[data-testid=start]");
  check(bad.length === 0, `the start screen with the list keeps to the design system: ${bad.slice(0, 4).join(", ")}`);
  await page.screenshot({ path: `${ROOT}/pl-recent.png` });
  const kept = readJson(recentFile);
  check(same(kept?.songs?.map((s) => s.path), [`${songs}/Gamma`, `${songs}/Beta`, `${songs}/Alpha`]) && kept?.open === false, `recent.json holds them, and Close Song marked the newest closed: ${JSON.stringify(kept)}`);

  // Return, with no button focused, opens the newest.
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press("Enter");
  check(await until(async () => (await openName()) === "Gamma" && !!(await page.$("[data-testid=timeline]"))), `Return opens the newest: ${await openName()}`);
  check(readJson(recentFile)?.open === true, "recent.json says it is open (so a launch now would open it again)");
  await run.api("graph/close", {});
  await page.waitForSelector("[data-testid=recent] [data-recent]", { timeout: 5000 });

  // A click opens one, which goes first.
  await page.click(`[data-recent="${songs}/Alpha"]`);
  check(await until(async () => (await openName()) === "Alpha"), `a click on Alpha opens it: ${await openName()}`);
  await run.api("graph/close", {});
  await page.waitForSelector("[data-testid=recent] [data-recent]", { timeout: 5000 });
  check(same((await rows(page)).map((r) => r.split(" | ")[0]), ["Alpha", "Gamma", "Beta"]), `and goes first: ${JSON.stringify(await rows(page))}`);

  // A folder that is gone: "Moved or deleted", and its × takes it off.
  fs.rmSync(`${songs}/Beta`, { recursive: true, force: true });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=recent] [data-missing]", { timeout: 5000 }).catch(() => {});
  check((await rows(page))[2] === "Beta | Moved or deleted", `Beta's folder is gone: ${(await rows(page))[2]}`);
  const faint = await page.$eval(`[data-recent="${songs}/Beta"] span`, (e) => getComputedStyle(e).color).catch(() => null);
  const plain = await page.$eval(`[data-recent="${songs}/Alpha"] span`, (e) => getComputedStyle(e).color).catch(() => null);
  check(!!faint && faint !== plain, `its name is fainter: ${faint} (Alpha's ${plain})`);
  await page.click(`[data-recent="${songs}/Beta"]`);
  await sleep(400);
  check((await openName()) === null, "a click on it opens nothing");
  await page.click(`[data-forget="${songs}/Beta"]`);
  check(await until(async () => same((await rows(page)).map((r) => r.split(" | ")[0]), ["Alpha", "Gamma"])), `its × takes it off the list: ${JSON.stringify(await rows(page))}`);
  check(same(readJson(recentFile)?.songs?.map((s) => s.path), [`${songs}/Alpha`, `${songs}/Gamma`]), "and off recent.json");
  await page.browserContext().close();
}

// ---------------------------------------------------------------- the end

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
for (const s of servers) s.kill("SIGKILL");
await browser.close();
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
