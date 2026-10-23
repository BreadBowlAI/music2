// Track 1 of the agents design (ag-steps): a change made outside the app is one undo step, shown
// as it lands (AGENT-2, AGENT-5). On the run's server with first-song open:
//   - a save of tracks/bass.ts from outside: one notice, "Changed outside the app: the code in
//     tracks/bass.ts", with Undo; it takes no focus;
//   - Bass's header and its changed knob glow, until a click; Undo's tooltip names the outside
//     change;
//   - the notice's Undo puts the text back while it is the newest step;
//   - once a newer change came (a Tx sent from outside the page), the notice says so and offers no
//     Undo; ⌘Z takes back the newer change, then the outside one;
//   - ⌘Z while Undo still names an older step takes back nothing it didn't name: a Tx another
//     program sent is said to be the newest, and stays;
//   - a change undo can't take back any more (its file changed since, with an audio file that went)
//     is said, with Skip it, and the notice drops its Undo; Skip it lets Undo reach the step before;
//   - a lane changed outside: its clips glow, until a key press;
//   - a WAV brought in with Import Audio makes one step, named, and no notice;
//   - lint, no console errors.
//
//   node e2e/ag-notice.mjs <root> <home> <project>     (the `fix` kind)
//   AG_SHOT=<folder> also saves a picture of the notice and the glow there.
import { createHash } from "node:crypto";
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, posts, api, key, show, toasts } = await open(1440, 900);
/** The server's state, read from here, not by the page. */
const server = async () => (await fetch(`${sess.url}/api/graph`, { headers: { "x-m2-token": sess.page_token } })).json();
/**
 * A Tx from outside the page on Bass's notes: its answer. With the page's key, the musician's (as
 * another window of theirs would send it); with the tools key (`sess.token`), an agent's, named by
 * `as` and `why` (AGENT-10).
 */
