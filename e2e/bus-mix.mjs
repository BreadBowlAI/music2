// Buses in the Mix view (RENDER-34; the bus plan's slice 3): the Add bus menu (Reverb, Delay,
// Group) writes a bus file and its channel, and its strip shows after the tracks, marked as a bus,
// with M, S and a meter; one undo takes it back. Every other strip gets a slot for it: a + adds a
// send (−12 dB after the fader), which then is a knob; the bus's meter moves once a playing track
// sends to it; turning the knob writes its level, one undo step. The send's menu sets it before
// the fader (a dot on the knob). The output picker takes Kick to a Group and back, one undo step.
// A choice that would make a loop is greyed and says why. In the timeline a bus is a row with no
// lanes, marked, whose menu has no Change instrument, and R on it records nothing. The lint.
//
//   node e2e/bus-mix.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, PROJ, read, ROOT, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, toasts, key, box } = await open(1440, 900);
const orig = read("song.ts");
const FLOOR = -60;

const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
const slot = (id, bus) => `${strip(id)} [data-send="${bus}"]`;
const exists = (path) => fs.existsSync(`${PROJ}/${path}`);
const has = async (sel) => !!(await page.$(sel));
const text = (sel) => page.$eval(sel, (e) => e.textContent).catch(() => null);
const strips = () => page.$$eval("[data-testid=mix] [data-strip]", (els) => els.map((e) => e.dataset.strip));
const step = async (combo) => {
  const rev = (await state()).view.revision;
  await key(combo);
  return until(async () => (await state()).view.revision !== rev);
};
const undo = () => step("Meta+z");
const redo = () => step("Meta+Shift+z");
const menuRows = (sel) =>
  page
    .waitForSelector(sel, { timeout: 2000 })
    .then(() => page.$$eval(`${sel} [role^=menuitem]`, (els) => els.map((e) => ({ text: e.textContent, id: e.dataset.output ?? e.dataset.busKind ?? e.dataset.menuItem, off: e.getAttribute("aria-disabled") === "true", title: e.title }))))
    .catch(() => []);
const closeMenus = async () => {
  await key("Escape");
  await sleep(150);
};
/** The loudest a strip's meter reads over `ms`. */
const loudest = async (id, ms) => {
  let top = FLOOR;
  for (const t = Date.now(); Date.now() - t < ms; ) {
    const v = Number(await page.$eval(`${strip(id)} [role=meter]`, (e) => e.getAttribute("aria-valuenow")).catch(() => FLOOR));
    top = Math.max(top, Number.isFinite(v) ? v : FLOOR);
    await sleep(60);
  }
  return top;
};
const playing = async () => (await api("/status")).body.playing;
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const addBus = async (kind) => {
  await page.click("[data-testid=add-bus]");
  const rows = await menuRows("[data-testid=add-bus-menu]");
  await page.click(`[data-testid=add-bus-menu] [data-bus-kind="${kind}"]`).catch(() => {});
  return rows;
};

// ---------------------------------------------------------------- Add bus

