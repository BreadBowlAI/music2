// Tune, from @m2/vocals (VOX-20, VOX-26): on a vocal track (an audio lane playing a sung clip), the +
// on its lane's card lists Tune under Voice, and picking it writes `tune(lane.audio("clips"), …)` in
// the track's own code, its import, the pack's [dependencies] line and its lock in one edit. Its card
// says "Every note allowed" with Set the key while the song has no key (its tooltip: "No key set:
// every note is allowed."); Set the key opens the key field, and once a key is picked the card says
// it ("Key: A major"). Glide and Amount are knobs: dragging each writes
// its new value into the call, one undo step each. The Robot preset writes Glide 0, one undo step.
// Each state linted.
//
//   node e2e/tune.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const RATE = 48000;

/** A mono 16-bit WAV of 2 s: a buzzy voice (a few harmonics of 216 Hz, a little flat of A). */
function wav(path) {
  const n = 2 * RATE;
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
    let v = 0;
    for (let h = 1; h <= 6; h++) v += Math.sin((2 * Math.PI * 216 * h * i) / RATE) / h;
    b.writeInt16LE(Math.round(0.2 * v * 32767), 44 + i * 2);
  }
  fs.writeFileSync(path, b);
}

const { page, errors, show, view, key, box } = await open(1440, 900);
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);
/** `sel` scrolled into the middle of the window, and where it is then. */
const seen = async (sel) => {
  await page.$eval(sel, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
  await sleep(150);
  return box(sel);
};
/** Drag the knob `sel` up by `px`. */
const turn = async (sel, px) => {
  const g = await seen(`${sel} svg`);
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
  await page.mouse.down();
  for (let i = 1; i <= 10; i++) await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2 - (px * i) / 10);
  await page.mouse.up();
};

// ---------------------------------------------------------------- the song: Vox, a sung clip

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox.wav`);
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write("data/vox/clips.json", JSON.stringify({ clips: [{ id: "voxclip1", start: 0, length: 4, asset: "vox.wav", offset: 0 }] }, null, 2));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";').replace("      kick: kick.out,", "      kick: kick.out,\n      vox: vox.out,"));
check(!!(await until(() => page.$('[data-module-tab="vox"]'), 20000)), "the Vox track is there");
const files = () => ["tracks/vox.ts", "music2.toml", "music2.lock"].map(read).join("\n----\n");

// ---------------------------------------------------------------- + Effect ▸ Tune

// The vocal track's own lane takes an effect around it, in the track's code (review U10).
await show("vox");
await view("graph");
const plus = '[data-module="vox"] button[aria-label="Add an effect after clips"]';
check(!!(await until(() => page.$(plus), 15000)), "the Vox track's lane has a +");
await page.click(plus);
await page.waitForSelector('[data-effect-browser] [data-effect="tune"]', { timeout: 5000 }).catch(() => {});
const group = await page.$eval('[data-effect-browser] [data-effect="tune"]', (e) => e.closest("[data-category]")?.dataset.category).catch(() => null);
check(group === "Voice", `Tune is listed under Voice: ${group}`);
await linted("[data-effect-browser]", "the effect browser");
await page.click('[data-effect-browser] [data-effect="tune"]');
const added = 'tune(lane.audio("clips"), { glide: 0.1, amount: 1 })';
check(await until(() => read("tracks/vox.ts").includes(added), 15000), `Tune goes around the vocal, in its track, at Natural: ${read("tracks/vox.ts").match(/=> .*/)?.[0]}`);
check(read("tracks/vox.ts").includes('import { tune } from "@m2/vocals";') && read("song.ts").includes("vox: vox.out,"), "imported there; the song reads the track as it did");
check(/^"@m2\/vocals" = "1\.0"$/m.test(read("music2.toml")), `music2.toml lists the pack: ${read("music2.toml").match(/"@m2\/vocals".*/)?.[0]}`);
check(/name = "@m2\/vocals"\nversion = "1\.0\.0"/.test(read("music2.lock")), "music2.lock locks it");

// ---------------------------------------------------------------- no key: the card says so (VOX-26)

const card = '[data-module="vox"] [data-node="tune"]';
check(!!(await until(() => page.$(`${card} [data-testid=card-key]`), 15000)), "Tune's card has its key line");
const line = () => page.$eval(`${card} [data-testid=card-key]`, (e) => e.textContent).catch(() => "");
check(await until(async () => (await line()) === "Every note allowedSet the key"), `with no key: ${await line()}`);
const tip = await page.$eval(`${card} [data-testid=card-key]`, (e) => e.title).catch(() => "");
check(tip.startsWith("No key set: every note is allowed."), `its tooltip says why: ${tip}`);
await linted(card, "Tune's card with no key");
await page.screenshot({ path: `${ROOT}/tune-card.png` });

// ---------------------------------------------------------------- Set the key

await seen(`${card} [data-testid=card-set-key]`);
await page.click(`${card} [data-testid=card-set-key]`);
check(!!(await until(() => page.$("[data-testid=key-picker]"), 3000)), "Set the key opens the key field's picker");
await page.click('[data-testid=key-picker] [data-root="A"]');
check(await until(() => /"key": \{"root": "A", "scale": "major"\}|"key":\s*\{\s*"root":\s*"A",\s*"scale":\s*"major"\s*\}/.test(read("data/arrangement.json"))), `the key is the song's: ${read("data/arrangement.json").match(/"key".*/)?.[0]}`);
check(await until(async () => (await line()) === "Key: A major"), `the card says the key: ${await line()}`);
check(!(await page.$(`${card} [data-testid=card-set-key]`)), "and Set the key is gone");
await linted(card, "Tune's card with a key");

