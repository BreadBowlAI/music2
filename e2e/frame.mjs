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
import { tips } from "./tiplib.mjs";

const { page, errors, api, view, box, posts } = await open(1440, 900, { device: "chain" });

/** A write or a pointer from outside the page: an agent's (AGENT-5, AGENT-6). */
const agent = (path, body) =>
  page.evaluate(
    async (url, key, path, body) => (await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify(body) })).status,
    sess.url,
    sess.token,
    path,
    body,
  );
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

export default track("wob", { name: "Wob" }, ({ lane }) => {
  const w = wob(lane.notes("notes"), { cutoff: 2000 });
  return lowpass(w, { cutoff: 5000 });
});
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
// The Lowpass's sound comes through a const (`lowpass(w, …)`): still a connection, no row (review F2).
check(!(await page.$(`${lp} [data-inlet], ${lp} [data-input]`)), "and none for the Lowpass's sound coming in, though a const names it");
check((await page.$eval(`${lp} [data-pack]`, (e) => e.textContent).catch(() => null)) === "std", "Lowpass's header names its package, std (CARD-43)");
check(!(await page.$(`${wob} [data-pack]`)), "Wob, the song's own, names none");
await linted("[data-testid=chain]", "the chain");
const untipped = await page.evaluate(tips, "[data-testid=chain]", {});
check(untipped.length === 0, `every control in the chain has a tooltip${untipped.length ? `: ${untipped.slice(0, 3).join(" | ")}` : ""}`);
await page.screenshot({ path: `${ROOT}/frame-chain.png` });
// At the dock's first height the whole card is in sight: its face, its tabs and its code line (review F12).
const dockBox = await box("#dock-body");
const wobBox = await box(wob);
check(wobBox.y + wobBox.height <= dockBox.y + dockBox.height + 1, `Wob's whole card fits the dock at its first height: ${Math.round(wobBox.y + wobBox.height)} of ${Math.round(dockBox.y + dockBox.height)}`);

// The Song is a mix (review F5): a row for each track's sound it mixes, with that sound's effects and
// a + after it (CARD-36), then the mixer as one compact card (MX-6) and the master. A second effect
// keeps it so. The Duck says what it ducks in its code line and what it listens to (CARD-35).
write("song.ts", read("song.ts").replace('import { duck } from "std/fx";', 'import { compress, duck } from "std/fx";').replace("keys: { in: keys.out,", "keys: { in: compress(keys.out),"));
await page.click('[data-module-tab="song"]');
const songChain = '[data-testid=chain] [data-module="song"]';
const rowsOf = () =>
  page.$$eval(`${songChain} [data-branch]`, (els) => els.map((e) => `${e.dataset.branch}:${[...e.querySelectorAll("[data-node-title]")].map((t) => t.textContent).join(",")}`)).catch(() => []);
check(await until(async () => (await rowsOf()).includes("Keys:Compress"), 15000), `two effects in the song keep its chain: ${JSON.stringify(await rowsOf())}`);
const rows = await rowsOf();
check(JSON.stringify([...rows].sort()) === JSON.stringify(["Bass:Duck", "Hats:", "Keys:Compress", "Kick:", "Wob:"]), `a row for each track's sound the song mixes, with its effects: ${JSON.stringify(rows)}`);
const trunk = await page.$$eval(`${songChain} [data-frame]`, (els) => els.filter((e) => !e.closest("[data-branch]")).map((e) => e.querySelector("[data-node-title]")?.textContent));
check(JSON.stringify(trunk) === '["Mixer","Master"]', `then the mix: ${JSON.stringify(trunk)}`);
check(!!(await page.$(`${songChain} [data-branch="Bass"] button[aria-label="Add an effect after Bass.out"]`)), "each track's sound has a + after it (CARD-36)");
check((await page.$eval(`${songChain} [data-node="duck"] [data-code-line]`, (e) => e.textContent).catch(() => "")) === "duck(bass.out, kick.env, { depth: db(-9) })", "the Duck's code line names the track it ducks");
check(/Listens to:\s*Kick/.test(await page.$eval(`${songChain} [data-node="duck"] [data-listens]`, (e) => e.textContent).catch(() => "")), "the Duck's card says it listens to Kick");
check(!!(await page.$(`${songChain} [data-testid=open-mix]`)) && !(await page.$(`${songChain} [data-node="mixer"] [data-knob]`)), "the mixer is one compact card, its levels in Mix");
// An instrument's card wears its track's colour along its top (CARD-43, review F13); an effect's doesn't.
await page.click('[data-module-tab="keys"] [data-testid=track-label]');
const bar = '[data-testid=chain] [data-module="keys"] [data-node="pluck"] [data-colour-bar]';
check(await until(async () => !!(await page.$(bar))), "Keys' Pluck has its track's colour bar");
const chip = await page.$eval("[data-testid=dock-chip] span", (e) => getComputedStyle(e).backgroundColor);
check((await page.$eval(bar, (e) => getComputedStyle(e).backgroundColor)) === chip && (await page.$eval(bar, (e) => e.getBoundingClientRect().height)) === 3, `in the track's own colour, 3 px tall: ${chip}`);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await until(async () => !!(await page.$(wob)));
check(!(await page.$(`${lp} [data-colour-bar]`)), "the Lowpass, an effect, has none");

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
check((await page.$eval(`${lp} [data-code-line]`, (e) => e.textContent)) === "lowpass(w, { cutoff: 5000 })", "the Lowpass's names its sound as the code does");
check(await page.$eval(`${wob} [data-code-line]`, (e) => getComputedStyle(e).whiteSpace !== "nowrap" && e.scrollWidth <= e.clientWidth), "the code line wraps: the whole call is in sight (review F10)");

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

