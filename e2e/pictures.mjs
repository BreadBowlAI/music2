// Track pictures: a pack's picture on its card (CARD-60 to CARD-70). A test pack, @test/curve, has an
// EQ-like sound, Curve EQ, whose face is its picture: a curve worked out from its two bands, and a
// dot for each band (across its frequency, up and down its gain, the wheel its q, a double-click its
// on). Device's chain draws the curve; dragging band 2's dot moves the curve while the dot is held
// (the server draws it again from the drag's leases, CARD-68), and the release writes both values in
// one edit, one undo step, which ⌘Z takes back byte for byte. Picking a dot shows its band's tab
// (taste.md DV-18); a double-click turns its band off (one write). An agent asking for the picture reads
// the same shapes the window drew, as numbers, and each dot's knob ids (CARD-67). Each state linted.
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
      face: ["curve"],
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

done(errors);
