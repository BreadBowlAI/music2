// Step 5 in the app (descriptors and cells), on a fresh copy of examples/first-song:
//   knobs    every declared control of every component call in the song, written or left out, is a knob
//            with the range its descriptor declares; one left out plays its declared default and is drawn
//            as a default (PKG-3, PKG-8, HOME-1). Screenshot: <root>/s5-bass.png, the Bass graph.
//   insert   turning a knob the call leaves out writes the number into the call: as a new options object
//            (hat's decay) and after the options written (follow's attack); the knob is then written, and
//            undo takes out exactly what was inserted (D5, WRITE-21). An ⌥-click puts a knob back at
//            its declared default, which writes nothing.
//   browser  the effect browser groups by the descriptors' categories and shows what adding writes (their
//            `insert`); adding lowpass writes it, and its q, left out, is a knob whose first turn is written
//            positionally: lowpass(x, 1000, q) (PKG-2, PKG-7). Screenshot: <root>/s5-browser.png.
//   alias    `import { delay as echo }`, saved by another editor, is the Delay effect with delay's three
//            knobs, and removing it unwraps the call (PKG-9).
//   mix      the kick's fader, left out of its bare mixer entry, is written into it on its first turn,
//            in the style of the mixer's other entries (design-8b §5.2).
//   node run.mjs s5-knobs fix
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, toasts, show, box } = await open(1440, 900);

/** The view's knobs, each with its module and node: `{ module, node, k }`. */
const knobs = async () =>
  (await state()).view.modules.flatMap((m) => m.nodes.flatMap((n) => n.knobs.map((k) => ({ module: m.key, node: n, k }))));
const knobOf = async (module, component, slot) => (await knobs()).find((x) => x.module === module && x.k.component === component && x.k.slot === slot);
const close = (a, b) => Math.abs(a - b) <= 1e-9 * Math.max(1, Math.abs(a), Math.abs(b));
const sameRange = (a, b) => (a === null && b === null) || (a !== null && b !== null && close(a[0], b[0]) && close(a[1], b[1]));

// ------------------------------------------------------------------ knobs
// Every declared control the song's calls reach, as [module, component, slot, written, range, log]: written
// in the code, or left out and playing the descriptor's default. The bass's cutoff is connected to a
// signal (`blend(...)`), so it is no knob, and the mixer's kick and bass channels leave out their
// optional pan (PKG-5), which makes no knob either. Every channel has a gain, its fader, since 8b.3, and
// so does the master: the master's db and the kick's and the bass's, left out, are knobs at 0 dB, whose
// first turn writes them in (8b.3b). xrange's lo and hi declare no range (`control.any()`), so the page
// picks one.
const EXPECTED = [
  ["song", "std:duck", "depth", true, [-60, 0], false],
  ["song", "std:mixer", "channels.kick.db", false, [-60, 12], false],
  ["song", "std:mixer", "channels.bass.db", false, [-60, 12], false],
  ["song", "std:mixer", "channels.hats.db", true, [-60, 12], false],
  ["song", "std:mixer", "channels.hats.pan", true, [-1, 1], false],
  ["song", "std:mixer", "channels.keys.db", true, [-60, 12], false],
  ["song", "std:mixer", "channels.keys.pan", true, [-1, 1], false],
  ["song", "std:master", "db", false, [-60, 12], false],
  ["song", "std:master", "ceiling", false, [-24, 0], false],
  ["bass", "std:lfo", "period", true, [1 / 16, 16], true],
  ["bass", "std:lfo", "phase", false, [0, 1], false],
  ["bass", "std:xrange", "lo", true, null, false],
  ["bass", "std:xrange", "hi", true, null, false],
  ["bass", "std:bass", "drive", false, [0, 36], false],
  ["hats", "std:hat", "decay", false, [0.005, 1], true],
  ["keys", "std:pluck", "brightness", false, [200, 20000], true],
  ["kick", "std:kick", "tune", false, [20, 200], true],
  ["kick", "std:kick", "decay", false, [0.01, 2], true],
  ["kick", "std:follow", "attack", false, [0.0001, 1], true],
  ["kick", "std:follow", "release", true, [0.001, 5], true],
];
const all = await knobs();
const mixerNode = (await state()).view.modules.find((m) => m.key === "song").nodes.find((n) => n.component === "std:mixer");
check(mixerNode.readonly.length === 0, `the mixer's box holds no value only the code can change (${JSON.stringify(mixerNode.readonly)})`);
check(all.length === EXPECTED.length, `the song has ${EXPECTED.length} knobs: ${all.map((x) => `${x.module}:${x.k.component}.${x.k.slot}`).join(", ")}`);
for (const [module, component, slot, written, range, log] of EXPECTED) {
  const x = all.find((x) => x.module === module && x.k.component === component && x.k.slot === slot);
  const what = `${module}: ${component} ${slot}`;
  if (!x) {
    check(false, `${what} is a knob`);
    continue;
  }
  check(
    x.k.written === written && sameRange(x.k.range, range) && x.k.log === log && x.k.home === "code",
    `${what}: ${written ? "written" : "left out"}, range ${JSON.stringify(range)}${log ? " log" : ""}, home code (${JSON.stringify([x.k.written, x.k.range, x.k.log, x.k.home])})`,
  );
  if (!written) check(x.k.literal === "" && x.k.default !== null && close(x.k.value, x.k.default), `${what} plays its declared default ${x.k.default}`);
  if (range) check(range[0] <= x.k.value && x.k.value <= range[1], `${what}: ${x.k.value} lies in its range`);
}

