// Track views: the card's menus, switches, groups and views (CARD-20 to CARD-29), and a lane of
// names on the timeline. A sound of the song's own, Wobble, declares a choice, a switch, an
// envelope's and a filter's controls, regions, all four views and three groups. Its card shows
// them under their groups' titles (More last), and folds to its first; its shape is a menu whose
// pick writes `shape: "square"`, one undo step ("Undo Wobble shape"), ⌘Z taking it back byte for
// byte; its switch writes `sub: true`. The envelope's attack corner, dragged right, writes a longer
// attack, which the attack knob shows, one undo step; the filter's peak moves cutoff and q in one
// edit. The keys view draws its region. While the song plays, the scope draws the sound's frames.
// Automate on the shape makes a lane of names: a clip on it shows its name, and a click on the clip
// picks a name from a menu there, one points gesture. Each state linted.
//
//   node e2e/card-views.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, key, view, box, posts } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// What the page sends on its socket (audition), as it goes: CDP sees a socket made after it listens.
const socket = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameSent", ({ response }) => {
  try {
    socket.push(JSON.parse(response.payloadData));
  } catch {}
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });

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

/** A mono 16-bit WAV at 44.1 kHz: a 220 Hz tone, a second. */
function wav(file) {
  const rate = 44100;
  const n = rate;
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(0.7 * Math.sin((2 * Math.PI * 220 * i) / rate) * 32767), 44 + i * 2);
  fs.writeFileSync(file, b);
}

// ---------------------------------------------------------------- Wobble, a sound of the song's own

fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/tone.wav`);
fs.mkdirSync(`${PROJ}/data/wobble`, { recursive: true });
write("data/wobble/notes.json", JSON.stringify({ clips: [{ id: "w1", start: 0, length: 4, notes: [{ pitch: 57, start: 0, length: 4, velocity: 0.8 }] }] }));
const TRACK = `import { component, control, port, setting, track, view } from "@m2/core";
import { lowpass } from "std/filter";
import { sampler } from "std/sampler";

const wobble = component(
  {
    id: "wobble",
    label: "Wobble",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(["saw", "square", "sine"], "saw"),
      sub: control.bool(false),
      cutoff: control.hz(2000).range(40, 8000).log(),
      q: control.scalar(0.7),
      attack: control.secs(0.01),
      decay: control.secs(0.2),
      sustain: control.frac(0.6),
      release: control.secs(0.3),
    },
    settings: { regions: setting.regions() },
    output: port.audio(),
    editor: {
      views: { amp: view.envelope("attack", "decay", "sustain", "release"), filter: view.filter("cutoff", "q"), keys: view.keys("regions"), scope: view.scope() },
      groups: { Sound: ["shape", "sub", "scope"], Filter: ["filter", "cutoff", "q"], Amp: ["amp", "attack", "decay", "sustain", "release"] },
    },
  },
  (notes, { cutoff, q, attack, release, regions }) => lowpass(sampler(notes, { regions, attack, release, mode: "gate" }), { cutoff, q }),
);

export default track("wobble", { name: "Wobble" }, ({ lane }) =>
  wobble(lane.notes("notes"), { shape: "saw", regions: [{ file: "tone.wav", root: 57, keys: [45, 69] }] }),
);
`;
write("tracks/wobble.ts", TRACK);
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport wobble from "./tracks/wobble";').replace("      kick: kick.out,", "      kick: kick.out,\n      wobble: wobble.out,"));

await until(async () => !!(await page.$('[data-module-tab="wobble"]')), 15000);
await page.click('[data-module-tab="wobble"] [data-testid=track-label]');
await view("graph");
const card = '[data-module="wobble"] [data-node="wobble"]';
check(await until(async () => !!(await page.$(`${card} [data-group]`)), 15000), "Wobble's card shows its groups");
await page.$eval(card, (e) => e.scrollIntoView({ block: "start", inline: "center" }));
await sleep(200);

// ---------------------------------------------------------------- groups (CARD-25)

const groups = () => page.$$eval(`${card} [data-group]`, (els) => els.map((e) => e.textContent));
check(JSON.stringify(await groups()) === '["Sound","Filter","Amp","More"]', `its groups' titles, in their order, More last: ${JSON.stringify(await groups())}`);
const order = await page.$$eval(`${card} [data-node-body] > *`, (els) => els.map((e) => e.dataset.group ?? e.dataset.knob ?? e.dataset.view ?? e.dataset.setting ?? e.dataset.input ?? "?"));
const at = (x) => order.indexOf(x);
check(
  at("Sound") < at("shape") && at("shape") < at("sub") && at("sub") < at("scope") && at("scope") < at("Filter") && at("Filter") < at("filter") && at("amp") < at("attack") && at("More") < at("regions") && at("regions") < at("keys"),
  `each row under its group, as the sound names them: ${order.join(" ")}`,
);
await linted(card, "Wobble's card");
await page.screenshot({ path: `${ROOT}/views-card.png` });
await page.click(`${card} [data-fold]`);
check(await until(async () => JSON.stringify(await groups()) === '["Sound"]'), `folded, it shows its first group: ${JSON.stringify(await groups())}`);
check(!!(await page.$(`${card} [data-unfold]`)), "and says how many more rows there are");
// A knob the folded card hides is still a knob (AGENT-5, AGENT-6): an agent's change to it lights
// the "N more" row, an agent's pointer at it shows there, and Show unfolds the card to it.
const state = (await api("")).body;
const sustainId = state.view.modules.find((m) => m.key === "wobble").nodes.find((n) => n.title === "Wobble").knobs.find((k) => k.slot === "sustain").id;
const agent = (path, body) =>
  page.evaluate(
    async (url, key, path, body) => (await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify(body) })).status,
    sess.url,
    sess.token,
    path,
    body,
  );
