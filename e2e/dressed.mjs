// music2's own sounds on the card (CARD-30 to CARD-33, wave 3), on a fresh copy of examples/first-song
// with a Lead track added. The Lead's card shows its groups; its wave shape is a menu of four names,
// and picking sine writes `shape: "sine"`, one undo step, and the sound changes while it plays: the
// scope's frames lose the saw's sharp edge (their slope's energy falls). Its
// envelope's attack corner, dragged right, writes a longer attack; its filter's peak, dragged up, a
// higher resonance in hundredths, and its words say resonance. The kick plays One drum: its menu (a
// control) lists the nine drums, and picking the snare writes `drum: "snare"`, one undo step named
// by its label ("One drum sound"), ⌘Z taking it back byte for byte. Each card linted.
//
//   node e2e/dressed.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, key, view, box, posts, caughtUp } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

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
/** A route called as an agent (its own token), with what it answers. */
const agent = (path, body) =>
  page.evaluate(
    async (url, key, path, body) => {
      const r = await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify(body) });
      return { status: r.status, body: await r.json().catch(() => ({})) };
    },
    sess.url,
    sess.token,
    path,
    body,
  );

// ---------------------------------------------------------------- a Lead, holding one note

fs.mkdirSync(`${PROJ}/data/lead`, { recursive: true });
write("data/lead/notes.json", JSON.stringify({ clips: [{ id: "l1", start: 0, length: 4, notes: [{ pitch: 57, start: 0, length: 4, velocity: 0.8 }] }] }));
write(
  "tracks/lead.ts",
  'import { track } from "@m2/core";\nimport { synth } from "@m2/synths";\n\nexport default track("lead", { name: "Lead" }, ({ lane }) => synth.lead(lane.notes("notes")));\n',
);
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport lead from "./tracks/lead";').replace("      kick: kick.out,", "      kick: kick.out,\n      lead: lead.out,"));
await until(async () => !!(await page.$('[data-module-tab="lead"]')), 15000);
await page.click('[data-module-tab="lead"] [data-testid=track-label]');
await view("graph");
const card = '[data-module="lead"] [data-node="synth.lead"]';
check(await until(async () => !!(await page.$(`${card} [data-group]`)), 15000), "the Lead's card shows its groups");
const groups = await page.$$eval(`${card} [data-group]`, (els) => els.map((e) => e.textContent));
check(JSON.stringify(groups.slice(0, 3)) === '["Sound","Filter","Loudness"]', `Sound, Filter and Loudness first (CARD-31): ${JSON.stringify(groups)}`);
await linted(card, "the Lead's card");

// ---------------------------------------------------------------- the wave shape (CARD-32)

