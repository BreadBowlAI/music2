// Track 3 of the agents design (ag-see) and its review: what an agent sees beside the musician
// (AGENT-6), and the Compare key, V (D2). On the run's server with first-song open:
//   - Bass selected and two of its notes picked in the roll is what `m2 focus` says, by key and by
//     note; the page posts its focus record, which changes no file;
//   - a pointer (`m2 point`) shows a note at its target, by the agent's name, and never moves the
//     view or the selection; off screen it waits at the edge with Show, and only Show moves the
//     view; OK takes it away for everyone; a knob of a track the dock doesn't show is named beside
//     that track's header, and nothing is selected; a press on the note's words reaches what is
//     under it;
//   - V on the newest outside step while the loop plays: the pill says Before, then After, and
//     Compare writes nothing; Space stops and plays again and the pill stays (comparing is
//     listening); an agent's write while the pill is open is refused, busy (`m2` exits 3); a
//     pointer's OK keeps the pill; Esc on After keeps After; Esc on Before undoes the step, saying
//     so, with Redo; a click elsewhere on Before goes back to After; V on the musician's own change
//     says what V is for;
//   - a file written outside while the pill is open closes it, saying so, and the change stays;
//   - two quick Vs, V V on the open pill, and V held: the pill always says what the server plays;
//   - a page that missed a pointer and Compare reads both again once it hears `lagged`;
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
// A knob of a track the dock doesn't show (the review's F10): the note names it beside that track's
// header, and Show selects nothing, so the MIDI keyboard's target stays too.
const kickMod = (await state()).view.modules.find((m) => m.key === "kick");
const kickKnob = kickMod?.nodes.flatMap((n) => n.knobs ?? [])[0];
seen = await steady();
const midi = JSON.stringify((await asAgent("/api/focus")).midi);
o = m2(["point", PROJ, `knob:${kickKnob?.id}`, "this one", "--as", "Claude"]);
check(o.status === 0 && (await until(async () => (await pointerNote())?.text.includes("this one"))), `a pointer at one of Kick's knobs: ${o.out}${o.err}`);
const noted = await (await page.$("[data-testid=pointer-note]")).boundingBox();
const kickHead = await (await page.$('[data-module-tab="kick"]')).boundingBox();
check(
  (await pointerNote()).text.includes(kickKnob?.label) && Math.abs(noted.y + noted.height / 2 - (kickHead.y + kickHead.height / 2)) < 24 && noted.x >= kickHead.x + kickHead.width - 4,
  `beside Kick's header, naming the knob: ${JSON.stringify(await pointerNote())} ${JSON.stringify(noted)} ${JSON.stringify(kickHead)}`,
);
await shot("pointer-knob", "[data-testid=pointer-note]");
if (await page.$("[data-testid=pointer-show]")) await page.click("[data-testid=pointer-show]");
await sleep(400);
check(JSON.parse(await viewNow()).tab === JSON.parse(seen).tab && JSON.stringify((await asAgent("/api/focus")).midi) === midi, `nothing is selected: ${await viewNow()}`);
// The note takes no press but its own buttons': one on its words reaches what is under it.
const reaches = await page.evaluate(() => {
  const words = document.querySelector("[data-testid=pointer-note] span")?.getBoundingClientRect();
  const at = words && document.elementFromPoint(words.left + words.width / 2, words.top + words.height / 2);
  return !!at && !at.closest("[data-testid=pointer-note]");
});
check(reaches, "a press on the note's words reaches what is under it");
await page.click("[data-testid=pointer-ok]");
check(await until(async () => !(await pointerNote())), "OK takes it away");

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
await sleep(500);
check(read("tracks/bass.ts") === changed && (await state()).undo_name?.step === step, "Compare writes nothing: the saved file and undo stay as they were");
let s = await status();
check(s.playing && s.loop?.on, "the loop plays on");
check((await asAgent("/api/focus")).comparing?.step === step, "an agent sees what is compared");
await shot("compare-before", "[data-testid=compare-pill]");
await key("KeyV");
check(await until(async () => /^After:/.test((await pill()) ?? "")), `V again: After: ${await pill()}`);
check(read("tracks/bass.ts") === changed, "still as saved");
// Comparing is listening: Space stops and plays again, and the pill stays.
await key("Space");
check(await until(async () => !(await status()).playing) && /^After:/.test((await pill()) ?? ""), `Space stops playback and the pill stays: ${await pill()}`);
await key("Space");
check(await until(async () => (await status()).playing) && /^After:/.test((await pill()) ?? ""), "Space plays again, still comparing");
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
// An agent's pointer: its OK leaves the pill open (the review's F10).
o = m2(["point", PROJ, "track:kick", "the kick", "--as", "Claude"]);
check(await until(async () => !!(await page.$("[data-testid=pointer-ok]"))), `a pointer shows while comparing: ${o.out}${o.err}`);
await page.click("[data-testid=pointer-ok]");
await sleep(400);
check(/^After:/.test((await pill()) ?? "") && (await asAgent("/api/focus")).comparing?.step === step, `a pointer's OK keeps the pill: ${await pill()}`);
// Esc on After closes it, keeping what plays.
await key("Escape");
check(await until(async () => (await pill()) === null), "Esc closes the pill");
check(await until(async () => (await asAgent("/api/focus")).comparing === null), "for the server too");
s = await status();
check(read("tracks/bass.ts") === changed && s.playing && (await state()).undo_name?.step === step, "what plays stays, and so does the song");
check((await sendTx()).status === 200, "agents write again");
const agentStep = (await state()).undo_name;
// Esc on Before keeps Before: the agent's change is undone then, one undo step, saying so, with Redo.
const withHit = read("data/bass/notes.json");
check(await until(async () => (await state()).undo_name?.source === "agent"), "the agent's step is the newest");
await page.mouse.move(1200, 880);
await key("KeyV");
check(await until(async () => /^Before: Claude: a new bass hit/.test((await pill()) ?? "")), `V: Before: ${await pill()}`);
check(read("data/bass/notes.json") === withHit, "the file still has the agent's hit");
await key("Escape");
check(await until(async () => (await pill()) === null), "Esc on Before closes the pill");
check(await until(() => read("data/bass/notes.json") === notes), "and keeps Before: the agent's hit is undone");
check(await until(async () => (await toasts()).some((t) => /Claude: a new bass hit is undone/.test(t))), `saying so: ${await toasts()}`);
check(await until(async () => !(await page.$("[data-testid=notice]"))), "and its notice offers no Undo any more");
check(await until(async () => (await state()).redo_name?.step === agentStep.step), "Redo has it");
await shot("compare-kept-before", "[data-testid=toast]");
await page.click("[data-testid=toast] button");
check(await until(() => read("data/bass/notes.json") === withHit), "the toast's Redo brings it back");
// Anything else on Before goes back to the song as it is.
check(await until(async () => (await state()).undo_name?.step === agentStep.step), "the agent's step is the newest again");
await page.mouse.move(1200, 880);
await key("KeyV");
check(await until(async () => /^Before:/.test((await pill()) ?? "")), `V: Before: ${await pill()}`);
const tl = await (await page.$("[data-testid=timeline]")).boundingBox();
await page.mouse.click(tl.x + tl.width - 60, tl.y + tl.height - 30);
check(await until(async () => (await pill()) === null && (await asAgent("/api/focus")).comparing === null), "a click elsewhere closes it");
await sleep(400);
check(read("data/bass/notes.json") === withHit && (await state()).undo_name?.step === agentStep.step, "at After: the song as it is, nothing undone");
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
check(read("data/bass/notes.json").includes('"agentcm2"'), "and the agent's change is still in the song");

