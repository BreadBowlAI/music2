// An effect card's ⋯ menu (next, lane D): turn an effect off and on again (D1). Each is one undo
// step, written in the effect's own call; the card says so at once, so no toast does.
//
//   node e2e/nx-effects.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, toasts, show, view, key, box } = await open(1440, 900);
const orig = { keys: read("tracks/keys.ts"), song: read("song.ts") };

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const card = (module, label) => `[data-module="${module}"] [data-node="${label}"]`;
/** The ⋯ menu of `label`'s card in `module`, open; its rows by their words. */
const menu = async (module, label) => {
  if (!(await page.$("[data-testid=effect-menu]"))) {
    const button = await page.$(`${card(module, label)} [data-effect-menu]`);
    if (!button) return [];
    await button.click();
  }
  await until(async () => !!(await page.$("[data-testid=effect-menu]")), 3000);
  return page.$$eval("[data-testid=effect-menu] [role=menuitem]", (rows) => rows.map((r) => ({ text: r.textContent, title: r.title, disabled: r.disabled })));
};
const closeMenu = async () => {
  if (await page.$("[data-testid=effect-menu]")) await key("Escape");
  await until(async () => !(await page.$("[data-testid=effect-menu]")));
};
const row = (words) =>
  page.evaluate((words) => {
    const r = [...document.querySelectorAll("[data-testid=effect-menu] [role=menuitem]")].find((x) => x.textContent === words);
    r?.click();
    return !!r;
  }, words);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (b) => b.title);

// ---------------------------------------------------------------- D1: turn an effect off and on

await show("keys");
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector('[data-effect="reverb"]');
await page.click('[data-effect="reverb"]');
const reverbed = 'reverb(pluck(lane.notes("chords")), { size: 0.6, damping: 0.5, mix: 0.25 })';
check(await until(() => read("tracks/keys.ts").includes(reverbed)), "Reverb added after the pluck");
await page.waitForSelector(card("keys", "reverb"));
await until(async () => !(await toasts()).length, 9000);
const added = read("tracks/keys.ts");

const rows = await menu("keys", "reverb");
check(rows.map((r) => r.text).join() === "Turn off", `the Reverb card's ⋯ menu: ${rows.map((r) => r.text).join()}`);
check(rows[0]?.title === "Turn Reverb off (keeps its settings)" && !rows[0]?.disabled, `its tooltip: ${rows[0]?.title}`);
await linted("[data-testid=effect-menu]", "the ⋯ menu");
const before = await box(card("keys", "reverb"));
await row("Turn off");
const off = added.replace(reverbed, 'reverb(pluck(lane.notes("chords")), { size: 0.6, damping: 0.5, mix: 0.25, bypass: true })');
check(await until(() => read("tracks/keys.ts") === off), "Turn off writes bypass: true into the call, and nothing else");
check(await until(async () => !!(await page.$(`${card("keys", "reverb")}[data-bypassed]`))), "the card is off");
const look = await page.$eval(card("keys", "reverb"), (e) => {
  const title = e.querySelector("[data-node-title]");
  const body = e.querySelector("[data-node-body]");
  const muted = getComputedStyle(document.documentElement).getPropertyValue("--color-fg-muted");
  const probe = document.createElement("span");
  probe.style.color = `var(--color-fg-muted)`;
  document.body.append(probe);
  const want = getComputedStyle(probe).color;
  probe.remove();
  return { text: title?.textContent, color: getComputedStyle(title).color, want, muted: !!muted, opacity: body && getComputedStyle(body).opacity };
});
check(look.text === "Reverb · Off", `its title says so: ${look.text}`);
check(look.color === look.want, `in fg-muted: ${look.color} (${look.want})`);
check(look.opacity === "0.5", `its body is half seen: ${look.opacity}`);
const after = await box(card("keys", "reverb"));
check(!!before && !!after && Math.abs(before.x - after.x) < 1 && Math.abs(before.y - after.y) < 1, `the card keeps its place: ${JSON.stringify([before?.x, before?.y])} → ${JSON.stringify([after?.x, after?.y])}`);
const reverbId = await page.$eval(card("keys", "reverb"), (e) => e.dataset.nodeId);
const dashed = await page.$$eval(`[data-module="keys"] path[data-from="${reverbId}"]`, (ps) => ps.map((p) => [p.dataset.bypassed, p.getAttribute("stroke-dasharray")].join()));
check(dashed.length === 1 && dashed[0] === "true,6 5", `the line out of it is dashed: ${dashed}`);
check((await toasts()).length === 0, `no toast: the card says it (${(await toasts()).join(" | ")})`);
check(/^Undo turning Reverb off \(/.test(await undoTitle()), `Undo says what it undoes: ${await undoTitle()}`);
await linted(card("keys", "reverb"), "the card turned off");
await menu("keys", "reverb");
check((await page.$$eval("[data-testid=effect-menu] [role=menuitem]", (r) => r.map((x) => `${x.textContent}|${x.title}`))).join() === "Turn on|Turn Reverb back on", "its menu says Turn on now");
await closeMenu();

// The code editor takes `bypass` as an option of every effect: no marker.
await view("code");
await page.waitForSelector("[data-testid=code] .monaco-editor");
await sleep(3000);
check(!(await page.$("[data-testid=code] .squiggly-error")), "the code editor marks nothing in it");
await view("graph");
await page.waitForSelector(card("keys", "reverb"));

await key("Meta+z");
check(await until(() => read("tracks/keys.ts") === added), "⌘Z takes it back");
check(await until(async () => !(await page.$(`${card("keys", "reverb")}[data-bypassed]`))), "the card is on again");
await menu("keys", "reverb");
await row("Turn off");
check(await until(() => read("tracks/keys.ts") === off), "off again");
await until(async () => !!(await page.$(`${card("keys", "reverb")}[data-bypassed]`)));
await menu("keys", "reverb");
await row("Turn on");
check(await until(() => read("tracks/keys.ts") === added), "Turn on takes `bypass: true` out, and the options object is as it was");
check(await until(async () => /^Undo turning Reverb on \(/.test(await undoTitle())), `Undo: ${await undoTitle()}`);

// A PC page's menu says the same.
{
  const pc = await open(1440, 900, { pc: true });
  await pc.show("keys");
  await pc.page.waitForSelector(card("keys", "reverb"));
  await pc.page.click(`${card("keys", "reverb")} [data-effect-menu]`);
  await pc.page.waitForSelector("[data-testid=effect-menu]");
  const words = await pc.page.$$eval("[data-testid=effect-menu] [role=menuitem]", (r) => r.map((x) => x.textContent));
  check(words.join() === "Turn off", `a PC page's menu: ${words.join()}`);
  check(pc.errors.length === 0, `no console errors on the PC page ${pc.errors.slice(0, 2).join(" | ")}`);
  await pc.browser.close();
}

check(read("song.ts") === orig.song, "song.ts is untouched");
done(errors);
