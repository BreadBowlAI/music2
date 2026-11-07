// Track measure: curves and hits measured from the sound itself (CARD-80 to CARD-86). A test pack,
// @test/measured, has a lowpass, Tone, whose face is its response: the curve the server measures by
// playing a click through a copy of the call; and an instrument, Thump, whose face is its hit: one
// note's sound, played by a copy of the call. Device's chain draws each. Dragging Tone's cutoff knob
// moves the measured curve while the knob is held (the server measures it again at the dragged
// value, CARD-85); stepping Thump's decay knob draws its hit again, shorter. An agent asking for the
// picture reads the points the window drew (CARD-86). Each state linted.
//
//   node e2e/measure.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import path from "node:path";
import { PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, view, box } = await open(1440, 900, { device: "chain" });

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

// ---------------------------------------------------------------- the test pack, and two tracks playing it

const PACK = path.join(ROOT, "measuredpack");
fs.mkdirSync(path.join(PACK, "src"), { recursive: true });
fs.writeFileSync(
  path.join(PACK, "m2pkg.toml"),
  `[package]\nname = "@test/measured"\nversion = "0.1.0"\nlicense = "MIT"\n\n[dependencies]\n# p.response and p.hit, since 2.4 (CARD-80).\n"@m2/core" = "2.4"\nstd = "0.10"\n`,
);
fs.writeFileSync(
  path.join(PACK, "src/index.ts"),
  `import { axis, component, control, poly, port, view } from "@m2/core";
import { lowpass } from "std/filter";
import { adsr, osc, vca } from "std/dsp";

/** A lowpass whose curve is measured from its own sound. */
export const tone = component(
  {
    id: "tone",
    label: "Tone",
    inputs: { x: port.audio() },
    controls: { cutoff: control.hz(1000).range(20, 20000).log(), q: control.scalar(0.7).range(0.3, 20).log() },
    output: port.audio(),
    editor: { views: { curve: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) }, face: ["curve"] },
  },
  (x, { cutoff, q }) => lowpass(x, { cutoff, q }),
  {
    curve: {
      draw: (v, p) => [p.response({ name: "curve" }), p.handle(v.cutoff, 0, { x: "cutoff" })],
      words: (v) => \`A lowpass at \${v.cutoff} Hz\`,
    },
  },
);

/** A plucked sine whose hit is drawn from its own sound. */
export const thump = component(
  {
    id: "thump",
    label: "Thump",
    inputs: { notes: port.notes() },
    controls: { decay: control.secs(0.3).range(0.01, 2).log() },
    output: port.audio(),
    editor: { views: { hit: view.picture({ x: axis.time(0.5), y: axis.lin("bipolar", -1, 1) }) }, face: ["hit"] },
  },
  (notes, { decay }) => poly(notes, (n) => vca(osc.sine(n.pitch), adsr(n.gate, { attack: 0.001, decay, sustain: 0, release: 0.05 }))),
  { hit: { draw: (v, p) => [p.hit({ note: 48 })], words: (v) => \`A pluck that fades in \${v.decay} s\` } },
);
`,
);
const manifest = read("music2.toml");
write("music2.toml", manifest.replace("[dependencies]\n", `[dependencies]\n"@test/measured" = { path = ${JSON.stringify(path.relative(PROJ, PACK))} }\n`));
write(
  "tracks/tone.ts",
  `import { track } from "@m2/core";\nimport { osc } from "std/dsp";\nimport { tone } from "@test/measured";\n\nexport default track("tone", { name: "Tone" }, () => tone(osc.saw(110), { cutoff: 2000, q: 2 }));\n`,
);
write(
  "tracks/thump.ts",
  `import { track } from "@m2/core";\nimport { thump } from "@test/measured";\n\nexport default track("thump", { name: "Thump" }, ({ lane }) => thump(lane.notes("notes"), { decay: 0.6 }));\n`,
);
const song = read("song.ts");
write(
  "song.ts",
  song
    .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport tone from "./tracks/tone";\nimport thump from "./tracks/thump";')
    .replace("      kick: kick.out,", "      kick: kick.out,\n      tone: tone.out,\n      thump: thump.out,"),
);

// ---------------------------------------------------------------- the response follows a knob's drag

await until(async () => !!(await page.$('[data-module-tab="tone"]')), 20000);
await page.click('[data-module-tab="tone"] [data-testid=track-label]');
await view("graph");
const card = '[data-testid=chain] [data-module="tone"] [data-node="tone"]';
const pic = `${card} [data-picture="curve"]`;
const curve = () => page.$eval(`${pic} polyline[data-response]`, (e) => e.getAttribute("points")).catch(() => null);
check(await until(async () => ((await curve()) ?? "").split(" ").length === 256, 20000), `Tone's face draws its measured curve, 256 points (CARD-86): ${((await curve()) ?? "").split(" ").length}`);
const drawnCount = () => page.$eval(pic, (e) => Number(e.dataset.drawn ?? 0));
await linted(card, "the measured curve's card");
await page.screenshot({ path: `${ROOT}/measure-curve.png` });