// ---------------------------------------------------------------- V, quickly (the review's F1, F2)

const sha = (t) => `sha256:${createHash("sha256").update(t).digest("hex")}`;
let hits = 0;
/** An agent's step: a bass clip `id` at `start`, by Claude, saying `why`. */
const hit = async (id, start, why) => {
  const t = { id: `agent-hit-${++hits}`, base: { "data/bass/notes.json": sha(read("data/bass/notes.json")) }, ops: [{ ...tx.ops[0], put: [{ ...tx.ops[0].put[0], id, start }] }], expect: tx.expect };
  const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify({ tx: t, as: "Claude", why }) });
  return { status: res.status, body: await res.json() };
};
const has = (id) => read("data/bass/notes.json").includes(`"${id}"`);
const comparing = async () => (await asAgent("/api/focus")).comparing;
check((await hit("older1", 8, "older bass hit")).status === 200, "an older agent step");
const newer = await hit("newer1", 12, "newer bass hit");
check(newer.status === 200 && (await until(async () => (await state()).undo_name?.step === newer.body.step)), "and a newer one, which the page knows");
// Two quick Vs with the pill closed: one compares the newest change, the other changes nothing.
await page.mouse.move(1200, 880);
await page.keyboard.press("v");
await page.keyboard.press("v");
await sleep(1500);
let c = await comparing();
const shows = await pill();
check(c?.step === newer.body.step && shows !== null && shows.includes("newer bass hit") && shows.startsWith(c.after ? "After:" : "Before:"), `two quick Vs compare the newest change, and the pill says what the server plays: ${shows} / ${JSON.stringify(c)}`);
check(has("older1") && has("newer1"), "nothing was undone");
// V V on the open pill: the pill and the server agree.
await page.keyboard.press("v");
await page.keyboard.press("v");
await sleep(1500);
c = await comparing();
check(c !== null && (await pill())?.startsWith(c.after ? "After:" : "Before:"), `V V on the open pill: the pill says what the server plays: ${await pill()} / ${JSON.stringify(c)}`);
// V held: its repeats change nothing.
await page.mouse.click(tl.x + tl.width - 60, tl.y + tl.height - 30);
check(await until(async () => (await comparing()) === null && (await pill()) === null), "a click closes it");
check(has("older1") && has("newer1"), "at After");
const held = await hit("held1", 28, "held hit");
check(held.status === 200 && (await until(async () => (await state()).undo_name?.step === held.body.step)), "another agent step");
await page.mouse.move(1200, 880);
await page.keyboard.down("v");
for (let i = 0; i < 8; i++) {
  await page.keyboard.down("v", { autoRepeat: true });
  await sleep(35);
}
await page.keyboard.up("v");
await sleep(1500);
c = await comparing();
check(c?.step === held.body.step && c.after === false && /^Before: Claude: held hit/.test((await pill()) ?? ""), `V held is one V: Before: ${await pill()} / ${JSON.stringify(c)}`);
await page.mouse.click(tl.x + tl.width - 60, tl.y + tl.height - 30);
check(await until(async () => (await comparing()) === null && (await pill()) === null), "a click closes it, for both");
check(has("held1"), "at After");
// ---------------------------------------------------------------- a page that fell behind (F9)

