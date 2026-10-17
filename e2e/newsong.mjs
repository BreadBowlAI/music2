// New Song, templates and examples, on a headless server started with no project and a scratch HOME.
// node run.mjs newsong   (node newsong.mjs <scratch> <home>)
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const S = process.argv[2];
const H = process.argv[3];
const songs = `${H}/Music/music2`;
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const HDR = { "x-m2-token": sess.token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: HDR } : { method: "POST", headers: { ...HDR, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const text = () => page.evaluate(() => document.body.innerText);
const folders = () => (fs.existsSync(songs) ? fs.readdirSync(songs).filter((n) => !n.startsWith(".")).sort() : []);
const manifestName = (dir) => /name = "(.*)"/.exec(fs.readFileSync(`${dir}/music2.toml`, "utf8"))?.[1];
const graphName = async () => (await api("graph")).json?.name;

// ---------------------------------------------------------------- the catalog
const cat = await api("project/templates");
check(cat.status === 200, `GET /api/project/templates: ${cat.status}`);
check(JSON.stringify(cat.json.templates.map((t) => t.id)) === '["starter","empty"]', `templates: ${cat.json.templates.map((t) => `${t.id} (${t.name})`).join(", ")}`);
check(cat.json.templates.every((t) => t.name && t.description), "each with a name and a description");
check(cat.json.examples.some((e) => e.id === "first-song" && e.name === "First song"), `examples: ${cat.json.examples.map((e) => e.id).join(", ")}`);
check(cat.json.songs === songs, `the songs folder is under HOME: ${cat.json.songs}`);
check(!fs.existsSync(songs), "and isn't made until a song is");

// ---------------------------------------------------------------- the start screen
await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=start]", { timeout: 8000 });
await page.waitForSelector("[data-testid=new-song]");
check(!!(await page.$("[data-testid=open-example]")), "the start screen has New Song and Open an Example");
check(!(await page.$("[data-testid=open-project]")), "and no Open Project Folder outside the app");
check(await until(async () => (await text()).includes("New songs are saved in ~/Music/music2")), "it says where new songs go");
await sleep(400);
await page.screenshot({ path: `${S}/newsong-start.png` });

// ---------------------------------------------------------------- New Song: Starter
await page.click("[data-testid=new-song]");
await page.waitForSelector("[data-testid=new-song-dialog]");
await page.waitForSelector("[data-testid=template-starter]");
check((await page.$eval("[data-testid=song-name]", (e) => e.value)) === "My song", "the name starts as “My song”");
check((await page.$eval("[data-testid=template-starter]", (e) => e.getAttribute("aria-checked"))) === "true", "Starter beat is picked");
check((await page.$eval("[data-testid=song-folder]", (e) => e.textContent)) === "~/Music/music2", "it saves in ~/Music/music2");
check(await page.$eval("[data-testid=song-name]", (e) => document.activeElement === e && e.selectionStart === 0 && e.selectionEnd === e.value.length), "the name is focused and selected");
await sleep(400);
await page.screenshot({ path: `${S}/newsong-dialog.png` });

// Esc cancels, and nothing is made.
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$("[data-testid=new-song-dialog]"))), "Esc closes the dialog");
check(folders().length === 0, "and makes nothing");

await page.click("[data-testid=new-song]");
await page.waitForSelector("[data-testid=template-starter]");
await page.keyboard.type("Sunday");
await page.click("[data-testid=template-starter]");
await page.click("[data-testid=song-name]");
await page.keyboard.press("Enter");
check(await until(async () => !!(await page.$("[data-testid=timeline]"))), "Enter creates the song and it opens");
check(!(await page.$("[data-testid=new-song-dialog]")), "the dialog is gone");
check(await until(async () => (await graphName()) === "Sunday"), `the open song is “Sunday” (${await graphName()})`);
check(fs.existsSync(`${songs}/Sunday/song.ts`) && manifestName(`${songs}/Sunday`) === "Sunday", "~/Music/music2/Sunday has the song, named Sunday");
check(!fs.existsSync(`${songs}/Sunday/template.toml`), "without the template's own file");
// FMT-1, FMT-20: a new song is format 1, and keeps this machine's .music2/ out of git.
check(/^format = 1$/m.test(fs.readFileSync(`${songs}/Sunday/music2.toml`, "utf8")) && fs.existsSync(`${songs}/Sunday/data/arrangement.json`), "in format 1");
check(fs.readFileSync(`${songs}/Sunday/.gitignore`, "utf8").split("\n").includes(".music2/"), "with .music2/ in its .gitignore");
const tracks = await page.$$eval("[data-track]", (els) => els.map((e) => e.dataset.track));
check(["bass", "chords", "clap", "hats", "hook", "kick"].every((t) => tracks.includes(t)), `its tracks are in the timeline: ${tracks.join(", ")}`);
const clips = await page.$$eval("[data-track] [data-clip], [data-track] [data-lane] > div div[style*='left']", (els) => els.length).catch(() => 0);
check(await until(async () => (await text()).includes("Made “Sunday” in ~/Music/music2. Press Space to play it.")), "a toast says where it was made");