const tracked = read("tracks/lead.ts");
const shape = `${card} [data-knob="shape"]`;
await seen(shape);
check((await page.$eval(shape, (e) => `${e.dataset.kind} ${e.dataset.value}`).catch(() => null)) === 'choice "saw"', "the shape is a menu, at saw");
await page.click(`${shape} [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const shapes = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(shapes) === '["saw","square","triangle","sine"]', `its menu lists the four waves: ${JSON.stringify(shapes)}`);
await linted("[data-testid=setting-options]", "the shape's menu");
await page.keyboard.press("Escape");
await page.mouse.click(10, 10);

// While the song plays, the Lead's scope: its frames' slope energy against their energy, which a
// saw's edge makes high and a sine's roundness low, whatever the phase the frames start at.
const s0 = (await api("")).body;
const leadId = s0.view.modules.find((m) => m.key === "lead").nodes.find((n) => n.title === "Lead").id;
const scope = async () => {
  const r = await agent("/api/graph/scope", { session: s0.session, client: "dressed", module: "lead", node: leadId, on: true });
  const x = r.body.scope?.frames?.[0] ?? [];
  let e = 0, d = 0;
  for (let i = 1; i < x.length; i++) {
    e += x[i] * x[i];
    d += (x[i] - x[i - 1]) ** 2;
  }
  return e > 1e-6 ? d / e : null;
};
/** The ratio, once the scope has frames that sound: the median of five, a few frames apart. */
const edge = async () => {
  const seen = [];
  for (let i = 0; i < 40 && seen.length < 5; i++) {
    const r = await scope();
    if (r !== null) seen.push(r);
    await sleep(120);
  }
  return seen.length ? seen.sort((a, b) => a - b)[Math.floor(seen.length / 2)] : null;
};
await post("/loop", { start: 0, end: 4, on: true });
await post("/transport", { action: "play" });
check(await until(async () => (await api("/status")).body.playing), "the song plays");
const sawEdge = await edge();
check(sawEdge !== null, `the scope shows the Lead's saw (${sawEdge?.toFixed(4)})`);
let n = posts.length;
await page.click(`${shape} [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
await page.click('[data-testid=setting-options] [data-option="sine"]');
check(await until(() => read("tracks/lead.ts") === tracked.replace('synth.lead(lane.notes("notes"))', 'synth.lead(lane.notes("notes"), { shape: "sine" })')), `picking sine writes its name: ${read("tracks/lead.ts").match(/synth\.lead\([^\n]*/)?.[0]}`);
check(posts.slice(n).filter((p) => p.url === "/api/graph/set").length === 1, "one write");
check(await stepIs("Lead shape"), `one undo step: ${await undoTitle()}`);
let sineEdge = null;
await until(async () => {
  sineEdge = await edge();
  return sineEdge !== null && sawEdge !== null && sineEdge < sawEdge / 3;
}, 15000);
check(sineEdge !== null && sawEdge !== null && sineEdge < sawEdge / 3, `and the sound turns round: the frames' slope energy ${sawEdge?.toFixed(4)} → ${sineEdge?.toFixed(4)}`);
await post("/transport", { action: "stop" });
await agent("/api/graph/scope", { session: s0.session, client: "dressed", module: "lead", node: leadId, on: false });
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/lead.ts") === tracked), "⌘Z takes the code back, byte for byte");

// ---------------------------------------------------------------- the envelope's corner (CARD-31)

// A drag goes from the revision the page shows, and ⌘Z changed this call: the page shows it first.
await caughtUp();
const attack = `${card} [data-knob="attack"]`;
const attackBefore = await page.$eval(attack, (e) => Number(e.dataset.value));
const handle = await seen(`${card} [data-view="amp"] [data-handle="attack"]`);
check(!!handle, "the Lead's envelope draws its attack corner");
await linted(`${card} [data-view="amp"]`, "the envelope");
await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
await page.mouse.down();
await page.mouse.move(handle.x + handle.width / 2 + 30, handle.y + handle.height / 2, { steps: 10 });
await sleep(200);
await page.mouse.up();
check(await until(() => /attack: [0-9.]+/.test(read("tracks/lead.ts"))), `dragging the attack corner right writes the attack: ${read("tracks/lead.ts").match(/attack: [0-9.]+/)?.[0]}`);
const written = Number(read("tracks/lead.ts").match(/attack: ([0-9.]+)/)?.[1]);
check(written > attackBefore, `a longer attack: ${attackBefore} → ${written}`);
check(await stepIs("Lead attack"), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/lead.ts") === tracked), "⌘Z takes it back");
// ---------------------------------------------------------------- the filter's resonance (CARD-30)

// The Lead's filter view draws its resonance (0 to 1) as the q it stands for; its peak dragged up
// writes a higher resonance, in hundredths, inside 0 to 1, and its words say resonance. Once the
// page shows the revision ⌘Z made, as the corner's drag did.
await caughtUp();
const peakSel = `${card} [data-view="filter"] [data-handle="peak"]`;
const peakAt = await seen(peakSel);
const said = await page.$eval(peakSel, (e) => e.querySelector("title")?.textContent ?? "");
check(!!peakAt && said.includes("resonance") && !said.includes("· q "), `the Lead's filter view names its resonance: ${said.split("\n")[0]}`);
const resonanceBefore = await page.$eval(`${card} [data-knob="resonance"]`, (e) => Number(e.dataset.value));
await page.mouse.move(peakAt.x + peakAt.width / 2, peakAt.y + peakAt.height / 2);
await page.mouse.down();
await page.mouse.move(peakAt.x + peakAt.width / 2, peakAt.y + peakAt.height / 2 - 25, { steps: 10 });
await sleep(200);
await page.mouse.up();
check(await until(() => /resonance: [0-9.]+/.test(read("tracks/lead.ts"))), `dragging the peak up writes the resonance: ${read("tracks/lead.ts").match(/synth\.lead\([^\n]*/)?.[0]}`);
const res = read("tracks/lead.ts").match(/resonance: ([0-9.]+)/)?.[1] ?? "";
check(Number(res) > resonanceBefore && Number(res) <= 1 && /^\d(\.\d{1,2})?$/.test(res), `higher, in hundredths, inside 0 to 1: ${resonanceBefore} → ${res}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/lead.ts") === tracked), "⌘Z takes it back");
await page.screenshot({ path: `${ROOT}/dressed-lead.png` });

// ---------------------------------------------------------------- One drum's menu (CARD-33)

await page.click('[data-module-tab="kick"] [data-testid=track-label]');
await view("graph");
const drum = '[data-module="kick"] [data-node="drums.drum"]';
check(await until(async () => !!(await page.$(`${drum} [data-knob="sound"]`)), 15000), "the kick's One drum shows its drum menu");
check(JSON.stringify(await page.$$eval(`${drum} [data-group]`, (els) => els.map((e) => e.textContent))) === '["Drum","More"]', "under its Drum group");
const kick = read("tracks/kick.ts");
const menu = `${drum} [data-knob="sound"]`;
await seen(menu);
check((await page.$eval(menu, (e) => e.dataset.value)) === '"kick"', "at kick, its default");
await page.click(`${menu} [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const drums = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(drums) === '["kick","snare","clap","hats","tom","rim","cowbell","cymbal","shaker"]', `it lists the nine drums: ${JSON.stringify(drums)}`);
await linted("[data-testid=setting-options]", "the drum menu");
n = posts.length;
await page.click('[data-testid=setting-options] [data-option="snare"]');
check(await until(() => read("tracks/kick.ts") === kick.replace('drums.drum(lane.notes("hits"))', 'drums.drum(lane.notes("hits"), { drum: "snare" })')), `picking the snare writes it: ${read("tracks/kick.ts").match(/drums\.drum\([^\n]*/)?.[0]}`);
check(posts.slice(n).filter((p) => p.url === "/api/graph/set").length === 1, "one write");
check(await stepIs("One drum sound"), `one undo step, named by its label: ${await undoTitle()}`);
check(await until(async () => (await page.$eval(menu, (e) => e.dataset.value).catch(() => null)) === '"snare"'), "the menu shows the snare");
for (const k of ["tune", "decay ×", "punch ±"]) check(!!(await page.$(`${drum} [data-knob="${k}"]`)), `its ${k} knob stays`);
await linted(drum, "One drum's card");
await page.screenshot({ path: `${ROOT}/dressed-drum.png` });
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === kick), "⌘Z takes it back, byte for byte");

done(errors);
