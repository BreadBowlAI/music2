// Track frame: Device's chain and the card's frame (CARD-42 to CARD-47). A sound of the song's own,
// Wob, names a face (its amp envelope and three knobs), six bands as groups (each with an `on…`
// switch) and a choice of three; a Lowpass follows it. Device shows the two as a chain, left to
// right, with no ports and no cables; Wob's face is drawn big, its tabs are its bands, More and All,
// and its code line says what its call writes. A segmented pick and a knob dragged in the grid are
// each one undo step; the keys move a knob (PageUp, End, Home, ⌫) and the tab bar (← →). The three
// sizes change nothing in the song. A knob on a hidden tab lights its tab when an agent changes it,
// and Show opens that tab. A sound built from parts shows its patch and says so; the switch picks
// the patch for a chain too, and the window keeps the choice. Each state linted.
//
//   node e2e/frame.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, view, box, posts } = await open(1440, 900, { device: "chain" });

const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const stepIs = async (want) => until(async () => (await undoTitle()) === `Undo ${want} (⌘Z)`);

// ---------------------------------------------------------------- Wob, then a Lowpass: a chain

const BANDS = [1, 2, 3, 4, 5, 6];
const TRACK = `import { component, control, port, track, view } from "@m2/core";
import { lowpass } from "std/filter";
import { osc } from "std/dsp";

const wob = component(
  {
    id: "wob",
    label: "Wob",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(["saw", "square", "sine"], "saw"),
      cutoff: control.hz(2000).range(40, 8000).log(),
      attack: control.secs(0.01),
      decay: control.secs(0.2),
      sustain: control.frac(0.6),
      release: control.secs(0.3),
${BANDS.map((b) => `      on${b}: control.bool(${b === 1}),\n      gain${b}: control.db(0).range(-24, 24),`).join("\n")}
    },
    output: port.audio(),
    editor: {
      views: { amp: view.envelope("attack", "decay", "sustain", "release") },
      face: ["amp", "shape", "cutoff", "release"],
      groups: { ${BANDS.map((b) => `"Band ${b}": ["on${b}", "gain${b}"]`).join(", ")} },
    },
  },
  () => osc.saw(110),
);

export default track("wob", { name: "Wob" }, ({ lane }) =>
  lowpass(wob(lane.notes("notes"), { cutoff: 2000 }), { cutoff: 5000 }),
);
`;
write("tracks/wob.ts", TRACK);
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport wob from "./tracks/wob";').replace("      kick: kick.out,", "      kick: kick.out,\n      wob: wob.out,"));

await until(async () => !!(await page.$('[data-module-tab="wob"]')), 15000);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await view("graph");
const chain = '[data-testid=chain] [data-module="wob"]';
check(await until(async () => !!(await page.$(`${chain} [data-frame]`)), 15000), "Device shows Wob's sound as a chain (CARD-42)");
const titles = await page.$$eval(`${chain} [data-frame] [data-node-title]`, (els) => els.map((e) => e.textContent));
check(JSON.stringify(titles) === '["Wob","Lowpass"]', `its cards left to right, as the sound flows: ${JSON.stringify(titles)}`);
check(!(await page.$(`[data-testid=chain] [data-port], [data-testid=chain] [data-port-dot], [data-testid=chain] path[data-from]`)), "no ports and no cables in the chain");
check(!(await page.$(`${chain} [data-input="notes"]`)), "and no row for its notes coming in");
const wob = `${chain} [data-node="wob"]`;
const lp = `${chain} [data-node="lowpass"]`;
check((await page.$eval(`${lp} [data-pack]`, (e) => e.textContent).catch(() => null)) === "std", "Lowpass's header names its package, std (CARD-43)");
check(!(await page.$(`${wob} [data-pack]`)), "Wob, the song's own, names none");
await linted("[data-testid=chain]", "the chain");
await page.screenshot({ path: `${ROOT}/frame-chain.png` });

// ---------------------------------------------------------------- the face, the tabs, All (CARD-43, CARD-45)

