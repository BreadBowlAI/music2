// Track ab's sampler in the app: std's Sampler is in the side browser in a group of its own, and
// + Track adds it, its card in Device saying "Drop a WAV or FLAC here". A WAV dropped on the card (as a
// browser drops it) comes into assets/ and the Sampler plays it: the call says `sample: "tone.wav"`,
// a toast says so, it is one undo step ("Undo setting Sampler to tone.wav"), and the export
// changes. ⌘Z takes the code back, and the export with it; the file stays. Each state linted.
//
//   node e2e/ab-sampler.mjs <root> <home> <project> <m2 cli>     (the `export` kind: examples/first-song open)
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const M2 = process.argv[5];
const { page, errors, key, api, view, toasts, box } = await open(1440, 900, { browser: true });

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);
const state = async () => (await api("")).body;

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

/** The song rendered by the m2 command line, as bytes. */
const render = (name) => {
  execFileSync(M2, ["render", PROJ, `${ROOT}/${name}`]);
  return fs.readFileSync(`${ROOT}/${name}`);
};

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

// ---------------------------------------------------------------- the browser and + Track

await page.waitForSelector("[data-browser]");
const listed = await page.$$eval('[data-browser-category="instruments:Sampler"] [data-browser-instrument]', (els) => els.map((e) => e.dataset.browserInstrument));
check(JSON.stringify(listed) === '["std:sampler"]', `the side browser lists Sampler in a group of its own: ${JSON.stringify(listed)}`);
const groups = await page.$$eval("[data-browser-category^='instruments:']", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.indexOf("Sampler") === groups.indexOf("Instruments") + 1, `after Instruments: ${groups}`);
await linted('[data-browser-category="instruments:Sampler"]', "the browser's Sampler group");

await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list] input");
await page.keyboard.type("sampler");
const found = await page.$$eval("[data-testid=instrument-list] [role=option]", (os) => os.map((o) => o.textContent));
check(found.length === 1 && found[0].startsWith("Sampler"), `typing "sampler" in + Track finds it: ${JSON.stringify(found)}`);
await page.keyboard.press("Enter");
const tracks = async () => (await state()).timeline.tracks.filter((t) => t.track).map((t) => t.module);
check(await until(async () => (await tracks()).includes("sampler")), `+ Track adds a Sampler track: ${await tracks()}`);
check(read("tracks/sampler.ts").includes('sampler(lane.notes("notes"))'), `its file plays its notes: ${read("tracks/sampler.ts")}`);

// Its notes, as the piano roll writes them: a C4 on each beat of the first bar.
const notes = [0, 1, 2, 3].map((b) => `    {"pitch": "C4", "start": ${b}, "length": 0.5, "velocity": 0.8}`).join(",\n");
write("data/sampler/notes.json", `{"clips": [\n  {"id": "abs4mplr", "start": 0, "length": 4, "notes": [\n${notes}\n  ]}\n]}\n`);
const heard = async () => (await state()).timeline.tracks.find((t) => t.module === "sampler")?.lanes.find((l) => l.key === "notes")?.clips?.[0]?.notes?.length ?? 0;
check(await until(async () => (await heard()) === 4), `its four notes are taken in: ${await heard()}`);

// ---------------------------------------------------------------- its card in Device

await view("graph");
const card = '[data-module="sampler"] [data-sampler-call]';
check(await until(async () => !!(await page.$(card))), "Device shows the Sampler's card");
await page.$eval(card, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(200);
const said = () => page.$eval(`${card} [data-testid=sampler-file]`, (e) => e.textContent).catch(() => null);
check((await said()) === "Drop a WAV or FLAC here", `with no file, it says so: ${await said()}`);
await linted(card, "the Sampler's card with no file");
const before = render("before.wav");

// ---------------------------------------------------------------- a WAV dropped on it

fs.mkdirSync(`${ROOT}/files`, { recursive: true });
wav(`${ROOT}/files/tone.wav`);
const b = await box(card);
const dropped = await dropFiles(b.x + b.width / 2, b.y + b.height / 2, [{ name: "tone.wav", path: `${ROOT}/files/tone.wav` }]);
check(dropped.hint === "Play it in Sampler" && dropped.outline === "sampler", `while dragging, the hint says so and outlines the card: ${JSON.stringify(dropped)}`);
check(await until(() => read("tracks/sampler.ts").includes('sampler(lane.notes("notes"), { sample: "tone.wav" })')), `its call names the file: ${read("tracks/sampler.ts")}`);
check(fs.existsSync(`${PROJ}/assets/tone.wav`) && fs.readFileSync(`${PROJ}/assets/tone.wav`).equals(fs.readFileSync(`${ROOT}/files/tone.wav`)), "the file came into assets/");
check(await until(async () => (await toasts()).some((t) => t.startsWith("Sampler plays tone.wav"))), `a toast says so: ${await toasts()}`);
check(await stepIs("setting Sampler to tone.wav"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await said()) === "tone.wav"), `the card names it: ${await said()}`);
await linted(card, "the Sampler's card with its file");
await page.screenshot({ path: `${ROOT}/ab-sampler-card.png` });
const after = render("after.wav");
check(!after.equals(before), "the export changes: the Sampler plays the file");

// ---------------------------------------------------------------- undo

await key("Meta+z");
check(await until(() => read("tracks/sampler.ts").includes('sampler(lane.notes("notes"))') && !read("tracks/sampler.ts").includes("sample:")), `⌘Z takes the code back: ${read("tracks/sampler.ts")}`);
check(fs.existsSync(`${PROJ}/assets/tone.wav`), "the file stays in assets/");
check(await until(async () => (await said()) === "Drop a WAV or FLAC here"), `the card says to drop one again: ${await said()}`);
check(render("undone.wav").equals(before), "the export is as it was");

done(errors);
