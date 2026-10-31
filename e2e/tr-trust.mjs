// Track trust: undo that survives closing (gaps #29, WRITE-23 to WRITE-26) and code that survives a
// crash (gaps #30, WRITE-27). On headless servers of its own, one after another, on one copy of
// first-song, with the run's scratch HOME:
//   - an edit in one server; that server stopped, another opened on the song: Undo names the edit,
//     and ⌘Z puts the file back byte for byte; ⇧⌘Z brings it back;
//   - closed again, a file changed outside: Undo says it isn't possible past that change;
//   - text typed in the code editor and not saved is kept in .music2/drafts/ a second later, never
//     in the song; the server killed (SIGKILL), another one opens the song: a notice says so, and
//     the code editor offers it back, Restore putting it in the editor, unsaved;
//   - killed again, the file changed meanwhile: the offer says the file changed; Discard throws the
//     text away; lint.
//
//   node e2e/tr-trust.mjs <root> <home>     (the `none` kind)
import puppeteer from "puppeteer-core";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { exe } from "./chrome.mjs";
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

const song = `${ROOT}/trust/First Song`;
fs.cpSync(`${REPO}/examples/first-song`, song, { recursive: true });
const read = (f) => fs.readFileSync(`${song}/${f}`, "utf8");
const drafts = () => {
  try {
    return fs.readdirSync(`${song}/.music2/drafts`).filter((n) => n.endsWith(".json")).map((n) => JSON.parse(fs.readFileSync(`${song}/.music2/drafts/${n}`, "utf8")));
  } catch {
    return [];
  }
};

/** A headless server of this suite's own with the song open; `stop` ends it as a quit does, `kill` as a crash. */
async function serve(name) {
  const file = `${ROOT}/${name}.json`;
  fs.rmSync(file, { force: true });
  const env = { ...process.env, HOME, M2_AUDIO: "none", M2_MIDI: "none" };
  delete env.CLAP_PATH;
  const proc = spawn(HEADLESS, [song, file, `${name}${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${ROOT}/${name}.log`, "w")] });
  let exited = false;
  proc.on("exit", () => (exited = true));
  servers.push(proc);
  await until(async () => fs.existsSync(file), 30000);
  await sleep(300);
  const s = JSON.parse(fs.readFileSync(file, "utf8"));
  const end = async (signal) => {
    proc.kill(signal);
    await until(async () => exited, 5000);
    if (!exited) proc.kill("SIGKILL");
    await until(async () => exited, 5000);
  };
  return { ...s, stop: () => end("SIGTERM"), kill: () => end("SIGKILL") };
}

const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });

/** A page on server `s`, in a browser context of its own, the timeline showing. */
async function pageOn(s) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.evaluateOnNewDocument((token) => (window.__M2__ = { native: false, token, platform: "macos" }), s.page_token);
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=timeline]", { timeout: 20000 });
  const state = () => page.evaluate(async (t) => (await fetch("/api/graph", { headers: { "x-m2-token": t } })).json(), s.page_token);
  return { page, state, close: () => context.close() };
}
const key = async (page, combo) => {
  const keys = combo.split("+");
  for (const k of keys.slice(0, -1)) await page.keyboard.down(k);
  await page.keyboard.press(keys.at(-1));
  for (const k of keys.slice(0, -1).reverse()) await page.keyboard.up(k);
};
const title = (page, label) => page.$eval(`button[aria-label="${label}"]`, (e) => e.title).catch(() => null);
const toasts = (page) => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
const unsaved = (page) => page.$eval("[data-testid=code-unsaved]", (e) => e.textContent ?? "").catch(() => null);
const shown = (page) => page.$eval("[data-testid=code] .view-lines", (e) => e.innerText.replace(/ /g, " ")).catch(() => "");

// ------------------------------------------------------------ an edit, the server stopped, undo

const notes = read("data/bass/notes.json");
let s = await serve("trust-1");
let p = await pageOn(s);
const tx = {
  id: "tx-trust-1",
  base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(notes).digest("hex")}` },
  ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id: "trust1", start: 0, length: 4, notes: [{ pitch: "E1", start: 0, length: 0.25 }] }], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
};
const posted = await p.page.evaluate(
  async (t, body) => (await fetch("/api/tx/commit", { method: "POST", headers: { "x-m2-token": t, "content-type": "application/json" }, body: JSON.stringify(body) })).status,
  s.page_token,
  { session: (await p.state()).session, tx },
);
const edited = read("data/bass/notes.json");
check(posted === 200 && edited !== notes, `an edit to Bass's notes lands: ${posted}`);
await p.close();
await s.stop();

