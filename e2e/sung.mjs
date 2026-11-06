// A voice's notes (VOX-40 to VOX-47, RENDER-30b, gap 51): a Vox track whose clip plays a sung
// file of two notes, A3 then C4, on first-song (which doesn't list @m2/vocals).
//   - the clip's menu has Find notes: one undo step ("Undo Find notes on Vox"), the clip's two
//     notes and its render written, the song given @m2/vocals; the Notes view shows them;
//   - a note dragged up two rows moves by 2 semitones; with ⌥ by cents; one undo step each;
//   - the second note dragged later writes the markers VOX-47 asks for: the first note plays where
//     it played, and the clip follows the tempo;
//   - Straighten (its right-click) writes 60%;
//   - while the song plays, a pitch drag is heard with no miss: the status never says the disk was
//     too slow;
//   - each state linted.
//
//   node e2e/sung.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { lint } from "./lintlib.mjs";
import { PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const RATE = 48000;
const SECONDS = 2.5;

/** A mono 16-bit WAV: a buzzy voice singing A3 from 0.2 to 1.0 s and C4 from 1.2 to 2.0 s. */
function wav(path) {
  const n = SECONDS * RATE;
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(RATE, 24);
  b.writeUInt32LE(RATE * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) {
    const t = i / RATE;
    const hz = t < 1.1 ? 220 : 261.63;
    const on = (t > 0.2 && t < 1.0) || (t > 1.2 && t < 2.0) ? 1 : 0;
    let v = 0;
    for (let h = 1; h < 8; h++) v += Math.sin(2 * Math.PI * hz * h * t) / h;
    b.writeInt16LE(Math.round(0.15 * on * v * 32767), 44 + i * 2);
  }
  fs.writeFileSync(path, b);
}

const { page, errors, api, key, toasts } = await open(1440, 900);
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const clip = () => {
  try {
    return JSON.parse(read("data/vox/clips.json")).clips[0];
  } catch {
    return null;
  }
};

// ---------------------------------------------------------------- the song: Vox, a sung clip

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox.wav`);
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write("data/vox/clips.json", JSON.stringify({ clips: [{ id: "voxclip1", start: 0, length: 5, asset: "vox.wav" }] }, null, 2));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";').replace("      kick: kick.out,", "      kick: kick.out,\n      vox: vox.out,"));
const onLane = '[data-lane="vox.clips"] [data-clip="voxclip1"]';
check(!!(await until(() => page.$(onLane), 20000)), "the Vox track's clip is on the timeline");
check(!read("music2.toml").includes("@m2/vocals"), "the song doesn't list @m2/vocals yet");

// ---------------------------------------------------------------- Find notes (VOX-43)

const c0 = await (await page.$(onLane)).boundingBox();
await page.mouse.click(c0.x + c0.width / 2, c0.y + c0.height / 2, { button: "right" });
const row = '[data-testid=clip-menu] [data-menu-item="find-notes"]';
check(!!(await until(() => page.$(row), 3000)), "the clip's menu has Find notes");
await page.click(row);
check(await until(() => (clip()?.sung ?? []).length === 2, 20000), `Find notes writes its two notes: ${JSON.stringify(clip()?.sung)}`);
let c = clip();
check(c.render === "@m2/vocals:voice", `and its render: ${c.render}`);
check(Math.abs(c.sung[0].pitch - 57) < 0.1 && Math.abs(c.sung[1].pitch - 60) < 0.1, `at A3 and C4: ${c.sung.map((n) => n.pitch)}`);
check(Math.abs(c.sung[0].at - 0.2) < 0.03 && Math.abs(c.sung[1].at - 1.2) < 0.03, `where they are sung: ${c.sung.map((n) => n.at)}`);
check(read("music2.toml").includes('"@m2/vocals"') && read("music2.lock").includes("@m2/vocals"), "the song lists and locks @m2/vocals now");
check(await until(async () => (await undoTitle()).startsWith("Undo Find notes on Vox"), 3000), `one undo step: ${await undoTitle()}`);
const notes = "[data-testid=sung-view] [data-testid=sung-note]";
check(await until(async () => (await page.$$(notes)).length === 2, 5000), "the Notes view shows the clip's two notes");
await linted("[data-testid=sung-view]", "the voice's notes");

// ---------------------------------------------------------------- a pitch drag (VOX-46)

const note = async (i) => (await page.$$(notes))[i].boundingBox();
const dragBy = async (i, dx, dy, alt = false) => {
  const b = await note(i);
  const [x, y] = [b.x + b.width / 2, b.y + b.height / 2];
  if (alt) await page.keyboard.down("Alt");
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (const t of [0.25, 0.5, 0.75, 1]) await page.mouse.move(x + t * dx, y + t * dy);
  await page.mouse.up();
  if (alt) await page.keyboard.up("Alt");
};
await dragBy(0, 0, -28);
check(await until(() => clip()?.sung?.[0]?.move === 2, 5000), `dragged up two rows: moved by 2 semitones (${clip()?.sung?.[0]?.move})`);
check(await until(async () => (await undoTitle()).startsWith("Undo Moving a note of Vox"), 3000), `one undo step: ${await undoTitle()}`);
await dragBy(0, 0, 5, true);
check(await until(() => { const m = clip()?.sung?.[0]?.move; return m !== undefined && m !== 2 && !Number.isInteger(m) && m > 1 && m < 2; }, 5000), `with ⌥, by cents: ${clip()?.sung?.[0]?.move}`);
await key("Meta+z");
check(await until(() => clip()?.sung?.[0]?.move === 2, 3000), "⌘Z takes the cents back");

// ---------------------------------------------------------------- a timing drag (VOX-47)

/** The clip's beat where its markers `w` play file second `s`: straight between two. */
const beatAt = (w, s) => {
  let i = w.findIndex((m) => m[0] > s);
  i = Math.min(Math.max(i < 0 ? w.length - 1 : i, 1), w.length - 1);
  return w[i - 1][1] + ((s - w[i - 1][0]) * (w[i][1] - w[i - 1][1])) / (w[i][0] - w[i - 1][0]);
};
await dragBy(1, 30, 0);
check(await until(() => (clip()?.warp ?? []).length >= 4, 5000), `the second note dragged later writes markers: ${JSON.stringify(clip()?.warp)}`);
c = clip();
const w = c.warp;
const [n0, n1] = c.sung;
const at = (s) => w.find((m) => Math.abs(m[0] - s) < 1e-6);
check(!!at(n0.at + n0.length) && Math.abs(at(n0.at + n0.length)[1] - (n0.at + n0.length) * (124 / 60)) < 1e-3, `a marker keeps the end of the first note where it plays: ${JSON.stringify(w)}`);
check(!!at(n1.at) && at(n1.at)[1] > n1.at * (124 / 60) + 0.01, `the second note's start is later: ${JSON.stringify(at(n1.at))}`);
check(w[0][0] === 0 && w[0][1] === 0 && Math.abs(w[w.length - 1][1] - 5) < 1e-6, `the clip's start and end are kept, so it follows the tempo: ${JSON.stringify(w)}`);
check(Math.abs(beatAt(w, n0.at) - n0.at * (124 / 60)) < 1e-6, `the first note plays where it played: beat ${beatAt(w, n0.at)}`);
check(await until(async () => (await undoTitle()).startsWith("Undo Retiming a note of Vox"), 3000), `one undo step: ${await undoTitle()}`);
await key("Meta+z");
check(await until(() => (clip()?.warp ?? []).length === 0, 3000), "⌘Z: no markers again");

// ---------------------------------------------------------------- Straighten

const b1 = await note(1);
await page.mouse.click(b1.x + b1.width / 2, b1.y + b1.height / 2, { button: "right" });
check(!!(await until(() => page.$("[data-testid=straighten]"), 3000)), "a note's right-click has Straighten");
await page.$eval("[data-testid=straighten]", (el) => {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  set.call(el, "60");
  el.dispatchEvent(new Event("input", { bubbles: true }));
});
const s = await (await page.$("[data-testid=straighten]")).boundingBox();
await page.mouse.click(s.x + s.width * 0.6, s.y + s.height / 2);
check(await until(() => Math.abs((clip()?.sung?.[1]?.straighten ?? 0) - 0.6) < 0.02, 5000), `straightened 60%: ${clip()?.sung?.[1]?.straighten}`);
await key("Escape");

// ---------------------------------------------------------------- an edit while the song plays (RENDER-30b)

await api("/loop", { method: "POST", body: JSON.stringify({ start: 0, end: 5, on: true }) });
await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
check(await until(async () => (await api("/status")).body?.playing, 3000), "the song plays, round the clip");
await sleep(400);
await dragBy(1, 0, 14);
check(await until(() => clip()?.sung?.[1]?.move === -1, 5000), `while it plays, a note moved down: ${clip()?.sung?.[1]?.move}`);
let said = null;
for (let k = 0; k < 15; k++) {
  const st = (await api("/status")).body;
  said = said ?? st?.disk ?? null;
  await sleep(100);
}
check(said === null, `the edit plays with no miss: the status never says the disk was too slow (${said})`);
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });
await linted("[data-testid=sung-view]", "the edited notes");
await sleep(100);
check(!(await toasts()).some((m) => /couldn't|error|can't/i.test(m)), `no toast says something went wrong: ${JSON.stringify(await toasts())}`);

done(errors);
