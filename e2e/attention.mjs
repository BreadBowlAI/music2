// Data a key rename left behind, on a fresh copy of the converted examples/first-song (format 1):
// rename the kick track's key in the code editor; the "Needs attention" strip explains it as
// `data.unclaimed` on the track's folder (ID-17, DIAG-6); its one-click fix moves the whole folder,
// undo moves it back, Delete and undo; a lane key renamed by hand is a per-file pick; every graph
// event carries the diagnostics (DIAG-4). Then knob undo after a call is inserted in an
// earlier-loaded file (S1), through the UI.
//   node run.mjs attention attn   (node attention.mjs <root> <home> <project>)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, toasts, show, view, key } = await open();
const OLD = "data/kick/hits.json";
const NEW = "data/kicks/hits.json";
const notes = read(OLD);
const exists = (path) => fs.existsSync(`${PROJ}/${path}`);
// What needs attention is in the status pill's popover (taste.md ST-1, ST-4): open it to read it.
const pop = async () => {
  // (The pill can go between the look and the click: then there is nothing to open.)
  if ((await page.$("[data-status~=problems]")) && !(await page.$("[data-testid=status-popover]"))) await page.click("[data-status]").catch(() => {});
};
const strip = async () => (await pop(), page.$("[data-testid=attention]"));
const rows = async () => (await pop(), page.$$eval("[data-testid=attention] [data-problem]", (els) => els.map((e) => ({ code: e.dataset.problem, severity: e.dataset.severity, src: e.dataset.src }))));
const messages = async () => (await pop(), page.$$eval("[data-testid=attention] [data-testid=problem-message]", (els) => els.map((e) => e.textContent)));
const clips = (lane) => page.$$eval(`[data-lane="${lane}"] [data-clip]`, (els) => els.length).catch(() => 0);
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)));
const likely = '[data-testid=attention] [data-fix][data-likely]';
const unclaimed = (s) => [...(s.problems ?? []), ...(s.warnings ?? [])].filter((d) => d.code === "data.unclaimed");

// Every graph event the server sends, as a client that only listens hears them.
await page.evaluate((token) => {
  window.__events = [];
  const ws = new WebSocket(`ws://${location.host}/api/ws?token=${token}`);
  ws.onmessage = (e) => window.__events.push(JSON.parse(e.data));
  return new Promise((r) => (ws.onopen = r));
}, sess.token);
const events = () => page.evaluate(() => window.__events.filter((e) => e.t === "graph"));
const said = (e) => [...(e.problems ?? []), ...(e.warnings ?? [])];

check(!(await strip()), "nothing needs attention at first");
const kickClips = await clips("kick.hits");
check(kickClips > 0, `the kick track's lane shows its clips (${kickClips})`);

// ---------------------------------------------------------------- a track's key renamed in the code editor

await show("kick");
await view("code");
await until(() => page.$eval("[data-testid=code-path]", (e) => e.textContent === "tracks/kick.ts").catch(() => false));
await page.click("[data-testid=code] .view-lines");
await key("Meta+f");
await page.waitForSelector("[data-testid=code] .find-widget.visible");
await page.keyboard.type('("kick');
await sleep(150);
await key("Escape");
// Right after the key (typing a quote would bring an automatic closing one).
await key("ArrowRight");
await page.keyboard.type("s");
await key("Meta+s");
check(await until(() => read("tracks/kick.ts").includes('track("kicks"')), "the track with its key renamed is saved");
check(await until(async () => !!(await strip())), "the strip says something needs attention");
// ID-17: a track key renamed by hand leaves its whole folder unclaimed, offered whole (DIAG-6).
const [row] = await rows();
check(row?.code === "data.unclaimed" && row.severity === "warning" && row.src === "data/kick", `data.unclaimed on its folder: ${JSON.stringify(row)}`);
const message = (await messages())[0] ?? "";
check(/Kick \(kick\)/.test(message) && /Kick \(kicks\)/.test(message), `in plain words, naming the track by label and key: ${message}`);
await pop();
const fix = await page.$eval(likely, (e) => ({ code: e.dataset.fix, to: e.dataset.to, text: e.textContent })).catch(() => null);
check(fix?.code === "relink" && fix.to === "data/kicks", `one click moves the folder to the new key: ${JSON.stringify(fix)}`);
check(await until(async () => (await clips("kicks.hits")) === 0), "the renamed track's lane is empty: it plays nothing");
const warned = await state();
const w = unclaimed(warned)[0];
check(w?.fixes?.[0]?.likely === true && w.fixes[0].code === "relink" && w.fixes.at(-1).code === "delete-data", "the state carries the problem and its fixes, the likeliest first");
check(
  await until(async () => (await events()).some((e) => e.revision === warned.view.revision && said(e).some((d) => d.code === "data.unclaimed" && d.src === "data/kick"))),
  "and so does the graph event for that revision",
);
await page.screenshot({ path: `${ROOT}/attention-strip.png` });

// ---------------------------------------------------------------- Move, and undo

