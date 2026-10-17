// After the old version's removal: the graph view, page keys, ⌘S, closing to the start screen, and the API catch-all.
// node start.mjs <scratch> [empty]   (empty: the server was started with no project)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const empty = process.argv[3] === "empty";
const proj = `${S}/proj`;
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const H = { "x-m2-token": sess.token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: H } : { method: "POST", headers: { ...H, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, type: r.headers.get("content-type"), json: await r.json().catch(() => null) };
};
let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const text = () => page.evaluate(() => document.body.innerText);

await page.goto(sess.url, { waitUntil: "domcontentloaded" });

if (!empty) {
  await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });
  check(true, "the graph view appears");

  // Space plays and stops (a page key, outside the app).
  await page.click("[data-testid=graph]");
  await page.keyboard.press("Space");
  check(await until(async () => (await api("graph/status")).json.playing), "Space plays");
  await page.keyboard.press("Space");
  check(await until(async () => !(await api("graph/status")).json.playing), "Space again stops");
  await page.keyboard.press("Enter");
  check(await until(async () => (await api("graph/status")).json.beat === 0), "Enter goes back to the start");

  // A knob drag (a pan, on the Mix view's strip), then ⌘Z undoes it, with no toast.
  await page.click('[data-view-tab="mix"]');
  await page.waitForSelector('[data-testid=mix] [data-knob="hats.pan"]');
  const songTs = () => fs.readFileSync(`${proj}/song.ts`, "utf8");
  const before = songTs();
  const knob = '[data-testid=mix] [data-knob="hats.pan"] svg';
  await page.$eval(knob, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const box = await (await page.$(knob)).boundingBox();
  const cx = box.x + box.width / 2, cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  // 60px of a knob's 300 (taste.md KN-1): 0.3 → 0.7 on a -1..1 range.
  for (let i = 1; i <= 8; i++) await page.mouse.move(cx, cy - i * 7.5);
  await page.mouse.up();
  check(await until(() => songTs().includes("pan: 0.7 }")), "a knob drag writes song.ts");
  // The knob shows it: a knob change makes no toast (taste.md §2.14).
  check(await until(async () => (await page.$eval('[data-testid=mix] [data-knob="hats.pan"]', (e) => e.dataset.value)) === "0.7") && !(await page.$("[data-testid=toast]")), "and the knob shows the edit, with no toast");
  const named = await page.$eval('button[aria-label="Undo"]', (b) => b.title);
  await page.keyboard.down("Meta"); await page.keyboard.press("z"); await page.keyboard.up("Meta");
  check(await until(() => songTs() === before), "⌘Z undoes it");
  // Its words are Undo's tooltip ("Undo Hats pan"); its result is on screen, so no toast (taste.md ST-5).
  await new Promise((r) => setTimeout(r, 400));
  check(named === "Undo Hats pan (⌘Z)" && !(await page.$("[data-testid=toast]")), `Undo named it in words (${named}), and made no toast`);

  // ⌘S with no code editor open: nothing to save, and it says so.
  await page.keyboard.down("Meta"); await page.keyboard.press("s"); await page.keyboard.up("Meta");
  check(await until(async () => (await text()).includes("Changes are saved as you make them.")), "⌘S says changes are saved as you make them");

  // Closing the project drops to the start screen.
  const closed = await api("graph/close", {});
  check(closed.status === 200, `POST /api/graph/close: ${closed.status}`);
}

check(await until(async () => !!(await page.$("[data-testid=start]")), 6000), "the start screen appears");
check(!!(await page.$("[data-testid=new-song]")) && !!(await page.$("[data-testid=open-example]")), "with New Song and Open an Example");
check(!(await page.$("[data-testid=open-project]")), "and no Open Project Folder outside the app");
check(await until(async () => (await text()).includes("To open a folder you already have, use File → Open Project Folder… in the music2 app.")), "and the line about the app's menu");
const t = await text();
check(t.includes("Make a song, or open one you’ve made.") && t.includes("New songs are saved in ~/Music/music2."), "and the plain hints");
await new Promise((r) => setTimeout(r, 400));
await page.screenshot({ path: `${S}/start-browser${empty ? "-empty" : ""}.png` });

// Old API paths are a clear 404 now, not the UI's index.html.
const old = await api("state");
check(old.status === 404 && old.json?.error === "no such API", `GET /api/state: ${old.status} ${JSON.stringify(old.json)}`);
const exec = await api("exec", { cmd: "track.add", params: {} });
check(exec.status === 404 && exec.json?.error === "no such API", `POST /api/exec: ${exec.status} ${JSON.stringify(exec.json)}`);
const anon = await fetch(`${sess.url}/api/state`);
check(anon.status === 401, `without the token: ${anon.status}`);
const spa = await fetch(`${sess.url}/some/page`);
check(spa.status === 200 && (await spa.text()).includes('<div id="root">'), "other paths still serve the UI");
check((await api("graph")).status === 400, "GET /api/graph with no project: 400");

// The same screen inside the app (a stubbed shell): the button and its shortcut.
const app = await browser.newPage();
await app.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const invoked = [];
await app.exposeFunction("__invoked", (cmd, args) => invoked.push([cmd, args]));
await app.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: true, token, platform: "macos" };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => { window.__invoked(cmd, JSON.parse(JSON.stringify(args ?? {}))); return 0; },
    transformCallback: () => 0,
    unregisterCallback: () => {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}, sess.token);
await app.goto(sess.url, { waitUntil: "domcontentloaded" });
await app.waitForSelector("[data-testid=open-project]", { timeout: 6000 });
const label = await app.$eval("[data-testid=start]", (e) => e.innerText);
check(label.includes("Open Project Folder…") && label.includes("⌘O"), `in the app: the button and ⌘O (${label.replace(/\n/g, " | ")})`);
await new Promise((r) => setTimeout(r, 400));
await app.screenshot({ path: `${S}/start-app.png` });
await app.click("[data-testid=open-project]");
await until(async () => invoked.some(([c, a]) => c === "action" && a.id === "open_project"), 2000);
check(invoked.some(([c, a]) => c === "action" && a.id === "open_project"), `the button asks the shell for open_project (${JSON.stringify(invoked.filter(([c]) => c === "action"))})`);
check(invoked.some(([c, a]) => c === "action" && a.id === "ready"), "and the page said ready");

// GET /api/graph answers 400 "no project is open" with none open: the browser logs that one itself.
const real = errors.filter((e) => !e.includes("status of 400 (Bad Request)"));
check(real.length === 0, `no console errors besides the expected 400 ${real.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