const faceCells = await page.$$eval(`${wob} [data-cell="36"]`, (els) => els.map((e) => e.dataset.knob));
check(JSON.stringify(faceCells) === '["cutoff","release"]', `the face's knobs are 36 px, in the face's order: ${JSON.stringify(faceCells)}`);
check(!!(await page.$(`${wob} [data-view="amp"]`)), "and its envelope is on the face");
const tabs = () => page.$$eval(`${wob} [role=tab]`, (els) => els.map((e) => e.dataset.tab));
// More holds what no band names (the envelope's other values), as a group does (CARD-25).
check(JSON.stringify(await tabs()) === JSON.stringify([...BANDS.map((b) => `Band ${b}`), "More", "All"]), `its tabs are its groups, More last, then All: ${JSON.stringify(await tabs())}`);
const on = await page.$$eval(`${wob} [role=tab][data-on]`, (els) => els.map((e) => `${e.dataset.tab}=${e.dataset.on}`));
check(on[0] === "Band 1=true" && on[1] === "Band 2=false" && on.length === BANDS.length, `a band's on switch is a dot on its tab, filled when on: ${on.join(" ")}`);
check((await page.$eval(`${wob} [role=tablist]`, (e) => e.dataset.fade ?? "")) === "right", "the tab bar too wide for the card fades at its right edge");
check(!!(await page.$(`${wob} [data-tab-panel="Band 1"] [data-cell="28"][data-knob="gain1"]`)), "the first tab shows, its knobs 28 px");
const state = (await api("")).body;
const card = () => state.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wob");
const gain3 = card().knobs.find((k) => k.slot === "gain3").id;
const hides = await page.$eval(`${wob} [data-tab="Band 3"]`, (e) => e.dataset.hides ?? "");
check(!(await page.$(`${wob} [data-knob-id="${gain3}"]`)) && hides.split(" ").includes(gain3), `a knob on a tab not shown isn't drawn, and its tab stands for it (CARD-47): ${hides}`);
await page.click(`${wob} [data-tab="All"]`);
const all = await page.$$eval(`${wob} [data-tab-panel="All"] [data-knob]`, (els) => els.map((e) => [e.dataset.knob, e.title.includes("Its id: ")]));
check(all.length === card().knobs.length && all.every(([, id]) => id), `All lists every value, each with its id: ${all.length} of ${card().knobs.length}`);
await linted(wob, "Wob's card, All shown");

// The tab bar's keys (CARD-46): → the next tab, ← back, wrapping.
await page.focus(`${wob} [data-tab="All"]`);
await page.keyboard.press("ArrowRight");
check(await until(async () => (await page.$eval(`${wob} [data-tab="Band 1"]`, (e) => e.getAttribute("aria-selected"))) === "true"), "→ on the last tab picks the first");
check(await until(() => page.$eval(`${wob} [data-tab="Band 1"]`, (e) => e === document.activeElement)), "and takes the focus there");
await page.keyboard.press("ArrowLeft");
check(await until(async () => (await page.$eval(`${wob} [data-tab="All"]`, (e) => e.getAttribute("aria-selected"))) === "true"), "← goes back to All");
await page.click(`${wob} [data-tab="Band 1"]`);

// ---------------------------------------------------------------- a segmented choice, a knob in the grid (CARD-45)

const before = read("tracks/wob.ts");
const seg = `${wob} [data-knob="shape"] [data-testid=setting-segmented]`;
const segs = await page.$$eval(`${seg} [role=radio]`, (els) => els.map((e) => `${e.dataset.option}:${e.getAttribute("aria-checked")}`));
check(JSON.stringify(segs) === '["saw:true","square:false","sine:false"]', `a choice of three is a segmented control, at its default: ${JSON.stringify(segs)}`);
await page.click(`${seg} [data-option="square"]`);
check(await until(() => read("tracks/wob.ts") === before.replace("wob(lane.notes(\"notes\"), { cutoff: 2000 })", "wob(lane.notes(\"notes\"), { cutoff: 2000, shape: \"square\" })")), "a pick writes shape: \"square\", and nothing else");
check(await stepIs("Wob shape"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await page.$eval(`${wob} [data-code-line]`, (e) => e.textContent)) === 'wob(lane.notes("notes"), { cutoff: 2000, shape: "square" })'), "Wob's code line says what its call now writes (CARD-41)");
check((await page.$eval(`${lp} [data-code-line]`, (e) => e.textContent)) === "lowpass(…, { cutoff: 5000 })", "the Lowpass's says its sound as …");

const cutoff = `${wob} [data-cell="36"][data-knob="cutoff"]`;
const k = await box(`${cutoff} svg[role=slider]`);
const written = read("tracks/wob.ts");
await page.mouse.move(k.x + k.width / 2, k.y + k.height / 2);
await page.mouse.down();
await page.mouse.move(k.x + k.width / 2, k.y + k.height / 2 - 40, { steps: 8 });
await page.mouse.up();
check(await until(() => read("tracks/wob.ts") !== written && /cutoff: \d/.test(read("tracks/wob.ts")) && !read("tracks/wob.ts").includes("cutoff: 2000,")), "a face knob dragged up writes a higher cutoff");
check(await stepIs("Wob cutoff"), `one undo step: ${await undoTitle()}`);

// The knob's keys (CARD-46), each one edit.
const gain1 = `${wob} [data-knob="gain1"]`;
const value = () => page.$eval(gain1, (e) => Number(e.dataset.value));
await page.$eval(`${gain1} svg[role=slider]`, (e) => e.focus());
await page.keyboard.press("End");
check(await until(async () => (await value()) === 24 && read("tracks/wob.ts").includes("gain1: 24")), "End: its highest, written");
// Written, the knob has a new id and mounts again: it keeps the keyboard's focus.
check(await until(() => page.$eval(`${gain1} svg[role=slider]`, (e) => e === document.activeElement)), "and the knob keeps the keyboard's focus");
await page.keyboard.press("Home");
check(await until(async () => (await value()) === -24 && read("tracks/wob.ts").includes("gain1: -24")), "Home: its lowest");
await page.keyboard.press("PageUp");
check(await until(async () => Math.abs((await value()) - -19.2) < 0.01), `PageUp: a tenth of its range up: ${await value()}`);
await page.keyboard.press("Backspace");
check(await until(async () => (await value()) === 0 && read("tracks/wob.ts").includes("gain1: 0")), "⌫: its default");

