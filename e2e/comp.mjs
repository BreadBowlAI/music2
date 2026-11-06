// Comping (VOX-1 to VOX-7, gap #25): a Vox track whose clip plays the third of three takes over a
// 4-beat loop (one file, its note listing where each take starts), as a loop recording leaves it.
//   - Takes ▸ on the track's header opens one row per take under its lane, Take 3's lit: no edit,
//     no undo step;
//   - a swipe along Take 2 over beats 1 to 2 plays it there: three clips of the one file, each join
//     10 ms of overlap with 10 ms fades, one undo step ("Undo Comp on Vox"); Take 2's row lit there;
//   - a click on a faint part of Take 1 plays it over the piece there;
//   - a swipe along Take 3 over the whole loop makes it one clip again, as it was (VOX-5); ⌘Z takes
//     that back, the lane file byte for byte;
//   - Takes again closes the rows; each state linted.
//
//   node e2e/comp.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { lint } from "./lintlib.mjs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const RATE = 48000;
// At 124 BPM a beat is 0.484 s; each take is the loop's 4 beats, to the sample.
const BEAT = 60 / 124;
const N = Math.round(4 * BEAT * RATE);
const L = N / RATE;
const TAKES = [0, L, 2 * L];

/** A mono 16-bit WAV of the three takes: each a 220 Hz sine, louder take by take. */
function wav(path) {
  const n = 3 * N;
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
    const take = Math.floor(i / N);
    b.writeInt16LE(Math.round(0.2 * (take + 1) * Math.sin((2 * Math.PI * 220 * i) / RATE) * 32767), 44 + i * 2);
  }
  fs.writeFileSync(path, b);
}

const { page, errors, key, state, toasts } = await open(1440, 900);
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const clips = () => {
  try {
    return JSON.parse(read("data/vox/clips.json")).clips;
  } catch {
    return [];
  }
};
const near = (a, b, eps = 1e-6) => Math.abs(a - b) < eps;

