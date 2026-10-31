// Arm, punch and takes (track recording: MON-9 to MON-15, gaps #24 and #25). The run's server has
// an injected sound input (run.mjs: M2_AUDIO_INPUT=inject, M2_AUDIO_INPUTS="Test mic,USB mic"):
//   - + Track ▸ Audio track; its header's R arms it: lit in `danger`, no undo step, no file
//     changed; right-click lists the input's channels; arming Keys then Bass leaves Bass armed
//     alone (one notes track at a time), and the keyboard piano plays it;
//   - the piano's − and = step its velocity, and C still loops;
//   - Record's menu: Punch and Overdub, kept in settings.json; with Punch on the loop's brace on
//     the ruler is red; R from beat 2 with the loop's range at 4 to 6, off: the take records only
//     4 to 6, one clip there, and the song plays on;
//   - the loop on at 8 to 10, Punch off: R goes round three times; R again ends it: one clip over
//     the loop, its file's note listing each pass, the third playing; its menu's Takes lists them,
//     the third ticked; Take 1 plays the first, one undo step ("Undo Audio take 1"); the lint.
//
//   node e2e/rec-takes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { lint } from "./lintlib.mjs";
import { HOME, PROJ, ROOT, check, done, open, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, key, toasts } = await open(1440, 900);
const settings = () => {
  try {
    return JSON.parse(fs.readFileSync(`${HOME}/.music2/settings.json`, "utf8"));
  } catch {
    return {};
  }
};
const status = async () => (await api("/status")).body;
const record = async () => (await state()).record ?? null;
const armed = async () => (await state()).arm?.armed ?? {};
const inject = (body) => api("/input/inject", { method: "POST", body: JSON.stringify(body) });
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const blur = () => page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
const press = async (k) => {
  await blur();
  await page.keyboard.press(k);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const clips = () => {
  try {
    return JSON.parse(fs.readFileSync(`${PROJ}/data/audio/clips.json`, "utf8")).clips;
  } catch {
    return [];
  }
};
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const menuRows = (sel) => page.$$eval(`${sel} [role^=menuitem]`, (els) => els.map((e) => ({ id: e.dataset.menuItem, text: e.textContent ?? "", checked: e.getAttribute("aria-checked") })));

// ---------------------------------------------------------------- arm (MON-9)

await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list] input");
await page.keyboard.type("audio");
await page.keyboard.press("Enter");
check(await until(async () => (await state()).timeline.tracks.some((t) => t.module === "audio"), 5000), "+ Track ▸ Audio track adds one");
const r = '[data-arm="audio"]';
check(await until(() => page.$(r), 2000), "its header has an R");
const rev0 = (await state()).view.revision;
const undo0 = await undoTitle();
await page.click(r);
check(await until(async () => (await armed()).audio === "clips", 2000), `R arms it on its audio lane (${JSON.stringify(await armed())})`);
const lit = await page.$eval(r, (e) => ({ pressed: e.getAttribute("aria-pressed"), bg: getComputedStyle(e).backgroundColor, color: getComputedStyle(e).color }));
check(lit.pressed === "true" && lit.bg === "rgb(175, 61, 54)" && lit.color === "rgb(255, 255, 255)", `lit: danger, a white R (${JSON.stringify(lit)})`);
check((await state()).view.revision === rev0 && (await undoTitle()) === undo0, "no edit, no undo step");
await page.click(r, { button: "right" });
check(await until(() => page.$("[data-testid=arm-menu]"), 2000), "right-click opens its input's menu");
const inputs = await menuRows("[data-testid=arm-menu]");
check(inputs.map((i) => i.text).join("|") === "Input 1 (mono)" && inputs[0].checked === "true", `the mono test input offers channel 1, ticked: ${JSON.stringify(inputs)}`);
await linted("[data-testid=arm-menu]", "the input menu");
await page.keyboard.press("Escape");
await page.click('[data-arm="keys"]');
check(await until(async () => (await armed()).keys === "chords", 2000), "R on Keys arms it on its notes lane");
await page.click('[data-arm="bass"]');
check(await until(async () => JSON.stringify(await armed()) === JSON.stringify({ audio: "clips", bass: "notes" }), 2000), `arming Bass disarms Keys: one notes track at a time (${JSON.stringify(await armed())})`);
await linted('[data-module-tab="bass"]', "an armed header");
await page.screenshot({ path: `${ROOT}/rec-takes-arm.png`, clip: { x: 0, y: 80, width: 520, height: 360 } });

// ---------------------------------------------------------------- the piano plays the armed track; its velocity (MON-15)

await page.click('[data-module-tab="kick"] [data-testid=track-label]');
await blur();
await key("Meta+k");
check(await until(() => page.$("[data-testid=keyboard-piano]"), 2000), "⌘K shows the keyboard piano");
const plays = await page.$eval("[data-testid=piano-plays]", (e) => e.textContent).catch(() => null);
check(plays === "Plays Bass", `it plays the armed notes track, not the selected Kick (${plays})`);
const velocity = () => page.$eval("[data-testid=piano-velocity]", (e) => ({ v: Number(e.dataset.velocity), text: e.textContent })).catch(() => null);
check((await velocity())?.text === "−80%=", `velocity 80% at first (${JSON.stringify(await velocity())})`);
await press("-");
await press("-");
check(await until(async () => (await velocity())?.v === 0.6, 1000), `− twice: 60% (${JSON.stringify(await velocity())})`);
await press("=");
check(await until(async () => (await velocity())?.v === 0.7, 1000), `= once: 70% (${JSON.stringify(await velocity())})`);
await press("c");
check(await until(async () => (await status()).loop?.on === true, 2000), "C still loops while the piano is on");
await press("c");
await until(async () => (await status()).loop?.on !== true, 2000);
await key("Meta+k");
await post("/arm", { session: (await state()).session, module: "bass", armed: false });
check(await until(async () => JSON.stringify(await armed()) === JSON.stringify({ audio: "clips" }), 2000), "Bass disarmed: the audio track alone");