// It plays: Space, and the playhead moves on.
await page.click("[data-testid=timeline]", { offset: { x: 5, y: 5 } }).catch(() => {});
await page.keyboard.press("Space");
check(await until(async () => (await api("graph/status")).json.playing), "Space plays the new song");
const t0 = (await api("graph/status")).json.seconds;
await sleep(700);
const t1 = (await api("graph/status")).json.seconds;
check(t1 > t0 + 0.3, `status advances while it plays (${t0.toFixed(2)} → ${t1.toFixed(2)} s)`);
await sleep(300);
await page.screenshot({ path: `${S}/newsong-open.png` });
await page.keyboard.press("Space");
check(await until(async () => !(await api("graph/status")).json.playing), "Space stops it");

// ---------------------------------------------------------------- unsaved code: the dialog warns first
await page.click('[data-view-tab="code"]');
check(await until(async () => !!(await page.$("[data-testid=code] .monaco-editor .view-lines"))), "the code editor opens");
await sleep(500);
await page.click("[data-testid=code] .view-lines");
await page.keyboard.down("Meta"); await page.keyboard.press("ArrowDown"); await page.keyboard.up("Meta");
await page.keyboard.type("\n// not saved yet");
await sleep(300);
await page.keyboard.down("Meta"); await page.keyboard.press("n"); await page.keyboard.up("Meta");
check(await until(async () => !!(await page.$("[data-testid=new-song-dialog]"))), "⌘N in the code editor opens the dialog");
check(await until(async () => (await page.$eval("[data-testid=new-song-dialog]", (e) => e.innerText)).includes("A file has unsaved edits in the code editor. Making a new song closes this one without saving them.")), "it warns about the unsaved code");
check((await page.$eval("[data-testid=create-song]", (e) => e.innerText.trim())) === "Create Anyway", "and the button says Create Anyway");
await sleep(300);
await page.screenshot({ path: `${S}/newsong-unsaved.png` });
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$("[data-testid=new-song-dialog]"))), "Esc closes it");
check(!fs.readFileSync(`${songs}/Sunday/${"tracks"}/bass.ts`, "utf8").includes("not saved yet") && (await graphName()) === "Sunday", "the song and its unsaved code stay as they were");
await page.click('[data-view-tab="graph"]');

// ---------------------------------------------------------------- New Song again (⌘N), Empty, with a song open
await page.keyboard.down("Meta"); await page.keyboard.press("n"); await page.keyboard.up("Meta");
check(await until(async () => !!(await page.$("[data-testid=new-song-dialog]"))), "⌘N opens the dialog with a song open");
await page.waitForSelector("[data-testid=template-empty]");
await page.click("[data-testid=template-empty]");
check((await page.$eval("[data-testid=template-empty]", (e) => e.getAttribute("aria-checked"))) === "true", "Empty is picked");
await sleep(300);
await page.screenshot({ path: `${S}/newsong-dialog-empty.png` });
await page.click("[data-testid=create-song]");
check(await until(async () => (await graphName()) === "My song"), `Create opens the empty song (${await graphName()})`);
check(await until(async () => !(await page.$("[data-testid=new-song-dialog]"))), "the dialog closes");
const g = (await api("graph")).json;
check(g.timeline.tracks.every((t) => !t.track), "the empty song has no tracks");
check(fs.existsSync(`${songs}/My song/song.ts`), "~/Music/music2/My song exists");
await page.waitForSelector("[data-testid=timeline]");
await sleep(400);
await page.screenshot({ path: `${S}/newsong-empty.png` });
await api("graph/transport", { action: "play" });
check(await until(async () => (await api("graph/status")).json.playing), "the empty song plays (silence)");
await api("graph/transport", { action: "stop" });

