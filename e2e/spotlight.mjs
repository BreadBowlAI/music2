// Track spotlight (LIB-1 to LIB-3): ⌘F's panel, one search over everything that can be added. ⌘F
// opens it; typing filters, grouped; ↓↑ move and Enter adds an instrument as a new track (Undo takes
// it back), a sound onto the selected track's instrument, an effect after the selected track's last;
// Space plays a preview (the server answers a WAV); Esc closes. The side browser's field asks the
// same search. Answers that come late (held back here) never move what was picked, nor let Enter
// act on an older search's rows, in the panel or the side browser; a click on a row keeps the keys
// in the panel; a sound row dragged onto a track loads into it. A sample (std's kick) is found in the
// Samples group, Space fetches its own file, and Enter copies it into assets/ with its note and adds
// its clip on a new audio track, which Undo takes back. A folder of yours added (Your folders, Add a
// Folder…, its path typed as the browser asks for it) says how many samples it has once looked
// through, and how many files don't read, with the list left open; a search typed meanwhile finds
// its sample once it is (LIB-5).
//
//   node e2e/spotlight.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, HOME, open, PROJ, read, sess, sleep, until } from "./fixlib.mjs";

const { page, errors, toasts, state, key, hold: holdBack } = await open(1440, 900, { browser: true });

// Requests held back until released, to stand in for a slow answer (a plugin scan, a busy session,
// a preview rendering); then they go on, or get `answer`.
const holds = [];
await holdBack((r) => {
  const h = holds.find((h) => r.url().includes(h.part));
  if (!h) return 0;
  if (!h.every) holds.splice(holds.indexOf(h), 1);
  return { until: h.until, answer: h.answer };
});
/** Hold the next request whose URL has `part` (with `every`, each one) until the function it returns is called. */
const hold = (part, answer, every = false) => {
  let go;
  const h = { part, answer, every, until: new Promise((r) => (go = r)) };
  holds.push(h);
  return () => {
    if (holds.includes(h)) holds.splice(holds.indexOf(h), 1);
    go();
  };
};
const playing = () => page.evaluate(async (t) => (await (await fetch("/api/graph/status", { headers: { "x-m2-token": t } })).json()).playing, sess.page_token);
const said = () => page.$eval("[data-spotlight-said]", (e) => e.textContent).catch(() => null);
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 10000);
const panel = () => page.$("[data-spotlight]");
const rows = () => page.$$eval("[data-spotlight-item]", (els) => els.map((e) => e.getAttribute("data-spotlight-item")));
const activeRow = () => page.$eval("[data-spotlight-item][data-active]", (e) => e.getAttribute("data-spotlight-item")).catch(() => null);
const modules = async () => (await state()).view.modules.map((m) => m.key);
/** ⌘F, then `words` typed, once the search answered for them. */
const find = async (words) => {
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await key("Meta+KeyF");
  await until(panel, 5000);
  const answered = page.waitForResponse((r) => r.url().includes(`/api/library?q=${encodeURIComponent(words)}`), { timeout: 10000 });
  await page.type("[data-spotlight] input", words);
  await answered;
  await until(async () => (await rows()).length > 0 || (await page.$eval("[data-spotlight]", (e) => e.textContent)).includes("Nothing matches"), 5000);
};

// ---------------------------------------------------------------- ⌘F opens; typing filters, grouped

// The plugin list, which the panel asks for as it first opens, comes late: as after a long scan.
const pluginsCome = hold("/api/plugins");
await key("Meta+KeyF");
check(await until(panel, 5000), "⌘F opens the panel");
check(await page.$eval("[data-spotlight] input", (e) => e === document.activeElement), "with its field focused");
await key("Escape");
check(await until(async () => !(await panel()), 3000), "Esc closes it");

// The row the arrows picked stays picked when the plugin list comes and the search answers again.
await find("synth");
await key("ArrowDown");
await key("ArrowDown");
await key("ArrowDown");
const picked = await activeRow();
const again = page.waitForResponse((r) => r.url().includes("/api/library?q=synth"), { timeout: 10000 });
pluginsCome();
await again;
await sleep(300);
check(picked !== null && (await activeRow()) === picked, `the row picked stays picked when the plugin list comes (${picked}, now ${await activeRow()})`);
await key("Escape");
await until(async () => !(await panel()), 3000);

