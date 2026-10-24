// Track 3 of the agents design (ag-see): what an agent sees beside the musician (AGENT-6), and the
// Compare key, V (D2). On the run's server with first-song open:
//   - Bass selected and two of its notes picked in the roll is what `m2 focus` says, by key and by
//     note; the page posts its focus record, which changes no file;
//   - a pointer (`m2 point`) shows a note at its target, by the agent's name, and never moves the
//     view or the selection; off screen it waits at the edge with Show, and only Show moves the
//     view; OK takes it away for everyone;
//   - V flips the newest outside step while the loop plays: the pill says Before, then After, and
//     the file follows; an agent's write while the pill is open is refused, busy (`m2` exits 3),
//     and After stays; Esc closes the pill and keeps what plays; V on the musician's own change
//     says what V is for;
//   - a file written outside while the pill is open closes it, saying so;
//   - lint, no console errors.
//
//   node e2e/ag-focus.mjs <root> <home> <project> <m2>     (the `export` kind)
//   AG_SHOT=<folder> also saves pictures of the pointer and the pill there.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { PROJ, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const M2 = process.argv[5];
const { page, errors, api, key, show, view } = await open(1440, 900);
const shot = async (name, sel) => {
  if (!process.env.AG_SHOT) return;
  fs.mkdirSync(process.env.AG_SHOT, { recursive: true });
  await page.screenshot({ path: `${process.env.AG_SHOT}/${name}.png` });
  const b = sel && (await (await page.$(sel))?.boundingBox());
  if (b) await page.screenshot({ path: `${process.env.AG_SHOT}/${name}-near.png`, clip: { x: Math.max(0, b.x - 60), y: Math.max(0, b.y - 40), width: b.width + 120, height: b.height + 80, scale: 2 } });
};
/** `m2 args…`, as an agent runs it beside the app: its status and what it said. */
const m2 = (args, env = {}) => {
  const r = spawnSync(M2, args, { env: { ...process.env, ...env }, encoding: "utf8", timeout: 60000 });
  return { status: r.status, out: r.stdout ?? "", err: r.stderr ?? "" };
};
/** What the server's state says, read with the tools key: as an agent sees it. */
const asAgent = async (p) => (await fetch(`${sess.url}${p}`, { headers: { "x-m2-token": sess.token } })).json();
const state = async () => (await api("")).body;
/** Every file of the song but its `.music2/`, hashed: what a write would change. */
const files = (dir = PROJ) =>
  fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.name !== ".music2")
    .flatMap((e) => (e.isDirectory() ? files(path.join(dir, e.name)) : [`${path.join(dir, e.name)} ${createHash("sha256").update(fs.readFileSync(path.join(dir, e.name))).digest("hex")}`]))
    .sort();
/** Where the view is and what is selected: what a pointer must never move. */
const viewNow = () =>
  page.evaluate(() => {
    const t = document.querySelector("[data-testid=timeline]");
    const roll = document.querySelector("[data-testid=roll-scroller]");
    return JSON.stringify({
      left: t?.scrollLeft,
      top: t?.scrollTop,
      roll: [roll?.scrollLeft, roll?.scrollTop],
      tab: document.querySelector("[data-module-tab][aria-selected=true]")?.dataset.moduleTab,
      dock: document.querySelector("[data-view-tab][aria-selected=true]")?.dataset.viewTab,
      notes: [...document.querySelectorAll("[data-testid=notes] [data-note][data-selected=true]")].map((e) => e.dataset.note),
    });
  });
/** The view once it holds still (the roll settles after it opens). */
const steady = async () => {
  let was = await viewNow();
  for (let i = 0; i < 20; i++) {
    await sleep(150);
    const now = await viewNow();
    if (now === was) return now;
    was = now;
  }
  return was;
};
const pointerNote = () => page.$eval("[data-testid=pointer-note]", (e) => ({ text: e.textContent ?? "", edge: e.dataset.edge ?? null })).catch(() => null);
const pill = () => page.$eval("[data-testid=compare-pill]", (e) => e.textContent ?? "").catch(() => null);
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
const status = async () => (await api("/status")).body;

// ---------------------------------------------------------------- what the musician looks at

