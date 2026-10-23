// Play first, save beside it (PLANE-5) in the app: a note added in the piano roll is written; one
// the disk refuses (here `.music2/tx/` made read-only, as a full or failing disk refuses a commit's
// journal) is heard as its preview, then taken back: a toast says what wasn't saved and that it was
// taken back (once, though the server tells every window too), the piano roll shows the clip as it
// was, the file and the revision are unchanged, and an agent's Tx is answered `taken_back` and said
// in the app too. With the transport stopped, nothing was heard, and the message doesn't say it
// played. Once the disk takes writes again, the next edit is saved and undo takes back the last
// one that was.
//
//   node e2e/g11-play.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { exe } from "./chrome.mjs";

const [S, , proj = `${S}/proj`] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${S}/testhome/.music2/session.json`, "utf8"));
const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1600, height: 1000 });
const consoleErrors = [];
// The refused writes answer 400 (the lane route) and 500 (the agent's Tx): expected here.
page.on("console", (m) => m.type() === "error" && !/400 \(Bad Request\)/.test(m.text()) && consoleErrors.push(m.text()));
page.on("pageerror", (e) => consoleErrors.push(String(e)));
// The dock as tall as in the notes suite, so the roll shows the rows clicked here.
await page.evaluateOnNewDocument((token) => { window.__M2__ = { native: false, token, platform: "macos" }; try { localStorage.setItem("m2.graph.panel", "450"); } catch {} }, sess.page_token);
const headers = { "x-m2-token": sess.page_token, "content-type": "application/json" };
const api = async (path) => (await fetch(`${sess.url}/api/${path}`, { headers })).json();
const post = async (path, body) => {
  const res = await fetch(`${sess.url}/api/${path}`, { method: "POST", headers, body: JSON.stringify(body) });
  return { status: res.status, json: await res.json().catch(() => ({})) };
};

let pass = 0, fail = 0;
const check = (ok, what) => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}`); ok ? pass++ : fail++; };
const until = async (f, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await f()) return true; await new Promise((r) => setTimeout(r, 80)); } return false; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const FILE = `${proj}/data/bass/notes.json`;
const TX = `${proj}/.music2/tx`;
const original = fs.readFileSync(FILE, "utf8");
const graph = () => api("graph");
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent));
// One toast shows at a time (taste.md §2.14), so a second showing of the same words would only
// replace the first: count the toasts that appeared, not the ones in view.
const appeared = () => page.evaluate(() => window.__appeared ?? []);
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 4000);
const notes = () => page.$$eval("[data-testid=notes] [data-note]", (els) => els.map((e) => Number(e.dataset.pitch)));
const box = async (sel) => { const el = await page.$(sel); return el && el.boundingBox(); };
/** The Bass clip's notes in the state's timeline. */
const bassNotes = (g) => g.timeline.tracks.find((t) => t.module === "bass").lanes.find((l) => l.key === "notes").clips[0].notes.length;

await page.goto(sess.url, { waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 8000 });
await page.evaluate(() => {
  window.__appeared = [];
  new MutationObserver((records) => {
    for (const r of records) for (const n of r.addedNodes) {
      if (!(n instanceof HTMLElement)) continue;
      for (const t of [n, ...n.querySelectorAll("[data-testid=toast]")]) if (t.dataset.testid === "toast") window.__appeared.push(t.textContent);
    }
  }).observe(document.body, { childList: true, subtree: true });
});
await post("graph/transport", { action: "play" });

// The Bass clip in the piano roll, middle C in view (the clip has nothing there).
const clipBox = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(clipBox.x + 60, clipBox.y + clipBox.height / 2, { count: 2 });
check(await until(async () => (await notes()).length === 48), "the Bass clip opens in the piano roll with its 48 notes");
// The roll's rows and its ruler (taste.md §3.5: 14px rows, the arrangement's 28px ruler).
const ROW = 14, RULER = 28;
await page.$eval("[data-testid=roll-scroller]", (el, row, ruler) => { el.scrollTop = ruler + (127 - 60) * row - el.clientHeight / 2; }, ROW, RULER);
await sleep(100);
const grid = await box("[data-testid=roll-grid]");
const ppb = await page.evaluate(() => {
  const n = [...document.querySelectorAll("[data-testid=notes] [data-note]")].find((e) => Number(e.dataset.start) > 0);
  return (n.getBoundingClientRect().left - document.querySelector("[data-testid=roll-grid]").getBoundingClientRect().left) / Number(n.dataset.start);
});
const cell = (pitch, beat) => ({ x: grid.x + beat * ppb, y: grid.y + (127 - pitch) * ROW + ROW / 2 });
// A double-click on empty grid adds a note; a single click selects nothing (D4).
const add = (at) => page.mouse.click(at.x, at.y, { count: 2 });

// ---------------------------------------------------------------- written, as always

