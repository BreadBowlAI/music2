// Step 6's write path in the app: an outside lane write is taken in once it settles (WRITE-19,
// PLANE-1); a lane file written in two halves never shows its lane empty; a project another program
// holds opens read-only, refuses edits with WRITE-18's message and writes again after Retry
// (WRITE-17); a project whose code doesn't compile opens in its code, silent, and plays once the code
// editor's save compiles (WRITE-20).
//
//   node e2e/s6-write.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { exe } from "./chrome.mjs";

const [S, , proj = `${S}/proj`] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const consoleErrors = [];
// The refused writes answer 423; with the project closed between parts, the state answers 400.
page.on("console", (m) => m.type() === "error" && !/(423 \(Locked\)|400 \(Bad Request\))/.test(m.text()) && consoleErrors.push(m.text()));
page.on("pageerror", (e) => consoleErrors.push(String(e)));
// Which request a "Failed to load resource" error was.
page.on("response", (r) => {
  const what = `${r.status()} ${r.request().method()} ${new URL(r.url()).pathname}`;
  if (r.status() >= 400 && r.status() !== 423 && what !== "400 GET /api/graph") consoleErrors.push(what);
});
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; }, sess.token);
const headers = { "x-m2-token": sess.token, "content-type": "application/json" };
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers })).json();
const post = async (path, body) => {
  const res = await fetch(`${sess.url}/api/${path}`, { method: "POST", headers, body: JSON.stringify(body) });
  return { status: res.status, json: await res.json().catch(() => ({})) };
};

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 100)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const read = (f) => (fs.existsSync(`${proj}/${f}`) ? fs.readFileSync(`${proj}/${f}`, "utf8") : null);
const write = (f, text) => fs.writeFileSync(`${proj}/${f}`, text);
const orig = { song: read("song.ts"), kick: read("tracks/kick.ts"), hits: read("data/kick/hits.json") };
const graph = () => api("graph");
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent));
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 4000);
const key = async (k, mods = []) => {
  for (const m of mods) await page.keyboard.down(m);
  await page.keyboard.press(k);
  for (const m of mods.reverse()) await page.keyboard.up(m);
};
/** The kick's notes in the state's timeline, and the revision. */
const kickNotes = (g) => {
  const lane = g.timeline.tracks.find((t) => t.module === "kick")?.lanes.find((l) => l.key === "hits");
  return lane ? lane.clips.reduce((n, c) => n + c.notes.length, 0) : -1;
};
/** A hits.json of `n` notes in one clip, written as an outside writer would. */
const hitsOf = (n) =>
  JSON.stringify({ clips: [{ id: "a", start: 0, length: 16, notes: Array.from({ length: n }, (_, i) => ({ pitch: "C1", start: i, length: 0.25 })) }] }, null, 2) + "\n";

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph]", { timeout: 8000 });
await page.waitForSelector('[data-track="kick"]');

// ---------------------------------------------------------------- an outside lane write (WRITE-19, PLANE-1)

const g0 = await graph();
const before = kickNotes(g0);
check(before > 0, `the kick plays ${before} notes`);
write("data/kick/hits.json", hitsOf(5));
const t0 = Date.now();
check(await until(async () => kickNotes(await graph()) === 5, 3000), "an agent's write to the kick's lane file is taken in");
const took = Date.now() - t0;
check(took >= 150, `once it settled: after ${took} ms, not before the 150 ms of quiet`);
const g1 = await graph();
check(g1.view.revision === g0.view.revision + 1, "as one revision");
check(await until(async () => (await page.$$eval('[data-track="kick"] [data-clip]', (e) => e.length).catch(() => -1)) === 1), "the timeline shows it: one clip");

// ---------------------------------------------------------------- written in two halves (FMT-9)