// The curve's highest point, in pixels from the top: a resonance at the cutoff.
const peakAt = async () => {
  const pts = ((await curve()) ?? "").split(" ").map((p) => p.split(",").map(Number));
  return pts.reduce((best, p) => (p[1] < best[1] ? p : best), [0, Infinity])[0];
};
const before = await curve();
const peak0 = await peakAt();
const code = read("tracks/tone.ts");
const knob = `${card} [data-knob="cutoff"] svg[role=slider]`;
check(await until(async () => !!(await page.$(knob)), 5000), "its cutoff knob shows");
const k = await box(knob);
const [x0, y0] = [k.x + k.width / 2, k.y + k.height / 2];
await page.mouse.move(x0, y0);
await page.mouse.down();
const drawn0 = await drawnCount();
for (let i = 1; i <= 10; i++) {
  await page.mouse.move(x0, y0 - i * 6);
  await sleep(30);
}
check(await until(async () => (await drawnCount()) > drawn0 && (await curve()) !== before, 5000), "while the knob is held, the measured curve follows it: measured again at the dragged value (CARD-85)");
const peak1 = await peakAt();
check(peak1 > peak0 + 10, `the resonance moved up the frequencies with the cutoff: from x ${peak0} to ${peak1}`);
check(read("tracks/tone.ts") === code, "nothing is written while it is held");
await page.mouse.up();
check(await until(async () => read("tracks/tone.ts") !== code, 8000), "the release writes the code");
const cutoff = Number(/cutoff: (\d+)/.exec(read("tracks/tone.ts"))?.[1]);
check(cutoff > 2000, `the drag moved the cutoff up: ${cutoff}`);

// An agent reads the same points, as numbers (CARD-86).
const state = (await api("")).body;
const node = state.view.modules.find((m) => m.key === "tone").nodes.find((n) => n.title === "Tone");
const asked = await agent("/api/graph/picture", { session: state.session, revision: state.view.revision, client: "reader", module: "tone", node: node.id, view: "curve", on: true });
const response = asked.picture?.shapes?.find((s) => s.kind === "response");
check(response?.name === "curve" && response?.points?.length === 512, `an agent reads the measured curve: ${response?.points?.length} numbers`);
// Each point the agent reads, placed on the picture as the window places it (a log axis across, dB
// up, above the axis's labels), is a point the window drew (review M11).
const placed = async () => {
  const [w, h] = (await page.$eval(`${pic} svg`, (e) => e.dataset.size)).split("x").map(Number);
  const plot = Math.max(1, h - 12);
  const out = [];
  for (let i = 0; i + 1 < response.points.length; i += 2) {
    const x = (Math.log(response.points[i] / 20) / Math.log(1000)) * w;
    const y = (1 - (response.points[i + 1] + 24) / 48) * plot;
    out.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  }
  return out.join(" ");
};
check(await until(async () => (await curve()) === (await placed())), "the points the window drew are the agent's, one for one");
const near1k = response.points.findIndex((x, i) => i % 2 === 0 && x >= 1000);
check(Math.abs(response.points[near1k + 1]) < 1, `below the cutoff a lowpass passes the sound: ${response.points[near1k + 1]} dB at ${Math.round(response.points[near1k])} Hz`);
const top = response.points.length - 1;
check(response.points[top] < -24, `far above it (${cutoff} Hz), it cuts past the picture's floor: ${response.points[top]} dB at ${Math.round(response.points[top - 1])} Hz`);

// ---------------------------------------------------------------- the hit is drawn again when a knob moves

await page.click('[data-module-tab="thump"] [data-testid=track-label]');
const card2 = '[data-testid=chain] [data-module="thump"] [data-node="thump"]';
const pic2 = `${card2} [data-picture="hit"]`;
const hit = () => page.$eval(`${pic2} polygon[data-hit]`, (e) => e.getAttribute("points")).catch(() => null);
check(await until(async () => ((await hit()) ?? "").split(" ").length === 512, 20000), `Thump's face draws its hit, 256 columns there and back (CARD-86): ${((await hit()) ?? "").split(" ").length}`);
await linted(card2, "the hit's card");
await page.screenshot({ path: `${ROOT}/measure-hit.png` });
// How tall the hit is in its last half, in pixels: a shorter decay leaves less.
const lateHeight = async () => {
  const pts = ((await hit()) ?? "").split(" ").map((p) => p.split(",").map(Number));
  const n = pts.length / 2;
  const width = Math.max(...pts.map((p) => p[0]));
  let sum = 0;
  for (let i = 0; i < n; i++) if (pts[i][0] > width / 2) sum += pts[pts.length - 1 - i][1] - pts[i][1];
  return sum;
};
const tall0 = await lateHeight();
check(tall0 > 10, `the pluck still sounds in its last half: ${Math.round(tall0)} px`);
const hit0 = await hit();
const code2 = read("tracks/thump.ts");
const decay = `${card2} [data-knob="decay"] svg[role=slider]`;
await page.$eval(decay, (e) => e.focus());
await page.keyboard.down("Shift");
for (let i = 0; i < 6; i++) await page.keyboard.press("ArrowDown");
await page.keyboard.up("Shift");
check(await until(async () => read("tracks/thump.ts") !== code2, 8000), `six ⇧↓ on its decay knob write a shorter decay: ${/decay: [^ }]*/.exec(read("tracks/thump.ts"))?.[0]}`);
check(await until(async () => (await hit()) !== hit0, 8000), "the hit is drawn again (CARD-85)");
check(await until(async () => (await lateHeight()) < tall0 * 0.8, 8000), `shorter: ${Math.round(await lateHeight())} px against ${Math.round(tall0)} px in its last half`);
await linted(card2, "the hit's card after the knob");

done(errors);
