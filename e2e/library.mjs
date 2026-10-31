// Track library-get (REG-13 to REG-16): ⌘F's From the library and Get, against a registry the
// robot built from two made-up packs (fixtures/library: @alice/drums, one kick; @bob/keys, an
// instrument and its sound Dark), served over http on this computer. "soft" finds the library's
// items after what you have, in a group of their own, each with Get; Space plays a library item's
// preview (its blob); Enter on one gets it, its progress in its row and the panel's foot, the other
// Gets waiting, and Esc cancels it, leaving nothing in the store; Enter again gets it and adds it: a
// sound adds a track of its instrument with the sound loaded, its pack's dependency and lock
// written; once got it is what you have. A sample's Get adds it on a new audio track with its note,
// and installs no pack. A Get that ends after another song was opened installs its pack, adds
// nothing to the song open then, and says so (fixtures/library's third pack, @carol/pad).
//
//   node e2e/library.mjs <root> <home> <project> <m2 cli>     (the `library` kind)
import crypto from "node:crypto";
import fs from "node:fs";
import { check, done, HOME, open, PROJ, read, ROOT, sess, until } from "./fixlib.mjs";

const { page, errors, toasts, state, key } = await open(1440, 900, { browser: true });

const KEYS = "library:@bob/keys:soft-keys";
const DARK = "library:sound:std/@bob/keys:soft-keys/Dark";
const KICK = "library:sample:@alice/drums:samples/kicks/Boom.wav";
const store = `${HOME}/Music/music2/Packs`;
const drip = `${ROOT}/library/drip`;

const said = () => page.$eval("[data-spotlight-said]", (e) => e.textContent).catch(() => "");
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 15000);
const panel = () => page.$("[data-spotlight]");
const rows = () => page.$$eval("[data-spotlight-item]", (els) => els.map((e) => e.getAttribute("data-spotlight-item")));
const activeRow = () => page.$eval("[data-spotlight-item][data-active]", (e) => e.getAttribute("data-spotlight-item")).catch(() => null);
const modules = async () => (await state()).view.modules.map((m) => m.key);
const has = (path) => fs.existsSync(`${PROJ}/${path}`);
const getting = () => (fs.existsSync(store) ? fs.readdirSync(store).filter((n) => n.startsWith(".getting")) : []);
/** ⌘F, then `words` typed, once the search answered for them. */
const find = async (words) => {
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await key("Meta+KeyF");
  await until(panel, 5000);
  const answered = page.waitForResponse((r) => r.url().includes(`/api/library?q=${encodeURIComponent(words)}`), { timeout: 10000 });
  await page.type("[data-spotlight] input", words);
  await answered;
};
/** ↓ until `id` is the row selected, at most through every row. */
const pickRow = async (id) => {
  for (let i = 0; i < 40 && (await activeRow()) !== id; i++) await key("ArrowDown");
  return (await activeRow()) === id;
};

// ---------------------------------------------------------------- From the library, after what you have