// The same name again: a number is added, nothing is overwritten.
const before = fs.readFileSync(`${songs}/Sunday/song.ts`, "utf8");
await page.keyboard.down("Meta"); await page.keyboard.press("n"); await page.keyboard.up("Meta");
await page.waitForSelector("[data-testid=template-starter]");
await page.keyboard.type("Sunday");
await page.keyboard.press("Enter");
check(await until(async () => (await graphName()) === "Sunday 2"), `a second “Sunday” is “Sunday 2” (${await graphName()})`);
check(fs.readFileSync(`${songs}/Sunday/song.ts`, "utf8") === before, "the first Sunday is untouched");

// NS-3: the Tempo field beside the name, the template's until typed; Tab moves to it.
await page.keyboard.down("Meta"); await page.keyboard.press("n"); await page.keyboard.up("Meta");
await page.waitForSelector("[data-testid=template-starter]");
check((await page.$eval("[data-testid=song-tempo]", (e) => e.value)) === "116", "the Tempo field shows Starter beat's 116");
await page.click("[data-testid=template-empty]");
check(await until(async () => (await page.$eval("[data-testid=song-tempo]", (e) => e.value)) === "120"), "and Empty's 120 when it is picked");
await page.click("[data-testid=song-name]", { clickCount: 3 });
await page.keyboard.type("Fast");
await page.keyboard.press("Tab");
check(await page.$eval("[data-testid=song-tempo]", (e) => document.activeElement === e && e.selectionStart === 0 && e.selectionEnd === e.value.length), "Tab moves from the name to the tempo, selected");
const made = folders().length;
await page.keyboard.type("5");
await page.keyboard.press("Enter");
const tooSlow = await page.$eval("[data-testid=song-tempo] ~ [role=alert]", (e) => e.textContent).catch(() => null);
check(tooSlow === "20 to 999 BPM" && folders().length === made && !!(await page.$("[data-testid=new-song-dialog]")), `5 BPM is refused in words, and nothing is made: ${tooSlow}`);
await page.click("[data-testid=song-tempo]", { clickCount: 3 });
await page.keyboard.type("140");
await page.keyboard.press("Enter");
check(await until(async () => (await graphName()) === "Fast"), `with 140 it is made and opens (${await graphName()})`);
check(JSON.stringify(JSON.parse(fs.readFileSync(`${songs}/Fast/data/arrangement.json`, "utf8")).tempo) === '[{"beat":0,"bpm":140}]' && (await api("graph")).json.timeline.tempo[0].bpm === 140, "its data/arrangement.json says 140");

// Cancel makes nothing.
await page.keyboard.down("Meta"); await page.keyboard.press("n"); await page.keyboard.up("Meta");
await page.waitForSelector("[data-testid=new-song-dialog]");
const n = folders().length;
await page.$$eval("[data-testid=new-song-dialog] button", (bs) => bs.find((b) => b.textContent.trim() === "Cancel").click());
check(await until(async () => !(await page.$("[data-testid=new-song-dialog]"))), "Cancel closes the dialog");
check(folders().length === n, "and makes nothing");

// ---------------------------------------------------------------- Open an example
// The close button is gone from the top bar (taste.md TB-11): ⌘W closes the song.
const closeSong = async () => {
  await page.click("[data-testid=timeline]");
  await page.keyboard.down("Meta"); await page.keyboard.press("w"); await page.keyboard.up("Meta");
};
await closeSong();
await page.waitForSelector("[data-testid=start]");
await page.waitForSelector("[data-testid=open-example]:not([disabled])");
await page.click("[data-testid=open-example]");
await page.waitForSelector('[data-example="first-song"]');
await sleep(300);
await page.screenshot({ path: `${S}/newsong-examples.png` });
await page.click('[data-example="first-song"]');
check(await until(async () => (await graphName()) === "First song"), `Open an Example opens a copy of First song (${await graphName()})`);
check(fs.existsSync(`${songs}/First song/data/kick/hits.json`), "copied to ~/Music/music2/First song");
check(!fs.existsSync(`${songs}/First song/example.toml`), "without the example's own file");
await page.waitForSelector("[data-testid=timeline]");
const exTracks = await page.$$eval("[data-track]", (els) => els.map((e) => e.dataset.track));
check(["bass", "hats", "keys", "kick"].every((t) => exTracks.includes(t)), `its tracks are in the timeline: ${exTracks.join(", ")}`);
// Again: another copy.
await closeSong();
await page.waitForSelector("[data-testid=open-example]:not([disabled])");
await page.click("[data-testid=open-example]");
await page.waitForSelector('[data-example="first-song"]');
await page.click('[data-example="first-song"]');
check(await until(async () => (await graphName()) === "First song 2"), `a second copy is “First song 2” (${await graphName()})`);