await pop();
await page.click(likely);
check(await until(() => exists(NEW) && !exists(OLD)), "Move renames the folder for the new key");
check(read(NEW) === notes, "byte for byte");
check(await until(async () => (await clips("kicks.hits")) === kickClips), "the track sounds again: its lane shows its notes");
check(await until(async () => !(await strip())), "and nothing needs attention");
check(await toasted("Moved data/kick to data/kicks."), "a toast says what moved");
check(await until(async () => (await events()).some((e) => !said(e).some((d) => d.code === "data.unclaimed"))), "the event for the move carries nothing unclaimed");
await page.screenshot({ path: `${ROOT}/attention-moved.png` });

await page.click('button[aria-label="Undo"]');
check(await until(() => exists(OLD) && !exists(NEW)), "Undo moves it back");
check(read(OLD) === notes, "byte for byte");
check(await until(async () => !!(await strip())), "and the strip is back");
check(await until(async () => (await clips("kicks.hits")) === 0), "with the lane empty again");
// The folder is back where it was, holding data no module reads.
// A structural undo says so, in the fix's own words: never a file or a diff (taste.md ST-5, ST-6).
const undid = () => toasts().then((ts) => ts.find((t) => t.startsWith("Undid “Move all to") && !t.includes("data/")));
check(await until(undid), `a toast says what was undone, in words: ${await undid()}`);

await page.click('button[aria-label="Redo"]');
check(await until(() => exists(NEW) && !exists(OLD) && read(NEW) === notes), "Redo moves it again");
await page.click('button[aria-label="Undo"]');
check(await until(() => exists(OLD) && !exists(NEW)), "and Undo back");

// ---------------------------------------------------------------- Delete, and undo

await until(async () => (await pop(), !!(await page.$('[data-testid=attention] [data-fix="delete-data"]:not([disabled])'))));
await page.click('[data-testid=attention] [data-fix="delete-data"]');
check(await until(() => !exists(OLD) && !exists(NEW)), "Delete removes the data");
check(await until(async () => !(await strip())), "and the strip goes");
check(await toasted("Deleted data/kick."), "a toast says what was deleted");
// The toast's own Undo.
await page.evaluate(() => [...document.querySelectorAll("[data-testid=toast] button")].find((b) => b.textContent === "Undo")?.click());
check(await until(() => exists(OLD)), "its Undo brings the file back");
check(read(OLD) === notes, "byte for byte");
check(await until(async () => !!(await strip())), "and the strip with it");

// ---------------------------------------------------------------- several places could take it

