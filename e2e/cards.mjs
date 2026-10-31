// Track cards: a call's settings on its card in Device (CARD-1 to CARD-7). Bass's LFO shows its
// shape as a menu, at its default (sine, in grey) and saying a change rebuilds the sound; picking
// saw writes `shape: "saw"` into its call, one undo step ("Undo LFO shape"), and ⌘Z takes the code
// back byte for byte. A track whose sound is a component of the song's own (Box) shows a switch, a
// number box and a recording: the switch writes `held: true`; the number box refuses 2.5 in words,
// writing nothing, and writes 6 on Return; a WAV dropped on the recording (as a browser drops it)
// comes into assets/ and the call names it (`sample: "tone.wav"`), one undo step ("Undo setting Box
// to tone.wav"), its waveform drawn. Each state linted.
//
//   node e2e/cards.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, key, view, toasts, box } = await open(1440, 900);

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);
/** Select track `m` and show its sound in Device. */
const device = async (m) => {
  await until(async () => !!(await page.$(`[data-module-tab="${m}"]`)));
  await page.click(`[data-module-tab="${m}"] [data-testid=track-label]`);
  await view("graph");
  await until(async () => !!(await page.$(`[data-module="${m}"] [data-node-body]`)));
};
/** A setting's row on card `card`: what it shows, its value as the view says it, and its tooltip. */
const row = (card, slot) =>
  page
    .$eval(`${card} [data-setting="${slot}"]`, (e) => ({
      text: e.textContent,
      value: e.dataset.value ? JSON.parse(e.dataset.value) : undefined,
      written: !!e.dataset.written,
      title: e.getAttribute("title") ?? "",
    }))
    .catch(() => null);

/** A mono 16-bit WAV at 44.1 kHz: a decaying 220 Hz tone, half a second. */
function wav(file) {
  const rate = 44100;
  const n = rate / 2;
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(0.8 * Math.sin((2 * Math.PI * 220 * i) / rate) * Math.exp((-4 * i) / rate) * 32767), 44 + i * 2);
  fs.writeFileSync(file, b);
}

/** Drag `files` over (`x`, `y`) as a browser does, then drop them there: what the hint and its outline said while over. */
async function dropFiles(x, y, files) {
  return page.evaluate(
    async (x, y, files) => {
      const dt = new DataTransfer();
      for (const f of files) dt.items.add(new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: "audio/wav" }));
      const at = document.elementFromPoint(x, y) ?? document.body;
      const fire = (type) => at.dispatchEvent(new DragEvent(type, { dataTransfer: dt, clientX: x, clientY: y, bubbles: true, cancelable: true }));
      fire("dragenter");
      fire("dragover");
      // The hint draws on the next frame.
      await new Promise((r) => setTimeout(r, 120));
      const hint = document.querySelector("[data-testid=drop-hint]")?.textContent ?? null;
      const outline = document.querySelector("[data-testid=drop-outline]")?.dataset.kind ?? null;
      fire("drop");
      return { hint, outline };
    },
    x,
    y,
    files.map((f) => ({ name: f.name, b64: fs.readFileSync(f.path).toString("base64") })),
  );
}

// ---------------------------------------------------------------- the LFO's shape: a menu

