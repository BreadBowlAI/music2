// Track graph-node: the graph's nodes (taste.md DV-12 to DV-16, DV-19; the graph note §4 to §7). A
// sound of the song's own, Wob, names a face (its envelope, a choice of three and two values), six
// bands as groups and a value the code works out; a Lowpass follows it, and an LFO, wobble, waits
// unwired. Each node starts collapsed: 216 px, its face's first picture 48 px tall, up to four
// values as bars, +N for the rest. The bar drags up and down only (⇧ fine), types on a
// double-click and resets on ⌥-click or ⌫; its fill follows the knob's curve, and a value with two
// sides fills from the middle. Expanded, a node shows its views across the top at 132 px, every
// group as a column and its code line; the ends of its wires stay where they were, the others move
// aside and the zoom stays. A click selects a node, Esc clears it, Tab goes through the nodes,
// Enter expands, Delete removes an effect. +N stands for the values it hides: an agent's change
// lights it, and Show expands the node. A wire dropped on a collapsed node offers its values. A
// value a wire, a lane or the code drives is a row naming its source. The expanded set is kept
// across an effect added before a node, and its undo. The song's mixer is one compact node, and
// a note effect is a node in the graph. Each state linted.
//
//   node e2e/nodes.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";
import { expand } from "./graphlib.mjs";

const { page, errors, api, view, box, posts, key, caughtUp, toasts } = await open(1440, 900);

/** A write or a pointer from outside the page: an agent's (AGENT-5, AGENT-6). */
const agent = (path, body) =>
  page.evaluate(
    async (url, token, path, body) => (await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify(body) })).status,
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
/** Once the nodes stop moving aside after one expands or collapses (DV-13). */
const still = () => until(async () => !(await page.$("[data-moving]")));
/**
 * The wires of module `m` that run behind a node they don't touch (taste.md DV-5): each path
 * sampled every 4 px, a point inside another node's box (2 px in) counting.
 */
const behind = (m) =>
  page.evaluate((m) => {
    const g = document.querySelector(`[data-testid=graph] [data-module="${m}"]`);
    const nodes = [...g.querySelectorAll("[data-node-id]")].map((n) => [n.dataset.nodeId, n.getBoundingClientRect()]);
    const out = [];
    for (const p of g.querySelectorAll("path[data-from]")) {
      const len = p.getTotalLength();
      const ctm = p.getScreenCTM();
      for (let s = 0; s <= len; s += 4) {
        const q = p.getPointAtLength(s).matrixTransform(ctm);
        const hit = nodes.find(([id, r]) => id !== p.dataset.from && id !== p.dataset.to && q.x > r.left + 2 && q.x < r.right - 2 && q.y > r.top + 2 && q.y < r.bottom - 2);
        if (hit) {
          out.push(`${p.dataset.from} → ${p.dataset.to} behind ${hit[0]}`);
          break;
        }
      }
    }
    return out;
  }, m);
const size = (sel) => page.$eval(sel, (e) => e.dataset.size).catch(() => null);
// Sizes as laid out, before the graph's zoom scales them on screen.
const width = (sel) => page.$eval(sel, (e) => e.offsetWidth).catch(() => null);
const height = (sel) => page.$eval(sel, (e) => e.offsetHeight).catch(() => null);

/** How many of examples/sounds' Song's nodes the dock's first view holds, at the least (DV-5): 3 before track graph-insert. */
const FILLED = 6;

// ---------------------------------------------------------------- Wob, then a Lowpass; an LFO waits

const BANDS = [1, 2, 3, 4, 5, 6];
const TRACK = `import { component, control, hz, khz, port, track } from "@m2/core";
import { lfo } from "std/control";
import { lowpass } from "std/filter";
import { osc } from "std/dsp";
import { xrange } from "std/math";
import { envelope, envelopePicture, envelopeWords } from "std/pictures";

const ENV = { attack: "attack", decay: "decay", sustain: "sustain", release: "release" } as const;

const wob = component(
  {
    id: "wob",
    label: "Wobbly Bass Synth",
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
      views: { amp: envelopePicture() },
      face: ["amp", "shape", "cutoff", "release"],
      groups: { ${BANDS.map((b) => `"Band ${b}": ["on${b}", "gain${b}"]`).join(", ")} },
    },
  },
  () => osc.saw(110),
  { amp: { draw: (v, p) => envelope(p, v, ENV), words: (v) => envelopeWords(v, ENV) } },
);

export default track("wob", { name: "Wob" }, ({ lane, time }) => {
  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));
  const w = wob(lane.notes("notes"), { cutoff: 2000, decay: 0.1 * 2 });
  return lowpass(w, { cutoff: 5000 });
});
`;
write("tracks/wob.ts", TRACK);
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport wob from "./tracks/wob";').replace("      kick: kick.out,", "      kick: kick.out,\n      wob: wob.out,"));

await until(async () => !!(await page.$('[data-module-tab="wob"]')), 15000);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await view("graph");
const g = '[data-testid=graph] [data-module="wob"]';
const wob = `${g} [data-node="wob"]`;
const lp = `${g} [data-node="lowpass"]`;
const state = async () => (await api("")).body;
const wobNode = (st) => st.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Wobbly Bass Synth");
const knobId = (st, slot) => wobNode(st).knobs.find((k) => k.slot === slot)?.id;

// ---------------------------------------------------------------- collapsed, the default (taste.md DV-12)

