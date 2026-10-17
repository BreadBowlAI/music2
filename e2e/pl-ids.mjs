// Polish A5: change a track's ID (design-8b §11.4, taste.md:120; the polish plan's C4, moved to
// lane A by X38). A right-click on tracks/bass.ts in the Code view's file list offers "Change Bass's
// track ID…"; the dialog holds the ID in a mono field and checks it as you type, in ID words; Space
// and M in it never reach the page; Tab goes round it; Esc writes nothing. Changing bass to low
// rewrites its track() call and its channel, moves data/bass/ to data/low/ and layout.json's entry,
// with a toast; Bass keeps its name, colour, mute and selection; one Undo puts every byte back and
// the selection follows back. The header menu has no ID row, and no words call an ID a key.
//
//   node e2e/pl-ids.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { check, done, open, PROJ, read, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, state, toasts, key, view } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });

/** Every project file but .music2's, by path. */
function files(dir = PROJ, rel = "") {
  const out = {};
  for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    const r = path.join(rel, e.name);
    if (r === ".music2") continue;
    if (e.isDirectory()) Object.assign(out, files(dir, r));
    else out[r] = fs.readFileSync(path.join(dir, r), "utf8");
  }
  return out;
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const DIALOG = "[data-testid=track-id-dialog]";
const FIELD = "[data-testid=track-id-field]";
const problem = () => page.$eval("[data-testid=track-id-problem]", (e) => e.textContent).catch(() => null);
const canChange = () => page.$eval("[data-testid=track-id-change]", (e) => !e.disabled).catch(() => null);
const typeId = async (id) => {
  await page.click(FIELD, { count: 3 });
  await page.keyboard.press("Backspace");
  await page.keyboard.type(id);
};
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};

// Bass gets a colour of its own (layout.json) and is muted while listening, so both can be seen to follow.
const session = (await state()).session;
check((await post("/layout", { session, colour: { module: "bass", colour: "green" } })).status === 200, "Bass is coloured green (layout.json)");
await post("/overlay", { session, mute: { bass: true } });
await page.click('[data-module-tab="bass"] [data-testid=track-label]');
await until(async () => (await page.$eval('[data-track="bass"]', (e) => e.dataset.selected).catch(() => null)) === "true");
const before = files();

// ---------------------------------------------------------------- the file list's menu

await view("code");
await page.waitForSelector('[data-testid=code] [data-file="tracks/bass.ts"]', { timeout: 15000 });
await page.click('[data-testid=code] [data-file="tracks/bass.ts"]', { button: "right" });
const rows = await page.waitForSelector("[data-testid=file-menu]", { timeout: 2000 }).then(() => page.$$eval("[data-testid=file-menu] [role=menuitem]", (els) => els.map((e) => e.textContent))).catch(() => []);
check(same(rows, ["Change Bass's track ID…"]), `a right-click on tracks/bass.ts offers Change Bass's track ID… (${rows})`);
await page.click('[data-testid=code] [data-file="song.ts"]', { button: "right" });
await new Promise((r) => setTimeout(r, 300));
check(!(await page.$("[data-testid=file-menu]")), "song.ts, which makes no track, has no such menu");
await page.click('[data-testid=code] [data-file="tracks/bass.ts"]', { button: "right" });
await page.waitForSelector("[data-testid=file-menu]");
await page.click("[data-testid=file-menu] [data-testid=change-track-id]");

// ---------------------------------------------------------------- the dialog

check(!!(await page.waitForSelector(DIALOG, { timeout: 2000 }).catch(() => null)), "it opens the dialog");
const d = await page.$eval(DIALOG, (e) => ({
  title: e.querySelector("h2")?.textContent,
  value: e.querySelector("input")?.value,
  mono: !!e.querySelector("input[data-code]") && /mono/i.test(getComputedStyle(e.querySelector("input")).fontFamily),
  focused: document.activeElement === e.querySelector("input"),
  selected: e.querySelector("input").selectionStart === 0 && e.querySelector("input").selectionEnd === 4,
  text: e.innerText,
}));
check(d.title === "Change Bass's track ID" && d.value === "bass" && d.mono, `titled for Bass, a mono field holding bass (${JSON.stringify(d)})`);
check(d.focused && d.selected, "the field has the focus, its ID selected");
check(d.text.includes("The track ID names this track in code and files its data (data/bass/). Changing it moves both, as one change you can undo."), "its help says what an ID is");
check(!/\bkey\b/i.test(d.text), "no word in it calls the ID a key");
check((await canChange()) === false, "Change ID is off while the ID is as it was");
await linted(DIALOG, "the dialog");