await find("soft");
check(await until(async () => (await rows()).includes(KEYS), 15000), `"soft" finds the library's Soft keys, once its catalog is here (${(await rows()).slice(-3)})`);
const groups = await page.$$eval("[data-spotlight-group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.at(-1) === "From the library" && groups.length > 1, `in a group of its own, after what you have (${groups})`);
const found = await rows();
check(found.indexOf("sound:std/@m2/synths:synth-pluck/Soft") < found.indexOf(KEYS) && found.includes(DARK), "what you have comes first; the pack's sound Dark is From the library too");
check(
  await page.$eval(`[data-spotlight-item="${KEYS}"]`, (e) => e.textContent.includes("@bob/keys") && !!e.querySelector("[data-spotlight-get]")),
  "its row names its pack, with Get",
);

// ---------------------------------------------------------------- Space plays a library item's preview

check(await pickRow(KEYS), "↓ reaches it");
const foot = await said();
check(foot.includes("Enter gets it (") && foot.includes("and adds a track") && foot.includes("Space plays it"), `the foot says what Enter does (${foot})`);
const heard = page.waitForResponse((r) => r.url().includes(`/api/library/preview?id=${encodeURIComponent(KEYS)}`), { timeout: 30000 });
await key("Space");
const answer = await heard;
check(answer.status() === 200 && answer.headers()["content-type"] === "audio/wav", `Space fetches its preview from the library: a WAV (${answer.status()})`);

// ---------------------------------------------------------------- Get: progress in its row and the foot; Esc cancels

// The instrument's code comes slowly: half, then nothing until the drip file goes.
const index = crypto.createHash("sha256").update(fs.readFileSync(`${ROOT}/library/keys/src/index.ts`)).digest("hex");
fs.writeFileSync(drip, `blobs/sha256/${index}`);
check(await pickRow(DARK), "↓ reaches its sound Dark");
await key("Enter");
const progressOf = (id) => page.$eval(`[data-spotlight-item="${id}"] [data-spotlight-progress]`, (e) => Number(e.getAttribute("data-spotlight-progress"))).catch(() => -1);
const progress = () => progressOf(DARK);
check(await until(async () => (await progress()) > 0, 15000), `Enter gets it: its row shows how far, with a bar (${await progress()}%)`);
const coming = await said();
check(coming.startsWith("Getting Dark…") && coming.includes(" of ") && coming.includes("Esc cancels"), `the foot says how far, and that Esc cancels (${coming})`);
check(await page.$eval(`[data-spotlight-item="${DARK}"] [data-spotlight-cancel]`, () => true).catch(() => false), "with Cancel in its row");
check(await page.$eval(`[data-spotlight-get="${KEYS}"]`, (e) => e.disabled), "one at a time: the other Gets wait");
await key("Escape");
check(await until(async () => (await said()).includes("Couldn't get Dark") && (await said()).includes("cancelled"), 10000), `Esc cancels it, saying so (${await said()})`);
check(!!(await panel()) && !fs.existsSync(`${store}/@bob`) && getting().length === 0, "the panel stays open, and nothing of it is kept");

// ---------------------------------------------------------------- Get again: got, and added

fs.rmSync(drip);
const before = await modules();
check((await activeRow()) === DARK, "Dark is still the row selected");
await key("Enter");
check(await until(async () => !(await panel()), 20000), "once got, the panel closes");
check(await until(async () => (await modules()).length === before.length + 1, 20000), "a track is added");
const track = (await modules()).find((m) => !before.includes(m));
check(
  await until(() => has(`tracks/${track}.ts`) && read(`tracks/${track}.ts`).includes("softKeys") && read(`tracks/${track}.ts`).includes("brightness: 600"), 15000),
  "it plays Soft keys, with Dark's settings loaded",
);
check(await toasted("Loaded Dark into Soft keys"), "and says so");
check(read("music2.toml").includes('"@bob/keys" = "0.1"') && has("music2.lock") && read("music2.lock").includes("@bob/keys"), "the pack's dependency and lock are written");
check(fs.existsSync(`${store}/@bob/keys/0.1.0/m2pack.json`) && getting().length === 0, "the pack is installed in the store, whole, and no folder of the Get is left");

// ---------------------------------------------------------------- once got, it is what you have

await find("soft keys");
check(await until(async () => (await rows()).includes("@bob/keys:soft-keys"), 10000), "Soft keys is now among what you have");
check(!(await rows()).includes(KEYS), "and no longer From the library");
await key("Escape");
await until(async () => !(await panel()), 3000);

// ---------------------------------------------------------------- a sample: its one file, with its note

await find("kick");
check(await until(async () => (await rows()).includes(KICK), 15000), "\"kick\" finds the library's kick");
const samplesBefore = await modules();
await page.click(`[data-spotlight-get="${KICK}"]`);
check(await until(async () => !(await panel()), 20000), "Get closes the panel once it is here");
check(await until(async () => (await modules()).length === samplesBefore.length + 1, 15000), "the sample is added on a new audio track");
check(await until(() => has("assets/Boom.wav") && has("assets/Boom.wav.json"), 10000), "its file is copied into assets/, with its note");
const note = has("assets/Boom.wav.json") ? JSON.parse(read("assets/Boom.wav.json")) : {};
check(note.from?.pack === "@alice/drums" && note.from?.version === "1.0.0" && note.from?.license === "CC0-1.0", `the note names its pack, version and licence (${JSON.stringify(note)})`);
check(await toasted("Added Boom on a new track"), "and says so, with Undo");
check(!fs.existsSync(`${store}/@alice`), "a sample comes alone: no pack is installed for it");

// ---------------------------------------------------------------- a Get that ends after another song was opened

const PAD = "library:@carol/pad:warm-pad";
const other = `${ROOT}/other-song`;
fs.cpSync(PROJ, other, { recursive: true });
const otherManifest = fs.readFileSync(`${other}/music2.toml`, "utf8");
const padIndex = crypto.createHash("sha256").update(fs.readFileSync(`${ROOT}/library/pad/src/index.ts`)).digest("hex");
fs.writeFileSync(drip, `blobs/sha256/${padIndex}`);
await find("warm pad");
check(await until(async () => (await rows()).includes(PAD), 15000), `"warm pad" finds the library's Warm pad (${(await rows()).slice(-3)})`);
check(await pickRow(PAD), "↓ reaches it");
await key("Enter");
check(await until(async () => (await progressOf(PAD)) > 0, 15000), "its Get comes, held half way");
const opened = await page.evaluate(
  async (token, path) => (await fetch("/api/graph/open", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ path }) })).status,
  sess.page_token,
  other,
);
check(opened === 200 && (await until(async () => (await state()).dir === other, 10000)), "another song is opened meanwhile");
const otherModules = await modules();
fs.rmSync(drip);
check(await toasted("isn't open now"), "once got, it says the song it was got for isn't open, so it wasn't added");
check(fs.readFileSync(`${other}/music2.toml`, "utf8") === otherManifest && (await modules()).length === otherModules.length, "nothing is added to the song open now");
check(fs.existsSync(`${store}/@carol/pad/0.1.0/m2pack.json`), "the pack is installed all the same");

done(errors);