check(await until(async () => (await size(wob)) === "collapsed", 15000), "the dock's tab is Graph, and Wob's node starts collapsed (DV-12)");
// Fit is 100%: the graph, taller than the dock, scrolls rather than shrinking its words (DV-5, review Z8).
check((await page.$eval("[data-zoom]", (e) => e.dataset.zoom)) === "1.00", `Fit is 100%: ${await page.$eval("[data-zoom]", (e) => e.dataset.zoom)}`);
check((await page.$eval('[data-view-tab="graph"]', (e) => e.textContent?.trim()).catch(() => "")) === "Graph", "the tab is called Graph");
check((await size(lp)) === "collapsed", "and so does the Lowpass's");
check((await width(wob)) === 216 && (await width(lp)) === 216, `a collapsed node is 216 px wide: ${await width(wob)} ${await width(lp)}`);
check(!!(await page.$(`${g} path[data-from]`)) && !!(await page.$(`${wob} [data-port-dot]`)), "the graph draws its wires and its inputs");
check(!!(await page.$(`${lp} [data-input] [data-port-dot][data-kind="audio"]`)), "the Lowpass's sound comes in on a row of its own, its circle on the left edge, though a const names it");
check(!(await page.$(`${lp} [data-inlet]`)), "and with no ✕: rewiring a sound is for code");
const ampH = await height(`${wob} [data-view="amp"]`);
check(ampH === 48, `its face's first view, the envelope, 48 px tall (DV-19): ${ampH}`);
const bars = await page.$$eval(`${wob} [data-knob]`, (els) => els.map((e) => e.dataset.knob));
check(JSON.stringify(bars) === '["shape","cutoff","release"]', `its face's values that can be set, in the face's order: ${JSON.stringify(bars)}`);
const [b0, b1] = [await box(`${wob} [data-knob="shape"]`), await box(`${wob} [data-knob="cutoff"]`)];
check(Math.abs(b0.y - b1.y) < 1 && b1.x > b0.x, "two to a row");
let st = await state();
const values = wobNode(st).knobs.length + wobNode(st).readonly.length;
const more = await page.$eval(`${wob} [data-more]`, (e) => [e.textContent?.trim(), e.dataset.hides ?? ""]).catch(() => [null, ""]);
check(more[0] === `+${values - 3}`, `+N in its header counts every value it hides: ${more[0]} of ${values}`);
check(more[1].split(" ").includes(knobId(st, "gain3")) && !more[1].split(" ").includes(knobId(st, "cutoff")), "and stands for each one it hides, never one drawn (DV-16)");
check(!(await page.$(`${wob} [data-code-line]`)) && !(await page.$(`${wob} [data-presets]`)), "collapsed, no code line and no Presets");
const title = await page.$eval(`${wob} [data-node-title]`, (e) => [e.textContent, e.scrollWidth <= e.clientWidth, getComputedStyle(e).textOverflow]);
check(title[0] === "Wobbly Bass Synth" && title[1] && title[2] !== "ellipsis", `a 17-letter label shows whole, never shortened: ${JSON.stringify(title)}`);
check(!(await page.$(`${lp} [data-colour-bar]`)), "an effect wears no track colour");
const lpTitleTip = await page.$eval(`${lp} [data-node-title]`, (e) => e.title).catch(() => "");
check(/std/.test(lpTitleTip), `the pack is the label's tooltip: ${lpTitleTip}`);
check((await height(`${wob} [data-node-head]`)) >= 24, `its header is 24 px or more: ${await height(`${wob} [data-node-head]`)}`);
const dockBox = await box("#dock-body");
const wobBox = await box(wob);
check(wobBox.y + wobBox.height <= dockBox.y + dockBox.height + 1, `the collapsed node fits the dock at its first height: ${Math.round(wobBox.y + wobBox.height)} of ${Math.round(dockBox.y + dockBox.height)}`);
await linted(g, "the graph, collapsed");
const untipped = await page.evaluate(tips, g, {});
check(untipped.length === 0, `every control in the graph has a tooltip${untipped.length ? `: ${untipped.slice(0, 3).join(" | ")}` : ""}`);
await page.screenshot({ path: `${ROOT}/nodes-collapsed.png` });

// ---------------------------------------------------------------- the bar (taste.md DV-14)

const slider = (name, at = wob) => `${at} [data-knob="${name}"] [role=slider]`;
const unit = (name, at = wob) => page.$eval(slider(name, at), (e) => Number(e.getAttribute("aria-valuenow"))).catch(() => NaN);
const fill = (name, at = wob) => page.$eval(slider(name, at), (e) => [Number(e.dataset.lo), Number(e.dataset.hi)]).catch(() => [NaN, NaN]);
// The cutoff, 2000 Hz from 40 to 8000 on a log scale: ln(50) / ln(200) of the way, not 0.25 as a straight line has it.
const [lo, hi] = await fill("cutoff");
check(lo === 0 && Math.abs(hi - Math.log(50) / Math.log(200)) < 0.01, `the cutoff fills on its log curve, from the left: ${lo} to ${hi}`);
check(!!(await page.$(`${wob} [data-knob="cutoff"] [data-fill]`)), "the fill is drawn");
const drag = async (sel, dx, dy, { shift = false } = {}) => {
  const b = await box(sel);
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  if (shift) await page.keyboard.down("Shift");
  await page.mouse.down();
  await page.mouse.move(b.x + b.width / 2 + dx, b.y + b.height / 2 + dy, { steps: 10 });
  await page.mouse.up();
  if (shift) await page.keyboard.up("Shift");
};
let text = read("tracks/wob.ts");
await drag(slider("cutoff"), 90, 0);
await sleep(800);
check(read("tracks/wob.ts") === text && Math.abs((await unit("cutoff")) - hi) < 0.005, "a sideways drag moves nothing: the bar drags up and down only");
// The wheel never edits a bar: it scrolls the graph (DV-14, review Z11).
{
  const b = await box(slider("cutoff"));
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await page.mouse.wheel({ deltaY: 200 });
  await sleep(900);
  check(read("tracks/wob.ts") === text && Math.abs((await unit("cutoff")) - hi) < 0.005, "the wheel over a bar writes nothing");
}
// Its tooltip names its whole label, its value and its range, though the bar may cut the label (review Z6).
const cutTip = await page.$eval(`${wob} [data-knob="cutoff"]`, (e) => e.title);
check(/\nCutoff: 2\.0 kHz, from 40 Hz to 8\.0 kHz\n/.test(cutTip), `the bar's tooltip says Cutoff, its value and its range: ${cutTip.replace(/\n/g, " / ")}`);
const u0 = await unit("cutoff");
await drag(slider("cutoff"), 0, -30);
check(await until(() => read("tracks/wob.ts") !== text && !read("tracks/wob.ts").includes("cutoff: 2000,")), "a drag up writes a higher cutoff");
check(await stepIs("Wobbly Bass Synth cutoff"), `one undo step: ${await undoTitle()}`);
const u1 = await unit("cutoff");
check(u1 - u0 > 0.07 && u1 - u0 < 0.13, `30 px up is a tenth of its range: ${(u1 - u0).toFixed(3)}`);
text = read("tracks/wob.ts");
await drag(slider("cutoff"), 0, -30, { shift: true });
check(await until(() => read("tracks/wob.ts") !== text), "a ⇧-drag writes too");
const u2 = await unit("cutoff");
check(u2 - u1 > 0.003 && u2 - u1 < 0.02, `⇧ drags in fine steps, a tenth as far: ${(u2 - u1).toFixed(4)}`);
// A double-click types a number.
await caughtUp();
const dc = await box(slider("cutoff"));
await page.mouse.click(dc.x + dc.width / 2, dc.y + dc.height / 2, { count: 2 });
check(await until(async () => !!(await page.$(`${wob} [data-knob="cutoff"] [data-testid=knob-type-in]`))), "a double-click opens a field to type into");
await page.keyboard.down("Control");
await page.keyboard.press("a");
await page.keyboard.up("Control");
await page.keyboard.type("1k");
await page.keyboard.press("Enter");
check(await until(() => read("tracks/wob.ts").includes("cutoff: 1000")), "typed 1k and Enter: cutoff 1000, written");
// ⌥-click: its default.
const c = await box(slider("cutoff"));
await page.keyboard.down("Alt");
await page.mouse.click(c.x + c.width / 2, c.y + c.height / 2);
await page.keyboard.up("Alt");
check(await until(() => read("tracks/wob.ts").includes("cutoff: 2000")), "⌥-click resets it to its default, 2000");
await linted(wob, "Wob after the bar's gestures");