await typeId("lead vox");
check(await until(async () => (await problem()) === "lead vox isn't an ID: IDs are lowercase letters, digits and hyphens, such as lead-vox"), `"lead vox" is checked as you type: ${await problem()}`);
check((await canChange()) === false && (await page.$eval(FIELD, (e) => getComputedStyle(e).borderColor)) === "rgb(175, 61, 54)", "Change ID is off, and the field's border is danger");
check(!(await state()).playing, "Space typed in the field didn't play the song");
await typeId("hats");
check(await until(async () => (await problem()) === "hats is taken: another track or its data has that ID"), `"hats" is taken: ${await problem()}`);
await typeId("low");
check(await until(async () => (await problem()) === null && (await canChange()) === true), "low is free: no problem shown, and Change ID is on");
await page.keyboard.press("m");
check(await until(async () => (await page.$eval(FIELD, (e) => e.value)) === "lowm") && same((await state()).overlays.mute, ["bass"]), "M types an m in the field and mutes nothing");
await page.keyboard.press("Backspace");
// Tab goes round the field, Cancel and Change ID.
const order = [];
for (let i = 0; i < 3; i++) {
  await page.keyboard.press("Tab");
  order.push(await page.evaluate(() => document.activeElement?.dataset.testid ?? document.activeElement?.tagName));
}
check(same(order, ["track-id-cancel", "track-id-change", "track-id-field"]), `Tab goes round the field and the buttons: ${order}`);
await key("Escape");
check(await until(async () => !(await page.$(DIALOG))) && same(files(), before), "Esc closes it and writes nothing");

// ---------------------------------------------------------------- the change

await page.click('[data-testid=code] [data-file="tracks/bass.ts"]', { button: "right" });
await page.waitForSelector("[data-testid=file-menu]");
await page.click("[data-testid=file-menu] [data-testid=change-track-id]");
await page.waitForSelector(FIELD);
await typeId("low");
await until(async () => (await canChange()) === true);
const rev = (await state()).view.revision;
await page.keyboard.press("Enter");
check(await until(() => read("tracks/bass.ts").includes('track("low", { name: "Bass" }')), `Return writes track("low", … in tracks/bass.ts (${read("tracks/bass.ts").split("\n").find((l) => l.includes("track("))})`);
check(read("song.ts") === before["song.ts"].replace("      bass: duck(bass.out", "      low: duck(bass.out"), "its channel is called low now; its import and binding stay");
check(fs.existsSync(`${PROJ}/data/low/notes.json`) && !fs.existsSync(`${PROJ}/data/bass`), "data/bass/ moved to data/low/");
const layout = JSON.parse(read("layout.json"));
check(layout.order.includes("low") && !layout.order.includes("bass") && layout.tracks.low?.colour === "green", `layout.json's order and colour follow: ${JSON.stringify(layout)}`);
check((await state()).view.revision === rev + 1, "one revision");
check(await until(async () => (await toasts()).some((t) => t.includes("Bass's ID is low now"))), `a toast says "Bass's ID is low now" (${await toasts()})`);
check(!(await page.$(DIALOG)), "the dialog closes");
const after = await state();
check(after.timeline.tracks.find((t) => t.module === "low")?.label === "Bass", "the track is still called Bass");
check(after.timeline.tracks.find((t) => t.module === "low")?.colour === "green", "and is still green");
check(same(after.overlays.mute, ["low"]), `its mute while listening follows it (${JSON.stringify(after.overlays.mute)})`);
await view("graph");
check(await until(async () => (await page.$eval('[data-track="low"]', (e) => e.dataset.selected).catch(() => null)) === "true"), "and it is still the selected track");
check(!(await toasts()).some((t) => /\bkey\b/i.test(t)), "nor does its toast");

// ---------------------------------------------------------------- undo

await key("Meta+z");
check(await until(() => same(files(), before)), "one Undo puts every byte back: code, data and layout");
check(await until(async () => (await page.$eval('[data-track="bass"]', (e) => e.dataset.selected).catch(() => null)) === "true"), "and the selection follows Bass back");
check(await until(async () => same((await state()).overlays.mute, ["bass"])), "and its mute");

// ---------------------------------------------------------------- the header menu

await page.hover('[data-module-tab="bass"]');
await page.click('[data-track-menu="bass"]');
await page.waitForSelector("[data-testid=track-menu]");
const header = await page.$$eval("[data-testid=track-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(!header.some((r) => /\bID\b|\bkey\b/i.test(r)), `the header menu has no ID row (${header.join(" | ")})`);
await key("Escape");

await browser.close();
done(errors);
