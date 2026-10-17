// Next lane C3: Undo says what it undoes (gaps #29, plan Q19 as X7 and X8 built it). On the run's
// server with first-song open:
//   - Hats' fader moved in Mix: Undo's tooltip "Undo Hats volume (⌘Z)", and the server keeps the
//     name (`undo_name.given`);
//   - a Tx posted from outside (an agent's, on Bass's notes): "Undo Bass notes (⌘Z)", a plain name;
//   - ⌘Z: Redo's tooltip names what it would redo; Undo's the step below;
//   - Delay added then undone: the toast "Undid adding Delay";
//   - a reload keeps the names; a second window sees them; a PC page says them with Ctrl;
//   - lint.
//
//   node e2e/nx-undo.mjs <root> <home> <project>     (the `fix` kind)
import { createHash } from "node:crypto";
import { check, done, open, read, sess, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, toasts, key, view } = await open(1440, 900);
const undoTitle = (p = page) => p.$eval('button[aria-label="Undo"]', (e) => e.title);
const redoTitle = (p = page) => p.$eval('button[aria-label="Redo"]', (e) => e.title);

// ---------------------------------------------------------------- a fader moved in this window

await key("Alt+3");
await until(async () => !!(await page.$('[data-testid=mix] [data-strip="hats"] [data-fader]')));
const thumb = '[data-testid=mix] [data-strip="hats"] [data-fader] [data-testid=fader-thumb]';
const before = read("song.ts");
const t = await (await page.$(thumb)).boundingBox();
await page.mouse.move(t.x + t.width / 2, t.y + t.height / 2);
await page.mouse.down();
await page.mouse.move(t.x + t.width / 2, t.y + t.height / 2 - 24, { steps: 8 });
await page.mouse.up();
check(await until(() => read("song.ts") !== before), "a drag on Hats' fader writes song.ts");
check(await until(async () => (await undoTitle()) === "Undo Hats volume (⌘Z)"), `Undo names it: ${await undoTitle()}`);
const named = await until(async () => (await state()).undo_name?.given === true);
const s1 = await state();
check(named && s1.undo_name?.name === "Hats volume" && s1.redo_name === null, `the server keeps the name: ${JSON.stringify(s1.undo_name)} / ${JSON.stringify(s1.redo_name)}`);
check((await toasts()).length === 0, "a direct change makes no toast (ST-5)");

// ---------------------------------------------------------------- a Tx from outside

const notes = read("data/bass/notes.json");
const tx = {
  id: "tx-agent-nx-undo",
  base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(notes).digest("hex")}` },
  ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id: "agent1", start: 0, length: 4, notes: [{ pitch: "E1", start: 0, length: 0.25 }] }], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
};
const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify({ session: (await state()).session, tx }) });
const posted = { status: res.status, body: await res.json().catch(() => null) };
check(posted.status === 200, `an agent's Tx on Bass's notes commits: ${posted.status} ${JSON.stringify(posted.body).slice(0, 200)}`);
check(await until(async () => (await undoTitle()) === "Undo Bass notes (⌘Z)"), `Undo names it plainly: ${await undoTitle()}`);

// ---------------------------------------------------------------- undo, redo

await key("Meta+KeyZ");
check(await until(() => read("data/bass/notes.json") === notes), "⌘Z undoes the agent's notes");
check(await until(async () => (await redoTitle()) === "Redo Bass notes (⇧⌘Z)"), `Redo names it: ${await redoTitle()}`);
check(await until(async () => (await undoTitle()) === "Undo Hats volume (⌘Z)"), `and Undo the step below: ${await undoTitle()}`);
await key("Meta+KeyZ");
check(await until(() => read("song.ts") === before), "⌘Z again undoes the fader");
check(await until(async () => (await redoTitle()) === "Redo Hats volume (⇧⌘Z)"), `Redo names the fader now: ${await redoTitle()}`);

// ---------------------------------------------------------------- a structural step says so when undone

await key("Alt+1");
await page.click('[data-module-tab="keys"]').catch(() => {});
const plus = '[data-module="keys"] button[aria-label="Add an effect after pluck"]';
await until(async () => !!(await page.$(plus)));
await page.click(plus);
await page.waitForSelector('[data-module="keys"] [data-effect="delay"]', { timeout: 5000 }).catch(() => null);
await page.click('[data-module="keys"] [data-effect="delay"]');
check(await until(async () => (await undoTitle()) === "Undo adding Delay (⌘Z)"), `adding Delay is Undo's: ${await undoTitle()}`);
await sleep(300);
await page.click('button[aria-label="Undo"]');
check(await until(async () => (await toasts()).some((x) => x.startsWith("Undid adding Delay"))), `undoing it says so in a toast: ${JSON.stringify(await toasts())}`);
check(await until(async () => (await redoTitle()) === "Redo adding Delay (⇧⌘Z)"), `Redo names it: ${await redoTitle()}`);

// ---------------------------------------------------------------- after a reload, in another window, on a PC

await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
check(await until(async () => (await redoTitle()) === "Redo adding Delay (⇧⌘Z)"), `a reload keeps the names: ${await redoTitle()}`);
const other = await open(1440, 900, { pc: true });
check(await until(async () => (await redoTitle(other.page)) === "Redo adding Delay (Ctrl+Shift+Z)"), `a second window, a PC's, says them too: ${await redoTitle(other.page)}`);
await other.browser.close();

const bad = await page.evaluate(lint, "header");
check(bad.length === 0, `the top bar passes the taste lint: ${bad.slice(0, 4).join(" | ")}`);
await view("notes").catch(() => {});
done(errors);