// ---------------------------------------------------------------- expanded (taste.md DV-12, DV-13)

const zoomBefore = await page.$eval("[data-zoom]", (e) => e.dataset.zoom);
const lpBefore = await box(lp);
// Where its first input and its first output are, from its top, read in one frame (the graph may be scrolling).
const portAt = () =>
  page.$eval(wob, (n) => {
    const top = n.getBoundingClientRect().top;
    const at = (sel) => { const r = n.querySelector(sel).getBoundingClientRect(); return Math.round(r.top + r.height / 2 - top); };
    return [at("[data-port-dot]"), at('[data-port="out"]')];
  });
const portsBefore = await portAt();
const sent = posts.length;
text = read("tracks/wob.ts");
await page.click(`${wob} [data-expand]`);
check(await until(async () => (await size(wob)) === "expanded"), "⤢ expands it");
await still();
const columns = await page.$$eval(`${wob} [data-column]`, (els) => els.map((e) => e.dataset.column));
check(JSON.stringify(columns) === JSON.stringify([...BANDS.map((b) => `Band ${b}`), "More"]), `every group is a column, More last: ${JSON.stringify(columns)}`);
const firstColumn = await page.$$eval(`${wob} [data-column="Band 1"] [data-knob]`, (els) => els.map((e) => e.dataset.knob));
check(JSON.stringify(firstColumn.slice(0, 3)) === '["shape","cutoff","release"]', `the face's values in no group go first in the first column: ${JSON.stringify(firstColumn)}`);
const colWidths = await page.$$eval(`${wob} [data-column]`, (els) => [...new Set(els.map((e) => e.offsetWidth))]);
check(JSON.stringify(colWidths) === "[176]", `each column is 176 px wide: ${JSON.stringify(colWidths)}`);
check((await width(wob)) === 4 * 176, `wrapping after four columns, the node is four wide: ${await width(wob)}`);
st = await state();
const drawn = await page.$$eval(`${wob} [data-knob-id]`, (els) => els.map((e) => e.dataset.knobId));
check(drawn.length === wobNode(st).knobs.length && new Set(drawn).size === drawn.length, `every value drawn once: ${drawn.length} of ${wobNode(st).knobs.length}`);
check(!!(await page.$(`${wob} [data-column="More"] [data-expression]`)), "the decay the code works out is a row naming its code, in More");
const ids = await page.$$eval(`${wob} [data-knob]`, (els) => els.every((e) => e.title.includes("Its id: ")));
check(ids, "each value's tooltip gives its id for m2 set");
check((await height(`${wob} [data-view="amp"]`)) === 132, "its face's view across the top, 132 px tall (DV-19)");
check((await width(`${wob} [data-view="amp"]`)) === 4 * 176, "as wide as the node");
const code = wobNode(st).code;
check((await page.$eval(`${wob} [data-code-line]`, (e) => e.textContent).catch(() => null)) === code, `its code line says what its call writes (CARD-41): ${code}`);
check(await page.$eval(`${wob} [data-code-line]`, (e) => getComputedStyle(e).whiteSpace !== "nowrap" && e.scrollWidth <= e.clientWidth), "wrapped to the node's width");
check(!(await page.$(`${lp} [data-pack]`)) && !(await page.$(`${lp} [data-presets]`)), "a collapsed Lowpass shows no pack and no Presets");
const portsAfter = await portAt();
check(JSON.stringify(portsAfter) === JSON.stringify(portsBefore), `the ends of its wires don't move on it: ${JSON.stringify(portsBefore)} ${JSON.stringify(portsAfter)}`);
const lpAfter = await box(lp);
check(lpAfter.x > lpBefore.x + 100, `the Lowpass moves aside: ${Math.round(lpBefore.x)} → ${Math.round(lpAfter.x)}`);
check((await page.$eval("[data-zoom]", (e) => e.dataset.zoom)) === zoomBefore, `the zoom stays as it was: ${zoomBefore}`);
await linted(wob, "Wob, expanded");
await page.screenshot({ path: `${ROOT}/nodes-expanded.png` });
// A value with two sides fills from the middle: band 1's gain, at 0, is empty; at −6, from the middle left.
const [g0, g1] = await fill("gain1");
check(g0 === 0.5 && g1 === 0.5, `a value with two sides at 0 fills nothing, from the middle: ${g0} ${g1}`);

// The bar's keys (taste.md DV-15), each one edit.
const gain1 = `${wob} [data-knob="gain1"]`;
const value = () => page.$eval(gain1, (e) => Number(e.dataset.value));
await page.$eval(slider("gain1"), (e) => e.focus());
await page.keyboard.press("End");
check(await until(async () => (await value()) === 24 && read("tracks/wob.ts").includes("gain1: 24")), "End: its highest, written");
check(await until(() => page.$eval(slider("gain1"), (e) => e === document.activeElement)), "and the bar keeps the keyboard's focus across its first write");
await page.keyboard.press("Home");
check(await until(async () => (await value()) === -24 && read("tracks/wob.ts").includes("gain1: -24")), "Home: its lowest");
await page.keyboard.press("PageUp");
check(await until(async () => Math.abs((await value()) - -19.2) < 0.01), `PageUp: a tenth of its range up: ${await value()}`);
const [n0, n1] = await fill("gain1");
check(Math.abs(n0 - 0.1) < 0.01 && n1 === 0.5, `below 0, it fills from the middle to the left: ${n0} ${n1}`);
await page.keyboard.press("Backspace");
check(await until(async () => (await value()) === 0 && read("tracks/wob.ts").includes("gain1: 0")), "⌫: its default");