const all = folders();
check(JSON.stringify(all) === JSON.stringify(["Fast", "First song", "First song 2", "My song", "Sunday", "Sunday 2"]), `folders in ~/Music/music2: ${all.join(" | ")}`);

// ---------------------------------------------------------------- the API refuses what it should
const bad = async (body, what) => {
  const r = await api("project/new", body);
  check(r.status === 400 && r.json?.error, `${what}: ${r.status} ${r.json?.error}`);
};
await bad({ name: "x", template: "../std" }, "a template id that is a path");
await bad({ name: "x", example: "nope" }, "an unknown example");
await bad({ name: "x", dir: "relative" }, "a relative folder");
await bad({ name: "x", dir: `${H}/missing` }, "a folder that isn't there");
const odd = await api("project/new", { name: "../.Hidden/one", template: "empty" });
check(odd.status === 200 && odd.json.name === "../.Hidden/one" && fs.existsSync(`${songs}/-.Hidden-one/song.ts`), `a name with slashes and dots stays in the songs folder: ${odd.json?.dir}`);
check(JSON.stringify(folders()) === JSON.stringify([...all, "-.Hidden-one"].sort()), "nothing else was made");
const other = `${H}/Elsewhere`;
fs.mkdirSync(other, { recursive: true });
const there = await api("project/new", { name: "Away", template: "empty", dir: other });
check(there.status === 200 && fs.existsSync(`${other}/Away/song.ts`), "dir makes it in another folder");

// ---------------------------------------------------------------- in the app: Choose Folder… and the ⌘N hint
const app = await browser.newPage();
await app.setViewport({ width: 1440, height: 900, deviceScaleFactor: 2 });
const invoked = [];
await app.exposeFunction("__invoked", (cmd, args) => invoked.push([cmd, args]));
await app.evaluateOnNewDocument((token, picked) => {
  window.__M2__ = { native: true, token, platform: "macos" };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      window.__invoked(cmd, JSON.parse(JSON.stringify(args ?? {})));
      return cmd === "action" && args?.id === "song_folder" ? picked : null;
    },
    transformCallback: () => 0,
    unregisterCallback: () => {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}, sess.token, other);
await api("graph/close", {});
await app.goto(sess.url, { waitUntil: "domcontentloaded" });
await app.waitForSelector("[data-testid=open-project]", { timeout: 6000 });
const label = await app.$eval("[data-testid=start]", (e) => e.innerText);
check(label.includes("New Song…") && label.includes("⌘N") && label.includes("Open Project Folder…") && label.includes("⌘O"), `in the app: the buttons and their shortcuts (${label.replace(/\n/g, " | ")})`);
await sleep(400);
await app.screenshot({ path: `${S}/newsong-start-app.png` });
await app.click("[data-testid=new-song]");
await app.waitForSelector("[data-testid=template-starter]");
const choose = await app.$$eval("[data-testid=new-song-dialog] button", (bs) => bs.some((b) => b.textContent.trim() === "Choose Folder…"));
check(choose, "the dialog offers Choose Folder…");
await app.$$eval("[data-testid=new-song-dialog] button", (bs) => bs.find((b) => b.textContent.trim() === "Choose Folder…").click());
check(await until(async () => (await app.$eval("[data-testid=song-folder]", (e) => e.textContent)) === "~/Elsewhere"), "the folder picked shows");
check(invoked.some(([c, a]) => c === "action" && a.id === "song_folder" && a.arg === songs), `it asked the shell for a folder, starting in the songs folder (${JSON.stringify(invoked.filter(([c]) => c === "action").map(([, a]) => a))})`);
await sleep(300);
await app.screenshot({ path: `${S}/newsong-dialog-app.png` });
await app.click("[data-testid=song-name]", { clickCount: 3 });
await app.keyboard.type("Picked");
await app.click("[data-testid=create-song]");
check(await until(async () => (await graphName()) === "Picked"), "Create makes it in the folder picked");
check(fs.existsSync(`${other}/Picked/song.ts`), `${other}/Picked exists`);
check(await until(async () => (await app.evaluate(() => document.body.innerText)).includes("Show in Finder")), "the toast offers Show in Finder");

const real = errors.filter((e) => !e.includes("status of 400 (Bad Request)"));
check(real.length === 0, `no console errors besides the expected 400 ${real.join(" | ")}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