// What the page's socket hears, held back as when it falls behind, then `lagged`: the page reads the
// pointer and the pill again, not only the song.
await page.evaluateOnNewDocument(() => {
  const Real = window.WebSocket;
  window.__sockets = [];
  window.WebSocket = class extends Real {
    constructor(...a) {
      super(...a);
      window.__sockets.push(this);
    }
  };
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=timeline] [data-module-tab]", { timeout: 10000 });
check(await until(async () => (await page.evaluate(() => window.__sockets?.at(-1)?.readyState)) === 1), "the page connects again");
await sleep(500);
await page.evaluate(() => {
  const ws = window.__sockets.at(-1);
  window.__heard = ws.onmessage;
  ws.onmessage = () => {};
});
o = m2(["point", PROJ, "track:kick", "you missed this", "--as", "Claude"]);
const missed = await hit("missed1", 20, "missed hit");
const session = (await state()).session;
const opened = await fetch(`${sess.url}/api/graph/compare`, {
  method: "POST",
  headers: { "x-m2-token": sess.page_token, "content-type": "application/json" },
  body: JSON.stringify({ session, step: missed.body.step, before: true }),
});
check(o.status === 0 && missed.status === 200 && opened.status === 200, `a pointer, a step and Compare the page didn't hear: ${opened.status}`);
await sleep(500);
check(!(await pointerNote()) && (await pill()) === null, "the page missed them");
await page.evaluate((session) => {
  const ws = window.__sockets.at(-1);
  ws.onmessage = window.__heard;
  ws.onmessage({ data: JSON.stringify({ t: "lagged", missed: 3, session }) });
}, session);
check(await until(async () => (await pointerNote())?.text.includes("you missed this")), `after lagged, it reads the pointer again: ${JSON.stringify(await pointerNote())}`);
check(await until(async () => /^Before: Claude: missed hit/.test((await pill()) ?? "")), `and the pill: ${await pill()}`);

done(errors);