// A bar left by a click doesn't take the keyboard's focus back when an agent writes it (review F1).
await page.$eval(slider("gain2"), (e) => e.focus());
const canvas = await box("[data-testid=graph]");
await page.mouse.click(canvas.x + canvas.width - 12, canvas.y + canvas.height - 12);
check(await page.evaluate(() => document.activeElement === document.body), "a click on the graph's empty space leaves the bar");
await caughtUp();
st = await state();
check((await agent("/api/graph/set", { session: st.session, revision: st.view.revision, id: knobId(st, "gain2"), gesture: "agent", value: 2 })) === 200 && (await until(() => read("tracks/wob.ts").includes("gain2: 2"))), "an agent writes band 2's gain, its first write");
await sleep(600);
check(!(await page.$eval(slider("gain2"), (e) => e === document.activeElement).catch(() => false)), "the bar doesn't take the focus back");
{
  const afterAgent = read("tracks/wob.ts");
  await page.keyboard.press("ArrowUp");
  await sleep(700);
  check(read("tracks/wob.ts") === afterAgent, "and ↑ then writes nothing (review Z12)");
}

// Enter on the header collapses it; nothing of the sizes is written.
await page.focus(`${wob} [data-node-head]`);
await page.keyboard.press("Enter");
check(await until(async () => (await size(wob)) === "collapsed"), "Enter on its header collapses it");
await page.keyboard.press("Enter");
check(await until(async () => (await size(wob)) === "expanded"), "and Enter expands it again");
await still();
await page.click(`${lp} [data-expand]`);
await until(async () => (await size(lp)) === "expanded");
const lpCode = await page.$eval(`${lp} [data-code-line]`, (e) => e.textContent).catch(() => null);
check(lpCode === "lowpass(w, { cutoff: 5000 })", `the Lowpass's code line names its sound as the code does (CARD-41, review Z12): ${lpCode}`);
await page.click("[data-testid=collapse-all]");
check(await until(async () => (await size(wob)) === "collapsed" && (await size(lp)) === "collapsed"), "Collapse all collapses every node");
await sleep(300);
// A picture's ask and its live layers' looks are no writes (CARD-67, TAP-8).
const sizesWrote = posts.slice(sent).filter((p) => !/^\/api\/graph\/(picture|look|set)$/.test(p.url) && !/gain/.test(p.body));
check(sizesWrote.length === 0, `expanding and collapsing write nothing: ${sizesWrote.map((p) => p.url).join(" ")}`);

// ---------------------------------------------------------------- selection and the keyboard (taste.md DV-15)

await page.click(`${lp} [data-node-title]`);
check(await until(async () => !!(await page.$(`${lp}[data-selected]`))), "a click on a node's header selects it: a ring");
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$(`${g} [data-selected]`))), "Esc clears the selection");
// Tab goes through the nodes in layout order; inside one, its header's buttons, then its values.
await page.focus(`${wob} [data-node-head]`);
const order = [];
for (let i = 0; i < 12; i++) {
  await page.keyboard.press("Tab");
  order.push(await page.evaluate(() => {
    const a = document.activeElement;
    const n = a?.closest("[data-node]");
    return `${n?.getAttribute("data-node") ?? "-"}:${a?.closest("[data-knob]") ? "value" : a?.hasAttribute("data-node-head") ? "head" : "button"}`;
  }));
}
const laidOut = await page.$$eval(`${g} [data-node]`, (els) => els.map((e) => [e.dataset.node, e.offsetLeft, e.offsetTop]).sort((a, b) => a[1] - b[1] || a[2] - b[2]).map((x) => x[0]));
const visited = order.map((o) => o.split(":")[0]).filter((n, i, a) => n !== "-" && a.indexOf(n) === i);
const wobPart = order.slice(0, order.findIndex((o) => !o.startsWith("wob")));
check(JSON.stringify(visited) === JSON.stringify(laidOut.slice(laidOut.indexOf("wob"), laidOut.indexOf("wob") + visited.length)), `Tab goes through the nodes in layout order: ${visited.join(" ")} of ${laidOut.join(" ")}`);
check(wobPart.length > 1 && wobPart.findIndex((o) => o.endsWith("value")) > wobPart.findIndex((o) => o.endsWith("button")), `inside a node, its header's buttons, then its values: ${wobPart.join(" ")}`);
// Delete removes the selected node where Remove is offered: the Lowpass, an effect; never Wob.
text = read("tracks/wob.ts");
await page.click(`${wob} [data-node-title]`);
await page.keyboard.press("Delete");
await sleep(800);
check(read("tracks/wob.ts") === text, "Delete on Wob, which isn't an effect, removes nothing");
await page.click(`${lp} [data-node-title]`);
await page.keyboard.press("Delete");
check(await until(() => !read("tracks/wob.ts").includes("lowpass(w")), "Delete on the selected Lowpass removes it, keeping what it worked on");
check(await stepIs("removing Lowpass"), `one undo step: ${await undoTitle()}`);
await key("Meta+z");
check(await until(() => read("tracks/wob.ts").includes("lowpass(w")), "⌘Z brings it back");
await until(async () => !!(await page.$(lp)));
// Remove is in the node's ⋯ menu too.
await page.click(`${lp} [data-node-menu]`);
check(await until(async () => !!(await page.$("[data-testid=node-remove]"))), "the Lowpass's ⋯ menu offers Remove");
await page.keyboard.press("Escape");

// ---------------------------------------------------------------- + on a wire (CARD-36, taste.md DV-7)