const before = files();
await show("bass");
const clip = await (await page.$('[data-lane="bass.notes"] :nth-child(1 of [data-clip])')).boundingBox();
await page.mouse.click(clip.x + 60, clip.y + clip.height / 2, { count: 2 });
check(await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length > 2), "Bass's notes clip opens in the roll");
const picked = await page.$$eval('[data-testid=notes] [data-note="0"], [data-testid=notes] [data-note="2"]', (els) =>
  els.map((e) => ({ pitch: Number(e.dataset.pitch), start: Number(e.dataset.start), length: Number(e.dataset.length) })),
);
await page.click('[data-testid=notes] [data-note="0"]');
await page.keyboard.down("Shift");
await page.click('[data-testid=notes] [data-note="2"]');
await page.keyboard.up("Shift");
check(await until(async () => (await page.$$("[data-testid=notes] [data-note][data-selected=true]")).length === 2), "two notes are selected");
let said = null;
check(
  await until(async () => {
    const o = m2(["focus", PROJ]);
    said = o.status === 0 ? JSON.parse(o.out) : o;
    return said.notes?.notes?.length === 2;
  }, 6000),
  `m2 focus says the two notes: ${JSON.stringify(said).slice(0, 300)}`,
);
check(said?.view === "notes" && said?.track === "bass", `by the view and the track's key: ${said?.view} ${said?.track}`);
const clipId = await page.$eval('[data-lane="bass.notes"] [data-clip][data-open]', (e) => e.dataset.clip);
check(said?.notes?.clip?.track === "bass" && said?.notes?.clip?.lane === "notes" && said?.notes?.clip?.id === clipId, `and their clip, by id: ${JSON.stringify(said?.notes?.clip)}`);
const asNotes = (list) => JSON.stringify((list ?? []).map((n) => [n.pitch, n.start, n.length]));
check(asNotes(said?.notes?.notes) === asNotes(picked), `each note by what it is: ${asNotes(said?.notes?.notes)} = ${asNotes(picked)}`);
check(Array.isArray(said?.seen) && said.seen[1] > said.seen[0] && said.playing === false && said.comparing === null, `with the bars shown and what the server knows: seen ${said?.seen}, playing ${said?.playing}`);
check(JSON.stringify(files()) === JSON.stringify(before), "focus changes no file");

// ---------------------------------------------------------------- pointers

let seen = await steady();
let o = m2(["point", PROJ, "track:kick", "the kick is here", "--as", "Claude"]);
check(o.status === 0 && /pointing at track kick/.test(o.out), `m2 point answers: ${o.out}${o.err}`);
check(await until(async () => (await pointerNote())?.text.includes("Claude: the kick is here")), `a note shows, by the agent's name: ${JSON.stringify(await pointerNote())}`);
const mark = await (await page.$("[data-testid=pointer-mark]"))?.boundingBox();
const kick = await (await page.$('[data-module-tab="kick"]'))?.boundingBox();
check(!!mark && !!kick && Math.abs(mark.y - kick.y) < 8 && Math.abs(mark.x - kick.x) < 8, `at the kick's header: ${JSON.stringify(mark)} ~ ${JSON.stringify(kick)}`);
check((await pointerNote())?.edge === null && !(await page.$("[data-testid=pointer-show]")), "in view: no edge and no Show");
check((await viewNow()) === seen, `the view and the selection stay: ${await viewNow()} (was ${seen})`);
check((await page.evaluate(lint, "[data-testid=pointer-note]")).length === 0, `the note is in the design system: ${(await page.evaluate(lint, "[data-testid=pointer-note]")).slice(0, 3)}`);
await shot("pointer", "[data-testid=pointer-note]");
// Out of view: zoomed in (a pinch over the timeline's start) so the last bars are off to the right.
const lanes = await (await page.$("[data-testid=ruler]")).boundingBox();
await page.mouse.move(lanes.x + 300, lanes.y + 60);
for (let i = 0; i < 10; i++) {
  const s = (await asAgent("/api/focus")).seen;
  if (s && s[1] < 20) break;
  await page.keyboard.down("Control");
  await page.mouse.wheel({ deltaY: -100 });
  await page.keyboard.up("Control");
  await sleep(300);
}
const shown = (await asAgent("/api/focus")).seen;
check(shown && shown[1] < 28, `zoomed in, the timeline shows beats ${shown}`);
seen = await steady();
const bar = 8;
o = m2(["point", PROJ, `bars:${bar}`, "listen to the end here", "--as", "Claude"]);
check(o.status === 0, `m2 point at bar ${bar}: ${o.out}${o.err}`);
check(await until(async () => (await pointerNote())?.edge === "right"), `out of view, it waits at the right edge: ${JSON.stringify(await pointerNote())}`);
check(!!(await page.$("[data-testid=pointer-show]")), "with Show");
check((await viewNow()) === seen, `nothing moved: ${await viewNow()} (was ${seen})`);
await shot("pointer-edge", "[data-testid=pointer-note]");
await page.click("[data-testid=pointer-show]");
check(await until(async () => (await pointerNote())?.edge === null && !!(await page.$("[data-testid=pointer-mark]"))), "Show brings the bars into view");
const after = JSON.parse(await viewNow());
check(after.left > JSON.parse(seen).left && after.tab === "bass" && after.notes.length === 2, `only the view moved, by Show: ${JSON.stringify(after)}`);
await shot("pointer-shown", "[data-testid=pointer-note]");
await page.click("[data-testid=pointer-ok]");
check(await until(async () => !(await pointerNote())), "OK takes it away");
check(await until(async () => (await state()).point === null), "for everyone");
check(JSON.stringify(files()) === JSON.stringify(before), "pointers change no file");

