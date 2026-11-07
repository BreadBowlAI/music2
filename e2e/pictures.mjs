// Track pictures: a pack's picture on its card (CARD-60 to CARD-70). A test pack, @test/curve, has an
// EQ-like sound, Curve EQ, whose face is its picture and its two gains: a curve worked out from its
// two bands, and a dot for each band (across its frequency, up and down its gain, the wheel its q, a double-click its
// on). Device's chain draws the curve; dragging band 2's dot moves the curve while the dot is held
// (the server draws it again from the drag's leases, CARD-68), and the release writes both values in
// one edit, one undo step, which ⌘Z takes back byte for byte. Picking a dot shows its band's tab
// (taste.md DV-18); a double-click turns its band off (one write). An agent asking for the picture reads
// the same shapes the window drew, as numbers, and each dot's knob ids (CARD-67). Each state linted.
// No axis label covers the curve, at each height a picture is drawn (taste.md DV-17, DV-19).
//
//   node e2e/pictures.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, view, box, posts } = await open(1440, 900, { device: "chain" });

/** A request from outside the page: an agent's. */
const agent = (path, body) =>
  page.evaluate(
    async (url, key, path, body) => (await fetch(`${url}${path}`, { method: "POST", headers: { "x-m2-token": key, "content-type": "application/json" }, body: JSON.stringify(body) })).json(),
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

// ---------------------------------------------------------------- the test pack, and a track playing it

const PACK = path.join(ROOT, "curvepack");
fs.mkdirSync(path.join(PACK, "src"), { recursive: true });
fs.writeFileSync(
  path.join(PACK, "m2pkg.toml"),
  `[package]\nname = "@test/curve"\nversion = "0.1.0"\nlicense = "MIT"\n\n[dependencies]\n# view.picture, since 2.4 (CARD-60).\n"@m2/core" = "2.4"\nstd = "0.10"\n`,
);
fs.writeFileSync(
  path.join(PACK, "src/index.ts"),
  `import { axis, component, control, live, port, view } from "@m2/core";
import { lowpass } from "std/filter";

const BANDS = [1, 2] as const;
/** A bell's height in dB at f: near enough a peaking filter's, for a test. */
const bell = (f: number, freq: number, gain: number, q: number) => {
  const x = Math.log2(f / freq) * q * 2;
  return gain / (1 + x * x);
};

export const curve = component(
  {
    id: "curve",
    label: "Curve EQ",
    inputs: { x: port.audio() },
    controls: {
      freq1: control.hz(200).range(20, 20000).log(), gain1: control.db(0).range(-24, 24), q1: control.scalar(1).range(0.1, 10).log(), on1: control.bool(true),
      freq2: control.hz(2000).range(20, 20000).log(), gain2: control.db(0).range(-24, 24), q2: control.scalar(1).range(0.1, 10).log(), on2: control.bool(true),
    },
    output: port.audio(),
    editor: {
      category: "EQ",
      views: {
        curve: view.picture({
          x: axis.log("hz", 20, 20000),
          y: axis.lin("db", -24, 24),
          live: [live.spectrum("out", { tone: "faint" }), live.playhead("beat")],
        }),
      },
      groups: { "Band 1": ["on1", "freq1", "gain1", "q1"], "Band 2": ["on2", "freq2", "gain2", "q2"] },
      face: ["curve", "gain1", "gain2"],
    },
  },
  (x, { freq2, q2 }) => lowpass(x, { cutoff: freq2, q: q2 }),
  {
    curve: {
      draw: (v, p) => {
        const points: number[] = [];
        for (let i = 0; i < 180; i++) {
          const f = 20 * 1000 ** (i / 179);
          let db = 0;
          for (const b of BANDS) if (v[\`on\${b}\`]) db += bell(f, v[\`freq\${b}\`] ?? 1000, v[\`gain\${b}\`] ?? 0, v[\`q\${b}\`] ?? 1);
          points.push(f, db);
        }
        return [
          p.mark("y", 0, "0 dB"),
          p.line(points, { name: "curve" }),
          ...BANDS.map((b) => p.handle(v[\`freq\${b}\`], v[\`gain\${b}\`], { x: \`freq\${b}\`, y: \`gain\${b}\`, wheel: \`q\${b}\`, toggles: \`on\${b}\`, opens: \`Band \${b}\`, mark: \`\${b}\`, off: !v[\`on\${b}\`] })),
        ];
      },
      words: (v) => BANDS.map((b) => \`Band \${b}: \${v[\`gain\${b}\`]} dB at \${v[\`freq\${b}\`]} Hz\`).join("; "),
    },
  },
);
`,
);
const manifest = read("music2.toml");
write("music2.toml", manifest.replace("[dependencies]\n", `[dependencies]\n"@test/curve" = { path = ${JSON.stringify(path.relative(PROJ, PACK))} }\n`));
write(
  "tracks/eq.ts",
  `import { track } from "@m2/core";\nimport { osc } from "std/dsp";\nimport { curve } from "@test/curve";\n\nexport default track("eq", { name: "EQ" }, () => curve(osc.saw(110), { freq2: 2000, gain2: 3 }));\n`,
);
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport eq from "./tracks/eq";').replace("      kick: kick.out,", "      kick: kick.out,\n      eq: eq.out,"));

await until(async () => !!(await page.$('[data-module-tab="eq"]')), 20000);
await page.click('[data-module-tab="eq"] [data-testid=track-label]');
await view("graph");
const card = '[data-testid=chain] [data-module="eq"] [data-node="curve"]';
const pic = `${card} [data-picture="curve"]`;
check(await until(async () => !!(await page.$(`${pic}[data-drawn] polyline[data-line="curve"]`)), 20000), "Curve EQ's face draws its picture, asked for while it shows (taste.md DV-17)");
check((await page.$eval(pic, (e) => e.dataset.asking)) === "true", "it asks while it is on screen");
const words = () => page.$eval(`${pic} [role=group]`, (e) => e.getAttribute("aria-label")).catch(() => null);
check((await words()) === "Band 1: 0 dB at 200 Hz; Band 2: 3 dB at 2000 Hz", `its words are its accessible name: ${await words()}`);
check((await page.$$(`${pic} [data-dot]`)).length === 2, "a dot for each band");
await linted(card, "the picture's card");
await page.screenshot({ path: `${ROOT}/pictures-card.png` });

// ---------------------------------------------------------------- a drag: the curve follows, one undo step

const curvePoints = () => page.$eval(`${pic} polyline[data-line="curve"]`, (e) => e.getAttribute("points"));
const drawnCount = () => page.$eval(pic, (e) => Number(e.dataset.drawn ?? 0));
const before = await curvePoints();
const code = read("tracks/eq.ts");
const dot = await box(`${pic} [data-dot="1"]`);
const [x0, y0] = [dot.x + dot.width / 2, dot.y + dot.height / 2];
await page.mouse.move(x0, y0);
await page.mouse.down();
const drawn0 = await drawnCount();
for (let i = 1; i <= 8; i++) {
  await page.mouse.move(x0 + i * 6, y0 - i * 3);
  await sleep(30);
}
check(await until(async () => (await drawnCount()) > drawn0 && (await curvePoints()) !== before, 5000), "while the dot is held, the curve follows it: the server drew it again from the drag (CARD-68)");
check(read("tracks/eq.ts") === code, "nothing is written while it is held");
const held = await page.$eval(`${pic} [data-dot="1"]`, (e) => e.dataset.marked);
check(held === "true", "the dot held is marked in the accent");
await page.mouse.up();
check(await until(async () => read("tracks/eq.ts") !== code, 8000), "the release writes the code");
const after = read("tracks/eq.ts");
const freq = Number(/freq2: (\d+)/.exec(after)?.[1]);
const gain = Number(/gain2: (-?[\d.]+)/.exec(after)?.[1]);
check(freq > 2000 && gain > 3, `the drag moved band 2's frequency up and its gain up, from where they were: freq2 ${freq}, gain2 ${gain}`);
check(await until(async () => (await undoTitle()) === "Undo Curve EQ freq2 and gain2 (⌘Z)"), `one undo step, named by what moved: ${await undoTitle()}`);
check(await until(async () => (await words()) === `Band 1: 0 dB at 200 Hz; Band 2: ${gain} dB at ${freq} Hz`), `the picture says what plays now: ${await words()}`);

// An agent reads the same shapes, as numbers, and each dot's knobs (CARD-67).
const state = (await api("")).body;
const node = state.view.modules.find((m) => m.key === "eq").nodes.find((n) => n.title === "Curve EQ");
const asked = await agent("/api/graph/picture", { session: state.session, revision: state.view.revision, client: "reader", module: "eq", node: node.id, view: "curve", on: true });
// The words are in the view an agent reads first (CARD-63), once the server worked them out.
const inView = async () => (await api("")).body.view.modules.find((m) => m.key === "eq").nodes.find((n) => n.title === "Curve EQ").views[0].words;
check(await until(async () => (await inView()) === asked.picture?.words), `and the view has the picture's words: ${await inView()}`);
const line = asked.picture?.shapes?.find((s) => s.kind === "line");
check(asked.picture?.words === (await words()), `an agent reads the picture's words: ${asked.picture?.words}`);
const drawnPoints = (await curvePoints()).split(" ").length;
check(line?.points?.length === 2 * drawnPoints && drawnPoints === 180, `and the curve the window drew, as numbers: ${line?.points?.length} numbers, ${drawnPoints} points drawn`);
const h2 = asked.picture?.shapes?.filter((s) => s.kind === "handle")[1];
const knobId = (slot) => node.knobs.find((k) => k.slot === slot)?.id;
check(h2?.x === freq && h2?.y === gain && h2?.moves?.x?.knob === knobId("freq2") && h2?.moves?.y?.knob === knobId("gain2"), `and band 2's dot, at its values, with its knobs: ${JSON.stringify(h2?.moves)}`);
check(h2?.opens === "Band 2" && h2?.mark === "2", "and the tab it opens, and its mark");

// ⌘Z takes the drag back, byte for byte, and the curve with it.
await page.keyboard.down("Meta");
await page.keyboard.press("z");
await page.keyboard.up("Meta");
check(await until(async () => read("tracks/eq.ts") === code, 8000), "⌘Z takes the drag back, byte for byte");
check(await until(async () => (await curvePoints()) === before, 8000), "and the curve is drawn as it was");

// ---------------------------------------------------------------- the dot and its tab (taste.md DV-18), and a flip (CARD-70)

await page.click(`${card} [data-tab="All"]`).catch(() => {});
await page.click(`${pic} [data-dot="0"]`);
check(await until(async () => !!(await page.$(`${card} [data-tab-panel="Band 1"]`))), "picking band 1's dot shows its tab");
await page.click(`${card} [data-tab="Band 2"]`);
check(await until(async () => (await page.$eval(`${pic} [data-dot="1"]`, (e) => e.dataset.marked).catch(() => null)) === "true"), "picking band 2's tab marks its dot");
const flipped = read("tracks/eq.ts");
await page.click(`${pic} [data-dot="1"]`, { count: 2 });
check(await until(async () => /on2: false/.test(read("tracks/eq.ts")), 8000), "a double-click on band 2's dot turns the band off: one write");
check(await until(async () => (await undoTitle()) === "Undo Curve EQ on2 (⌘Z)"), `one undo step: ${await undoTitle()}`);
check(await until(async () => (await words()) === "Band 1: 0 dB at 200 Hz; Band 2: 3 dB at 2000 Hz" && (await curvePoints()) !== before), "the curve drops the band it turned off");
await page.keyboard.down("Meta");
await page.keyboard.press("z");
await page.keyboard.up("Meta");
check(await until(async () => read("tracks/eq.ts") === flipped, 8000), "⌘Z turns it back on");
// The keys (CARD-70, review G4): five quick ↑ on a dot step its y five times from where each left
// it (0.1 dB each), one gesture written once they stop, one undo step.
const steps0 = read("tracks/eq.ts");
await page.$eval(`${pic} [data-dot="1"]`, (e) => e.focus());
for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowUp");
check(await until(async () => /gain2: 3\.5\b/.test(read("tracks/eq.ts")), 8000), `five quick ↑ on band 2's dot move its gain five steps, none lost: ${/gain2: [^,}]*/.exec(read("tracks/eq.ts"))?.[0]}`);
check(await until(async () => (await undoTitle()) === "Undo Curve EQ gain2 (⌘Z)"), `one undo step for the run: ${await undoTitle()}`);
await page.keyboard.down("Meta");
await page.keyboard.press("z");
await page.keyboard.up("Meta");
check(await until(async () => read("tracks/eq.ts") === steps0, 8000), "⌘Z takes the whole run back");
// The wheel (review G4, G5): a step every 40 px, as a knob's, never scrolling the chain under it.
const chainEl = "[data-testid=chain]";
const scrolled = () => page.$eval(chainEl, (e) => e.scrollTop + e.scrollLeft);
const s0 = await scrolled();
const d2 = await box(`${pic} [data-dot="1"]`);
await page.mouse.move(d2.x + d2.width / 2, d2.y + d2.height / 2);
for (let i = 0; i < 10; i++) await page.mouse.wheel({ deltaY: -3 });
await sleep(700);
check(read("tracks/eq.ts") === steps0, "30 px of wheel, less than a notch, writes nothing");
await page.mouse.wheel({ deltaY: -100 });
check(await until(async () => /q2: 1\.03\b/.test(read("tracks/eq.ts")), 8000), `then 100 px more makes 130 px, three notches: q2 three 1% steps, one write: ${/q2: [^,}]*/.exec(read("tracks/eq.ts"))?.[0]}`);
check((await scrolled()) === s0, `the wheel on a dot scrolls nothing under it: ${s0} then ${await scrolled()}`);
check(await until(async () => (await undoTitle()) === "Undo Curve EQ q2 (⌘Z)"), `one undo step: ${await undoTitle()}`);
await page.keyboard.down("Meta");
await page.keyboard.press("z");
await page.keyboard.up("Meta");
await until(async () => read("tracks/eq.ts") === steps0, 8000);
// Its text is never stretched (taste.md DV-17, review G9): an axis label is as wide Expanded as at Normal.
const label = () => page.$eval(`${pic} text[data-tick]`, (e) => { const r = e.getBoundingClientRect(); return [Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10]; });
const normal = await label();
await page.click(`${card} [data-size-to="expanded"]`);
await until(async () => (await page.$eval(`${pic} svg`, (e) => e.dataset.size).catch(() => "")) !== "", 3000);
await sleep(300);
const wide = await label();
check(Math.abs(wide[0] - normal[0]) < 1 && Math.abs(wide[1] - normal[1]) < 1, `an axis label keeps its size, Expanded: ${normal} then ${wide}`);
await page.click(`${card} [data-size-to="normal"]`);
await linted(card, "the card after its gestures");

// ---------------------------------------------------------------- live layers (CARD-73)

await api("/loop", { method: "POST", body: JSON.stringify({ start: 0, end: 4, on: true }) });
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
const liveReads = () => page.$eval(`${pic} canvas[data-live]`, (e) => Number(e.dataset.reads ?? 0)).catch(() => 0);
check(await until(async () => (await liveReads()) >= 5, 10000), `while the song plays, the curve's sound's spectrum is drawn under it (${await liveReads()} reads)`);
const looks = posts.filter((p) => p.url === "/api/graph/look").map((p) => JSON.parse(p.body));
check(looks.some((l) => l.kind === "spectrum" && l.at === "out" && l.on), `asked for as a look at the call's output: ${JSON.stringify(looks.slice(0, 2))}`);
await page.keyboard.press("Space");

// ---------------------------------------------------------------- the axes' labels never cover a line (taste.md DV-17)

// Band 1 at 20 Hz, 23 dB up and wide (q 0.1): the curve starts high at the picture's left edge,
// where the y axis's labels go. At each height a picture is drawn (104 px beside a Normal face's values, 132 Expanded,
// 48 on the patch's card, taste.md DV-19), no label's box holds a point of the curve's line or sits
// on a fill, where the curve shows.
write("tracks/eq.ts", `import { track } from "@m2/core";\nimport { osc } from "std/dsp";\nimport { curve } from "@test/curve";\n\nexport default track("eq", { name: "EQ" }, () => curve(osc.saw(110), { freq1: 20, gain1: 23, q1: 0.1, freq2: 2000, gain2: 3 }));\n`);
await until(async () => (await words()) === "Band 1: 23 dB at 20 Hz; Band 2: 3 dB at 2000 Hz", 10000);
/** What a picture's axis labels cover, at the size it is drawn: each label's box against every point of its lines and fills, where they show. */
const covered = (sel) =>
  page.$eval(`${sel} svg`, (svg) => {
    const shown = svg.querySelector("clipPath rect")?.getBBox() ?? { x: -1e9, y: -1e9, width: 2e9, height: 2e9 };
    const inside = (x, y, b) => x > b.x && x < b.x + b.width && y > b.y && y < b.y + b.height;
    const boxes = [...svg.querySelectorAll("text[data-tick]")].map((t) => t.getBBox());
    const hits = new Set();
    for (const shape of svg.querySelectorAll("polyline, polygon")) {
      const pts = (shape.getAttribute("points") ?? "").trim().split(/\s+/).filter(Boolean).map((p) => p.split(",").map(Number));
      if (shape.tagName === "polygon" && pts.length) pts.push(pts[0]);
      for (let i = 1; i < pts.length; i++) {
        const [[x0, y0], [x1, y1]] = [pts[i - 1], pts[i]];
        const n = Math.max(1, Math.ceil(2 * Math.hypot(x1 - x0, y1 - y0)));
        for (let k = 0; k <= n; k++) {
          const x = x0 + ((x1 - x0) * k) / n, y = y0 + ((y1 - y0) * k) / n;
          if (!inside(x, y, shown)) continue;
          boxes.forEach((b, j) => inside(x, y, b) && hits.add(j));
        }
      }
      if (shape.tagName === "polygon") boxes.forEach((b, j) => shape.isPointInFill(new DOMPoint(b.x + b.width / 2, b.y + b.height / 2)) && hits.add(j));
    }
    return { size: svg.dataset.size, labels: boxes.length, covered: [...hits].map((j) => svg.querySelectorAll("text[data-tick]")[j].textContent) };
  });
/** The picture `sel` once its row is `tall` px (its view's row, taste.md DV-19), and what its labels cover. */
const uncovered = async (sel, tall, where) => {
  // Its row's own height, unscaled: the patch is drawn at its zoom.
  const row = (s) => page.$eval(s, (e) => e.closest("[data-view]")?.offsetHeight ?? 0).catch(() => 0);
  await until(async () => (await row(sel)) === tall, 5000);
  await sleep(300);
  const [c, h] = [await covered(sel), await row(sel)];
  check(h === tall && c.labels > 0 && c.covered.length === 0, `${where}, ${h} px tall: no axis label covers the curve (${c.size}, ${c.labels} labels; covered: ${c.covered.join(" ") || "none"})`);
};
await uncovered(pic, 104, "beside the Normal face's values");
// Its frequencies' labels (taste.md DV-17, review S9): a log axis's decades first, then its 5s where
// they fit, so 100 and 1k show across the 198 px face (placed left to right, 500 took 1k's place).
const across = await page.$eval(`${pic} svg`, (svg) => ({ size: svg.dataset.size, labels: [...svg.querySelectorAll('text[data-tick]:not([text-anchor="end"])')].map((t) => t.textContent) }));
check(across.size?.startsWith("198x") && across.labels.includes("100") && across.labels.includes("1k"), `across the ${across.size} face its frequencies are labelled ${across.labels.join(" ")}: 100 and 1k among them`);
await (await page.$(card))?.screenshot({ path: `${ROOT}/pictures-labels-normal.png` });
await page.click(`${card} [data-size-to="expanded"]`);
await uncovered(pic, 132, "Expanded");
await (await page.$(card))?.screenshot({ path: `${ROOT}/pictures-labels-expanded.png` });
await page.click(`${card} [data-size-to="normal"]`);
await page.click('[data-device="patch"]');
const patchPic = '[data-testid=graph] [data-node="curve"] [data-picture="curve"]';
await until(async () => !!(await page.$(patchPic)), 15000);
await page.$eval(patchPic, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
await until(async () => !!(await page.$(`${patchPic}[data-drawn] polyline[data-line="curve"]`)), 15000);
await uncovered(patchPic, 48, "on the patch's card");
await (await page.$('[data-testid=graph] [data-node="curve"]'))?.screenshot({ path: `${ROOT}/pictures-labels-patch.png` });

// The live layers on the shapes' plot at any zoom (taste.md DV-17, review S10): zoomed, after it
// showed, to where the picture is 40.65 px tall on screen (a size that isn't whole: its y labels fit
// three at 41 px and one at 40.65), its live layers' plot, as the song plays, is the curve's.
const onScreen = () => page.$eval(`${patchPic} svg`, (e) => e.getBoundingClientRect().height);
const zoomNow = () => page.$eval("[data-zoom]", (e) => Number(e.dataset.zoom));
await page.click('button[aria-label="Actual size"]');
await until(async () => (await zoomNow()) === 1, 3000);
await sleep(300);
const cssTall = await onScreen();
// The wheel where the graph is bare: on a picture it would move a dot.
const bare = await box("[data-testid=graph]");
await page.mouse.move(bare.x + 6, bare.y + 6);
await page.keyboard.down("Control");
await page.mouse.wheel({ deltaY: -240 * Math.log(40.65 / cssTall) });
await page.keyboard.up("Control");
await until(async () => Math.abs((await onScreen()) - 40.65) < 0.05, 3000);
await page.$eval(patchPic, (e) => e.scrollIntoView({ block: "center", inline: "center" })).catch(() => {});
await until(async () => !!(await page.$(`${patchPic}[data-asking]`)), 5000);
const plotted = () => page.$eval(patchPic, (e) => ({ live: e.querySelector("canvas[data-live]")?.dataset.plot, reads: Number(e.querySelector("canvas[data-live]")?.dataset.reads ?? 0), shapes: ["x", "width", "height"].map((k) => e.querySelector("svg clipPath rect")?.getAttribute(k)).join(","), size: e.querySelector("svg")?.dataset.size }));
const r0 = (await plotted()).reads;
await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
await until(async () => (await plotted()).reads >= r0 + 3, 10000);
const laid = await plotted();
await (await page.$(patchPic))?.screenshot({ path: `${ROOT}/pictures-live-zoomed.png` });
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });
check(laid.reads >= r0 + 3 && laid.live === laid.shapes, `zoomed to ${(await onScreen()).toFixed(2)} px tall on screen (laid out at ${laid.size}), the live layers' plot is the curve's: ${laid.live} against ${laid.shapes} (left, width, height; ${laid.reads - r0} reads)`);
await page.click('button[aria-label="Fit to panel"]');

done(errors);