// No node's header and no pill has a + of its own: an effect goes in from + on a wire.
check(!(await page.$(`${g} [data-node] button[aria-label^="Add an"]`)) && !(await page.$('[data-testid=graph] [data-node] button[aria-label^="Add an"]')), "no node's header and no pill has a + of its own");
st = await state();
const wobMod = () => st.view.modules.find((m) => m.key === "wob");
const idOf = (title) => wobMod().nodes.find((n) => n.title === title)?.id;
const plusOn = (from, to) => `${g} [data-wire-plus][data-from="${from}"][data-to="${to}"]`;
const shown = (sel) => page.$eval(sel, (e) => !!e.dataset.shown && getComputedStyle(e).opacity === "1").catch(() => false);
const [wobN, lpN] = [idOf("Wobbly Bass Synth"), idOf("Lowpass")];
// Wob's body plays a saw, whose frequency comes in on a wire: its one wire in.
const inN = wobMod().port_edges.find((e) => e.to === wobN)?.from;
const out = plusOn(wobN, lpN), into = plusOn(inN, wobN), last = plusOn(lpN, "export:out");
check(!!(await page.$(out)) && !!(await page.$(into)) && !!(await page.$(last)), "each wire of Wob's has a + of its own");
// Nothing selected: a click on the canvas's empty corner clears the selection.
const corner = await box(`${g} [data-canvas]`);
await page.mouse.click(corner.x + corner.width - 4, corner.y + 4);
check(!(await shown(out)) && !(await shown(into)), "unseen until it is wanted");
// Under the pointer: a point on the wire a quarter of its way along, away from its +.
const along = await page.$eval(`${g} path[data-from="${wobN}"][data-to="${lpN}"]`, (p) => {
  const q = p.getPointAtLength(p.getTotalLength() / 4).matrixTransform(p.getScreenCTM());
  return { x: q.x, y: q.y };
});
await page.mouse.move(along.x, along.y);
check(await until(() => shown(out), 2000), "a wire under the pointer shows its + (the graph note's G20)");
check(!(await shown(into)), "and only its own");
await page.mouse.move(5, 5);
check(await until(async () => !(await shown(out)), 2000), "it goes again when the pointer leaves");
// The selected node's wires show theirs.
await page.click(`${wob} [data-node-title]`);
check(await until(async () => (await shown(out)) && (await shown(into))), "the selected node's wires show their +, in and out");
check(!(await shown(last)), "a wire it doesn't touch doesn't");
await page.keyboard.press("Escape");
check(await until(async () => !(await shown(out))), "Esc clears the selection, and its wires' + go");
await sleep(400);
await page.screenshot({ path: `${ROOT}/nodes-wire-plus.png` });
// Its browser offers what fits a sound's wire; a note effect is greyed, saying why.
await page.mouse.move(along.x, along.y);
await until(() => shown(out), 2000);
await page.click(out);
check(await until(async () => !!(await page.$("[data-effect-browser]"))), "+ on the wire after Wob opens the effect browser");
const arpWhy = await page.$eval('[data-effect-browser] [data-component="@m2/notes:arp"]', (e) => e.dataset.unfit ?? "").catch(() => "");
check(arpWhy === "Arpeggiator goes on notes, and this wire carries a sound", `a note effect is greyed on a sound's wire, saying why: ${arpWhy}`);
check(!(await page.$('[data-effect-browser] [data-effect="delay"][data-unfit]')), "an effect isn't");
await sleep(400);
await page.screenshot({ path: `${ROOT}/nodes-wire-browser.png` });
text = read("tracks/wob.ts");
await page.click('[data-effect-browser] [data-effect="delay"]');
check(await until(() => read("tracks/wob.ts").includes("lowpass(delay(w, { time: 0.25, feedback: 0.3, mix: 0.3 }), { cutoff: 5000 })")), "a Delay goes on that wire, around the Lowpass's `w`");
check(await stepIs("adding Delay"), `one undo step: ${await undoTitle()}`);
check(await until(async () => !!(await page.$(`${g} [data-node="delay"]`))), "the Delay's node shows between Wob and the Lowpass");
await sleep(400);
await page.screenshot({ path: `${ROOT}/nodes-wire-added.png` });
await key("Meta+z");
check(await until(() => read("tracks/wob.ts") === text), "⌘Z takes it out again");
await until(async () => !(await page.$(`${g} [data-node="delay"]`)));
// Keys' notes wire into its Pluck offers note effects; an effect there is greyed, saying why.
await page.click('[data-module-tab="keys"] [data-testid=track-label]');
st = await state();
const keysMod = st.view.modules.find((m) => m.key === "keys");
const pluckN = keysMod.nodes.find((n) => n.label === "pluck")?.id;
const chordsN = keysMod.port_edges.find((e) => e.to === pluckN && e.kind === "notes")?.from;
const chordsIn = `[data-testid=graph] [data-module="keys"] [data-wire-plus][data-from="${chordsN}"][data-to="${pluckN}"]`;
await until(async () => !!(await page.$(chordsIn)), 15000);
await page.click(`[data-testid=graph] [data-module="keys"] [data-node="pluck"] [data-node-title]`);
await until(() => shown(chordsIn));
await page.click(chordsIn);
check(await until(async () => !!(await page.$("[data-effect-browser]"))), "+ on the notes wire into Keys' Pluck opens the browser");
const delayWhy = await page.$eval('[data-effect-browser] [data-effect="delay"]', (e) => e.dataset.unfit ?? "").catch(() => "");
check(delayWhy === "Delay goes on a sound, and this wire carries notes", `an effect is greyed on a notes wire, saying why: ${delayWhy}`);
const keysText = read("tracks/keys.ts");
await page.click('[data-effect-browser] [data-component="@m2/notes:arp"]');
check(await until(() => read("tracks/keys.ts").includes("pluck(arp(lane.notes(\"chords\")))")), "an Arpeggiator goes on the notes wire, before the Pluck");
await key("Meta+z");
check(await until(() => read("tracks/keys.ts") === keysText), "⌘Z takes it out again");
await page.keyboard.press("Escape");
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await until(async () => !!(await page.$(wob)), 15000);
// Remove says why it can't, in ⋯ and with Delete: Wob is an instrument.
await page.click(`${wob} [data-node-menu]`);
const removeWhy = await page.$eval("[data-testid=node-remove]", (e) => [e.getAttribute("aria-disabled") ?? String(e.disabled), e.title]).catch(() => ["", ""]);
check(/true/.test(removeWhy[0]) && removeWhy[1] === "Wobbly Bass Synth is an instrument: change it with Change instrument, in its track's menu.", `⋯ greys Remove on Wob, saying why: ${JSON.stringify(removeWhy)}`);
await page.keyboard.press("Escape");
await page.click(`${wob} [data-node-title]`);
await page.keyboard.press("Delete");
check(await until(async () => (await toasts()).some((x) => x.includes("Wobbly Bass Synth is an instrument"))), "Delete on Wob says the same");
await page.keyboard.press("Escape");
// The song: a control wire's + is greyed, saying why, and a click says it.
await page.click('[data-module-tab="song"]');
const env = '[data-testid=graph] [data-module="song"] [data-wire-plus][data-from="import:kick.env"]';
check(await until(async () => !!(await page.$(env)), 9000), "the Kick's env wire into the Duck has a +");
const envWhy = await page.$eval(env, (e) => [e.getAttribute("aria-disabled"), e.title]).catch(() => [null, ""]);
check(envWhy[0] === "true" && envWhy[1] === "This wire carries a control: an effect goes on a sound, a note effect on notes.", `greyed, saying why: ${JSON.stringify(envWhy)}`);
await page.click(env);
check(await until(async () => (await toasts()).some((x) => x.includes("This wire carries a control"))) && !(await page.$("[data-effect-browser]")), "a click says it, and opens nothing");
// The kick's drum is read by its Follow and its out: + on its wire out wraps the record's `out`, so
// the kick's sound takes the effect and the Follow still reads it dry (CARD-36, review I2).
await page.click('[data-module-tab="kick"] [data-testid=track-label]');
st = await state();
const kickMod = st.view.modules.find((m) => m.key === "kick");
const drumN = kickMod.nodes.find((n) => n.label === "drums.drum")?.id;
const drumOut = `[data-testid=graph] [data-module="kick"] [data-wire-plus][data-from="${drumN}"][data-to="export:out"]`;
check(await until(async () => !!(await page.$(drumOut)) && !(await page.$(`${drumOut}[aria-disabled]`)), 9000), `the kick's wire out takes an effect: ${await page.$eval(drumOut, (e) => e.title).catch(() => "")}`);
check((await page.$eval(drumOut, (e) => e.getAttribute("aria-label")).catch(() => "")) === "Add an effect after Drum, before Out", "its + names both ends by their titles (DV-2)");
const kickText = read("tracks/kick.ts");
await page.click(drumOut);
await page.waitForSelector('[data-effect-browser] [data-effect="delay"]');
await page.click('[data-effect-browser] [data-effect="delay"]');
check(await until(() => /\{ out: delay\(out, \{[^}]*\}\), env: follow\(out,/.test(read("tracks/kick.ts"))), `the Delay wraps the record's out; the Follow reads the drum dry: ${read("tracks/kick.ts").match(/return .*/)?.[0]}`);
check(await stepIs("adding Delay"), `one undo step: ${await undoTitle()}`);
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === kickText), "⌘Z gives the kick's code back");
// Delete on a node that isn't a call says why (DV-15).
const hits = '[data-testid=graph] [data-module="kick"] [data-node="hits"] [data-node-title]';
await until(async () => !!(await page.$(hits)), 9000);
await page.click(hits);
await page.keyboard.press("Delete");
check(await until(async () => (await toasts()).some((x) => x.includes("Hits isn't an effect: change it in code."))), "Delete on the kick's lane says why");
await page.keyboard.press("Escape");
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await until(async () => !!(await page.$(wob)), 15000);