const agentTx = async (id, { key = sess.page_token, as, why } = {}) => {
  const now = read("data/bass/notes.json");
  const tx = {
    id,
    base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(now).digest("hex")}` },
    ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id, start: 0, length: 4, notes: [{ pitch: "E1", start: 0, length: 0.25 }] }], delete: [] }],
    expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
  };
  const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify({ tx, as, why }) });
  return { status: res.status, body: await res.json() };
};
const notices = () => page.$$eval("[data-testid=notice]", (els) => els.map((e) => e.textContent ?? ""));
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const glowing = () => page.$$eval("[data-glow]", (els) => els.map((e) => e.dataset.moduleTab ?? e.dataset.knobId ?? e.dataset.clip ?? e.tagName));
const focused = () => page.evaluate(() => {
  const e = document.activeElement;
  return e ? `${e.tagName}${e.dataset?.moduleTab ? `[${e.dataset.moduleTab}]` : ""}${e.closest("[data-testid=notice]") ? " in the notice" : ""}` : "none";
});

// ---------------------------------------------------------------- a save from another editor

await show("bass");
await until(async () => !!(await page.$('[data-knob-id^="tracks/bass.ts#"][data-knob-id$="period"]')));
const bass = read("tracks/bass.ts");
const changed = bass.replace("period: 0.5", "period: 0.25");
check(changed !== bass, "tracks/bass.ts has an lfo period of 0.5");
const before = await focused();
write("tracks/bass.ts", changed);
check(await until(async () => (await notices()).length === 1), `one notice: ${JSON.stringify(await notices())}`);
const said = (await notices())[0] ?? "";
check(said.startsWith("Changed outside the app: the code in tracks/bass.ts") && said.endsWith("Undo"), `it says what changed, with Undo: ${said}`);
check((await focused()) === before && !(await focused()).includes("notice"), `it takes no focus: ${before} → ${await focused()}`);
const lit = await glowing();
check(lit.includes("bass"), `Bass's header glows: ${lit}`);
check(lit.some((k) => /^tracks\/bass\.ts#.*period$/.test(k)), `and its changed knob: ${lit}`);
check(!lit.includes("kick") && !lit.includes("hats"), `nothing else does: ${lit}`);
check(await until(async () => (await undoTitle()) === "Undo outside change: the code in tracks/bass.ts (⌘Z)"), `Undo names it: ${await undoTitle()}`);
check((await page.evaluate(lint, "[data-testid=notice]")).length === 0, `the notice is in the design system: ${(await page.evaluate(lint, "[data-testid=notice]")).slice(0, 3)}`);
if (process.env.AG_SHOT) {
  fs.mkdirSync(process.env.AG_SHOT, { recursive: true });
  await page.screenshot({ path: `${process.env.AG_SHOT}/notice-and-glow.png` });
  // Close up, three times bigger: the notice, Bass's header and its knob.
  const near = async (sel, name) => {
    const b = await (await page.$(sel))?.boundingBox();
    if (b) await page.screenshot({ path: `${process.env.AG_SHOT}/${name}.png`, clip: { x: b.x - 24, y: b.y - 24, width: b.width + 48, height: b.height + 48, scale: 3 } });
  };
  await near("[data-testid=notice]", "notice");
  await near('[data-module-tab="bass"]', "header");
  await near('[data-knob-id$="period"]', "knob");
}
// Hovered, it stays; a click in the song ends the glow, not the notice.
await page.hover("[data-testid=notice]");
const ruler = await (await page.$("[data-testid=ruler]"))?.boundingBox();
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
await page.mouse.down();
await page.mouse.up();
check(await until(async () => (await glowing()).length === 0), `a click ends the glow: ${await glowing()}`);
check((await notices()).length === 1, "the notice stays for its time");
await page.hover("[data-testid=notice]");
const outside = (await server()).undo_name?.step;
await page.click("[data-testid=notice] button");
check(await until(() => read("tracks/bass.ts") === bass), "its Undo puts the text back");
const sent = JSON.parse(posts.filter((x) => x.url === "/api/graph/undo").at(-1)?.body ?? "{}");
check(typeof outside === "number" && sent.step === outside, `it sent its step's id: ${JSON.stringify(sent)}, the step ${outside}`);
check(await until(async () => (await notices()).length === 0), "and the notice goes");

// ---------------------------------------------------------------- a newer change since

write("tracks/bass.ts", changed);
check(await until(async () => (await notices()).length === 1), "another save from outside: a notice again");
await page.hover("[data-testid=notice]");
const notes = read("data/bass/notes.json");
const res = await agentTx("agent1");
check(res.status === 200 && typeof res.body.step === "number", `a Tx on Bass's notes lands after it, and answers with its step: ${res.status} ${res.body.step}`);
check(
  await until(async () => /newer change/.test((await notices())[0] ?? "") && !(await page.$("[data-testid=notice] button"))),
  `the notice says a newer change came, and offers no Undo: ${JSON.stringify(await notices())}`,
);
check(read("tracks/bass.ts") === changed, "nothing was undone");
if (process.env.AG_SHOT) {
  const b = await (await page.$("[data-testid=notice]"))?.boundingBox();
  if (b) await page.screenshot({ path: `${process.env.AG_SHOT}/notice-newer.png`, clip: { x: b.x - 24, y: b.y - 24, width: b.width + 48, height: b.height + 48, scale: 3 } });
}
check(await until(async () => (await undoTitle()) === "Undo Bass notes (⌘Z)"), `Undo names the newer change: ${await undoTitle()}`);
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
await key("Meta+KeyZ");
check(await until(() => read("data/bass/notes.json") === notes), "⌘Z takes the newer change back");
check(await until(async () => (await undoTitle()) === "Undo outside change: the code in tracks/bass.ts (⌘Z)"), `then Undo names the outside change: ${await undoTitle()}`);
await key("Meta+KeyZ");
check(await until(() => read("tracks/bass.ts") === bass), "and ⌘Z takes that back too");

// ---------------------------------------------------------------- ⌘Z takes back only what it names

write("tracks/bass.ts", changed);
check(await until(async () => (await undoTitle()) === "Undo outside change: the code in tracks/bass.ts (⌘Z)"), "another outside save is Undo's");
// The window's state is held back, so its Undo still names the outside save after a Tx lands.
let holding = true;
const held = [];
const hold = (r) => (holding && r.method() === "GET" && new URL(r.url()).pathname === "/api/graph" ? held.push(r) : r.continue());
await page.setRequestInterception(true);
page.on("request", hold);
check((await agentTx("agent2")).status === 200, "a Tx another program sent lands");
check(await until(async () => (await server()).undo_name?.name === "Bass notes"), "it is the newest step");
const withTx = read("data/bass/notes.json");
check((await undoTitle()) === "Undo outside change: the code in tracks/bass.ts (⌘Z)", `the window still names the outside save: ${await undoTitle()}`);
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
await key("Meta+KeyZ");
check(
  await until(async () => (await toasts()).some((t) => t.includes("Nothing was undone: it isn't the newest change now. The newest is Bass notes."))),
  `⌘Z takes back nothing it didn't name, and says which change is newest: ${JSON.stringify(await toasts())}`,
);
await sleep(300);
check(read("data/bass/notes.json") === withTx && read("tracks/bass.ts") === changed, "nothing was undone");
holding = false;
for (const r of held.splice(0)) r.continue();
page.off("request", hold);
await page.setRequestInterception(false);
check(await until(async () => (await undoTitle()) === "Undo Bass notes (⌘Z)"), `once the window reads the state again, Undo names the Tx: ${await undoTitle()}`);
await key("Meta+KeyZ");
check(await until(() => read("data/bass/notes.json") === notes), "⌘Z takes it back");

// ---------------------------------------------------------------- a change undo can't take back any more

// An audio file no lane names, then one batch: it goes (undo can't put it back) and tracks/bass.ts
// changes again, so the outside save's undo would write over that.
fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/spare.wav`, 0.1);
await sleep(600);
const again = changed.replace("period: 0.25", "period: 0.125");
fs.rmSync(`${PROJ}/assets/spare.wav`);
write("tracks/bass.ts", again);
check(
  await until(async () => (await toasts()).some((t) => t.includes("changed outside the app. That can't be undone here."))),
  `that batch says it can't be undone: ${JSON.stringify(await toasts())}`,
);
check(await until(async () => (await server()).undo_stuck === true), "the outside save is stuck on top");
check(!(await page.$('button[aria-label="Undo"][disabled]')), "Undo still answers");
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
await key("Meta+KeyZ");
const stuckSaid = "tracks/bass.ts changed since, so Undo can't take back that change.";
check(await until(async () => (await toasts()).some((t) => t.startsWith(stuckSaid) && t.endsWith("Skip it"))), `⌘Z says why, with Skip it: ${JSON.stringify(await toasts())}`);
check(read("tracks/bass.ts") === again, "and writes nothing");
if (process.env.AG_SHOT) {
  await page.screenshot({ path: `${process.env.AG_SHOT}/skip-it.png` });
  const b = await (await page.$$("[data-testid=toast]")).at(-1)?.boundingBox();
  if (b) await page.screenshot({ path: `${process.env.AG_SHOT}/skip-it-near.png`, clip: { x: b.x - 24, y: b.y - 24, width: b.width + 48, height: b.height + 48, scale: 3 } });
}
const skip = await page.$$eval("[data-testid=toast] button", (bs) => bs.findIndex((b) => b.textContent === "Skip it"));
await (await page.$$("[data-testid=toast] button"))[skip]?.click();
check(
  await until(async () => (await toasts()).some((t) => t.startsWith("Skipped it."))) && read("tracks/bass.ts") === again,
  `Skip it writes nothing, and says what Undo takes back now: ${JSON.stringify(await toasts())}`,
);
check(await until(async () => (await server()).undo_stuck === false), "nothing is stuck now");
// Away from where the notice shows, so it isn't held.
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
write("tracks/bass.ts", bass);
check(await until(async () => (await undoTitle()) === "Undo outside change: the code in tracks/bass.ts (⌘Z)"), "back as it was, from outside");
check(await until(async () => (await notices()).length === 0, 10000), "its notice goes in its time");

// ---------------------------------------------------------------- an import: one step, no notice

/** A mono 16-bit WAV of `seconds` at 48 kHz. */
function wav(path, seconds) {
  const n = Math.round(48000 * seconds);
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(48000, 24);
  b.writeUInt32LE(96000, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(8000 * Math.sin((2 * Math.PI * 220 * i) / 48000)), 44 + i * 2);
  fs.writeFileSync(path, b);
}
fs.mkdirSync(`${ROOT}/files`, { recursive: true });
wav(`${ROOT}/files/tone.wav`, 0.5);
const stepBefore = (await api("")).body.undo_name?.step ?? 0;
const [chooser] = await Promise.all([page.waitForFileChooser({ timeout: 5000 }).catch(() => null), key("Meta+Shift+KeyI")]);
check(!!chooser, "Import Audio opens a chooser");
await chooser?.accept([`${ROOT}/files/tone.wav`]);
check(await until(async () => (await undoTitle()) === "Undo importing tone.wav (⌘Z)"), `one step, named: ${await undoTitle()}`);
await sleep(1500);
check((await notices()).length === 0, `the page's own import shows no notice: ${JSON.stringify(await notices())}`);
const after = (await api("")).body.undo_name;
check(after?.source === "page" && after?.step > stepBefore, `its step is the page's: ${JSON.stringify(after)}`);

// ---------------------------------------------------------------- a lane changed outside

// With Kick not selected: its header and its clips are outlined, until a key press.
const hits = read("data/kick/hits.json");
write("data/kick/hits.json", hits.replace('"start": 0', '"start": 0.5'));
check(await until(async () => (await glowing()).includes("my53vvkc")), `Kick's clip glows: ${await glowing()}`);
check((await glowing()).includes("kick"), `and its header: ${await glowing()}`);
if (process.env.AG_SHOT) {
  await sleep(300);
  await page.screenshot({ path: `${process.env.AG_SHOT}/lane-glow.png` });
  const kick = await (await page.$('[data-module-tab="kick"]'))?.boundingBox();
  if (kick) await page.screenshot({ path: `${process.env.AG_SHOT}/lane-glow-near.png`, clip: { x: 0, y: kick.y - 12, width: 520, height: kick.height + 24, scale: 3 } });
}
await page.keyboard.press("Shift");
check(await until(async () => (await glowing()).length === 0), `a key press ends the glow: ${await glowing()}`);

// ---------------------------------------------------------------- an agent's change (track 2)

// With the tools key, a Tx is an agent's (AGENT-10): its notice and Undo say who, by its words.
await page.mouse.move(ruler.x + ruler.width - 40, ruler.y + ruler.height + 200);
const beforeAgent = read("data/bass/notes.json");
const byAgent = await agentTx("agent3", { key: sess.token, as: "Claude", why: "brighter hats" });
check(byAgent.status === 200, `an agent's Tx lands: ${byAgent.status} ${JSON.stringify(byAgent.body).slice(0, 200)}`);
check(await until(async () => ((await notices())[0] ?? "").startsWith("Claude: brighter hats")), `its notice says who and why: ${JSON.stringify(await notices())}`);
check(!!(await page.$("[data-testid=notice] button")), "with Undo");
check(await until(async () => (await undoTitle()) === "Undo Claude: brighter hats (⌘Z)"), `Undo names it: ${await undoTitle()}`);
check(await until(async () => (await glowing()).includes("bass")), `Bass glows: ${await glowing()}`);
if (process.env.AG_SHOT) {
  await sleep(300);
  await page.screenshot({ path: `${process.env.AG_SHOT}/agent-notice.png` });
  const b = await (await page.$("[data-testid=notice]"))?.boundingBox();
  if (b) await page.screenshot({ path: `${process.env.AG_SHOT}/agent-notice-near.png`, clip: { x: b.x - 24, y: b.y - 24, width: b.width + 48, height: b.height + 48, scale: 3 } });
  const u = await (await page.$('button[aria-label="Undo"]'))?.boundingBox();
  if (u) await page.screenshot({ path: `${process.env.AG_SHOT}/agent-undo-near.png`, clip: { x: u.x - 60, y: u.y - 8, width: u.width + 160, height: u.height + 16, scale: 3 } });
}
await page.click("[data-testid=notice] button");
check(await until(() => read("data/bass/notes.json") === beforeAgent), "the notice's Undo takes the agent's change back");
done(errors);