// On the page: each is a knob on its call's box, a default drawn as one (`data-default`, its value in
// the muted grey, never italic), turned to where its value sits in the declared range.
const LOOK = { song: [], bass: [], hats: [], keys: [], kick: [] };
for (const x of all) LOOK[x.module].push(x);
for (const [module, list] of Object.entries(LOOK)) {
  await show(module);
  for (const { node, k } of list) {
    const sel = `[data-module="${module}"] [data-node-id="${node.id}"] [data-knob-id="${k.id}"]`;
    const seen = await page.$eval(sel, (e) => ({
      dflt: e.dataset.default === "true",
      muted: ((v) => getComputedStyle(v).color === "rgb(101, 105, 111)" && getComputedStyle(v).fontStyle !== "italic")(e.querySelector("span.tabular-nums")),
      at: Number(e.querySelector("svg[role=slider]").getAttribute("aria-valuenow")),
      title: e.title,
    })).catch(() => null);
    if (!seen) {
      check(false, `${module}: the ${k.slot} knob is on ${node.label}'s box`);
      continue;
    }
    const where = k.range ? (k.log ? Math.log(k.value / k.range[0]) / Math.log(k.range[1] / k.range[0]) : (k.value - k.range[0]) / (k.range[1] - k.range[0])) : null;
    check(
      seen.dflt === !k.written && seen.muted === !k.written && (where === null || Math.abs(seen.at - where) < 0.002),
      `${module}: ${node.label}'s ${k.label} is drawn ${k.written ? "as written" : "as its default"}${where === null ? "" : `, turned to ${where.toFixed(3)} of its declared range`} (${JSON.stringify(seen)})`,
    );
  }
}
await show("bass");
check(!(await page.$('[data-module="bass"] [data-node="bass"] [data-knob="cutoff"]')), "the bass's cutoff, connected to a signal, is no knob");
await page.$eval('[data-module="bass"] [data-node="bass"]', (e) => e.scrollIntoView({ block: "center", inline: "center" }));
await sleep(300);
await page.screenshot({ path: `${ROOT}/s5-bass.png` });