const seen = [];
const watch = (async () => {
  const t = Date.now();
  while (Date.now() - t < 2500) {
    const g = await graph();
    seen.push({ revision: g.view.revision, notes: kickNotes(g), invalid: (g.problems ?? []).some((p) => p.code === "lane.invalid") });
    await sleep(30);
  }
})();
const eight = hitsOf(8);
const fd = fs.openSync(`${proj}/data/kick/hits.json`, "w");
fs.writeSync(fd, eight.slice(0, eight.length / 2));
await sleep(400);
fs.writeSync(fd, eight.slice(eight.length / 2));
fs.closeSync(fd);
await watch;
check(seen.every((s) => s.notes > 0), `the lane never played empty (${seen.length} reads)`);
const halves = [...new Map(seen.map((s) => [s.revision, s])).values()].filter((s) => s.revision > g1.view.revision);
check(halves.length === 2 && halves[0].notes === 5 && halves[0].invalid, "the first half, 400 ms alone, kept the last notes and said the file doesn't read");
check(halves.at(-1)?.notes === 8 && !halves.at(-1).invalid, "the whole file plays its 8 notes");

// ---------------------------------------------------------------- held by another program (WRITE-17, WRITE-18)

check((await post("graph/close", {})).status === 200, "the project closes");
// Another program takes the lock, as music2 or m2 would: flock on .music2/lock, its holder in it.
const holderJson = (pid) => JSON.stringify({ pid, program: "agent", host: "here", host_id: "x", token: "0", since: Math.floor(Date.now() / 1000) });
const holder = spawn("python3", ["-c", `
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
os.ftruncate(fd, 0)
os.write(fd, sys.argv[2].replace("PID", str(os.getpid())).encode())
print("ready", flush=True)
sys.stdin.read()
`, `${proj}/.music2/lock`, holderJson(0).replace('"pid":0', '"pid":PID')], { stdio: ["pipe", "pipe", "inherit"] });
await new Promise((r) => holder.stdout.once("data", r));
const opened = await post("graph/open", { path: proj });
check(opened.status === 200 && opened.json.read_only?.why === "held", "it opens, read-only");
check(opened.json.read_only?.holder?.program === "agent" && opened.json.read_only.holder.pid === holder.pid, "naming the holder");
// Said once, by the top bar's status pill; its popover says where it is open and how to edit (taste.md ST-2).
await page.waitForSelector("[data-testid=read-only-banner]", { timeout: 6000 }).catch(() => {});
const pill = await page.$eval("[data-testid=read-only-banner]", (e) => e.innerText).catch(() => "");
// Wait for the page to take in the read-only state before opening the popover.
await page.click("[data-testid=read-only-banner]").catch(() => {});
await page.waitForSelector("[data-testid=status-popover]", { timeout: 3000 }).catch(() => {});
const banner = await page.$eval("[data-testid=status-popover]", (e) => e.innerText).catch(() => "");
check(/Read only/.test(pill) && /Open in “agent”/.test(banner) && /close it there, then Retry/.test(banner) && !banner.includes("pid") && !banner.includes("run this again"), `the status pill says so: ${pill} | ${banner.replace(/\n/g, " | ")}`);
await page.screenshot({ path: `${S}/s6-read-only.png` });
const g2 = await graph();
const refused = await post("graph/lane", { session: g2.session, revision: g2.view.revision, module: "kick", lane: "hits", clips: [] });
check(refused.status === 423 && /nothing was changed/.test(refused.json.error) && refused.json.error.includes("agent"), `an edit is refused, 423: ${refused.json.error}`);
check(read("data/kick/hits.json") === eight, "and changes nothing");
// From the UI: editing isn't offered at all, rather than refused after (taste.md ST-2).
await page.keyboard.press("Escape");
await page.click('[data-track="kick"] [data-testid=track-label]', { count: 2 });
const field = await page.waitForSelector("[data-testid=track-rename]", { timeout: 1000 }).catch(() => null);
const why = await page.$eval('[data-track="kick"] [data-testid=track-label]', (e) => e.title).catch(() => "");
check(!field && /Read only/.test(why) && !(await toasts()).some((t) => t.includes("nothing was changed")), `a rename isn't offered, and the name says why: ${why.replace(/\n/g, " | ")}`);
check(read("tracks/kick.ts") === orig.kick, "kick.ts is unchanged");
const stillHeld = await post("graph/lock/retry", { session: g2.session });
check(stillHeld.status === 200 && stillHeld.json.read_only?.why === "held", "Retry while it is held leaves it read-only");
holder.stdin.end();
await new Promise((r) => holder.once("exit", r));
if (!(await page.$("[data-testid=status-popover]"))) await page.click("[data-testid=read-only-banner]");
await page.click("[data-testid=read-only-retry]");
check(await until(async () => !(await page.$("[data-testid=read-only-banner]"))), "once the other program lets go, Retry takes the lock: the pill goes");
check((await graph()).read_only === null, "and the state says so");
const ok = await post("graph/lane", { session: g2.session, revision: (await graph()).view.revision, module: "kick", lane: "hits", clips: [{ id: "a", start: 0, length: 4, notes: [{ pitch: "C1", start: 0, length: 1 }] }] });
check(ok.status === 200 && kickNotes(await graph()) === 1, "an edit goes ahead");