s = await serve("trust-2");
p = await pageOn(s);
check(await until(async () => (await title(p.page, "Undo")) === "Undo Bass notes (⌘Z)"), `opened again, Undo names the edit: ${await title(p.page, "Undo")}`);
await key(p.page, "Meta+KeyZ");
check(await until(async () => read("data/bass/notes.json") === notes), "⌘Z puts the file back byte for byte");
check(await until(async () => (await title(p.page, "Redo")) === "Redo Bass notes (⇧⌘Z)"), `Redo names it: ${await title(p.page, "Redo")}`);
await key(p.page, "Meta+Shift+KeyZ");
check(await until(async () => read("data/bass/notes.json") === edited), "⇧⌘Z brings it back");
await p.close();
await s.stop();

// ------------------------------------------------------------ an outside change while closed

fs.writeFileSync(`${song}/data/bass/notes.json`, edited.replace(/"E1"/, '"F1"'));
s = await serve("trust-3");
p = await pageOn(s);
const ended = "Undo isn't possible past the change made outside music2";
check(await until(async () => (await title(p.page, "Undo")) === ended), `after a change made outside, Undo says so: ${await title(p.page, "Undo")}`);
check((await p.state()).undo_end === "outside", "and the state says why");

// ------------------------------------------------------------ unsaved code, a crash

await p.page.click('[data-view-tab="code"]');
await until(async () => !!(await p.page.$("[data-testid=code] .monaco-editor .view-lines")));
const songTs = read("song.ts");
await p.page.click("[data-testid=code] [data-file='song.ts']").catch(() => {});
await until(async () => (await shown(p.page)).includes("song"));
await p.page.click("[data-testid=code] .view-lines");
await key(p.page, "Meta+ArrowDown");
await p.page.keyboard.type("\n// typed, not saved\n");
check(await until(async () => drafts().some((d) => d.file === "song.ts" && d.text.includes("// typed, not saved")), 4000), "text typed is kept in .music2/drafts/ a second later");
check(read("song.ts") === songTs, "never in the song's file");
await p.close();
await s.kill();

s = await serve("trust-4");
p = await pageOn(s);
check(await until(async () => (await toasts(p.page)).some((t) => t.includes("song.ts has changes that weren't saved."))), `the server killed, the song opened again says so: ${JSON.stringify(await toasts(p.page))}`);
await p.page.click('[data-view-tab="code"]');
check(await until(async () => (await unsaved(p.page)) === "song.ts has changes that weren't saved. Restore them?DiscardRestore"), `the code editor offers them back: ${await unsaved(p.page)}`);
const barBad = await p.page.evaluate(lintIn, "[data-testid=code-unsaved]");
check(barBad.length === 0, `the offer passes the taste lint: ${barBad.slice(0, 4).join(" | ")}`);
const restore = await p.page.evaluateHandle(() => [...document.querySelectorAll("[data-testid=code-unsaved] button")].find((b) => b.textContent.trim() === "Restore"));
await restore.click();
await sleep(200);
await key(p.page, "Meta+ArrowDown");
check(await until(async () => (await shown(p.page)).includes("// typed, not saved")), "Restore puts the text in the editor");
const edited2 = await until(async () => !!(await p.page.$("[data-testid=code] [data-file='song.ts'][data-dirty]")));
check(edited2 && read("song.ts") === songTs && (await unsaved(p.page)) === null, "unsaved (song.ts is marked edited), and the offer goes");
await p.close();
await s.kill();

// ------------------------------------------------------------ the file changed meanwhile

fs.writeFileSync(`${song}/song.ts`, `${songTs}// changed in another editor\n`);
s = await serve("trust-5");
p = await pageOn(s);
await p.page.click('[data-view-tab="code"]');
check(
  await until(async () => (await unsaved(p.page))?.startsWith("song.ts has changes that weren't saved, but song.ts was changed since. Restore them over it?")),
  `a draft whose file changed says so: ${await unsaved(p.page)}`,
);
const discard = await p.page.evaluateHandle(() => [...document.querySelectorAll("[data-testid=code-unsaved] button")].find((b) => b.textContent.trim() === "Discard"));
await discard.click();
check(await until(async () => drafts().length === 0 && (await unsaved(p.page)) === null), "Discard throws it away");
check(read("song.ts") === `${songTs}// changed in another editor\n`, "and the file stays as the other editor left it");
await p.close();
await s.stop();

check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