// ---------------------------------------------------------------- the song: Vox, a loop's take

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox take.wav`);
write("assets/vox take.wav.json", JSON.stringify({ takes: TAKES }) + "\n");
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
const first = { id: "voxclip1", start: 0, length: 4, asset: "vox take.wav", offset: TAKES[2] };
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write("data/vox/clips.json", JSON.stringify({ clips: [first] }, null, 2));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";').replace("      kick: kick.out,", "      kick: kick.out,\n      vox: vox.out,"));

const button = '[data-testid=takes][data-module="vox"]';
const rows = '[data-testid=take-row][data-lane="vox.clips"]';
const row = (k) => `${rows}[data-take="${k}"]`;
check(!!(await until(() => page.$('[data-lane="vox.clips"] [data-clip="voxclip1"]'), 20000)), "the Vox track's clip is on the timeline");
check(!!(await until(() => page.$(button), 5000)), "its header has Takes: its clip's file lists three takes");
check((await page.$$(rows)).length === 0, "no rows until it is clicked");

// ---------------------------------------------------------------- Takes opens the rows (VOX-7)

const rev0 = (await state()).view.revision;
const undo0 = await undoTitle();
await page.click(button);
check(await until(async () => (await page.$$(rows)).length === 3, 3000), `one row per take: ${(await page.$$(rows)).length}`);
check((await page.$eval(button, (e) => e.getAttribute("aria-expanded"))) === "true", "Takes says it is open");
const names = await page.$$eval(rows, (els) => els.map((e) => e.querySelector(".sticky")?.textContent));
check(names.join("|") === "Take 1|Take 2|Take 3", `named Take 1 to 3: ${names.join("|")}`);
const lit = (k) => page.$$eval(`${row(k)} [data-lit]`, (els) => els.map((e) => e.dataset.lit.split("-").map(Number)));
check((await lit(3)).length === 1 && (await lit(1)).length === 0 && (await lit(2)).length === 0, "Take 3, which plays, is lit; the others aren't");
check((await state()).view.revision === rev0 && (await undoTitle()) === undo0, "no edit, no undo step");
await linted('[data-track="vox"]', "the Vox track with its takes open");
await page.screenshot({ path: `${ROOT}/comp-open.png`, clip: { x: 0, y: 80, width: 900, height: 420 } });

/** The page's x of beat `b` in take `k`'s row. */
const xOf = async (k, b) => {
  const box = await (await page.$(`${row(k)} [data-comp]`)).boundingBox();
  const ppb = (box.width + 2) / 4;
  return box.x - 1 + b * ppb;
};
const yOf = async (k) => {
  const box = await (await page.$(row(k))).boundingBox();
  return box.y + box.height / 2;
};
const swipe = async (k, a, b) => {
  const y = await yOf(k);
  await page.mouse.move(await xOf(k, a), y);
  await page.mouse.down();
  for (const t of [0.25, 0.5, 0.75, 1]) await page.mouse.move((await xOf(k, a)) + t * ((await xOf(k, b)) - (await xOf(k, a))), y);
  await page.mouse.up();
};

// ---------------------------------------------------------------- a swipe along Take 2 (VOX-1, VOX-4)

// A join's half: 5 ms, in beats.
const H = (0.005 * 124) / 60;
await swipe(2, 1.02, 1.98);
check(await until(() => clips().length === 3, 5000), `a swipe over beats 1 to 2: three clips (${JSON.stringify(clips())})`);
let c = clips();
check(c[0].id === "voxclip1" && near(c[0].length, 1 + H) && near(c[0].offset, TAKES[2]) && near(c[0].fade_out, 0.01), `the first plays Take 3 to just past beat 1, its id kept: ${JSON.stringify(c[0])}`);
check(near(c[1].start, 1 - H) && near(c[1].length, 1 + 2 * H) && near(c[1].offset, TAKES[1] + BEAT - 0.005) && near(c[1].fade_in, 0.01) && near(c[1].fade_out, 0.01), `the middle plays Take 2 there, 10 ms fades: ${JSON.stringify(c[1])}`);
check(near(c[2].start, 2 - H) && near(c[2].start + c[2].length, 4) && near(c[2].offset, TAKES[2] + 2 * BEAT - 0.005), `the last plays Take 3 on: ${JSON.stringify(c[2])}`);
check(c.every((x) => x.asset === "vox take.wav"), "all of the one file");
check(await until(async () => (await undoTitle()).startsWith("Undo Comp on Vox"), 3000), `one undo step: ${await undoTitle()}`);
check(await until(async () => JSON.stringify((await lit(2)).map((p) => p.map((x) => Math.round(x * 100) / 100))) === "[[1,2]]", 3000), `Take 2 is lit over beats 1 to 2: ${JSON.stringify(await lit(2))}`);
check((await lit(3)).length === 2, "Take 3 either side");
await linted('[data-track="vox"]', "the comp");
await page.screenshot({ path: `${ROOT}/comp-swiped.png`, clip: { x: 0, y: 80, width: 900, height: 420 } });

// ---------------------------------------------------------------- a click on a faint part of Take 1

await page.mouse.click(await xOf(1, 3), await yOf(1));
check(
  await until(() => near(clips()[2]?.offset ?? -1, TAKES[0] + 2 * BEAT - 0.005), 5000),
  `Take 1 plays over the piece clicked, beats 2 to 4: ${JSON.stringify(clips()[2])}`,
);
check(clips().length === 3 && near(clips()[1].offset, TAKES[1] + BEAT - 0.005), "the rest as it was");

// ---------------------------------------------------------------- Take 3 over all of it: one clip again (VOX-5)

const comped = read("data/vox/clips.json");
await swipe(3, 0.05, 3.95);
check(await until(() => clips().length === 1, 5000), `one clip again: ${JSON.stringify(clips())}`);
c = clips()[0];
check(c && c.id === "voxclip1" && c.start === 0 && near(c.length, 4) && near(c.offset, TAKES[2]) && !c.fade_in && !c.fade_out, `as it was: ${JSON.stringify(c)}`);
await key("Meta+z");
check(await until(() => read("data/vox/clips.json") === comped, 3000), "⌘Z: the comp again, byte for byte");

// ---------------------------------------------------------------- Takes closes the rows

await page.click(button);
check(await until(async () => (await page.$$(rows)).length === 0, 2000), "Takes again closes the rows");
check((await page.$$('[data-lane="vox.clips"] [data-clip]')).length === 3, "the lane shows the comp as clips");
await sleep(100);
check(!(await toasts()).some((m) => /couldn't|error|can't/i.test(m)), `no toast says something went wrong: ${JSON.stringify(await toasts())}`);

done(errors);