// ---------------------------------------------------------------- the three sizes (CARD-44)

const width = async (sel) => Math.round((await box(sel)).width);
const sent = posts.length;
const text = read("tracks/wob.ts");
check((await width(wob)) === 460 && (await width(lp)) === 380, `Normal: 460 px with a picture on its face, else 380: ${await width(wob)} ${await width(lp)}`);
await page.click(`${wob} [data-size-to="folded"]`);
check(await until(async () => (await page.$eval(wob, (e) => e.dataset.frame)) === "folded"), "Fold folds it");
check((await width(wob)) === 240 && !(await page.$(`${wob} [data-code-line]`)) && !(await page.$(`${wob} [role=tab]`)), "Folded: 240 px, no tabs, no code line");
const sentence = await page.$eval(`${wob} [data-sentence]`, (e) => e.textContent);
check(/^Shape square · Cutoff [\d.]+ kHz · Release 300 ms$/.test(sentence), `and one sentence of its face's values: ${sentence}`);
await linted(wob, "Wob, folded");
await page.click(`${wob} [data-size-to="expanded"]`);
check(await until(async () => (await page.$eval(wob, (e) => e.dataset.frame)) === "expanded"), "Expand expands it");
const panels = await page.$$eval(`${wob} [data-tab-panel]`, (els) => els.map((e) => e.dataset.tabPanel));
check((await width(wob)) === 920 && panels.length === BANDS.length + 1 && !!(await page.$(`${wob} details[data-all]:not([open])`)), `Expanded: twice as wide, every tab side by side, All behind a disclosure: ${await width(wob)} ${panels.join(",")}`);
check(!!(await page.$(`${wob} [data-knob-id="${gain3}"]`)), "the third band's knob is drawn there");
await linted(wob, "Wob, expanded");
await page.screenshot({ path: `${ROOT}/frame-expanded.png` });
await page.click(`${wob} [data-size-to="normal"]`);
await until(async () => (await page.$eval(wob, (e) => e.dataset.frame)) === "normal");
await sleep(300);
check(posts.length === sent && read("tracks/wob.ts") === text, `the sizes write nothing: ${posts.slice(sent).map((p) => p.url).join(" ")}`);

// ---------------------------------------------------------------- an agent's change on a hidden tab (CARD-47)

const now = (await api("")).body;
const agent = (path, body) =>
  page.evaluate(
    async (url, key, path, body) => (await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify(body) })).status,
    sess.url,
    sess.token,
    path,
    body,
  );
const gain3Now = now.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wob").knobs.find((k) => k.slot === "gain3").id;
check((await agent("/api/graph/set", { session: now.session, revision: now.view.revision, id: gain3Now, gesture: "agent", value: -6 })) === 200, "an agent sets band 3's gain, on a tab not shown");
check(await until(async () => !!(await page.$(`${wob} [data-tab="Band 3"][data-glow]`))), "its change lights its tab");
const gain3Written = (await api("")).body.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wob").knobs.find((k) => k.slot === "gain3").id;
check((await agent("/api/point", { session: now.session, target: `knob:${gain3Written}`, words: "this one" })) === 200, "an agent points at it");
check(await until(async () => !!(await page.$("[data-testid=pointer-show]"))), "the pointer offers Show");
await page.click("[data-testid=pointer-show]");
check(await until(async () => !!(await page.$(`${wob} [data-tab-panel="Band 3"] [data-knob-id="${gain3Written}"]`))), "Show opens its tab, the knob in sight");
await page.click("[data-testid=pointer-ok]").catch(() => {});

// ---------------------------------------------------------------- Chain · Patch (CARD-42)

await page.click("[data-device=patch]");
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="wob"] [data-port]'))), "Patch shows the sound's patch, its ports and cables");
await page.reload({ waitUntil: "domcontentloaded" });
await until(async () => !!(await page.$('[data-module-tab="wob"]')), 15000);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="wob"]'))), "the window keeps the choice");
await page.click("[data-device=chain]");
check(await until(async () => !!(await page.$(chain))), "Chain brings the chain back");

// A sound built from parts: two oscillators mixed is no line, so Device shows its patch and says so.
write("tracks/wob.ts", `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { sum } from "std/math";

export default track("wob", { name: "Wob" }, () => sum(osc.saw(110), osc.saw(165)));
`);
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="wob"]')) && !!(await page.$("[data-testid=device-parts]")), 15000), "a sound built from parts shows its patch, and says so");
check((await page.$eval("[data-device=chain]", (e) => e.disabled)) === true, "Chain can't be picked for it");
check((await page.$eval("[data-testid=device-parts]", (e) => e.textContent)) === "This sound is built from parts. Showing its patch.", "in those words");

done(errors);