await key("Alt+3");
await until(() => has(strip("master")));
check(await has("[data-testid=add-bus]"), "the Mix view has an Add bus button");
const kinds = await addBus("reverb");
check(kinds.map((r) => r.id).join() === "reverb,delay,group", `its menu offers Reverb, Delay and Group (${kinds.map((r) => r.text)})`);
check(await until(() => exists("tracks/reverb.ts") && /bus\("reverb"/.test(read("tracks/reverb.ts")) && /reverb: reverb\.out/.test(read("song.ts"))), "Reverb writes tracks/reverb.ts with bus(…) and a channel `reverb: reverb.out`");
check(await until(async () => (await strips()).join() === "kick,hats,bass,keys,reverb,master"), `its strip comes after the tracks, before Master (${await strips()})`);
const marked = await page.$eval(strip("reverb"), (e) => ({ bus: e.dataset.bus, tag: e.querySelector("[data-testid=bus-tag]")?.textContent, m: !!e.querySelector("[data-strip-mute]"), meter: !!e.querySelector("[role=meter]") })).catch(() => ({}));
check(marked.bus === "true" && marked.tag === "Bus" && marked.m && marked.meter, `it is marked as a bus, with M, S and a meter (${JSON.stringify(marked)})`);
check(!(await has(`${strip("reverb")} [data-testid=change-instrument]`)), "and no instrument");
const slots = await Promise.all(["kick", "hats", "bass", "keys"].map(async (id) => (await page.$eval(slot(id, "reverb"), (e) => e.dataset.state).catch(() => null))));
check(slots.every((s) => s === "off") && !(await has(slot("reverb", "reverb"))), `every track strip has a Reverb send slot with a +, the bus none to itself (${slots})`);

check((await undo()) && (await until(() => !exists("tracks/reverb.ts") && read("song.ts") === orig)), "one undo takes the bus back: its file gone, song.ts as it was");
check(await until(async () => !(await strips()).includes("reverb")), "and its strip");
check((await redo()) && (await until(async () => (await strips()).includes("reverb"))), "redo brings it back");

// ---------------------------------------------------------------- a send, heard

await page.keyboard.press("Space");
check(await until(playing, 3000), "the song plays");
check((await loudest("reverb", 1500)) <= FLOOR, "with nothing sent, the Reverb's meter stays at the floor");
await page.click(`${slot("keys", "reverb")} [data-send-add]`);
check(await until(() => /keys: \{ in: keys\.out, db: db\(-5\), pan: -0\.25, sends: \{ reverb: db\(-12\) \} \}/.test(read("song.ts"))), `the + writes a send at −12 dB (${read("song.ts").split("\n").find((l) => l.includes("keys:"))?.trim()})`);
check(await until(async () => (await page.$eval(slot("keys", "reverb"), (e) => [e.dataset.state, e.querySelector("[data-value]")?.dataset.value]).catch(() => [])).join() === "on,-12"), "the slot is a knob now, at −12 dB");
check((await loudest("reverb", 4000)) > FLOOR, "and the Reverb's meter moves: the send is heard");
const knob = await box(`${slot("keys", "reverb")} [role=slider]`);
const before = read("song.ts");
await page.mouse.move(knob.x + knob.width / 2, knob.y + knob.height / 2);
await page.mouse.down();
await page.mouse.move(knob.x + knob.width / 2, knob.y - 40, { steps: 8 });
await page.mouse.up();
check(await until(() => read("song.ts") !== before && /sends: \{ reverb: db\(-?\d+(\.\d+)?\) \}/.test(read("song.ts")) && !read("song.ts").includes("reverb: db(-12)")), `turning the knob up writes its level (${read("song.ts").split("\n").find((l) => l.includes("keys:"))?.trim()})`);
check((await undo()) && (await until(() => read("song.ts") === before)), "one undo puts −12 dB back");
await page.keyboard.press("Space");
await until(async () => !(await playing()), 3000);

// ---------------------------------------------------------------- before the fader

await page.click(`${slot("keys", "reverb")} [role=slider]`, { button: "right" });
const sendRows = await menuRows("[data-testid=send-menu]");
check(sendRows.some((r) => r.id === "pre" && /Before the fader/.test(r.text)) && sendRows.some((r) => r.id === "remove"), `the send's menu has Before the fader and Remove send (${sendRows.map((r) => r.text)})`);
await page.click("[data-testid=send-menu] [data-menu-item=pre]");
check(await until(() => read("song.ts").includes("sends: { reverb: { db: db(-12), pre: true } }")), "Before the fader writes `{ db: db(-12), pre: true }`");
check(await until(async () => (await page.$eval(slot("keys", "reverb"), (e) => e.dataset.pre === "true" && !!e.querySelector("[data-testid=pre-dot]")).catch(() => false))), "and the knob shows a dot");
check((await undo()) && (await until(() => read("song.ts") === before)), "one undo puts it after the fader again");

// ---------------------------------------------------------------- a group and the output picker

await addBus("group");
check(await until(async () => (await strips()).join() === "kick,hats,bass,keys,reverb,group,master" && /group: group\.out/.test(read("song.ts"))), `Group adds a bus strip (${await strips()})`);
check((await text(`${strip("kick")} [data-testid=strip-output]`)) === "Master", `Kick's output says Master (${await text(`${strip("kick")} [data-testid=strip-output]`)})`);
await page.click(`${strip("kick")} [data-testid=strip-output]`);
const outs = await menuRows("[data-testid=output-menu]");
check(outs.map((r) => r.id).join() === "master,reverb,group" && outs.every((r) => !r.off), `its picker offers Master, Reverb and Group (${outs.map((r) => r.text)})`);
const grouped = read("song.ts");
await page.click("[data-testid=output-menu] [data-output=group]");
check(await until(() => read("song.ts").includes('kick: { in: kick.out, to: "group" }')), "Group writes `to: \"group\"` on Kick");
check(await until(async () => (await text(`${strip("kick")} [data-testid=strip-output]`)) === "Group"), "and Kick's output says Group");
check((await undo()) && (await until(() => read("song.ts") === grouped)), "one undo sends Kick back to the master");
await until(async () => (await text(`${strip("kick")} [data-testid=strip-output]`)) === "Master");

// ---------------------------------------------------------------- a loop is greyed, with why

await page.click(`${slot("group", "reverb")} [data-send-add]`);
check(await until(() => /group: \{ in: group\.out, sends: \{ reverb: db\(-12\) \} \}/.test(read("song.ts"))), "the Group sends to the Reverb");
const looped = await until(async () => (await page.$eval(slot("reverb", "group"), (e) => e.dataset.state).catch(() => null)) === "loop");
const why = await page.$eval(`${slot("reverb", "group")} [data-send-add]`, (e) => [e.disabled || e.getAttribute("aria-disabled") === "true", e.title]).catch(() => [false, ""]);
check(looped && why[0] && why[1] === "Group already feeds Reverb, so Reverb can't send to Group.", `on the Reverb strip, a send to the Group is greyed and says why (${why})`);
await page.click(`${strip("reverb")} [data-testid=strip-output]`);
const reverbOuts = await menuRows("[data-testid=output-menu]");
const g = reverbOuts.find((r) => r.id === "group");
check(g?.off && g.title === "Group already feeds Reverb, so Reverb can't go to Group." && !reverbOuts.some((r) => r.id === "reverb"), `and its output picker greys the Group, with why (${JSON.stringify(reverbOuts)})`);
await linted("[data-testid=mix]", "the Mix view with buses, sends and a loop greyed");
await page.screenshot({ path: `${ROOT}/bus-mix.png` });
await closeMenus();

// ---------------------------------------------------------------- the timeline

const row = '[data-testid=timeline] [data-track="reverb"]';
const r = await page.$eval(row, (e) => ({ bus: e.dataset.bus, tag: e.querySelector("[data-testid=bus-tag]")?.textContent })).catch(() => ({}));
check(r.bus === "true" && r.tag === "Bus", `the timeline shows the Reverb as a bus row (${JSON.stringify(r)})`);
check(!(await has('[data-testid=timeline] [data-lane^="reverb."]')), "with no lanes");
await page.click(`${row} [data-track-menu]`).catch(() => {});
const trackRows = await menuRows("[data-testid=track-menu]");
check(trackRows.length > 0 && !trackRows.some((x) => /Change instrument/.test(x.text)), `its menu has no Change instrument (${trackRows.map((x) => x.text)})`);
await closeMenus();
await page.click(`[data-module-tab="reverb"]`);
await until(() => page.$eval('[data-module-tab="reverb"]', (e) => e.getAttribute("aria-selected") === "true"));
await page.keyboard.press("r");
check(await until(async () => (await toasts()).some((t) => t.includes("Select a notes track or an audio track"))), "R on the bus records nothing, and says why");
check((await page.$eval("[data-testid=record]", (e) => e.dataset.phase)) === "off", "no take starts");

done(errors);