const bass = read("tracks/bass.ts");
await device("bass");
const lfo = '[data-module="bass"] [data-node="lfo"]';
await page.$eval(lfo, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(150);
const shape = await row(lfo, "shape");
check(shape?.value === "sine" && !shape.written && shape.text.includes("sine"), `the LFO's card shows its shape at its default, sine: ${JSON.stringify(shape)}`);
check(!!shape?.title.includes("rebuilds the sound"), `pointing at it says a change rebuilds the sound: ${shape?.title}`);
await linted(lfo, "the LFO's card with its shape");
await page.screenshot({ path: `${ROOT}/cards-lfo.png` });
await page.click(`${lfo} [data-setting="shape"] [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const options = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(options) === '["sine","tri","saw","square"]', `its menu lists its shapes: ${JSON.stringify(options)}`);
await linted("[data-testid=setting-options]", "the shape's menu");
await page.click('[data-testid=setting-options] [data-option="saw"]');
const picked = "lfo(time.beats, { period: 0.5, shape: \"saw\" })";
check(await until(() => read("tracks/bass.ts") === bass.replace("lfo(time.beats, { period: 0.5 })", picked)), `picking saw writes it into the call: ${read("tracks/bass.ts").match(/lfo\(.*\)\)?/)?.[0]}`);
check(await stepIs("LFO shape"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await row(lfo, "shape"))?.value === "saw" && (await row(lfo, "shape"))?.written), `the card shows saw, written: ${JSON.stringify(await row(lfo, "shape"))}`);
check((await toasts()).length === 0, `no toast: its result is on the card (${await toasts()})`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/bass.ts") === bass), "⌘Z takes the code back, byte for byte");
check(await until(async () => (await row(lfo, "shape"))?.value === "sine"), `and the card shows sine again: ${JSON.stringify(await row(lfo, "shape"))}`);

// ---------------------------------------------------------------- a sound of the song's own

const song = read("song.ts");
write(
  "tracks/box.ts",
  `import { component, port, setting, track } from "@m2/core";
import { sampler } from "std/sampler";

const box = component(
  {
    id: "box",
    label: "Box",
    inputs: { notes: port.notes() },
    settings: { held: setting.bool(false), voices: setting.int(4), sample: setting.asset().optional() },
    output: port.audio(),
  },
  (notes, p) => sampler(notes, { sample: p.sample, mode: p.held ? "gate" : "oneshot", voices: p.voices }),
);

export default track("box", { name: "Box" }, ({ lane }) => box(lane.notes("notes")));
`,
);
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport box from "./tracks/box";').replace("      kick: kick.out,", "      kick: kick.out,\n      box: box.out,"));
await device("box");
const card = '[data-module="box"] [data-node="box"]';
check(await until(async () => !!(await page.$(`${card} [data-setting="held"]`))), "Box's card shows its settings");
await page.$eval(card, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(150);
const kinds = await page.$$eval(`${card} [data-setting]`, (els) => els.map((e) => `${e.dataset.setting}:${e.dataset.kind}`));
check(JSON.stringify(kinds) === '["held:switch","voices:number","sample:recording"]', `in its descriptor's order, each by its kind: ${JSON.stringify(kinds)}`);
await linted(card, "Box's card");

// A switch.
const sw = `${card} [data-setting="held"] [role=switch]`;
check((await page.$eval(sw, (e) => e.getAttribute("aria-checked"))) === "false", "held is off");
await page.click(sw);
check(await until(() => read("tracks/box.ts").includes('box(lane.notes("notes"), { held: true })')), `flipping it writes held: true: ${read("tracks/box.ts").match(/box\(lane.*\)/)?.[0]}`);
check(await until(async () => (await page.$eval(sw, (e) => e.getAttribute("aria-checked")).catch(() => null)) === "true"), "and it shows on");
check(await stepIs("Box held"), `one undo step: ${await undoTitle()}`);

// A whole number: a fraction is refused, said under the box; a whole one is written on Return.
const num = `${card} [data-setting="voices"] [data-testid=setting-number]`;
const typed = read("tracks/box.ts");
await page.click(num, { clickCount: 3 });
await page.keyboard.type("2.5");
await page.keyboard.press("Enter");
await sleep(300);
const bad = await page.$eval(`${card} [data-setting="voices"] [role=alert]`, (e) => e.textContent).catch(() => null);
check(bad === "A whole number, like 8" && read("tracks/box.ts") === typed, `2.5 is refused in words, and nothing is written: ${bad}`);
// Esc puts the value back and leaves the box; a click in it again takes the whole value.
await page.keyboard.press("Escape");
const back = await page.$eval(num, (e) => ({ value: e.value, invalid: e.getAttribute("aria-invalid"), focused: document.activeElement === e }));
check(back.value === "4" && !back.invalid && !back.focused, `Esc puts 4 back: ${JSON.stringify(back)}`);
await page.click(num);
await page.keyboard.type("6");
await page.keyboard.press("Enter");
check(await until(() => read("tracks/box.ts").includes("{ held: true, voices: 6 }")), `6 on Return writes voices: 6: ${read("tracks/box.ts").match(/box\(lane.*\)/)?.[0]}`);
check(await stepIs("Box voices"), `one undo step: ${await undoTitle()}`);

// A recording: a WAV dropped on it.
fs.mkdirSync(`${ROOT}/files`, { recursive: true });
wav(`${ROOT}/files/tone.wav`);
const rec = `${card} [data-setting="sample"] [data-testid=recording]`;
check((await page.$eval(rec, (e) => e.textContent).catch(() => null)) === "Drop a WAV or FLAC here", "the recording says to drop a file");
const b = await box(rec);
const dropped = await dropFiles(b.x + b.width / 2, b.y + b.height / 2, [{ name: "tone.wav", path: `${ROOT}/files/tone.wav` }]);
check(dropped.hint === "Play it in Box" && dropped.outline === "recording", `while dragging, the hint says so and outlines it: ${JSON.stringify(dropped)}`);
check(await until(() => read("tracks/box.ts").includes('{ held: true, voices: 6, sample: "tone.wav" }')), `its call names the file: ${read("tracks/box.ts").match(/box\(lane.*\)/)?.[0]}`);
check(fs.existsSync(`${PROJ}/assets/tone.wav`) && fs.readFileSync(`${PROJ}/assets/tone.wav`).equals(fs.readFileSync(`${ROOT}/files/tone.wav`)), "the file came into assets/");
check(await until(async () => (await toasts()).some((t) => t.startsWith("Box plays tone.wav"))), `a toast says so: ${await toasts()}`);
check(await stepIs("setting Box to tone.wav"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await page.$eval(rec, (e) => e.textContent).catch(() => null)) === "tone.wav" && !!(await page.$(`${rec} svg[data-wave]`))), "the recording shows its waveform and its name");
await linted(card, "Box's card with its settings written");
await page.screenshot({ path: `${ROOT}/cards-box.png` });

done(errors);