await find("synth pluck");
const found = await rows();
check(found[0] === "@m2/synths:synth-pluck" && (await activeRow()) === "@m2/synths:synth-pluck", `the best hit is first and selected (${found.slice(0, 3)})`);
check(found.includes("sound:std/@m2/synths:synth-pluck/Glass") && found.includes("sound:std/@m2/synths:synth-pluck/Soft"), "its sounds are found too");
const groups = await page.$$eval("[data-spotlight-group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.join() === "Instruments,Sounds", `grouped, empty groups hidden (${groups})`);
check(!found.some((id) => id === "@m2/synths:synth-pad" || id === "@m2/effects:reverb"), "what doesn't match is left out");

// ---------------------------------------------------------------- ↓↑ and Enter add an instrument, with Undo

const before = await modules();
await key("ArrowDown");
check((await activeRow()) === "sound:std/@m2/synths:synth-pluck/Glass", "↓ moves into the next group");
await key("ArrowUp");
check((await activeRow()) === "@m2/synths:synth-pluck", "↑ moves back");
await key("Enter");
check(await until(async () => !(await panel()), 3000), "Enter closes the panel");
check(await until(async () => (await modules()).length === before.length + 1, 10000), "Enter adds a track playing it");
const pluck = (await modules()).find((m) => !before.includes(m));
check(await toasted("Added Synth pluck"), "and says so, with Undo");
await page.click('button[aria-label="Undo"]');
check(await until(async () => !(await modules()).includes(pluck), 10000), "Undo takes the track back");

// ---------------------------------------------------------------- a sound loads onto the selected track

await find("synth pluck");
await key("Enter");
check(await until(async () => (await modules()).length === before.length + 1, 10000), "a Synth pluck track again");
const track = (await modules()).find((m) => !before.includes(m));
await find("glass");
check((await activeRow()) === "sound:std/@m2/synths:synth-pluck/Glass", "Glass is the best hit for glass");

// ---------------------------------------------------------------- Space plays a preview

await key("ArrowDown");
await key("ArrowUp");
const heard = page.waitForResponse((r) => r.url().includes("/api/library/preview?id="), { timeout: 30000 });
await key("Space");
const answer = await heard;
check(answer.status() === 200 && answer.headers()["content-type"] === "audio/wav", `Space fetches its preview: a WAV (${answer.status()} ${answer.headers()["content-type"]})`);
// Asked again in the page: it is kept, so this renders nothing.
const head = await page.evaluate(async (token) => {
  const r = await fetch(`/api/library/preview?id=${encodeURIComponent("sound:std/@m2/synths:synth-pluck/Glass")}`, { headers: { "x-m2-token": token } });
  const b = new Uint8Array(await r.arrayBuffer());
  return String.fromCharCode(...b.subarray(0, 4)) + String.fromCharCode(...b.subarray(8, 12));
}, sess.page_token);
check(head === "RIFFWAVE", `the answer is a WAV file (${head})`);
check(await page.$eval("[data-spotlight] input", (e) => e.value === "glass"), "Space after the arrows types nothing");

await key("Enter");
check(await until(() => read(`tracks/${track}.ts`).includes("brightness: 9000"), 10000), "Enter loads the sound's settings into the selected track's instrument");
check(await toasted("Loaded Glass into Synth pluck"), "and says so");

// ---------------------------------------------------------------- a sound row drags onto a track

await find("soft");
await page.evaluate((into) => {
  const row = document.querySelector('[data-spotlight-item="sound:std/@m2/synths:synth-pluck/Soft"]');
  const at = document.querySelector(`[data-testid=timeline] [data-track="${into}"]`);
  const dt = new DataTransfer();
  row.dispatchEvent(new DragEvent("dragstart", { dataTransfer: dt, bubbles: true, cancelable: true }));
  for (const type of ["dragenter", "dragover", "drop"]) at.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
  row.dispatchEvent(new DragEvent("dragend", { dataTransfer: dt, bubbles: true }));
}, track);
check(await until(() => read(`tracks/${track}.ts`).includes("brightness: 1800"), 10000), "a sound row dropped onto a track loads into its instrument");
check(await toasted("Loaded Soft into Synth pluck"), "and says so");

// ---------------------------------------------------------------- a click on a row keeps the keys in the panel

await find("glass");
await page.click('[data-spotlight-item="sound:std/@m2/synths:synth-pluck/Glass"]');
check(await page.$eval("[data-spotlight] input", (e) => e === document.activeElement), "a click on a row gives the keys back to the field");
const clicked = page.waitForResponse((r) => r.url().includes("/api/library/preview?id="), { timeout: 30000 }).catch(() => null);
await key("Space");
check((await clicked)?.status() === 200 && !(await playing()), "Space then plays the row clicked, not the song");
await key("Escape");
check(await until(async () => !(await panel()), 3000), "and Esc closes the panel");

// ---------------------------------------------------------------- a preview's late answer never says itself under another row

await find("synth");
await key("ArrowDown");
const first = await activeRow();
const refuse = hold(`/api/library/preview?id=${encodeURIComponent(first)}`, { status: 404, contentType: "application/json", body: JSON.stringify({ error: "Held back and refused" }) });
await key("Space");
await key("ArrowDown");
refuse();
await sleep(500);
check(!(await said())?.includes("Held back"), `an older preview's answer leaves the note of the row moved to as it is (${await said()})`);
await key("Escape");
await until(async () => !(await panel()), 3000);

// ---------------------------------------------------------------- an effect goes on the selected track

await page.click('[data-module-tab="keys"] [data-testid=track-label]');
await find("reverb");
check((await activeRow()) === "@m2/effects:reverb", "Reverb is the best hit for reverb");
await key("Enter");
check(await until(() => /reverb\(/.test(read("tracks/keys.ts")), 10000), "Enter adds the effect to the selected track");
check(await toasted("Added Reverb after"), "and says where");

// ---------------------------------------------------------------- Enter right after typing adds the newest words' best hit

await page.click('[data-module-tab="keys"] [data-testid=track-label]');
const keysBefore = read("tracks/keys.ts");
const tracksBefore = await modules();
await find("pa");
await page.keyboard.type("d");
await key("Enter");
check(await until(async () => (await modules()).length === tracksBefore.length + 1, 10000), "typing pad then Enter at once adds the best hit for pad: a track");
check((await toasted("Added Synth pad")) && read("tracks/keys.ts") === keysBefore, "a Synth pad track, not pa's best hit on Keys");
await page.click('button[aria-label="Undo"]');
await until(async () => (await modules()).length === tracksBefore.length, 10000);

// ---------------------------------------------------------------- nothing found; the side browser asks the same search

await find("zzzq");
check(await until(async () => (await page.$eval("[data-spotlight]", (e) => e.textContent)).includes("Nothing matches “zzzq”"), 3000), "a search that finds nothing says so");
await key("Escape");
check(await until(async () => !(await panel()), 3000), "Esc closes it again");

const asked = page.waitForResponse((r) => r.url().includes("/api/library?q=hall"), { timeout: 10000 });
await page.type("[data-browser] input", "hall");
check((await asked).status() === 200, "the side browser's field asks the library's search");
check(
  await until(async () => (await page.$$eval("[data-browser-item]", (els) => els.map((e) => e.getAttribute("data-browser-item")))).join() === "@m2/effects:reverb", 5000),
  "and lists what it found: Reverb, by its description",
);

// The side browser filters as you type, before the search answers; Enter then waits for the answer
// to the words typed, and adds the row picked.
await page.click('[data-module-tab="keys"] [data-testid=track-label]');
await page.focus("[data-browser] input");
await page.$eval("[data-browser] input", (e) => e.select());
await page.keyboard.press("Backspace");
await until(async () => (await page.$$eval("[data-browser-item]", (els) => els.length)) > 1, 3000);
const reverbAnswer = hold("/api/library?q=", null, true);
await page.type("[data-browser] input", "reverb");
const effects = await page.$$eval("[data-browser-item]", (els) => els.map((e) => e.getAttribute("data-browser-item")));
check(effects[0] === "@m2/effects:reverb" && !effects.includes("std:lowpass"), `the side browser filters as you type, before the search answers (${effects.slice(0, 4)})`);
const sideBefore = await modules();
const reverbs = () => (read("tracks/keys.ts").match(/reverb\(/g) ?? []).length;
const reverbsBefore = reverbs();
// ↓↑: Reverb picked, on the rows the window filtered.
await key("ArrowDown");
await key("ArrowUp");
await key("Enter");
await sleep(300);
reverbAnswer();
check(await until(() => reverbs() === reverbsBefore + 1, 10000), "typing reverb, ↓↑ and Enter in the side browser adds Reverb to the selected track");
check((await modules()).length === sideBefore.length, "and no track");

// ---------------------------------------------------------------- samples: found, heard, added (LIB-4 to LIB-7)

const has = (path) => {
  try {
    read(path);
    return true;
  } catch {
    return false;
  }
};
await find("kick one-shot");
const kicks = await rows();
check(kicks[0] === "sample:std:samples/kit/kick.wav" && (await page.$('[data-spotlight-group="sample"]')) !== null, `std's kick is the best hit, in the Samples group (${kicks.slice(0, 3)})`);
check(
  await page.$eval('[data-spotlight-item="sample:std:samples/kit/kick.wav"]', (e) => e.textContent.includes("One-shot") && e.textContent.includes("music2")),
  "its row says it is a one-shot from music2",
);
check(await page.$eval('[data-spotlight-filter="one-shot"]', (e) => e.getAttribute("aria-pressed") === "true"), "the One-shots filter shows as on, typed");
await key("ArrowDown");
await key("ArrowUp");
const kickHeard = page.waitForResponse((r) => r.url().includes("/api/library/preview?id=sample"), { timeout: 30000 });
await key("Space");
const kickAnswer = await kickHeard;
// Asked again in the page: its bytes, to set beside the file it adds.
const kickBytes = await page.evaluate(async (token) => {
  const r = await fetch(`/api/library/preview?id=${encodeURIComponent("sample:std:samples/kit/kick.wav")}`, { headers: { "x-m2-token": token } });
  const b = new Uint8Array(await r.arrayBuffer());
  return { head: String.fromCharCode(...b.subarray(0, 4)) + String.fromCharCode(...b.subarray(8, 12)), size: b.length };
}, sess.page_token);
check(
  kickAnswer.status() === 200 && kickAnswer.headers()["content-type"] === "audio/wav" && kickBytes.head === "RIFFWAVE",
  `Space fetches the sample's own file, a WAV (${kickAnswer.status()}, ${kickBytes.head}, ${kickBytes.size} bytes)`,
);
const samplesBefore = await modules();
await key("Enter");
check(await until(async () => (await modules()).length === samplesBefore.length + 1, 10000), "Enter adds the sample on a new audio track");
const kickTrack = (await modules()).find((m) => !samplesBefore.includes(m));
check(
  await until(() => has("assets/kick.wav") && read(`data/${kickTrack}/clips.json`).includes("kick.wav"), 10000) && fs.statSync(`${PROJ}/assets/kick.wav`).size === kickBytes.size,
  "its file is copied into assets/, the same bytes Space played, and its clip names it",
);
const note = has("assets/kick.wav.json") ? JSON.parse(read("assets/kick.wav.json")) : {};
check(note.from?.pack === "std" && note.from?.file === "samples/kit/kick.wav" && note.from?.license === "MIT OR Apache-2.0", `with a note of where it came from (${JSON.stringify(note)})`);
check(await toasted("Added kick on a new track"), "and says so, with Undo");
await page.click('button[aria-label="Undo"]');
check(await until(async () => !(await modules()).includes(kickTrack), 10000), "Undo takes the clip and its track back");
check(has("assets/kick.wav") && has("assets/kick.wav.json"), "the file and its note stay");

// ---------------------------------------------------------------- your folder: looked through, then found (LIB-5)

const mine = `${HOME}/My Samples`;
fs.mkdirSync(`${mine}/Kicks`, { recursive: true });
fs.copyFileSync(`${PROJ}/assets/kick.wav`, `${mine}/Kicks/Big Boom.wav`);
fs.writeFileSync(`${mine}/Kicks/broken.wav`, "RIFF....WAVE");
const boom = `sample:folder:${fs.realpathSync(mine)}/Kicks/Big Boom.wav`;
await find("boom");
await page.click("[data-spotlight-folders]");
await until(() => page.$("[data-spotlight-add-folder]"), 5000);
page.once("dialog", (d) => void d.accept(mine));
await page.click("[data-spotlight-add-folder]");
const folderRow = () => page.$eval("[data-spotlight-folder]", (e) => e.textContent).catch(() => "");
check(
  await until(async () => (await folderRow()).includes("1 samples · 1 don't read"), 15000),
  `the folder's row says what it found once looked through, the list left open (${await folderRow()})`,
);
check(await until(async () => (await rows()).includes(boom), 15000), `the search typed meanwhile finds its sample once it is (${(await rows()).slice(0, 3)})`);
check(
  await page.$eval(`[data-spotlight-item="${boom}"]`, (e) => e.textContent.includes("One-shot") && e.textContent.includes("Yours")).catch(() => false),
  "its row says it is a one-shot of yours",
);
await key("Escape");

done(errors);