// ---------------------------------------------------------------- code that doesn't compile (WRITE-20)

check((await post("graph/close", {})).status === 200, "closed again");
write("song.ts", `${orig.song}const broken = ;`);
const broken = await post("graph/open", { path: proj });
check(broken.status === 200, "a project whose code doesn't compile opens");
check(broken.json.has_code === false && broken.json.view.modules.length === 0, "with nothing to show or play");
check(broken.json.diagnostics?.[0]?.file === "song.ts", `its problem placed in song.ts (${JSON.stringify(broken.json.diagnostics?.[0])})`);
await page.waitForSelector("[data-status~=compile]", { timeout: 6000 }).catch(() => {});
await page.click("[data-status~=compile]").catch(() => {});
await page.waitForSelector("[data-testid=compile-banner]", { timeout: 3000 }).catch(() => {});
const compile = await page.$eval("[data-testid=compile-banner]", (e) => e.innerText).catch(() => "");
check(/Nothing plays until it does/.test(compile), `the status popover says nothing plays until it compiles: ${compile.replace(/\n/g, " | ")}`);
await page.keyboard.press("Escape");
// D2: the timeline shows the song's tracks and clips from data/, greyed, never "No tracks yet".
check(
  await until(async () => !!(await page.$("[data-testid=waiting]")) && (await page.$$('[data-track="kick"] [data-clip]')).length > 0 && !(await page.evaluate(() => document.body.innerText.includes("No tracks yet")))),
  "the timeline shows the tracks from data/, waiting for the code to compile",
);
check(await until(async () => !!(await page.$("[data-testid=code] .monaco-editor .view-lines")), 10000), "it opens in the code editor");
const path = () => page.$eval("[data-testid=code-path]", (e) => e.textContent).catch(() => "");
check(await until(async () => (await path()) === "song.ts"), `on song.ts, where the problem is (${await path()})`);
await page.screenshot({ path: `${S}/s6-broken.png` });
await post("graph/transport", { action: "play" });
await sleep(300);
check(!(await api("graph/status")).playing, "pressing play plays nothing");
// Fix it in the editor: the broken line goes, and ⌘S saves.
await page.click("[data-testid=code] .view-lines");
await key("ArrowDown", ["Meta"]);
await key("End");
await key("Home", ["Shift"]);
await key("Backspace");
await key("s", ["Meta"]);
check(await until(() => read("song.ts") === orig.song), "the save writes the fixed code");
check(await until(async () => (await graph()).has_code === true), "which compiles");
const fixed = await graph();
check(fixed.view.modules.length > 0 && kickNotes(fixed) === 1, "the song is shown, with its data");
check(await until(async () => !(await page.$("[data-testid=compile-banner]"))), "the banner goes");
const strip = () => page.$eval("[data-testid=problems]", (e) => `${e.dataset.state} ${e.innerText}`).catch(() => "");
check(await until(async () => (await strip()).startsWith("ok")), `and the editor's strip says it compiles (${await strip()})`);
await post("graph/transport", { action: "play" });
check(await until(async () => (await api("graph/status")).beat > 0.2), "and it plays");
await post("graph/transport", { action: "stop" });
await page.screenshot({ path: `${S}/s6-fixed.png` });

check(consoleErrors.length === 0, `no console errors${consoleErrors.length ? `: ${consoleErrors.join(" | ")}` : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
