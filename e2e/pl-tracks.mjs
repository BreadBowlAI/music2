// Polish C5: the delete question's keys (design-8b §3.5, taste.md §2.14), as the export question's
// since af99eec: Return presses the button with the focus ring (Delete at first), Space presses a
// focused button and does nothing elsewhere, Tab and Shift+Tab go round the dialog's buttons, Esc
// cancels, and no key reaches the page behind it (Space would play, M would mute).
//
//   node e2e/pl-tracks.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint as lintIn } from "./lintlib.mjs";

const { page, errors, api, state, toasts, key } = await open(1440, 900);
const exists = (f) => fs.existsSync(`${PROJ}/${f}`);
const files = ["song.ts", "tracks/kick.ts", "data/kick/hits.json", "tracks/bass.ts"];
const snapshot = () => Object.fromEntries(files.map((f) => [f, exists(f) ? read(f) : null]));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const dialog = () => page.$("[data-testid=delete-dialog]");
const focused = () => page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.tagName ?? null);
const playing = async () => (await api("/status")).body.playing;
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 5000);
const shiftTab = async () => {
  await page.keyboard.down("Shift");
  await page.keyboard.press("Tab");
  await page.keyboard.up("Shift");
};

/** The taste lint over what is inside `sel` (e2e/lintlib.mjs, as taste.mjs runs it). */
const lint = (sel) => page.evaluate(lintIn, sel);

/** Ask to delete Kick from its header menu: Bass ducks to it, so the question opens. */
const ask = async () => {
  await page.hover('[data-module-tab="kick"]');
  await page.click('[data-track-menu="kick"]');
  await page.waitForSelector("[data-testid=track-menu]", { timeout: 2000 });
  await page.evaluate(() => [...document.querySelectorAll("[data-testid=track-menu] [role=menuitem]")].find((e) => e.textContent.startsWith("Delete"))?.click());
  return !!(await page.waitForSelector("[data-testid=delete-dialog]", { timeout: 3000 }).catch(() => null));
};

await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');
const before = snapshot();
const beat = async () => (await api("/status")).body.beat;
await api("/seek", { method: "POST", body: JSON.stringify({ beat: 8 }) });
await until(async () => Math.abs((await beat()) - 8) < 0.01);

// ---------------------------------------------------------------- what it says, and where the focus is

check(await ask(), "Kick's Delete opens the question: Bass ducks to Kick");
check((await focused()) === "delete-confirm", `the Delete button has the focus at first: ${await focused()}`);
const bad = await lint("[data-testid=delete-dialog]");
check(bad.length === 0, `the question keeps to the design system: ${bad.slice(0, 4).join(", ")}`);
await page.screenshot({ path: `${ROOT}/pl-delete-dialog.png` });

// ---------------------------------------------------------------- no key reaches the page behind it

await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("Space");
await sleep(500);
check(!(await playing()) && !!(await dialog()) && same(snapshot(), before), "Space with the focus on nothing does nothing: nothing plays, nothing is deleted");
await page.keyboard.press("m");
await sleep(400);
check(same((await state()).overlays.mute, []) && !!(await dialog()), "M mutes nothing behind the question");

// ---------------------------------------------------------------- Tab goes round the two buttons

const tabs = [];
for (const back of [false, false, true, true]) {
  if (back) await shiftTab();
  else await page.keyboard.press("Tab");
  tabs.push(await focused());
}
check(same(tabs, ["delete-cancel", "delete-confirm", "delete-cancel", "delete-confirm"]), `Tab and Shift+Tab go round the two buttons, never behind the question: ${tabs.join(" → ")}`);

// ---------------------------------------------------------------- Return and Space press the focused button

await shiftTab();
check((await focused()) === "delete-cancel", "Shift+Tab moves the focus ring to Cancel");
await page.keyboard.press("Enter");
await sleep(800);
check(exists("tracks/kick.ts") && same(snapshot(), before), `Return on Cancel deletes nothing: ${exists("tracks/kick.ts") ? "kick.ts is there" : "kick.ts is gone"}`);
check(!(await dialog()), "and closes the question");
check(!(await playing()) && Math.abs((await beat()) - 8) < 0.01, `and Return doesn't reach the page either: the start point stays at beat 8 (${await beat()})`);

check(await ask(), "asked again");
await shiftTab();
await page.keyboard.press("Space");
await sleep(800);
check(!(await dialog()) && same(snapshot(), before) && !(await playing()), "Space on Cancel cancels too: nothing plays, nothing is deleted");

check(await ask(), "asked again");
await page.keyboard.press("Escape");
await sleep(400);
check(!(await dialog()) && same(snapshot(), before), "Esc cancels");

check(await ask(), "asked again");
await page.keyboard.press("Enter");
check(await until(() => !exists("tracks/kick.ts")), "Return on Delete deletes Kick");
check(!read("song.ts").includes("duck(") && read("song.ts").includes("      bass: bass.out,\n"), "and the duck comes off: bass: bass.out");
check(await toasted("Deleted Kick and its Duck"), "the toast says what went with it");
await key("Meta+z");
check(await until(() => same(snapshot(), before)), "⌘Z puts every file back, byte for byte");

done(errors);