// ------------------------------------------------------------------ insert
/** Drag the knob `sel` up by `dy` pixels and let go. */
const turn = async (sel, dy) => {
  await page.$eval(`${sel} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await box(`${sel} svg`);
  const cx = b.x + b.width / 2, cy = b.y + b.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) await page.mouse.move(cx, cy - (i * dy) / 8);
  await page.mouse.up();
};
const undo = async () => {
  const before = (await state()).view.revision;
  await page.click('button[aria-label="Undo"]');
  await until(async () => (await state()).view.revision !== before);
};

// A new options object: `hat(lane.notes("hits"))` leaves out its decay.
const hats = read("tracks/hats.ts");
await show("hats");
const decay = '[data-module="hats"] [data-node="hat"] [data-knob="decay"]';
await turn(decay, 32);
const insertedHat = /hat\(lane\.notes\("hits"\), \{ decay: (-?[0-9.e-]+) \}\)/;
check(await until(() => insertedHat.test(read("tracks/hats.ts"))), `turning hat's decay writes it into the call (${read("tracks/hats.ts").split("\n").at(-2)})`);
const written = Number(read("tracks/hats.ts").match(insertedHat)?.[1]);
check(written > 0.045 && written <= 1, `as a plain number in seconds, up from the default 0.045: ${written}`);
check(read("tracks/hats.ts").replace(insertedHat, 'hat(lane.notes("hits"))') === hats, "and nothing else in the file changed");
check(
  await until(async () => {
    const x = await knobOf("hats", "std:hat", "decay");
    return x?.k.written && close(x.k.value, written) && !(await page.$(`${decay}[data-default]`));
  }),
  "the knob is written now, no longer drawn as a default",
);
await undo();
check(await until(() => read("tracks/hats.ts") === hats), "undo takes out exactly what was inserted");
check(await until(async () => (await knobOf("hats", "std:hat", "decay"))?.k.written === false), "and the knob plays its default again");

// After the options written: `follow(out, { release: ms(150) })` leaves out its attack.
const kick = read("tracks/kick.ts");
await show("kick");
const attack = '[data-module="kick"] [data-node="follow"] [data-knob="attack"]';
await turn(attack, 24);
const insertedAttack = /follow\(out, \{ release: ms\(150\), attack: (-?[0-9.e-]+) \}\)/;
check(await until(() => insertedAttack.test(read("tracks/kick.ts"))), `turning follow's attack writes it after the options written (${read("tracks/kick.ts").match(/follow\(.*\)/)?.[0]})`);
await undo();
check(await until(() => read("tracks/kick.ts") === kick), "undo takes it out again");

// An ⌥-click puts a knob at its declared default (taste.md §5.2; a double-click types a value now):
// for one already there, that writes nothing.
const tune = '[data-module="kick"] [data-node="kick"] [data-knob="tune"]';
await page.$eval(`${tune} svg`, (e) => e.scrollIntoView({ block: "center" }));
const tb = await box(`${tune} svg`);
const revision = (await state()).view.revision;
await page.keyboard.down("Alt");
await page.mouse.click(tb.x + tb.width / 2, tb.y + tb.height / 2);
await page.keyboard.up("Alt");
await sleep(1200);
check(read("tracks/kick.ts") === kick && (await state()).view.revision === revision, "an ⌥-click on a knob at its default writes nothing");

// ------------------------------------------------------------------ browser
const keys = read("tracks/keys.ts");
await show("keys");
// The edits' toasts fade first, so the screenshot shows the browser alone.
await until(async () => (await toasts()).length === 0, 15000);
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector("[data-effect-browser] [data-category]");
await sleep(300);
await page.screenshot({ path: `${ROOT}/s5-browser.png` });
const effects = (await state()).effects;
const groups = await page.$$eval("[data-effect-browser] [data-category]", (els) => els.map((e) => e.dataset.category));
const categories = [...new Set(effects.map((e) => e.category))];
check(
  groups.length === categories.length && categories.every((c) => groups.includes(c)),
  `the browser's groups are the descriptors' categories: ${groups.join(", ")}`,
);
check(JSON.stringify(groups) === JSON.stringify(["Filter", "EQ", "Dynamics", "Distortion", "Delay & Reverb", "Modulation", "Utility"]), "in the browser's own order");
for (const e of effects) {
  const where = await page.$eval(`[data-effect-browser] [data-component="${e.component}"]`, (el) => el.closest("[data-category]")?.dataset.category).catch(() => null);
  check(where === e.category, `${e.component} (${e.label}) is under ${e.category}`);
}
const footer = async () => page.$eval("[data-effect-browser] .font-mono", (e) => e.textContent);
await page.type('[data-effect-browser] input[aria-label="Search effects"]', "lowpass");
check(await until(async () => (await footer()) === "lowpass(pluck(…), 1000)"), `its footer shows what adding writes: ${await footer()}`);
await page.keyboard.press("Enter");
const filtered = 'export default track("keys", { name: "Keys" }, ({ lane }) => lowpass(pluck(lane.notes("chords")), 1000));';
check(await until(() => read("tracks/keys.ts").includes(filtered)), "adding lowpass writes its insert: lowpass(pluck(…), 1000)");
check(/import \{ lowpass \} from "std\/filter";/.test(read("tracks/keys.ts")), "and imports it");
const added = read("tracks/keys.ts");
// Its q is left out: a knob at 0.707, in the range lowpass declares for it.
check(
  await until(async () => {
    const q = await knobOf("keys", "std:lowpass", "q");
    return q && !q.k.written && close(q.k.value, 0.707) && sameRange(q.k.range, [0.3, 20]) && q.k.log;
  }),
  "the lowpass's q, left out, is a knob at 0.707 with its declared range 0.3..20, log",
);
const cutoff = await knobOf("keys", "std:lowpass", "cutoff");
check(cutoff?.k.written && cutoff.k.value === 1000 && sameRange(cutoff.k.range, [20, 20000]), "its cutoff is the 1000 written, in 20..20000");
await page.waitForSelector('[data-module="keys"] [data-node="lowpass"] [data-knob="q"][data-default]');
await turn('[data-module="keys"] [data-node="lowpass"] [data-knob="q"]', 24);
const positional = /lowpass\(pluck\(lane\.notes\("chords"\)\), 1000, (-?[0-9.e-]+)\)/;
check(await until(() => positional.test(read("tracks/keys.ts"))), `turning q writes it as the next argument (${read("tracks/keys.ts").match(/lowpass\(pluck.*\)\)/)?.[0]})`);
await undo();
check(await until(() => read("tracks/keys.ts") === added), "undo takes out the q");
await undo();
check(await until(() => read("tracks/keys.ts") === keys), "and undo again takes out the lowpass, import and all");

// ------------------------------------------------------------------ mix
// The kick's fader, left out of `kick: kick.out`: its first turn wraps the entry, written as db(…) like
// the mixer's other entries, and nothing else in the file changes.
const songFile = read("song.ts");
await show("song");
const kickDb = '[data-module="song"] [data-node="mixer"] [data-knob="kick.db"]';
await turn(kickDb, -24);
const wrapped = /kick: \{ in: kick\.out, db: db\((-?[0-9.e-]+)\) \},/;
check(await until(() => wrapped.test(read("song.ts"))), `turning the kick's fader writes it into its entry (${read("song.ts").match(/kick: .*,/)?.[0]})`);
const level = Number(read("song.ts").match(wrapped)?.[1]);
check(level < 0 && read("song.ts").replace(wrapped, "kick: kick.out,") === songFile, `down from 0 dB to ${level}, and nothing else changed`);
await undo();
check(await until(() => read("song.ts") === songFile), "undo puts the bare entry back");

// ------------------------------------------------------------------ alias
const echo = `import { track } from "@m2/core";\nimport { delay as echo } from "std/fx";\nimport { pluck } from "std/instruments";\n\nexport default track("keys", { name: "Keys" }, ({ lane }) => echo(pluck(lane.notes("chords"))));\n`;
write("tracks/keys.ts", echo);
const echoNode = async () => (await state()).view.modules.find((m) => m.key === "keys")?.nodes.find((n) => n.component === "std:delay");
check(await until(async () => !!(await echoNode()), 10000), "a delay imported as echo, saved by another editor, is a std:delay call");
const node = await echoNode();
check(node?.label === "echo" && node.effect === "Delay", `shown as echo, the Delay effect (${node?.label}, ${node?.effect})`);
const delayKnobs = (node?.knobs ?? []).map((k) => [k.slot, k.written, k.range, k.log]);
check(
  JSON.stringify(delayKnobs) === JSON.stringify([["time", false, [0.001, 2], true], ["feedback", false, [0, 0.95], false], ["mix", false, [0, 1], false]]),
  `with delay's three knobs, at their defaults, in their declared ranges: ${JSON.stringify(delayKnobs)}`,
);
await show("keys");
const remove = '[data-module="keys"] [data-node="echo"] button[aria-label="Remove Delay"]';
check(await until(async () => !!(await page.$(remove))), "its box offers Remove Delay");
await page.click(remove);
check(await until(() => read("tracks/keys.ts").includes('({ lane }) => pluck(lane.notes("chords"))')), "removing it unwraps the pluck");
write("tracks/keys.ts", keys);
await until(async () => !(await echoNode()));

done(errors);
await browser.close();