// ---------------------------------------------------------------- Glide and Amount

const before = files();
const glide = `${card} [data-knob="glide"]`;
check(!!(await until(() => page.$(glide), 5000)), "Glide is a knob");
await until(async () => (await page.$eval(glide, (e) => e.getAttribute("aria-disabled")).catch(() => "true")) !== "true");
await turn(glide, 30);
check(await until(() => /tune\(lane\.audio\("clips"\), \{ glide: 0\.1[1-9]\d*, amount: 1 \}\)/.test(read("tracks/vox.ts"))), `dragging Glide up writes a longer glide: ${read("tracks/vox.ts").match(/=> .*/)?.[0]}`);
check(await stepIs("Tune glide"), `one undo step: ${await undoTitle()}`);
const amount = `${card} [data-knob="amount"]`;
check(!!(await until(() => page.$(amount), 5000)), "Amount is a knob");
await until(async () => (await page.$eval(amount, (e) => e.getAttribute("aria-disabled")).catch(() => "true")) !== "true");
await turn(amount, -30);
check(await until(() => /amount: 0\.\d+ \}\)/.test(read("tracks/vox.ts"))), `dragging Amount down writes less: ${read("tracks/vox.ts").match(/=> .*/)?.[0]}`);
check(await stepIs("Tune amount"), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
await key("Meta+z");
check(await until(() => files() === before), "two ⌘Z take Amount and Glide back, byte for byte");

// ---------------------------------------------------------------- the Robot preset (review U11)

await seen(`${card} [data-presets]`);
await page.click(`${card} [data-presets]`);
check(!!(await until(() => page.$("[data-preset-group=std] [data-preset='Robot']"), 5000)), "Presets lists Robot and Natural, music2's own");
check(!!(await page.$("[data-preset-group=std] [data-preset='Natural']")), "and Natural");
await page.click("[data-preset-group=std] [data-preset='Robot']");
check(await until(() => read("tracks/vox.ts").includes('tune(lane.audio("clips"), { glide: 0, amount: 1 })')), `loading Robot writes Glide 0: ${read("tracks/vox.ts").match(/=> .*/)?.[0]}`);
check(await until(async () => /Undo .*Robot/.test((await undoTitle()) ?? "")), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => files() === before), "⌘Z takes it back, byte for byte");

done(errors);
