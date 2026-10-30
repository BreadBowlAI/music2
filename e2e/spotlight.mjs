// Track spotlight (LIB-1 to LIB-3): ⌘F's panel, one search over everything that can be added. ⌘F
// opens it; typing filters, grouped; ↓↑ move and Enter adds an instrument as a new track (Undo takes
// it back), a sound onto the selected track's instrument, an effect after the selected track's last;
// Space plays a preview (the server answers a WAV); Esc closes. The side browser's field asks the
// same search.
//
//   node e2e/spotlight.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sess, until } from "./fixlib.mjs";

const { page, errors, toasts, state, key } = await open(1440, 900, { browser: true });
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

await key("Meta+KeyF");
check(await until(panel, 5000), "⌘F opens the panel");
check(await page.$eval("[data-spotlight] input", (e) => e === document.activeElement), "with its field focused");
await key("Escape");
check(await until(async () => !(await panel()), 3000), "Esc closes it");

await find("synth pluck");
const found = await rows();
check(found[0] === "std:synth-pluck" && (await activeRow()) === "std:synth-pluck", `the best hit is first and selected (${found.slice(0, 3)})`);
check(found.includes("sound:std/std:synth-pluck/Glass") && found.includes("sound:std/std:synth-pluck/Soft"), "its sounds are found too");
const groups = await page.$$eval("[data-spotlight-group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.join() === "Instruments,Sounds", `grouped, empty groups hidden (${groups})`);
check(!found.some((id) => id === "std:synth-pad" || id === "std:reverb"), "what doesn't match is left out");

// ---------------------------------------------------------------- ↓↑ and Enter add an instrument, with Undo

const before = await modules();
await key("ArrowDown");
check((await activeRow()) === "sound:std/std:synth-pluck/Glass", "↓ moves into the next group");
await key("ArrowUp");
check((await activeRow()) === "std:synth-pluck", "↑ moves back");
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
check((await activeRow()) === "sound:std/std:synth-pluck/Glass", "Glass is the best hit for glass");

// ---------------------------------------------------------------- Space plays a preview

await key("ArrowDown");
await key("ArrowUp");
const heard = page.waitForResponse((r) => r.url().includes("/api/library/preview?id="), { timeout: 30000 });
await key("Space");
const answer = await heard;
check(answer.status() === 200 && answer.headers()["content-type"] === "audio/wav", `Space fetches its preview: a WAV (${answer.status()} ${answer.headers()["content-type"]})`);
// Asked again in the page: it is kept, so this renders nothing.
const head = await page.evaluate(async (token) => {
  const r = await fetch(`/api/library/preview?id=${encodeURIComponent("sound:std/std:synth-pluck/Glass")}`, { headers: { "x-m2-token": token } });
  const b = new Uint8Array(await r.arrayBuffer());
  return String.fromCharCode(...b.subarray(0, 4)) + String.fromCharCode(...b.subarray(8, 12));
}, sess.page_token);
check(head === "RIFFWAVE", `the answer is a WAV file (${head})`);
check(await page.$eval("[data-spotlight] input", (e) => e.value === "glass"), "Space after the arrows types nothing");

await key("Enter");
check(await until(() => read(`tracks/${track}.ts`).includes("brightness: 9000"), 10000), "Enter loads the sound's settings into the selected track's instrument");
check(await toasted("Loaded Glass into Synth pluck"), "and says so");

// ---------------------------------------------------------------- an effect goes on the selected track

await page.click('[data-module-tab="keys"] [data-testid=track-label]');
await find("reverb");
check((await activeRow()) === "std:reverb", "Reverb is the best hit for reverb");
await key("Enter");
check(await until(() => /reverb\(/.test(read("tracks/keys.ts")), 10000), "Enter adds the effect to the selected track");
check(await toasted("Added Reverb after"), "and says where");

// ---------------------------------------------------------------- nothing found; the side browser asks the same search

await find("zzzq");
check(await until(async () => (await page.$eval("[data-spotlight]", (e) => e.textContent)).includes("Nothing matches “zzzq”"), 3000), "a search that finds nothing says so");
await key("Escape");
check(await until(async () => !(await panel()), 3000), "Esc closes it again");

const asked = page.waitForResponse((r) => r.url().includes("/api/library?q=hall"), { timeout: 10000 });
await page.type("[data-browser] input", "hall");
check((await asked).status() === 200, "the side browser's field asks the library's search");
check(
  await until(async () => (await page.$$eval("[data-browser-item]", (els) => els.map((e) => e.getAttribute("data-browser-item")))).join() === "std:reverb", 5000),
  "and lists what it found: Reverb, by its description",
);

done(errors);