// A knob left by a click doesn't take the keyboard's focus back when an agent writes it (review F1).
const q = `${lp} [data-knob="q"] svg[role=slider]`;
await page.$eval(q, (e) => e.focus());
const chainBox = await box("[data-testid=chain]");
await page.mouse.click(chainBox.x + chainBox.width - 12, chainBox.y + chainBox.height - 12);
check(await page.evaluate(() => document.activeElement === document.body), "a click on the chain's empty space leaves the knob");
const st = (await api("")).body;
const qId = st.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Lowpass").knobs.find((k) => k.slot === "q").id;
check((await agent("/api/graph/set", { session: st.session, revision: st.view.revision, id: qId, gesture: "agent", value: 2 })) === 200 && (await until(() => read("tracks/wob.ts").includes("q: 2"))), "an agent writes the Lowpass's q, its first write");
await sleep(600);
check(!(await page.$eval(q, (e) => e === document.activeElement).catch(() => false)), "the knob doesn't take the focus back");
const afterAgent = read("tracks/wob.ts");
await page.keyboard.press("ArrowUp");
await sleep(700);
check(read("tracks/wob.ts") === afterAgent, "and ↑ then writes nothing");

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
// A Folded effect keeps its name: its header is the name and the sizes only (review F3).
await page.click(`${lp} [data-size-to="folded"]`);
await until(async () => (await page.$eval(lp, (e) => e.dataset.frame)) === "folded");
const lpTitle = await page.$eval(`${lp} [data-node-title]`, (e) => [e.textContent, Math.round(e.getBoundingClientRect().width)]);
check(lpTitle[0] === "Lowpass" && lpTitle[1] >= 50, `a Folded Lowpass still shows its name: ${JSON.stringify(lpTitle)}`);
await page.click(`${lp} [data-size-to="normal"]`);
await page.click(`${wob} [data-size-to="expanded"]`);
check(await until(async () => (await page.$eval(wob, (e) => e.dataset.frame)) === "expanded"), "Expand expands it");
const panels = await page.$$eval(`${wob} [data-tab-panel]`, (els) => els.map((e) => e.dataset.tabPanel));
check((await width(wob)) === 920 && panels.length === BANDS.length + 1 && !!(await page.$(`${wob} details[data-all]:not([open])`)), `Expanded: twice as wide, every tab side by side, All behind a disclosure: ${await width(wob)} ${panels.join(",")}`);
check(!!(await page.$(`${wob} [data-knob-id="${gain3}"]`)), "the third band's knob is drawn there");
check(/^All values$/.test((await page.$eval(`${wob} details[data-all] > summary`, (e) => e.textContent?.trim()).catch(() => "")) ?? "") && !!(await page.$(`${wob} details[data-all] > summary svg`)), "All opens from \"All values\", with its arrow (review F11)");
await linted(wob, "Wob, expanded");
await page.screenshot({ path: `${ROOT}/frame-expanded.png` });
await page.click(`${wob} [data-size-to="normal"]`);
await until(async () => (await page.$eval(wob, (e) => e.dataset.frame)) === "normal");
await sleep(300);
check(posts.length === sent && read("tracks/wob.ts") === text, `the sizes write nothing: ${posts.slice(sent).map((p) => p.url).join(" ")}`);

// ---------------------------------------------------------------- an agent's change on a hidden tab (CARD-47)

const now = (await api("")).body;
// A face knob changed by an agent glows where it is drawn; no tab lights for it (review F8).
const releaseId = now.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wob").knobs.find((k) => k.slot === "release").id;
check((await agent("/api/graph/set", { session: now.session, revision: now.view.revision, id: releaseId, gesture: "agent", value: 0.5 })) === 200, "an agent sets the release, on the face");
check(await until(async () => !!(await page.$(`${wob} [data-knob="release"][data-glow]`))), "the face's knob glows");
check(!(await page.$(`${wob} [role=tab][data-glow]`)), "and no tab lights for it");
const later = (await api("")).body;
const gain3Now = later.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wob").knobs.find((k) => k.slot === "gain3").id;
check((await agent("/api/graph/set", { session: later.session, revision: later.view.revision, id: gain3Now, gesture: "agent", value: -6 })) === 200, "an agent sets band 3's gain, on a tab not shown");
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
check((await page.$eval("[data-testid=device-chain-hint]", (e) => e.textContent).catch(() => "")) === "This sound is a chain: Chain shows it as cards.", "and says Chain is there (review F15)");
await page.reload({ waitUntil: "domcontentloaded" });
await until(async () => !!(await page.$('[data-module-tab="wob"]')), 15000);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="wob"]'))), "the window keeps the choice");
await page.click("[data-device=chain]");
check(await until(async () => !!(await page.$(chain))), "Chain brings the chain back");

// Two sounds mixed are no line: Device shows the patch and says what makes it so (review F15).
write("tracks/wob.ts", `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { sum } from "std/math";

export default track("wob", { name: "Wob" }, () => sum(osc.saw(110), osc.saw(165)));
`);
check(await until(async () => !!(await page.$('[data-testid=graph] [data-module="wob"]')) && !!(await page.$("[data-testid=device-parts]")), 15000), "a sound built from parts shows its patch, and says so");
check((await page.$eval("[data-device=chain]", (e) => e.disabled)) === true, "Chain can't be picked for it");
check((await page.$eval("[data-testid=device-parts]", (e) => e.textContent)) === "Saw and Saw are mixed here. Showing its patch.", `in those words: ${await page.$eval("[data-testid=device-parts]", (e) => e.textContent)}`);

done(errors);