check((await agent("/api/graph/set", { session: state.session, revision: state.view.revision, id: sustainId, gesture: "agent", value: 0.5 })) === 200, "an agent sets the sustain, which the folded card hides");
check(await until(async () => !!(await page.$(`${card} [data-unfold][data-glow]`))), "its change lights the row that stands for it");
// Written now, the knob has its written id.
const sustainNow = (await api("")).body.view.modules.find((m) => m.key === "wobble").nodes.find((n) => n.title === "Wobble").knobs.find((k) => k.slot === "sustain").id;
check((await agent("/api/point", { session: state.session, target: `knob:${sustainNow}`, words: "this one" })) === 200, "an agent points at it");
check(await until(async () => !!(await page.$("[data-testid=pointer-show]"))), "the pointer offers Show");
await page.click("[data-testid=pointer-show]");
check(await until(async () => (await groups()).length === 4 && !!(await page.$(`${card} [data-knob-id="${sustainNow}"]`))), "Show unfolds the card to the knob");
await page.click("[data-testid=pointer-ok]").catch(() => {});

// ---------------------------------------------------------------- a choice's menu and a switch (CARD-20, CARD-21)

const tracked = read("tracks/wobble.ts");
const shape = `${card} [data-knob="shape"]`;
check((await page.$eval(shape, (e) => `${e.dataset.kind} ${e.dataset.value} ${!!e.dataset.knobId}`)) === 'choice "saw" true', "shape is a menu, at saw, and still a knob (its id)");
await page.click(`${shape} [data-testid=setting-menu]`);
await page.waitForSelector("[data-testid=setting-options]");
const options = await page.$$eval("[data-testid=setting-options] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(options) === '["saw","square","sine"]', `its menu lists its names: ${JSON.stringify(options)}`);
await linted("[data-testid=setting-options]", "the shape's menu");
let n = posts.length;
await page.click('[data-testid=setting-options] [data-option="square"]');
check(await until(() => read("tracks/wobble.ts") === tracked.replace('{ shape: "saw",', '{ shape: "square",')), `picking square writes its name: ${read("tracks/wobble.ts").match(/shape: "\w+"/)?.[0]}`);
check(posts.slice(n).filter((p) => p.url === "/api/graph/set").length === 1, "one write, no drag");
check(await stepIs("Wobble shape"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await page.$eval(shape, (e) => e.dataset.value)) === '"square"'), "the menu shows square");
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/wobble.ts") === tracked), "⌘Z takes the code back, byte for byte");
const sub = `${card} [data-knob="sub"] [role=switch]`;
await seen(sub);
check((await page.$eval(sub, (e) => e.getAttribute("aria-checked"))) === "false", "sub is a switch, off");
await page.click(sub);
check(await until(() => read("tracks/wobble.ts").includes("sub: true }")), `flipping it writes sub: true: ${read("tracks/wobble.ts").match(/wobble\(lane[^\n]*/)?.[0]}`);
check(await stepIs("Wobble sub"), `one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/wobble.ts") === tracked), "and ⌘Z takes it back");

// ---------------------------------------------------------------- the envelope's corner (CARD-27, CARD-22)

const attack = `${card} [data-knob="attack"]`;
const attackBefore = await page.$eval(attack, (e) => Number(e.dataset.value));
const handle = await seen(`${card} [data-view="amp"] [data-handle="attack"]`);
check(!!handle, "the envelope draws its attack corner");
await linted(`${card} [data-view="amp"]`, "the envelope");
n = posts.length;
await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
await page.mouse.down();
await page.mouse.move(handle.x + handle.width / 2 + 30, handle.y + handle.height / 2, { steps: 10 });
await sleep(200);
await page.mouse.up();
check(await until(() => /attack: [0-9.]+/.test(read("tracks/wobble.ts"))), `dragging the attack corner right writes the attack: ${read("tracks/wobble.ts").match(/attack: [0-9.]+/)?.[0]}`);
const written = Number(read("tracks/wobble.ts").match(/attack: ([0-9.]+)/)?.[1]);
check(written > attackBefore, `a longer attack: ${attackBefore} → ${written}`);
check(posts.slice(n).some((p) => p.url === "/api/graph/preview"), "heard while dragged (a lease)");
check(await until(async () => Math.abs((await page.$eval(attack, (e) => Number(e.dataset.value))) - written) < 1e-9), `the attack knob shows it: ${await page.$eval(attack, (e) => e.dataset.value)}`);
check(await stepIs("Wobble attack"), `one undo step, named by what moved: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/wobble.ts") === tracked), "⌘Z takes it back");

// The filter's peak: cutoff and q in one edit.
const peak = await seen(`${card} [data-view="filter"] [data-handle="peak"]`);
await page.mouse.move(peak.x + peak.width / 2, peak.y + peak.height / 2);
await page.mouse.down();
await page.mouse.move(peak.x + peak.width / 2 - 25, peak.y + peak.height / 2 - 15, { steps: 10 });
await sleep(200);
await page.mouse.up();
check(
  await until(() => /cutoff: \d+/.test(read("tracks/wobble.ts")) && /q: [0-9.]+/.test(read("tracks/wobble.ts"))),
  `dragging the filter's peak writes cutoff and q: ${read("tracks/wobble.ts").match(/wobble\(lane[^\n]*/)?.[0]}`,
);
check(await stepIs("Wobble cutoff and q"), `as one undo step: ${await undoTitle()}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/wobble.ts") === tracked), "one ⌘Z takes both back");
// Dragged far past the cutoff's declared range (40 Hz to 8 kHz), the peak stops at its end, as a knob's drag does.
const peak2 = await seen(`${card} [data-view="filter"] [data-handle="peak"]`);
await page.mouse.move(peak2.x + peak2.width / 2, peak2.y + peak2.height / 2);
await page.mouse.down();
await page.mouse.move(peak2.x + 400, peak2.y + peak2.height / 2, { steps: 10 });
await sleep(200);
await page.mouse.up();
check(await until(() => /cutoff: 8000\b/.test(read("tracks/wobble.ts"))), `it stops at 8 kHz: ${read("tracks/wobble.ts").match(/cutoff: \d+/)?.[0]}`);
await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/wobble.ts") === tracked), "and ⌘Z takes it back");

// The keys view: its region over its keys; a click plays the key under it, as hard as it is high.
await seen(`${card} [data-view="keys"]`);
await page.screenshot({ path: `${ROOT}/views-pictures.png` });
check(!!(await page.$(`${card} [data-view="keys"] [data-region="tone.wav"]`)), "the keys view draws tone.wav's region");
const keyA3 = await box(`${card} [data-view="keys"] [data-key="57"]`);
const regionBox = await box(`${card} [data-view="keys"] [data-region="tone.wav"]`);
const from = socket.length;
await page.mouse.move(keyA3.x + keyA3.width / 2, regionBox.y + 2);
await page.mouse.down();
check(await until(async () => !!(await page.$(`${card} [data-view="keys"] [data-key="57"][data-playing]`))), "pressed, A3 plays");
await page.mouse.up();
const struck = socket.slice(from).find((m) => m.t === "audition" && m.on?.length);
check(struck?.module === "wobble" && struck.lane === "notes" && struck.on[0].pitch === 57 && struck.on[0].velocity > 0.9, `on Wobble's notes lane, hard near the top: ${JSON.stringify(struck)}`);
check(await until(() => socket.slice(from).some((m) => m.t === "audition" && m.off?.length)), "and let go when the pointer lifts");

// ---------------------------------------------------------------- the scope while playing (CARD-29)

await post("/loop", { start: 0, end: 4, on: true });
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
await seen(`${card} [data-scope]`);
const frames = () => page.$eval(`${card} [data-scope]`, (e) => Number(e.dataset.frames ?? 0)).catch(() => 0);
check(await until(async () => (await frames()) >= 5, 10000), `while the song plays, the scope draws the sound's frames (${await frames()} drawings)`);
await page.keyboard.press("Space");
// Out of sight, the card stops asking (TAP-4): scrolled out of the Device view, or the view left.
const scopeAsks = () => posts.filter((p) => p.url === "/api/graph/scope").map((p) => JSON.parse(p.body).on);
await page.$eval("[data-testid=graph]", (e) => e.scrollTo({ top: 0, left: 0 }));
await page.$eval(`${card} [data-scope]`, (e) => {
  const g = document.querySelector("[data-testid=graph]");
  g.scrollTop += e.getBoundingClientRect().bottom - g.getBoundingClientRect().top + 50;
});
check(await until(() => scopeAsks().at(-1) === false), `scrolled out of sight, the scope stops asking: ${JSON.stringify(scopeAsks().slice(-3))}`);
await seen(`${card} [data-scope]`);
check(await until(() => scopeAsks().at(-1) === true), "back in sight, it asks again");
await view("mix");
check(await until(() => scopeAsks().at(-1) === false), "the Device view left, it stops");
await view("graph");

// ---------------------------------------------------------------- a lane of names (CARD-23, CARD-24)

const knobBox = await seen(shape);
await page.mouse.click(knobBox.x + 40, knobBox.y + knobBox.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu]");
await page.click('[data-testid=knob-menu] [data-menu-item="automate"]');
check(await until(() => read("tracks/wobble.ts").includes('lane.control("shape", "choice", "saw")')), `Automate makes a lane of names: ${read("tracks/wobble.ts").match(/lane\.control\([^)]*\)/)?.[0]}`);
check(await until(async () => !!(await page.$('[data-lane="wobble.shape"]'))), "the timeline shows it");
const row = await seen('[data-lane="wobble.shape"] > div:last-child');
await page.mouse.click(row.x + 40, row.y + row.height / 2, { count: 2 });
const clip = '[data-lane="wobble.shape"] [data-clip]';
check(await until(async () => !!(await page.$(`${clip} [data-step="saw"]`))), "a double-click makes a clip holding saw, its name on it");
const file = () => JSON.parse(read("data/wobble/shape.json"));
const c = await box(clip);
n = posts.length;
await page.mouse.click(c.x + c.width * 0.6, c.y + c.height / 2);
check(await until(async () => !!(await page.$("[data-testid=clip-menu] [data-menu-item]"))), "a click on the clip opens the menu of names there");
await linted("[data-testid=clip-menu]", "the menu of names");
const names = await page.$$eval("[data-testid=clip-menu] [data-menu-item]", (els) => els.map((e) => e.textContent));
check(JSON.stringify(names) === '["saw","square","sine"]', `it lists the names: ${JSON.stringify(names)}`);
await page.click('[data-testid=clip-menu] [data-menu-item="name-sine"]');
check(await until(() => file().clips[0]?.points.length === 2 && file().clips[0].points[1][1] === "sine"), `picking sine draws a point of it: ${JSON.stringify(file().clips[0]?.points)}`);
const sent = posts.slice(n).filter((p) => p.url === "/api/graph/clips").map((p) => JSON.parse(p.body).gesture?.op);
check(JSON.stringify(sent) === '["points"]', `one points gesture: ${JSON.stringify(sent)}`);
check(await until(async () => !!(await page.$(`${clip} [data-step="sine"]`))), "the clip shows sine from there");
check(await stepIs("adding a Shape point"), `one undo step: ${await undoTitle()}`);
await page.screenshot({ path: `${ROOT}/views-steps.png` });
await linted('[data-lane="wobble.shape"]', "the lane of names");

// A step's mark dragged moves its point, and picks it; Esc unpicks it; ⌫ deletes the one picked.
const mark = async () => {
  const b = await box(`${clip} [data-step="sine"]`);
  return { x: b.x + 1, y: b.y + b.height / 2 };
};
let m = await mark();
await page.mouse.move(m.x, m.y);
await page.mouse.down();
await page.mouse.move(m.x + 15, m.y, { steps: 6 });
await page.mouse.up();
check(await until(() => file().clips[0]?.points[1]?.[0] > 2.5), `dragging sine's mark moves its point: ${JSON.stringify(file().clips[0]?.points)}`);
check(await until(async () => !!(await page.$(`${clip} [data-step="sine"][data-picked]`))), "and picks it");
await key("Escape");
check(await until(async () => !(await page.$(`${clip} [data-step="sine"][data-picked]`))), "Esc unpicks it");
m = await mark();
await page.mouse.move(m.x, m.y);
await page.mouse.down();
await page.mouse.move(m.x - 8, m.y, { steps: 3 });
await page.mouse.up();
await until(async () => !!(await page.$(`${clip} [data-step="sine"][data-picked]`)));
await sleep(300);
await key("Backspace");
check(await until(() => file().clips[0]?.points.length === 1), `⌫ deletes the point picked: ${JSON.stringify(file().clips[0]?.points)}`);
check(await stepIs("deleting a Shape point"), `one undo step: ${await undoTitle()}`);

done(errors);