// ---------------------------------------------------------------- Compare (V)

// The loop plays.
await api("/loop", { method: "POST", body: JSON.stringify({ start: 0, end: 16, on: true }) });
await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
check(await until(async () => (await status()).playing), "the loop plays");
const bass = read("tracks/bass.ts");
const changed = bass.replace("period: 0.5", "period: 0.25");
check(changed !== bass, "tracks/bass.ts has an lfo period of 0.5");
write("tracks/bass.ts", changed);
check(await until(async () => (await state()).undo_name?.source === "outside"), "a save from outside is the newest step");
const step = (await state()).undo_name.step;
await page.mouse.move(1200, 880);
await key("KeyV");
check(await until(async () => /^Before:/.test((await pill()) ?? "")), `V: the pill says Before: ${await pill()}`);
check((await pill())?.includes("outside change: the code in tracks/bass.ts"), `naming the change: ${await pill()}`);
check(await until(() => read("tracks/bass.ts") === bass), "and the change is undone");
let s = await status();
check(s.playing && s.loop?.on, "the loop plays on");
check((await asAgent("/api/focus")).comparing?.step === step, "an agent sees what is compared");
await shot("compare-before", "[data-testid=compare-pill]");
await key("KeyV");
check(await until(async () => /^After:/.test((await pill()) ?? "")), `V again: After: ${await pill()}`);
check(await until(() => read("tracks/bass.ts") === changed), "the change is back");
check((await page.evaluate(lint, "[data-testid=compare-pill]")).length === 0, `the pill is in the design system: ${(await page.evaluate(lint, "[data-testid=compare-pill]")).slice(0, 3)}`);
await shot("compare-after", "[data-testid=compare-pill]");
// An agent's write while it is open: busy, and After stays.
const notes = read("data/bass/notes.json");
const tx = {
  id: "agent-compare",
  base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(notes).digest("hex")}` },
  ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id: "agentcmp", start: 0, length: 4, notes: [{ pitch: "E1", start: 0, length: 0.25 }] }], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
};
const sendTx = async () => {
  const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify({ tx, as: "Claude", why: "a new bass hit" }) });
  return { status: res.status, body: await res.json() };
};
const busy = await sendTx();
check(busy.status === 423 && busy.body.busy?.what === "compare", `an agent's write is busy: ${busy.status} ${JSON.stringify(busy.body)}`);
o = m2(["tempo", PROJ, "130", "--as", "Claude"], { M2_BUSY_WAIT: "1" });
check(o.status === 3 && /you're comparing/.test(o.err), `m2 waits, then exits 3 saying why: ${o.status} ${o.err.trim().split("\n").at(-1)}`);
check(read("data/bass/notes.json") === notes && read("tracks/bass.ts") === changed, "nothing changed: After stays");
check(/^After:/.test((await pill()) ?? "") && (await state()).undo_name?.step === step, "the pill and the step are as they were");
// Esc closes it, keeping what plays.
await key("Escape");
check(await until(async () => (await pill()) === null), "Esc closes the pill");
check(await until(async () => (await asAgent("/api/focus")).comparing === null), "for the server too");
s = await status();
check(read("tracks/bass.ts") === changed && s.playing, "what plays stays");
check((await sendTx()).status === 200, "agents write again");
// The newest change is the musician's own: V says what it is for.
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });
const own = await state();
await api("/title", { method: "POST", body: JSON.stringify({ session: own.session, name: "Compare me" }) });
check(await until(async () => (await state()).undo_name?.source === "page"), "the musician's own change is the newest");
await page.mouse.move(1200, 880);
await key("KeyV");
check(await until(async () => (await toasts()).some((t) => /V compares the newest change an agent or another program made/.test(t))), `V says what it is for: ${await toasts()}`);
check((await pill()) === null, "and opens no pill");

// ---------------------------------------------------------------- a file written outside closes it

const tx2 = { ...tx, id: "agent-compare-2", base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(read("data/bass/notes.json")).digest("hex")}` } };
tx2.ops = [{ ...tx.ops[0], put: [{ ...tx.ops[0].put[0], id: "agentcm2", start: 4 }] }];
const r2 = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify({ tx: tx2, as: "Claude", why: "another hit" }) });
check(r2.status === 200, `an agent's step: ${r2.status}`);
check(await until(async () => (await state()).undo_name?.source === "agent"), "is the newest");
await page.mouse.move(1200, 880);
await key("KeyV");
check(await until(async () => /^Before: Claude: another hit/.test((await pill()) ?? "")), `V: Before, by the agent's name: ${await pill()}`);
write("tracks/hats.ts", read("tracks/hats.ts").replace("track(", "// a line from another editor\ntrack("));
check(await until(async () => (await pill()) === null, 4000), "a file written outside closes the pill");
check(await until(async () => (await toasts()).some((t) => /A file changed outside the app, so Compare closed/.test(t))), `saying so: ${await toasts()}`);

done(errors);