// ---------------------------------------------------------------- +N (taste.md DV-16)

await until(async () => (await size(wob)) === "collapsed");
st = await state();
// A value drawn on the collapsed node glows where it is; +N doesn't light for it.
check((await agent("/api/graph/set", { session: st.session, revision: st.view.revision, id: knobId(st, "release"), gesture: "agent", value: 0.5 })) === 200, "an agent sets the release, drawn on the collapsed node");
check(await until(async () => !!(await page.$(`${wob} [data-knob="release"][data-glow]`))), "its bar glows");
check(!(await page.$(`${wob} [data-more][data-glow]`)), "and +N doesn't light for it");
st = await state();
check((await agent("/api/graph/set", { session: st.session, revision: st.view.revision, id: knobId(st, "gain3"), gesture: "agent", value: -6 })) === 200, "an agent sets band 3's gain, hidden");
check(await until(async () => !!(await page.$(`${wob} [data-more][data-glow]`))), "+N lights for it (AGENT-5)");
st = await state();
check((await agent("/api/point", { session: st.session, target: `knob:${knobId(st, "gain3")}`, words: "this one" })) === 200, "an agent points at it");
check(await until(async () => !!(await page.$("[data-testid=pointer-show]"))), "the pointer offers Show");
await page.click("[data-testid=pointer-show]");
check(await until(async () => (await size(wob)) === "expanded" && !!(await page.$(`${wob} [data-knob-id="${knobId(st, "gain3")}"]`))), "Show expands the node, the value in sight (AGENT-6)");
await page.click("[data-testid=pointer-ok]").catch(() => {});
await page.click("[data-testid=collapse-all]");
await until(async () => (await size(wob)) === "collapsed");
await page.click(`${wob} [data-more]`);
check(await until(async () => (await size(wob)) === "expanded"), "a click on +N expands the node");
await page.click("[data-testid=collapse-all]");
await until(async () => (await size(wob)) === "collapsed");

// ---------------------------------------------------------------- a wire, a lane and the code drive values (taste.md DV-6, DV-14)