// ---------------------------------------------------------------- Punch (MON-10)

const openRecordMenu = async () => {
  await page.click("[data-testid=record]", { button: "right" });
  return until(() => page.$("[data-testid=record-menu]"), 2000);
};
check(await openRecordMenu(), "right-click on Record opens its options");
const opts = await menuRows("[data-testid=record-menu]");
check(["punch", "overdub"].every((id) => opts.some((o) => o.id === id && o.checked === "false")), `Punch and Overdub are there, off: ${JSON.stringify(opts.map((o) => o.text))}`);
await linted("[data-testid=record-menu]", "Record's menu");
await page.click("[data-testid=record-menu] [data-menu-item=punch]");
check(await until(() => settings().record?.punch === true, 3000), `Punch on, kept: ${JSON.stringify(settings().record)}`);
await openRecordMenu();
await page.click("[data-testid=record-menu] [data-menu-item=count_in]");
await until(() => settings().record?.count_in === false, 3000);

await post("/loop", { start: 4, end: 6, on: false });
check(await until(() => page.$("[data-testid=loop-range][data-punch]"), 2000), "the loop's brace on the ruler is red while Punch is on");
await post("/seek", { beat: 2 });
await sleep(200);
await press("r");
check(await until(async () => (await record())?.from === 4 && (await record())?.until === 6, 3000), `R records from the loop's start to its end (${JSON.stringify(await record())})`);
await inject({ tone: { hz: 440, seconds: 3, gain: 0.5 } });
check(await until(() => clips().length === 1, 8000), `the take ends at the range's end by itself: one clip (${JSON.stringify(clips())})`);
const p = clips()[0];
check(p?.start === 4 && Math.abs(p.length - 2) < 1e-6, `it covers 4 to 6 exactly (${p?.start}, ${p?.length})`);
check((await status()).playing && (await status()).beat > 6, "the song plays on past the range");
await press("Space");
await until(async () => !(await status()).playing, 2000);

// ---------------------------------------------------------------- takes over the loop (MON-11)

await openRecordMenu();
await page.click("[data-testid=record-menu] [data-menu-item=punch]");
await until(() => settings().record?.punch === false, 3000);
await post("/loop", { start: 8, end: 10, on: true });
await post("/seek", { beat: 8 });
await sleep(200);
await press("r");
check(await until(async () => (await record())?.phase === "take", 3000), "R records over the loop");
await inject({ tone: { hz: 220, seconds: 6, gain: 0.5 } });
check(await until(async () => ((await record())?.pass ?? 0) >= 3, 8000), `it goes round, a pass at a time (pass ${(await record())?.pass})`);
await press("r");
check(await until(() => clips().length === 2, 8000), `R ends it: one clip more (${clips().length})`);
const t = clips().find((c) => c.start === 8);
const note = (() => {
  try {
    return JSON.parse(fs.readFileSync(`${PROJ}/assets/${t?.asset}.json`, "utf8"));
  } catch {
    return null;
  }
})();
check(t && Math.abs(t.length - 2) < 1e-6, `over the loop, 8 to 10 (${JSON.stringify(t)})`);
check(Array.isArray(note?.takes) && note.takes.length === 4, `its file's note lists each pass: ${JSON.stringify(note)}`);
check(t && Math.abs(t.offset - note?.takes?.[2]) < 1e-6, `the third plays, the newest whole pass (${t?.offset})`);
await press("Space");
await until(async () => !(await status()).playing, 2000);
const clipEl = `[data-clip="${t?.id}"]`;
await until(() => page.$(clipEl), 3000);
const cb = await (await page.$(clipEl))?.boundingBox();
await page.mouse.click(cb.x + cb.width / 2, cb.y + cb.height / 2, { button: "right" });
check(await until(() => page.$("[data-testid=clip-menu] [data-menu-item=take-1]"), 2000), "its menu lists the takes");
const takes = (await menuRows("[data-testid=clip-menu]")).filter((o) => o.id?.startsWith("take-"));
check(takes.map((o) => o.text).join("|").startsWith("Take 1") && takes.length === 4 && takes[2].checked === "true", `Take 1 to 4, the third ticked: ${JSON.stringify(takes)}`);
await linted("[data-testid=clip-menu]", "the clip menu with its takes");
await page.screenshot({ path: `${ROOT}/rec-takes-menu.png` });
await page.click("[data-testid=clip-menu] [data-menu-item=take-1]");
check(await until(() => clips().find((c) => c.id === t?.id)?.offset === 0, 3000), "Take 1 plays the first pass");
check(await until(async () => (await undoTitle()).startsWith("Undo Audio take 1"), 3000), `one undo step: ${await undoTitle()}`);
await key("Meta+z");
check(await until(() => Math.abs((clips().find((c) => c.id === t?.id)?.offset ?? -1) - note?.takes?.[2]) < 1e-6, 3000), "⌘Z: the third again");
check(!(await toasts()).some((m) => /couldn't|error/i.test(m)), `no toast says something went wrong: ${JSON.stringify(await toasts())}`);

done(errors);