const g0 = await graph();
let at = cell(60, 1.1);
await add(at);
check(await until(() => fs.readFileSync(FILE, "utf8").includes('"pitch": "C4"')), "a double-click adds C4, written to data/bass/notes.json");
check(await until(async () => (await graph()).view.revision === g0.view.revision + 1), "as one revision");
check((await toasts()).length === 0, "no message");
const saved = fs.readFileSync(FILE, "utf8");
const g1 = await graph();

// ---------------------------------------------------------------- refused by the disk: heard, then taken back

fs.chmodSync(TX, 0o500);
let root = false;
try {
  fs.writeFileSync(`${TX}/probe`, "");
  fs.rmSync(`${TX}/probe`);
  root = true;
} catch {}
if (root) {
  fs.chmodSync(TX, 0o755);
  console.log("SKIP  .music2/tx/ stays writable when read-only (run as root): the refused write isn't tested");
} else {
  at = cell(64, 3.1);
  await add(at);
  check(await toasted("taken back"), "a double-click the disk refuses: a message says it was taken back");
  const said = (await toasts()).find((t) => t.includes("taken back")) ?? "";
  check(said.startsWith("data/bass/notes.json couldn't be saved") && said.endsWith("It played for a moment and was taken back."), `and what wasn't saved: “${said}”`);
  await sleep(700); // the message's rise, and the server's word to every window (every 300 ms)
  const once = (await appeared()).filter((t) => t.includes("taken back")).length;
  check(once === 1, `said once, though the server tells every window too (${once})`);
  await page.screenshot({ path: `${S}/g11-taken-back.png` });
  check(await until(async () => (await notes()).length === 49 && !(await notes()).includes(64)), "the piano roll shows the clip as it was: 49 notes, no E4");
  check(fs.readFileSync(FILE, "utf8") === saved, "the file is as it was");
  const g2 = await graph();
  check(g2.view.revision === g1.view.revision && bassNotes(g2) === bassNotes(g1), "no revision, and the timeline holds the saved notes");
  check(g2.can_undo === g1.can_undo, "undo is as it was");
  // An agent's Tx, through the API: refused the same way, and told so.
  const sha = (await import("node:crypto")).createHash("sha256").update(saved).digest("hex");
  const tx = {
    id: "tx-agent",
    base: { "data/bass/notes.json": `sha256:${sha}` },
    ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id: JSON.parse(saved).clips[0].id, start: 0, length: 4, notes: [] }], delete: [] }],
    expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
  };
  const agent = await post("tx/commit", { session: g2.session, tx });
  check(agent.status === 500 && agent.json.io?.taken_back === true && /taken back\.$/.test(agent.json.io?.message ?? ""), `an agent's Tx is answered taken_back: ${agent.status} ${JSON.stringify(agent.json).slice(0, 160)}`);
  check(fs.readFileSync(FILE, "utf8") === saved, "and changes nothing");
  check(await toasted(`An agent's edit: ${agent.json.io?.message}`), "and the app says so too: its listener heard it go");

  // Stopped: nothing is heard, and the message says only what wasn't saved.
  await post("graph/transport", { action: "stop" });
  await until(async () => !(await api("graph/status")).playing);
  await until(async () => (await toasts()).length === 0, 8000); // the messages fade
  at = cell(64, 3.1);
  await add(at);
  check(await toasted("couldn't be saved"), "stopped, a double-click the disk refuses: a message says what wasn't saved");
  const stopped = (await toasts()).find((t) => t.includes("couldn't be saved")) ?? "";
  check(stopped.startsWith("data/bass/notes.json couldn't be saved") && !/played|plays/.test(stopped), `and not that it played: “${stopped}”`);
  check(await until(async () => (await notes()).length === 49 && !(await notes()).includes(64)), "the piano roll shows the clip as it was");
  check(fs.readFileSync(FILE, "utf8") === saved, "the file is as it was");
  await post("graph/transport", { action: "play" });
  fs.chmodSync(TX, 0o755);
}

// ---------------------------------------------------------------- the disk takes writes again

await until(async () => (await toasts()).length === 0, 8000); // the messages fade
at = cell(67, 5.1);
await add(at);
check(await until(() => fs.readFileSync(FILE, "utf8").includes('"pitch": "G4"')), "once the disk takes writes again, the next double-click is saved");
check(await until(async () => (await notes()).length === 50), "and shows: 50 notes");
check(!(await toasts()).some((t) => t.includes("taken back")), "with no message");
await page.screenshot({ path: `${S}/g11-saved.png` });
for (let k = 0; k < 2; k++) {
  await page.keyboard.down("Meta");
  await page.keyboard.press("z");
  await page.keyboard.up("Meta");
  await sleep(400);
}
check(await until(() => fs.readFileSync(FILE, "utf8") === original), "two undos take back the two saved notes: the file is byte for byte what it was");
check(await until(async () => (await notes()).length === 48), "48 notes again");
await post("graph/transport", { action: "stop" });

check(consoleErrors.length === 0, `no console errors${consoleErrors.length ? `: ${consoleErrors.join(" | ")}` : ""}`);
console.log(`\n${pass} passed, ${fail} failed`);
await browser.close();
process.exit(fail ? 1 : 0);
