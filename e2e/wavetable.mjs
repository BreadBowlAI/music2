// The wavetable synth, a bundled pack (PACK-27). + Track lists Wavetable among its instruments, as
// music2's own; picking it adds a track that plays it, the song listing and locking its pack
// (PACK-10). Its card in Device shows its groups (Oscillators, Filter, Envelopes, LFO, Unison, then
// More); its settings read by their labels (Table A, never TableA: CARD-34); table A is a menu of
// the five built-in tables, and picking vowels writes `tableA: "vowels"`, one undo step ("Undo
// Wavetable table A") that ⌘Z takes back byte for byte; once the page shows the revision ⇧⌘Z made
// (a drag goes from the revision the page shows), position A dragged up writes a new
// position; the amp envelope's picture draws its three corners, and the filter envelope's and the
// filter's pictures are there too. A note played on it from a MIDI keyboard (M2_MIDI=inject) moves
// its header's meter, and stops when the key is let go. Each state linted.
//
//   node e2e/wavetable.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, key, view, box, toasts } = await open(1440, 900);
/** Every answer of a knob's routes that refused something, as it came. */
const refused = [];
page.on("response", (r) => r.status() >= 400 && /\/api\/graph\/(preview|set)\b/.test(r.url()) && refused.push(`${r.url().replace(/^.*\/api/, "/api")} ${r.status()}`));
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);

// ---------------------------------------------------------------- + Track

const song = read("song.ts");
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => {});
const item = '[data-testid=instrument-list] [data-instrument="@m2/wavetable:wavetable"]';
check(await until(() => page.$(item)), "+ Track lists the Wavetable");
const said = await page.$eval(item, (e) => e.textContent).catch(() => "");
check(said.includes("Wavetable"), `by its name: ${said}`);
await linted("[data-testid=instrument-list]", "+ Track's list");
await page.click(item);
const file = "tracks/wavetable.ts";
check(await until(() => fs.existsSync(`${PROJ}/${file}`)), "picking it adds tracks/wavetable.ts");
const added = fs.existsSync(`${PROJ}/${file}`) ? read(file) : "";
check(added.includes('import { wavetable } from "@m2/wavetable";') && added.includes('wavetable(lane.notes("notes"))'), `a track that plays it from its notes: ${added}`);
check(/"@m2\/wavetable" = "1\.0"/.test(read("music2.toml")) && read("music2.lock").includes('name = "@m2/wavetable"'), "the song lists its pack and locks it");
check(read("song.ts") !== song, "the song mixes the new track");

// ---------------------------------------------------------------- its card

await until(async () => !!(await page.$('[data-module-tab="wavetable"]')), 15000);
await page.click('[data-module-tab="wavetable"] [data-testid=track-label]');
await view("graph");
const card = '[data-module="wavetable"] [data-node="wavetable"]';
check(await until(async () => !!(await page.$(`${card} [data-group]`)), 15000), "the Wavetable's card shows its groups");
await page.$eval(card, (e) => e.scrollIntoView({ block: "start", inline: "center" }));
await sleep(200);
const groups = await page.$$eval(`${card} [data-group]`, (els) => els.map((e) => e.textContent));
check(JSON.stringify(groups) === '["Oscillators","Filter","Envelopes","LFO","Unison","More"]', `its groups, in order, More last: ${JSON.stringify(groups)}`);
await linted(card, "the Wavetable's card");
await page.screenshot({ path: `${ROOT}/wavetable-card.png` });

// ---------------------------------------------------------------- a table from its menu

const fresh = read(file);
const steps = await undoTitle();
// Its tables and files are named by their labels, never `TableA` (CARD-34).
const rows = await page.$$eval(`${card} [data-setting] > span:first-child`, (els) => els.map((e) => e.textContent));
check(rows.includes("Table A") && rows.includes("Table B") && !rows.some((r) => /Table[AB]|File[AB]/.test(r ?? "")), `its settings by their labels: ${JSON.stringify(rows)}`);
await page.click(`${card} [data-setting="tableA"] [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const options = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(options) === '["basic","pulse","vowels","harmonics","digital"]', `table A's menu lists the built-in tables: ${JSON.stringify(options)}`);
await linted("[data-testid=setting-options]", "the table's menu");
await page.click('[data-testid=setting-options] [data-option="vowels"]');
const vowels = fresh.replace('wavetable(lane.notes("notes"))', 'wavetable(lane.notes("notes"), { tableA: "vowels" })');
check(await until(() => read(file) === vowels), `picking vowels writes it into the call: ${read(file).match(/wavetable\(lane[^\n]*/)?.[0]}`);
check(await until(async () => (await undoTitle()) !== steps && (await undoTitle()) === "Undo Wavetable table A (⌘Z)"), `one undo step, named by its label (CARD-34): ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read(file) === fresh), "⌘Z takes the code back, byte for byte");
await key("Meta+Shift+z");
check(await until(() => read(file) === vowels), "and ⇧⌘Z puts vowels back");

// ---------------------------------------------------------------- position, turned

// Right after ⇧⌘Z, which changed this call: the drag waits for the page to show the redo, and goes
// from it (AGENT-2), never refused ("the code changed around this control …").
const position = `${card} [data-knob="position A"]`;
check(await until(async () => !!(await page.$(position))), "position A is a knob");
await until(async () => (await page.$eval(position, (e) => e.getAttribute("aria-disabled")).catch(() => "true")) !== "true");
await page.$eval(`${position} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
const b = await box(`${position} svg`);
await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
await page.mouse.down();
for (let i = 1; i <= 12; i++) await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2 - (30 * i) / 12);
await page.mouse.up();
check(await until(() => /positionA: 0\.1\b/.test(read(file))), `30 px up moves it a tenth of its range: ${read(file).match(/wavetable\(lane[^\n]*/)?.[0]}`);
const shownToasts = await toasts();
check(refused.length === 0 && !shownToasts.some((t) => /changed around this control|Nothing was changed|The song changed/.test(t)), `the drag right after ⇧⌘Z is not refused: ${JSON.stringify({ refused, shownToasts })}`);

// ---------------------------------------------------------------- the pictures

for (const corner of ["attack", "decay", "release"]) {
  check(!!(await page.$(`${card} [data-view="amp"] [data-handle="${corner}"]`)), `the amp envelope's picture draws its ${corner} corner`);
}
check(!!(await page.$(`${card} [data-view="filterEnvelope"] [data-handle="attack"]`)), "the filter envelope has its picture");
check(!!(await page.$(`${card} [data-view="curve"] [data-handle="peak"]`)), "and the filter its curve");
await linted(`${card} [data-view="amp"]`, "the amp envelope's picture");

// ---------------------------------------------------------------- a note: the meter moves

const meter = '[data-testid=timeline] [data-track="wavetable"] [role=meter]';
const level = () => page.$eval(meter, (e) => Number(e.getAttribute("aria-valuenow") ?? -60)).catch(() => -60);
const inject = (body) => post("/midi/inject", { device: "Test Keys", ...body });
check((await inject({ plugged: true })).status === 200, "a MIDI keyboard plugged in");
check(await until(async () => (await api("")).body.midi?.target?.module === "wavetable", 3000), "its keys play the selected track, the Wavetable");
check((await level()) <= -60, `its meter rests: ${await level()} dB`);
await inject({ bytes: [0x90, 60, 100] });
check(await until(async () => (await level()) > -40, 4000), `a key down plays a note: the meter moves (${await level()} dB)`);
await inject({ bytes: [0x80, 60, 0] });
check(await until(async () => (await level()) <= -60, 6000), `let go, the note ends and the meter falls (${await level()} dB)`);

done(errors);
