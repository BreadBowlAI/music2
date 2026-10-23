// Track 1 of the agents design (ag-steps): a change made outside the app is one undo step, shown
// as it lands (AGENT-2, AGENT-5). On the run's server with first-song open:
//   - a save of tracks/bass.ts from outside: one notice, "Changed outside the app: the code in
//     tracks/bass.ts", with Undo; it takes no focus;
//   - Bass's header and its changed knob glow, until a click; Undo's tooltip names the outside
//     change;
//   - the notice's Undo puts the text back while it is the newest step;
//   - once a newer change came (a Tx sent from outside the page), the notice says so and offers no
//     Undo; ⌘Z takes back the newer change, then the outside one;
//   - a WAV brought in with Import Audio makes one step, named, and no notice;
//   - lint, no console errors.
//
//   node e2e/ag-notice.mjs <root> <home> <project>     (the `fix` kind)
//   AG_SHOT=<folder> also saves a picture of the notice and the glow there.
import { createHash } from "node:crypto";
import fs from "node:fs";
import { ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, key, show } = await open(1440, 900);
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
await page.click("[data-testid=notice] button");
check(await until(() => read("tracks/bass.ts") === bass), "its Undo puts the text back");
check(await until(async () => (await notices()).length === 0), "and the notice goes");

// ---------------------------------------------------------------- a newer change since

write("tracks/bass.ts", changed);
check(await until(async () => (await notices()).length === 1), "another save from outside: a notice again");
await page.hover("[data-testid=notice]");
const notes = read("data/bass/notes.json");
const tx = {
  id: "tx-ag-notice",
  base: { "data/bass/notes.json": `sha256:${createHash("sha256").update(notes).digest("hex")}` },
  ops: [{ op: "clips", lane: ["bass", "notes"], put: [{ id: "agent1", start: 0, length: 4, notes: [{ pitch: "E1", start: 0, length: 0.25 }] }], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["bass", "notes"]], modules: [] } },
};
const res = await fetch(`${sess.url}/api/tx/commit`, { method: "POST", headers: { "x-m2-token": sess.token, "content-type": "application/json" }, body: JSON.stringify({ tx }) });
check(res.status === 200, `a Tx on Bass's notes lands after it: ${res.status}`);
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
// The picture of a lane changed from outside, with Kick not selected: its clips outlined.
if (process.env.AG_SHOT) {
  write("data/kick/hits.json", read("data/kick/hits.json").replace('"start": 0', '"start": 0.5'));
  await until(async () => (await notices()).length === 1);
  await sleep(300);
  await page.screenshot({ path: `${process.env.AG_SHOT}/lane-glow.png` });
  const kick = await (await page.$('[data-module-tab="kick"]'))?.boundingBox();
  if (kick) await page.screenshot({ path: `${process.env.AG_SHOT}/lane-glow-near.png`, clip: { x: 0, y: kick.y - 12, width: 520, height: kick.height + 24, scale: 3 } });
}
done(errors);