// A wire from the LFO onto the Lowpass's cutoff: the bar becomes a row naming the wire.
const center = async (sel) => {
  const b = await box(sel);
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
};
const pull = async (from, to) => {
  const a = await center(from), b = await center(to);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 12 });
  await page.mouse.up();
};
check(!!(await page.$('[data-outlet="wobble"]')), "the LFO, wobble, has an output to pull a wire from");
await still();
await pull('[data-outlet="wobble"]', slider("cutoff", lp));
check(await until(() => read("tracks/wob.ts").includes("cutoff: wobble")), "a wire dropped on the Lowpass's cutoff writes its name");
check(await until(async () => /wobble/.test((await page.$eval(`${lp} [data-inlet]`, (e) => e.textContent).catch(() => "")) ?? "") && !(await page.$(`${lp} [data-knob="cutoff"]`))), "the cutoff is now a row naming the wire, not a bar");
// Its picture can't be measured now: collapsed, it says so in one short line, its whole why in its tooltip (DV-12, review Z9).
const why = await page.$eval(`${lp} [data-greyed]`, (e) => { const p = e.querySelector("[data-why]"); return [p?.textContent ?? "", p ? p.scrollHeight <= 18 && p.scrollWidth <= p.clientWidth : false, e.title]; }).catch(() => ["", false, ""]);
check(why[1] && why[2].length > why[0].length && /can't/.test(why[2]), `the picture says why in one line, whole in its tooltip: ${JSON.stringify(why)}`);
// Dropped on a collapsed node, a menu offers its values (DV-6).
await still();
await pull('[data-outlet="wobble"]', `${wob} [data-node-title]`);
check(await until(async () => !!(await page.$("[data-testid=drop-menu]"))), "a wire dropped on collapsed Wob offers its values in a menu");
const offered = await page.$$eval("[data-testid=drop-menu] [data-drop-knob]", (els) => els.map((e) => e.dataset.dropKnob));
check(offered.includes("gain3") && offered.length >= 10, `its values, hidden ones too: ${offered.length}`);
await page.click('[data-testid=drop-menu] [data-drop-knob="gain3"]');
check(await until(() => read("tracks/wob.ts").includes("gain3: wobble")), "picking band 3's gain wires it");
await linted(g, "the graph with its wires");
// A value the timeline's lane drives is a row naming the lane: the Lowpass's q, automated.
await page.click(`${lp} [data-knob="q"]`, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu] [data-menu-item=automate]", { timeout: 5000 }).catch(() => {});
await page.click("[data-testid=knob-menu] [data-menu-item=automate]").catch(() => {});
check(await until(async () => /lane/.test((await page.$eval(`${lp} [data-lane-input]`, (e) => e.textContent).catch(() => "")) ?? "") && !(await page.$(`${lp} [data-knob="q"]`)), 15000), "automated, the Lowpass's q is a row naming its lane, not a bar");

// ---------------------------------------------------------------- the expanded set follows each node (taste.md DV-13)

await page.click(`${lp} [data-expand]`);
await until(async () => (await size(lp)) === "expanded");
check((await page.$eval(`${lp} [data-pack]`, (e) => e.textContent).catch(() => null)) === "std" && !!(await page.$(`${lp} [data-presets]`)), "expanded, the Lowpass's header names its pack, std, and has Presets");
st = await state();
const wobId = wobNode(st).id;
const lpKey = st.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Lowpass").key;
check((await agent("/api/graph/effect/add", { session: st.session, revision: st.view.revision, call: wobId, component: "@m2/effects:reverb" })) === 200, "an agent puts a Reverb after Wob, written around its call");
check(await until(() => read("tracks/wob.ts").includes("reverb(wob(")), "written before Wob and the Lowpass in the file");
const after = await state();
const lpNow = after.view.modules.find((m) => m.key === "wob").nodes.find((n) => n.title === "Lowpass");
check(lpNow.key === lpKey, `the Lowpass keeps its key: ${lpKey}`);
check(await until(async () => !!(await page.$(`${g} [data-node="reverb"]`))), "the Reverb shows");
check((await size(lp)) === "expanded" && (await size(wob)) === "collapsed", "the Lowpass stays expanded, Wob collapsed");
check((await size(`${g} [data-node="reverb"]`)) === "collapsed", "a node an edit adds starts collapsed");
await key("Meta+z");
check(await until(() => !read("tracks/wob.ts").includes("reverb(")), "⌘Z takes the Reverb out");
await sleep(300);
check((await size(lp)) === "expanded", "the Lowpass still expanded");

// ---------------------------------------------------------------- the song's mixer, and a note effect

await page.click('[data-module-tab="song"]');
const songG = '[data-testid=graph] [data-module="song"]';
check(await until(async () => !!(await page.$(`${songG} [data-testid=open-mix]`))) && !(await page.$(`${songG} [data-node="mixer"] [data-knob]`)), "the song's mixer is one compact node, its levels in Mix (MX-6)");
check(!(await page.$(`${songG} [data-node="mixer"] [data-expand]`)) && !(await page.$(`${songG} [data-node="mixer"][data-size]`)), "and it has one size: no ⤢ (review Z10)");
check(JSON.stringify(await behind("song")) === "[]", `no wire of the song runs behind a node it doesn't touch (DV-5, review Z7): ${JSON.stringify(await behind("song"))}`);
check(/Listens to:\s*Kick/.test(await page.$eval(`${songG} [data-node="duck"] [data-listens]`, (e) => e.textContent).catch(() => "")), "the collapsed Duck says it listens to Kick (CARD-35)");
await page.click(`${songG} [data-node="duck"] [data-expand]`);
check((await page.$eval(`${songG} [data-node="duck"] [data-code-line]`, (e) => e.textContent).catch(() => "")) === "duck(bass.out, kick.env, { depth: db(-9) })", "expanded, the Duck's code line names the track it ducks");
// An instrument wears its track's colour along its top (DV-12): Keys' Pluck.
await page.click('[data-module-tab="keys"] [data-testid=track-label]');
const colourBar = '[data-testid=graph] [data-module="keys"] [data-node="pluck"] [data-colour-bar]';
check(await until(async () => !!(await page.$(colourBar))), "Keys' Pluck, an instrument, has its track's colour along its top");
const chip = await page.$eval("[data-testid=dock-chip] span", (e) => getComputedStyle(e).backgroundColor);
check((await page.$eval(colourBar, (e) => [getComputedStyle(e).backgroundColor, e.offsetHeight])).join() === `${chip},3`, `in the track's own colour, 3 px tall: ${chip}`);
st = await state();
const pluck = st.view.modules.find((m) => m.key === "keys").nodes.find((n) => n.kind === "call" && n.audio);
const added = await agent("/api/graph/effect/add", { session: st.session, revision: st.view.revision, call: pluck.id, component: "@m2/notes:arp" });
check(added === 200 && (await until(() => read("tracks/keys.ts").includes("arp(lane.notes"), 15000)), `an agent puts an Arpeggiator before the Keys' pluck: ${added}`);
await page.click('[data-module-tab="keys"] [data-testid=track-label]');
check(await until(async () => /Arpeggiator/.test(await page.$eval('[data-testid=graph] [data-module="keys"]', (e) => e.textContent ?? "").catch(() => "")), 15000), "the graph shows the Arpeggiator's node, on the notes wire");
// The kick's drum reaches its output past its Follow: the wire passes through a gap (DV-5, review Z7).
await page.click('[data-module-tab="kick"] [data-testid=track-label]');
await until(async () => !!(await page.$('[data-testid=graph] [data-module="kick"] path[data-from]')), 15000);
check(JSON.stringify(await behind("kick")) === "[]", `no wire of the kick runs behind a node it doesn't touch: ${JSON.stringify(await behind("kick"))}`);
await page.click('[data-module-tab="wob"] [data-testid=track-label]');
await until(async () => !!(await page.$(`${wob}`)), 15000);
check(JSON.stringify(await behind("wob")) === "[]", `nor any of Wob's: ${JSON.stringify(await behind("wob"))}`);

// ---------------------------------------------------------------- per note (DV-12, CARD-25, review Z3)

// A voice whose noise reads no note: it plays once for each note all the same.
write("tracks/vx.ts", `import { poly, track } from "@m2/core";
import { adsr, noise, osc, vca } from "std/dsp";
import { lowpass } from "std/filter";
import { sum } from "std/math";

export default track("vx", { name: "Vx" }, ({ lane }) =>
  lowpass(poly(lane.notes("notes"), (n) => vca(lowpass(sum(osc.saw(n.pitch), noise()), { cutoff: 1200 }), adsr(n.gate))), { cutoff: 4000 }),
);
`);
write("song.ts", read("song.ts").replace('import wob from "./tracks/wob";', 'import wob from "./tracks/wob";\nimport vx from "./tracks/vx";').replace("      wob: wob.out,", "      wob: wob.out,\n      vx: vx.out,"));
await until(async () => !!(await page.$('[data-module-tab="vx"]')), 15000);
await page.click('[data-module-tab="vx"] [data-testid=track-label]');
const vx = '[data-testid=graph] [data-module="vx"]';
check(await until(async () => !!(await page.$(`${vx} [data-node="noise"]`)), 15000), "Vx's graph shows its voice's noise");
const perNote = await page.$$eval(`${vx} [data-node]`, (els) => els.map((e) => `${e.dataset.node}:${!!e.querySelector("[data-per-note]")}`).sort());
check(perNote.includes("noise:true") && perNote.includes("osc.saw:true") && perNote.includes("adsr:true"), `the voice's parts say per note, the noise too: ${perNote.join(" ")}`);
check(perNote.filter((x) => x.startsWith("lowpass:")).sort().join() === "lowpass:false,lowpass:true" && perNote.includes("poly:false"), "the Lowpass after the voices and the poly call don't");

// ---------------------------------------------------------------- a node with no groups, expanded (DV-12, review Z10)

// A track's own mixer of ten sounds: a value each, no groups. Expanded, they go in columns of eight.
write("tracks/vx.ts", `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { mixer } from "std/mix";

export default track("vx", { name: "Vx" }, () =>
  mixer({ ${[..."abcdefghij"].map((c, i) => `${c}: osc.sine(${110 * (i + 1)})`).join(", ")} }),
);
`);
const mx = `${vx} [data-node="mixer"]`;
check(await until(async () => !!(await page.$(mx)), 15000), "Vx is a mixer of ten sounds");
await expand(page, mx);
const cols = await page.$$eval(`${mx} [data-column]`, (els) => els.map((e) => `${e.dataset.column}:${e.querySelectorAll("[data-knob]").length}`));
const mixed = await page.$$eval(`${mx} [data-knob]`, (els) => els.length);
check(mixed >= 10 && cols[0] === "Values:8" && cols[1] === `Values 2:${Math.min(8, mixed - 8)}`, `its ${mixed} values in columns of eight: ${cols.join(" ")}`);

// ---------------------------------------------------------------- the graph fills the dock before it scrolls (taste.md DV-5)

// examples/sounds' Song: nineteen tracks' sounds and two Ducks into one mixer, far taller than the
// dock. Its column of sources hangs from where its wires land, so the mixer keeps the top of the view.
const made = await page.evaluate(
  async (token) => (await fetch("/api/project/new", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ name: "Sounds", example: "sounds" }) })).status,
  sess.page_token,
);
check(made === 200, `examples/sounds opens as a new song: ${made}`);
await until(async () => !!(await page.$('[data-module-tab="ride"]')), 20000);
await page.click('[data-module-tab="song"]');
const soundsG = '[data-testid=graph] [data-module="song"]';
await until(async () => (await page.$$(`${soundsG} [data-node]`)).length > 20, 15000);
await sleep(500);
const firstView = await page.evaluate((sel) => {
  const dock = document.querySelector("[data-testid=graph]").getBoundingClientRect();
  const nodes = [...document.querySelectorAll(`${sel} [data-node]`)].map((n) => n.getBoundingClientRect());
  const area = document.querySelector(`${sel} [data-canvas]`).getBoundingClientRect();
  const inView = nodes.filter((r) => r.left >= dock.left - 1 && r.right <= dock.right + 1 && r.top >= dock.top - 1 && r.bottom <= dock.bottom + 1).length;
  const inside = (r) => r.left >= dock.left - 1 && r.right <= dock.right + 1 && r.top >= dock.top - 1 && r.bottom <= dock.bottom + 1;
  const mixer = document.querySelector(`${sel} [data-node="mixer"]`);
  return { inView, all: nodes.length, mixer: !!mixer && inside(mixer.getBoundingClientRect()), wide: Math.round(area.width), tall: Math.round(area.height), dockH: Math.round(dock.height) };
}, soundsG);
check(firstView.inView >= FILLED, `more of its nodes in the dock's first view: ${firstView.inView} of ${firstView.all} (${JSON.stringify(firstView)})`);
check(firstView.mixer, "the mixer the tracks go to is among them, at the top of the view");
check(JSON.stringify(await behind("song")) === "[]", `no wire of it runs behind a node it doesn't touch (DV-5): ${JSON.stringify(await behind("song"))}`);
await page.screenshot({ path: `${ROOT}/nodes-sounds-song.png` });
// The mixer selected: its nineteen wires in meet at one input. Only those 24 px apart show their +,
// near the mixer; the rest show under the pointer (DV-7, review I7).
await page.click(`${soundsG} [data-node="mixer"] [data-node-title]`);
await sleep(300);
const pluses = await page.$$eval(`${soundsG} [data-wire-plus][data-shown]`, (els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; }));
const close = pluses.flatMap((a, i) => pluses.slice(i + 1).filter((b) => Math.hypot(a[0] - b[0], a[1] - b[1]) < 24));
check(pluses.length > 0 && close.length === 0, `the selected mixer's + don't crowd: ${pluses.length} shown, ${close.length} pairs under 24 px`);
await page.screenshot({ path: `${ROOT}/nodes-mixer-selected.png` });

done(errors);