const kickTs = read("tracks/kick.ts");
// The places it could go are a menu now, not a native select (taste.md ST-4): its button, then its rows.
const pick = '[data-testid=attention] [data-fix="relink-pick"]';
const menu = async () => {
  await pop();
  if (!(await page.$("[data-relink-menu]"))) await page.click(pick);
  await page.waitForSelector("[data-relink-menu]", { timeout: 2000 });
};
const options = async () => {
  await menu();
  const items = await page.$$eval("[data-relink-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
  const title = await page.$eval(pick, (e) => e.textContent);
  await page.keyboard.press("Escape");
  return [title, ...items];
};
const choose = async (i) => {
  await menu();
  await page.$$eval("[data-relink-menu] [role=menuitem]", (els, i) => els[i].click(), i);
};
write("tracks/kick.ts", kickTs.replace('  const out = kick(lane.notes("hits"));', '  lane.notes("accents");\n  const out = kick(lane.notes("hits"));'));
await sleep(500);
check(await until(async () => (await pop(), !!(await page.$(pick)))), "with two lanes free, the person can also pick a lane");
check((await page.$eval(likely, (e) => e.dataset.to).catch(() => null)) === "data/kicks", "the folder on the renamed track stays one click");
const opts = await options();
check(opts.length === 3 && opts.slice(1).every((o) => /^Move to Kick \(kicks\) › (hits|accents)$/.test(o)), `and each lane is a pick: ${opts}`);
const accents = opts.findIndex((o) => o.endsWith("accents")) - 1;
await choose(accents);
check(await until(() => exists("data/kicks/accents.json") && !exists(OLD)), "the one picked gets it");
await page.click('button[aria-label="Undo"]');
check(await until(() => exists(OLD) && !exists("data/kicks/accents.json")), "and Undo takes it back");

// A lane key renamed on a track that keeps its key, with two lanes free: neither is plainly its own, so only a pick.
write("tracks/kick.ts", kickTs.replace('track("kicks"', 'track("kick"').replace('  const out = kick(lane.notes("hits"));', '  lane.notes("accents");\n  const out = kick(lane.notes("beats"));'));
check(
  await until(async () => JSON.stringify(await options().catch(() => [])) === JSON.stringify(["Move to…", "Move to Kick (kick) › accents", "Move to Kick (kick) › beats"])),
  `two lanes as likely: the person picks (${await options().catch(() => [])})`,
);
await pop();
check(!(await page.$('[data-testid=attention] [data-fix="relink"]')), "with no one-click guess");
check((await rows())[0]?.src === OLD, "the problem is on the lane's file");
check(
  (await messages())[0] ===
    "Notes on Kick (kick) › hits aren't playing. Kick (kick) has no lane called hits any more. Did you rename it? " +
      "Move them to the lane they belong to now (Kick (kick) › accents or Kick (kick) › beats), or delete them.",
  `saying so: ${(await messages())[0]}`,
);
await choose(1);
check(await until(() => exists("data/kick/beats.json") && !exists(OLD)), "the one picked gets it");
await page.click('button[aria-label="Undo"]');
check(await until(() => exists(OLD) && !exists("data/kick/beats.json")), "and Undo takes it back");

// A toast's Undo once something else changed: it doesn't take back that other change.
check(kickTs.includes('track("kicks"'), "(the track's key is kicks in the code by now)");
write("tracks/kick.ts", kickTs);
await until(async () => (await pop(), !!(await page.$(`${likely}:not([disabled])`))));
await page.click(likely);
check(await until(() => exists(NEW) && !exists(OLD)), "moved again");
// One toast shows at a time, so the other change is one made outside the app, which makes none.
const songBefore = read("song.ts");
const revBefore = (await state()).view.revision;
// The page too (D7): a toast's Undo judges by the revision the page shows, so the page must have the
// outside edit's before the click: the page's own read of the song at that revision.
const fresh = page
  .waitForResponse(async (r) => r.request().method() === "GET" && new URL(r.url()).pathname === "/api/graph" && ((await r.json().catch(() => null))?.view?.revision ?? 0) > revBefore, { timeout: 8000 })
  .catch(() => null);
write("song.ts", `${songBefore}// an edit made outside the app\n`);
await until(async () => (await state()).view.revision > revBefore);
// The outside edit is no undo step (gaps #29: the server names the steps): Undo still takes back the move, and says so.
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (b) => b.title);
check(!!(await fresh) && (await undoTitle()) === "Undo “Move all to Kick (kicks)” (⌘Z)", `the page has the outside edit; its Undo still names the move (${await undoTitle()})`);
const clicked = await page.evaluate(() => {
  const moved = [...document.querySelectorAll("[data-testid=toast]")].filter((t) => t.textContent.startsWith("Moved data/kick to data/kicks."));
  moved.at(-1)?.querySelector("button")?.click();
  return moved.length > 0;
});
check(clicked, "the move's toast is still up");
check(await toasted("can't be undone from here"), "the move's toast Undo says it can't any more");
await sleep(500);
check(exists(NEW) && !exists(OLD), "and undoes nothing");
await page.click('button[aria-label="Undo"]');
check(await until(() => exists(OLD) && !exists(NEW)), "the move is undone from the top bar");
write("song.ts", songBefore);

// ---------------------------------------------------------------- S1: knob undo after a call inserted earlier

write("tracks/kick.ts", kickTs.replace('track("kicks"', 'track("kick"'));
check(await until(async () => !(await strip())), "the key back as it was: nothing needs attention");
await show("bass");
await view("graph");
const hi = '[data-module="bass"] [data-node="xrange"] [data-knob="hi"]';
await page.waitForSelector(`${hi} svg`);
const bass = read("tracks/bass.ts");
await page.$eval(`${hi} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const box = await (await page.$(`${hi} svg`)).boundingBox();
const [cx, cy] = [box.x + box.width / 2, box.y + box.height / 2];
await page.mouse.move(cx, cy);
await page.mouse.down();
for (let i = 1; i <= 6; i++) await page.mouse.move(cx, cy + i * 4);
await page.mouse.up();
check(await until(() => read("tracks/bass.ts") !== bass), "dragging the bass's hi knob wrote tracks/bass.ts");
const set = read("tracks/bass.ts");
check(!set.includes("khz(2.5)") && set.replace(/khz\([0-9.]+\)/, "khz(2.5)") === bass, `only the value changed: ${set.match(/khz\([0-9.]+\)/)?.[0]}`);
const before = (await state()).view.revision;
// From outside the app: a call that makes no sound, in song.ts, which loads before tracks/bass.ts.
const song = read("song.ts");
write("song.ts", song.replace("export default", "Math.max(1, 2);\n\nexport default"));
check(await until(async () => (await state()).view.revision > before), "the outside edit is taken in");
check(await until(async () => (await state()).can_undo), "undo is offered");
// The outside edit came between, and is no undo step: Undo takes back the knob, and its tooltip says so (gaps #29).
const unnamed = await page.$eval('button[aria-label="Undo"]', (b) => b.title);
await page.click('button[aria-label="Undo"]');
check(await until(() => read("tracks/bass.ts") === bass), "and works: the knob's value is back");
check(read("song.ts").includes("Math.max(1, 2);"), "the outside edit stays");
await sleep(400);
check(unnamed === "Undo Xrange hi (⌘Z)" && !(await toasts()).some((t) => t.includes("tracks/")), `Undo names the knob it takes back (${unnamed}), and no toast names a file`);
check(!(await toasts()).some((t) => t.includes("rejected")), "no refusal");

await browser.close();
done(errors);
