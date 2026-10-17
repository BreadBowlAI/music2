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
//   C3  Save As (⇧⌘S, Q19): a copy under a new name, byte for byte without `.music2/`, which
//       opens, the song kept as it was, both in Recent; and Rename Song… from the menu (Q20): the
//       name and the folder, the name only, a taken folder refused in words. Neither dialog lets
//       a key reach the song behind it. A PC page's keys; the app's Choose Folder….
//
//   node e2e/pl-projects.mjs <root> <home>     (the `none` kind: no project open)
//   PL_ONLY=c3 runs one part alone (a red run on an older build, where an earlier part stops it).
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
/** Whether part `name` runs: every part, unless PL_ONLY names one. */
const part = (name) => !process.env.PL_ONLY || process.env.PL_ONLY === name;
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

/**
 * A page as the desktop app's (`native`), on server `s`: its menu bar's commands are sent with
 * `window.__menu(id)` as the shell sends them (a `menu` event), and the shell's folder dialog
 * answers `picked`.
 */
async function appPage(s, picked) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.evaluateOnNewDocument(
    (token, picked) => {
      window.__M2__ = { native: true, token, platform: "macos" };
      const callbacks = new Map();
      const menu = [];
      let next = 1;
      window.__TAURI_INTERNALS__ = {
        invoke: async (cmd, args) => {
          if (cmd === "plugin:event|listen" && args?.event === "menu") menu.push(args.handler);
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
    picked,
  );
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=start], [data-testid=timeline]", { timeout: 10000 });
  return page;
}

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

if (part("c1")) {
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
if (part("c2")) {
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

// ---------------------------------------------------------------- C3: Save As, and Rename Song

if (part("c3")) {
  const song = `${songs}/First Song`;
  fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
  fs.writeFileSync(`${song}/music2.toml`, fs.readFileSync(`${song}/music2.toml`, "utf8").replace('name = "first-song"', 'name = "First Song"'));
  await run.api("graph/open", { path: song });
  const status = async () => (await run.api("graph/status")).json;
  const files = (dir) => {
    const out = {};
    const walk = (d) => {
      for (const e of fs.readdirSync(`${dir}/${d}`, { withFileTypes: true })) {
        const rel = d ? `${d}/${e.name}` : e.name;
        if (e.isDirectory()) walk(rel);
        else out[rel] = fs.readFileSync(`${dir}/${rel}`, "utf8");
      }
    };
    for (const d of ["tracks", "data"]) walk(d);
    out["song.ts"] = fs.readFileSync(`${dir}/song.ts`, "utf8");
    return out;
  };
  const before = files(song);
  const page = await pageOn(run);
  await page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
  // A view setting of the song's own, which the copy doesn't take.
  await run.api("graph/view", { session: (await run.api("graph")).json.session, set: { audition: false } });

  await page.keyboard.down("Meta");
  await page.keyboard.down("Shift");
  await page.keyboard.press("s");
  await page.keyboard.up("Shift");
  await page.keyboard.up("Meta");
  const dialog = await page.waitForSelector("[data-testid=save-as-dialog]", { timeout: 3000 }).catch(() => null);
  check(!!dialog, "⇧⌘S opens Save As");
  const said = dialog ? await dialog.evaluate((e) => e.innerText) : "";
  check(said.includes("Save a copy of First Song") && said.includes("in ~/Music/music2"), `“Save a copy of First Song”, in ~/Music/music2: ${said.replace(/\n+/g, " | ")}`);
  const field = await page.$eval("[data-testid=save-as-name]", (e) => [e.value, document.activeElement === e && e.selectionStart === 0 && e.selectionEnd === e.value.length]).catch(() => []);
  check(same(field, ["First Song 2", true]), `the name “First Song 2”, focused and selected: ${JSON.stringify(field)}`);
  const bad = await lint(page, "[data-testid=save-as-dialog]");
  check(bad.length === 0, `the dialog keeps to the design system: ${bad.slice(0, 4).join(", ")}`);
  await page.screenshot({ path: `${ROOT}/pl-save-as.png` });
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press("Space");
  await page.keyboard.press("m");
  await sleep(500);
  check(!(await status()).playing && same((await run.api("graph")).json.overlays.mute, []) && !!(await page.$("[data-testid=save-as-dialog]")), "Space and M reach nothing behind it");

  await page.click("[data-testid=save-as-name]");
  await page.keyboard.press("Enter");
  const copy = `${songs}/First Song 2`;
  check(await until(async () => (await openName()) === "First Song 2" && (await run.api("graph")).json?.dir === copy), `Return makes “First Song 2” and opens it: ${await openName()}`);
  check(same(files(copy), before), "its song.ts, tracks/ and data/ are the song's, byte for byte");
  check(!fs.existsSync(`${copy}/.music2/view.json`) && !fs.existsSync(`${copy}/example.toml`), "without the song's .music2/ view settings");
  check(/^name = "First Song 2"$/m.test(fs.readFileSync(`${copy}/music2.toml`, "utf8")), "its name is in its music2.toml");
  check(same(files(song), before) && /^name = "First Song"$/m.test(fs.readFileSync(`${song}/music2.toml`, "utf8")) && readJson(`${song}/.music2/view.json`)?.audition === false, "the song itself is as it was");
  check(same(readJson(recentFile)?.songs?.slice(0, 2).map((s) => s.path), [copy, song]), "both are in Recent, the copy first");
  check(await until(async () => (await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent))).some((t) => t.includes("Now working on First Song 2. First Song is kept as it was."))), "a toast says which one is open now");
  await page.browserContext().close();

  // A PC page: Ctrl+Shift+S, and the sheet says so.
  const pc = await pageOn(run, { pc: true });
  await pc.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
  await pc.keyboard.down("Control");
  await pc.keyboard.down("Shift");
  await pc.keyboard.press("s");
  await pc.keyboard.up("Shift");
  await pc.keyboard.up("Control");
  check(!!(await pc.waitForSelector("[data-testid=save-as-dialog]", { timeout: 3000 }).catch(() => null)), "on a PC page, Ctrl+Shift+S opens it");
  await pc.keyboard.press("Escape");
  await pc.keyboard.down("Alt");
  await pc.keyboard.press("k");
  await pc.keyboard.up("Alt");
  await pc.waitForSelector("[data-testid=shortcuts]", { timeout: 3000 }).catch(() => {});
  const row = await pc.$$eval("[data-testid=shortcuts] dl > div", (els) => els.map((e) => e.innerText.replace(/\s+/g, " ")).find((t) => t.startsWith("Save a copy")) ?? null);
  check(row === "Save a copy (Save As) Ctrl+Shift+S", `the shortcuts sheet: ${row}`);
  await pc.browserContext().close();

  // Rename Song…, from the app's menu.
  const elsewhere = `${HOME}/Elsewhere`;
  fs.mkdirSync(elsewhere, { recursive: true });
  const app = await appPage(run, elsewhere);
  await app.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
  await app.evaluate(() => window.__menu("rename_song"));
  const rename = await app.waitForSelector("[data-testid=rename-song-dialog]", { timeout: 3000 }).catch(() => null);
  check(!!rename, "File ▸ Rename Song… opens its dialog");
  const words = rename ? await rename.evaluate((e) => e.innerText) : "";
  const box = await app.$eval("[data-testid=rename-song-folder]", (e) => e.checked).catch(() => null);
  check(
    words.includes("Rename First Song 2") && box === true && words.includes("The song closes and opens again. After that, Undo can't go back past this."),
    `“Rename First Song 2”, Also rename its folder (on), and what that does: ${words.replace(/\n+/g, " | ")}`,
  );
  const badRename = await lint(app, "[data-testid=rename-song-dialog]");
  check(badRename.length === 0, `the dialog keeps to the design system: ${badRename.slice(0, 4).join(", ")}`);
  await app.screenshot({ path: `${ROOT}/pl-rename.png` });
  await app.evaluate(() => document.activeElement?.blur());
  await app.keyboard.press("Space");
  await app.keyboard.press("m");
  await sleep(500);
  check(!(await status()).playing && same((await run.api("graph")).json.overlays.mute, []) && !!(await app.$("[data-testid=rename-song-dialog]")), "Space and M reach nothing behind it");

  // A taken folder: refused in words, and nothing changes.
  await app.click("[data-testid=rename-song-name]", { clickCount: 3 });
  await app.keyboard.type("first song");
  await app.keyboard.press("Enter");
  const refused = await app.waitForSelector("[data-testid=rename-song-error]", { timeout: 3000 }).then((e) => e.evaluate((e) => e.textContent)).catch(() => null);
  check(refused === "A folder called first song is already there. Pick another name.", `a folder that is there is refused: ${refused}`);
  check(fs.existsSync(`${copy}/song.ts`) && /^name = "First Song 2"$/m.test(fs.readFileSync(`${copy}/music2.toml`, "utf8")), "and nothing changed");

  // With the box on: the name and the folder; the song opens again from there.
  await app.click("[data-testid=rename-song-name]", { clickCount: 3 });
  await app.keyboard.type("Night Drive");
  await app.keyboard.press("Enter");
  const drive = `${songs}/Night Drive`;
  check(await until(async () => (await run.api("graph")).json?.dir === drive, 10000), `the song is open from its new folder: ${(await run.api("graph")).json?.dir}`);
  check(!fs.existsSync(copy) && /^name = "Night Drive"$/m.test(fs.readFileSync(`${drive}/music2.toml`, "utf8")), "its folder and its name are Night Drive");
  check(readJson(recentFile)?.songs?.[0]?.path === drive && !readJson(recentFile)?.songs?.some((s) => s.path === copy), "Recent follows the folder");

  // With the box off: only the name.
  await app.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
  await app.evaluate(() => window.__menu("rename_song"));
  await app.waitForSelector("[data-testid=rename-song-dialog]", { timeout: 3000 });
  await app.click("[data-testid=rename-song-folder]");
  check(!(await app.$("[data-testid=rename-song-note]")), "with the box off, the line about closing goes");
  await app.click("[data-testid=rename-song-name]", { clickCount: 3 });
  await app.keyboard.type("Night Ride");
  await app.keyboard.press("Enter");
  check(await until(async () => /^name = "Night Ride"$/m.test(fs.readFileSync(`${drive}/music2.toml`, "utf8"))), "with it off: only the name changes");
  check((await run.api("graph")).json?.dir === drive && (await run.api("graph")).json?.can_undo === true, "the folder stays, and it is one undo step");

  // Save As in the app: Choose Folder….
  await app.evaluate(() => window.__menu("save_as"));
  await app.waitForSelector("[data-testid=save-as-dialog]", { timeout: 3000 });
  await app.$$eval("[data-testid=save-as-dialog] button", (bs) => bs.find((b) => b.textContent.trim() === "Choose Folder…")?.click());
  check(await until(async () => (await app.$eval("[data-testid=save-as-folder]", (e) => e.textContent)) === "~/Elsewhere"), "in the app, Choose Folder… picks where the copy goes");
  await app.click("[data-testid=save-as-save]");
  check(await until(async () => (await run.api("graph")).json?.dir === `${elsewhere}/Night Ride 2`), `the copy is made there and opens: ${(await run.api("graph")).json?.dir}`);
  await app.browserContext().close();
}

// ---------------------------------------------------------------- the end

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
for (const s of servers) s.kill("SIGKILL");
await browser.close();
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
