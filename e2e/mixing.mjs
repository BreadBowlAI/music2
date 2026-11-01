// @m2/mixing's parametric EQ from + Effect (PACK-26, PACK-10): added after the keys' pluck, it
// writes its call, its import, the pack's [dependencies] line and its lock in one edit. Its card
// shows its eight bands under their headings, and no picture (no view draws an EQ's whole curve
// yet). A band's shape is a menu: picking one writes its name, one undo step, and ⌘Z takes the code
// back byte for byte; turning band 3's frequency knob writes it, one undo step, undone as one. One
// more ⌘Z takes the EQ out, with its import, dependency and lock. Each state linted.
//
//   node e2e/mixing.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, show, key, box, caughtUp } = await open(1440, 900);

/** The lint over what is inside `sel`. */
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
const files = () => ["tracks/keys.ts", "music2.toml", "music2.lock"].map(read).join("\n----\n");
const before = files();

// ---------------------------------------------------------------- + Effect

await show("keys");
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector('[data-effect-browser] [data-effect="eq"]');
const group = await page.$eval('[data-effect-browser] [data-effect="eq"]', (e) => e.closest("[data-category]")?.dataset.category);
check(group === "EQ", `the parametric EQ is listed under EQ: ${group}`);
await page.click('[data-effect-browser] [data-effect="eq"]');
const added = 'eq(pluck(lane.notes("chords")))';
check(await until(() => read("tracks/keys.ts").includes(added), 15000), `the EQ wraps the pluck: ${read("tracks/keys.ts").match(/=> .*/)?.[0]}`);
check(read("tracks/keys.ts").includes('import { eq } from "@m2/mixing";'), "and imports it from @m2/mixing");
check(/^"@m2\/mixing" = "1\.0"$/m.test(read("music2.toml")), `music2.toml lists the pack: ${read("music2.toml").match(/"@m2\/mixing".*/)?.[0]}`);
check(/name = "@m2\/mixing"\nversion = "1\.0\.0"/.test(read("music2.lock")), "music2.lock locks it");
const withEq = files();

// ---------------------------------------------------------------- its card: eight bands, band 8's curve

const card = '[data-module="keys"] [data-node="eq"]';
check(await until(async () => !!(await page.$(`${card} [data-group]`)), 15000), "the EQ's card shows its groups");
const groups = await page.$$eval(`${card} [data-group]`, (els) => els.map((e) => e.textContent));
const bands = Array.from({ length: 8 }, (_, i) => `Band ${i + 1}`);
check(JSON.stringify(groups) === JSON.stringify(bands), `its eight bands, in order: ${JSON.stringify(groups)}`);
const rows = await page.$$eval(`${card} [data-node-body] > *`, (els) => els.map((e) => e.dataset.group ?? e.dataset.knob ?? e.dataset.view ?? "?"));
const at = (x) => rows.indexOf(x);
check(
  at("Band 3") < at("On 3") && at("On 3") < at("Shape 3") && at("Shape 3") < at("Frequency 3") && at("Gain 3") < at("Q 3") && at("Q 3") < at("Band 4"),
  `each band's values under its heading: ${rows.slice(at("Band 3"), at("Band 4") + 1).join(", ")}`,
);
check(at("Band 8") < at("Q 8") && at("Q 8") === rows.length - 1, `band 8 ends with its Q: ${rows.slice(at("Band 8")).join(", ")}`);
const kinds = await page.$$eval(`${card} [data-knob="Shape 3"], ${card} [data-knob="On 3"], ${card} [data-knob="Gain 3"]`, (els) => els.map((e) => `${e.dataset.knob} ${e.dataset.kind} ${e.dataset.value}`));
check(JSON.stringify(kinds) === JSON.stringify(["On 3 switch true", 'Shape 3 choice "bell"', "Gain 3 number 0"]), `a band's switch, menu and knob: ${JSON.stringify(kinds)}`);
check(!(await page.$(`${card} [data-view]`)), "it draws no picture: no view draws an EQ's whole curve yet");
await linted(card, "the EQ's card");
await page.screenshot({ path: `${process.argv[2]}/mixing-eq.png` });

// ---------------------------------------------------------------- a band's shape (CARD-20, CARD-21)

const shape = `${card} [data-knob="Shape 3"]`;
await seen(shape);
await page.click(`${shape} [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const options = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(options) === '["low cut","low shelf","bell","high shelf","high cut"]', `its menu lists the shapes: ${JSON.stringify(options)}`);
await linted("[data-testid=setting-options]", "the shape's menu");
await page.click('[data-testid=setting-options] [data-option="high shelf"]');
const shelved = withEq.replace(added, `eq(pluck(lane.notes("chords")), { shape3: "high shelf" })`);
check(await until(() => files() === shelved), `picking high shelf writes its name: ${read("tracks/keys.ts").match(/=> .*/)?.[0]}`);
check(await stepIs("Parametric EQ shape 3"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await page.$eval(shape, (e) => e.dataset.value)) === '"high shelf"'), "the menu shows high shelf");
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => files() === withEq), "⌘Z takes the code back, byte for byte");

// ---------------------------------------------------------------- a band's frequency knob (PKG-3)

// A drag goes from the revision the page shows, and ⌘Z changed this call: the page shows it first.
await caughtUp();
const freq = `${card} [data-knob="Frequency 3"]`;
const grip = await seen(`${freq} svg`);
await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
await page.mouse.down();
for (let i = 1; i <= 8; i++) await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2 - i * 4);
await page.mouse.up();
const turned = () => read("tracks/keys.ts").match(/eq\(pluck\(lane\.notes\("chords"\)\), \{ freq3: ([0-9.]+) \}\)/);
check(await until(() => !!turned()), `turning band 3's frequency up writes it into the call: ${read("tracks/keys.ts").match(/=> .*/)?.[0]}`);
check(Number(turned()?.[1]) > 250, `above its 250 Hz: ${turned()?.[1]} Hz`);
check(await stepIs("Parametric EQ frequency 3"), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => files() === withEq), "⌘Z takes it back, byte for byte");

// ---------------------------------------------------------------- undo the EQ itself

await key("Meta+z");
check(await until(() => files() === before), "one more ⌘Z takes out the EQ, its import, its dependency and its lock");
check(await until(async () => !(await page.$(card))), "and its card");

done(errors);
