// The graph-side fixes: the effect browser and Enter, a Hz knob at its end, the canvas, feedback
// loops, one name at two inputs, std helpers as nodes, editor saves and undo, renames, unsaved
// files, and diagnostics columns.  node fixes-graph.mjs <root> <home> <project>
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, caughtUp, toasts, show, view, key, box, drag } = await open();
const keysOriginal = read("tracks/keys.ts");
const plus = (after) => `[data-module="keys"] button[aria-label="Add an effect after ${after}"]`;

// ---------------------------------------------------------------- the effect browser

await show("keys");
await page.click(plus("pluck"));
await page.waitForSelector("[data-effect-browser]");
for (const word of ["louder", "quieter", "volume"]) {
  await page.$eval("[data-effect-browser] input", (e) => e.select());
  await page.keyboard.type(word);
  check(await until(() => page.$('[data-effect-browser] [data-effect="gain"]').then(Boolean), 2000), `“${word}” finds gain`);
}
await page.$eval("[data-effect-browser] input", (e) => e.select());
await page.keyboard.type("zzzz");
const hint = await page.$eval("[data-effect-browser]", (e) => e.textContent);
check(/Try a word like/.test(hint) && hint.includes("louder"), "the empty list suggests words, among them “louder”");
await key("Escape");
check(await until(async () => !(await page.$("[data-effect-browser]"))), "Escape closes the browser");

// Enter adds the effect while the song plays, and playback goes on where it was.
await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
check(await until(async () => (await api("/status")).body.seconds > 1.2, 6000), "playing");
posts.length = 0;
await page.click(plus("pluck"));
await page.waitForSelector("[data-effect-browser] input");
await page.keyboard.type("reverb");
await page.keyboard.press("Enter");
check(await until(() => read("tracks/keys.ts").includes("reverb(pluck(")), "Enter in the browser adds the reverb");
await sleep(400);
const status = (await api("/status")).body;
check(!posts.some((p) => p.url === "/api/graph/seek"), `no seek after Enter (${posts.map((p) => p.url).join(", ")})`);
check(status.playing && status.seconds > 1.2, `playback went on from where it was: ${status.seconds.toFixed(2)} s`);
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });

// ---------------------------------------------------------------- a Hz knob at its end

await page.waitForSelector(plus("reverb"));
await page.click(plus("reverb"));
await page.waitForSelector('[data-effect-browser] [data-effect="lowpass"]');
await page.click('[data-effect-browser] [data-effect="lowpass"]');
check(await until(() => /lowpass\(reverb\(.*\), 1000\)/.test(read("tracks/keys.ts"))), "lowpass goes after the reverb at 1000 Hz");
const cutoff = '[data-module="keys"] [data-node="lowpass"] [data-knob="cutoff"]';
await page.waitForSelector(cutoff);
const knob = async () => {
  await page.$eval(`${cutoff} svg`, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  return page.$(`${cutoff} svg`).then((e) => e.boundingBox());
};
let k = await knob();
await drag({ x: k.x + 15, y: k.y + 15 }, { x: k.x + 15, y: k.y + 400 }, 20);
check(await until(() => /lowpass\(reverb\(.*\), 20\)/.test(read("tracks/keys.ts"))), "dragging the cutoff all the way down writes 20 Hz");
check(
  await until(async () => (await page.$eval(`${cutoff} svg`, (e) => Number(e.getAttribute("aria-valuenow")))) === 0),
  "and the knob sits at the bottom of its range, not the top",
);
k = await knob();
// Half its 300px throw (taste.md KN-1): half way up its log range, about 630 Hz.
await drag({ x: k.x + 15, y: k.y + 15 }, { x: k.x + 15, y: k.y + 15 - 150 }, 12);
const up = () => Number(/lowpass\(reverb\(.*\), ([\d.]+)\)/.exec(read("tracks/keys.ts"))?.[1]);
check(await until(() => up() > 200), `dragging it up turns it back up: ${up()} Hz`);

// ---------------------------------------------------------------- the canvas

const section = await box('[data-module="keys"]');
const canvas = await box('[data-module="keys"] [data-canvas]');
const scroller = await box("[data-testid=graph]");
const zoombar = await box("[data-zoom]");
check(Math.abs(section.y + section.height - (scroller.y + scroller.height - 16)) < 3, `the card fills the panel (${Math.round(section.y + section.height)} vs ${Math.round(scroller.y + scroller.height - 16)})`);
check(canvas.width > section.width - 48 && canvas.y + canvas.height > section.y + section.height - 24, "the dotted canvas fills the card");
check(
  zoombar.x > section.x && zoombar.x + zoombar.width < section.x + section.width && zoombar.y + zoombar.height < section.y + section.height,
  "the zoom bar sits inside the card",
);
await page.screenshot({ path: `${ROOT}/fix-keys-graph.png` });

// ---------------------------------------------------------------- a feedback loop, one name at two inputs, std helpers

const graphNow = async () => (await state()).view.modules.find((m) => m.key === "keys");
const revisionNow = async () => (await state()).view.revision;
const save = async (text) => {
  const r = await revisionNow();
  write("tracks/keys.ts", text);
  return until(async () => (await revisionNow()) > r);
};
check(
  await save(`import { track, feedback, type AudioKind } from "@m2/core";
import { pluck } from "@m2/synths";
import { lowpass } from "std/filter";
import { add, scale } from "std/math";

export default track("keys", ({ lane }) => {
  const echo = feedback<AudioKind>("audio<1>");
  const out = add(pluck(lane.notes("chords")), scale(echo, 0.5));
  echo.set(lowpass(out, 2000));
  return out;
});
`),
  "a feedback loop written in code is taken in",
);
await page.waitForSelector('[data-module="keys"] path[data-loop]');
const fit = await page.evaluate(() => {
  const s = document.querySelector('[data-module="keys"]').getBoundingClientRect();
  const inside = (r) => r.left >= s.left - 1 && r.right <= s.right + 1 && r.top >= s.top - 1 && r.bottom <= s.bottom + 1;
  const paths = [...document.querySelectorAll('[data-module="keys"] svg path')].map((p) => p.getBoundingClientRect());
  const cards = [...document.querySelectorAll('[data-module="keys"] [data-node]')].map((n) => n.getBoundingClientRect());
  return { paths: paths.every(inside), cards: cards.every(inside), loops: document.querySelectorAll("[data-loop]").length };
});
check(fit.loops === 1, "the edge back to the feedback is drawn as a loop");
check(fit.paths && fit.cards, "every edge and node stays inside the card");
const order = await page.evaluate(() => {
  const x = (label) => document.querySelector(`[data-module="keys"] [data-node="${label}"]`).getBoundingClientRect().left;
  return { feedback: x("feedback"), scale: x("scale"), add: x("add"), lowpass: x("lowpass"), pluck: x("pluck") };
});
check(order.feedback < order.scale && order.scale < order.add && order.add < order.lowpass && order.pluck < order.add, `the loop reads left to right: ${JSON.stringify(order)}`);
await page.screenshot({ path: `${ROOT}/fix-feedback.png` });

check(
  await save(`import { track } from "@m2/core";
import { pluck } from "@m2/synths";
import { mixer } from "std/mix";

export default track("keys", ({ lane }) => {
  const dry = pluck(lane.notes("chords"));
  return mixer({ left: { in: dry, pan: -1 }, right: { in: dry, pan: 1 } });
});
`),
  "one name written at two of the mixer's inputs is taken in",
);
let m = await graphNow();
const mixer = m.nodes.find((n) => n.label === "mixer"), pluckNode = m.nodes.find((n) => n.label === "pluck");
const into = m.port_edges.filter((e) => e.from === pluckNode.id && e.to === mixer.id).map((e) => mixer.inputs.find((p) => p.id === e.to_port)?.label);
check(JSON.stringify(into) === JSON.stringify(["left.in", "right.in"]), `an edge into each: ${into}`);
check(
  await until(() => page.$$eval(`[data-module="keys"] path[data-to="${mixer.id}"]`, (ps) => ps.length === 2)),
  "both are drawn",
);

check(
  await save(`import { track } from "@m2/core";
import { pluck } from "@m2/synths";
import { lowpass } from "std/filter";
import { sum } from "std/math";

export default track("keys", ({ lane }) => {
  const dry = pluck(lane.notes("chords"));
  const dark = lowpass(dry, 500);
  return sum(dry, dark, lowpass(dry, 3000));
});
`),
  "a sum of three signals is taken in",
);
m = await graphNow();
const sum = m.nodes.find((n) => n.label === "sum");
check(!!sum && sum.inputs.length === 3 && sum.outputs.length === 1, `sum is a node with three inputs and an output: ${sum?.inputs.map((p) => p.label)}`);
check(m.port_edges.some((e) => e.from === sum?.id && e.to === "export:out"), "and the export reads it");
check(m.port_edges.filter((e) => e.to === sum?.id).length === 3, "every signal summed has its edge");
await page.screenshot({ path: `${ROOT}/fix-sum.png` });
write("tracks/keys.ts", keysOriginal);
await until(async () => (await graphNow()).nodes.some((n) => n.label === "pluck") && !(await graphNow()).nodes.some((n) => n.label === "sum"));

// ---------------------------------------------------------------- diagnostics columns

const bass = read("tracks/bass.ts");
const broken = bass.replace("export default", 'const s = "🎸🎸"; export default track("x", (() => 1);\nexport default');
let r = await api("/file", { method: "PUT", body: JSON.stringify({ path: "tracks/bass.ts", base: bass, text: broken }) });
const problem = r.body.diagnostics?.[0];
const line = broken.split("\n")[(problem?.line ?? 1) - 1] ?? "";
check(problem && line.slice(problem.col - 1, problem.col) === ";", `an error after two emoji points at the ';': col ${problem?.col} of ${JSON.stringify(line.slice(0, 50))}`);
r = await api("/file", { method: "PUT", body: JSON.stringify({ path: "tracks/bass.ts", base: broken, text: bass }) });
check(r.status === 200 && r.body.diagnostics.length === 0, "fixed again");

// ---------------------------------------------------------------- the code editor: unsaved files, saves and undo

await show("song");
await view("code");
await until(() => page.$eval("[data-testid=code-path]", (e) => e.textContent === "song.ts").catch(() => false));
await page.waitForSelector("[data-testid=code] .view-lines");
posts.length = 0;
await page.click("[data-testid=code] .view-lines");
await key("Meta+ArrowUp");
await key("Home");
await page.keyboard.type("// my first edit\n");
check(
  await until(() => posts.some((p) => p.url === "/api/graph/unsaved" && JSON.parse(p.body).files.includes("song.ts"))),
  "the app is told song.ts has unsaved edits",
);
const song = read("song.ts");
await key("Meta+s");
check(await until(() => read("song.ts").startsWith("// my first edit\n")), "⌘S saves it");
check(await until(() => posts.some((p) => p.url === "/api/graph/unsaved" && JSON.parse(p.body).files.length === 0)), "and then that nothing is unsaved");
check(await until(async () => (await state()).can_undo), "a saved file can be undone");

// A lane edit after it, then undo goes back through both.
const s = await state();
const keysClips = s.timeline.tracks.find((t) => t.module === "keys").lanes[0].clips;
const more = keysClips.map((c, i) => (i === 0 ? { ...c, notes: [...c.notes, { pitch: 84, start: 0, length: 1, velocity: 0.8 }] } : c));
r = await api("/lane", { method: "POST", body: JSON.stringify({ session: s.session, revision: s.view.revision, module: "keys", lane: "chords", clips: more }) });
check(r.status === 200, "a note added to Keys");
const notesAfter = read("data/keys/chords.json");
// Undo takes back the step the page shows, and the page didn't make this one: it shows it first.
await caughtUp();
await page.click('button[aria-label="Undo"]');
check(await until(() => read("data/keys/chords.json") !== notesAfter), "Undo takes the note back");
await page.click('button[aria-label="Undo"]');
check(await until(() => read("song.ts") === song.replace("// my first edit\n", "")), "Undo again takes the save back");
check(await until(() => page.$eval("[data-testid=code] .view-lines", (e) => !e.textContent.includes("my first edit"))), "and the editor shows the file as it is");
await page.click('button[aria-label="Redo"]');
check(await until(() => read("song.ts").startsWith("// my first edit\n")), "Redo saves it again");

// ---------------------------------------------------------------- renaming a track's key in the code

await show("keys");
await until(() => page.$eval("[data-testid=code-path]", (e) => e.textContent === "tracks/keys.ts").catch(() => false));
await page.click("[data-testid=code] .view-lines");
await key("Meta+f");
await page.waitForSelector("[data-testid=code] .find-widget.visible");
await page.keyboard.type("keys");
await sleep(150);
await key("Escape");
await page.keyboard.type("piano-keys");
await key("Meta+s");
check(await until(() => read("tracks/keys.ts").includes('track("piano-keys"')), "the track with its key renamed is saved");
check(await until(async () => (await page.$eval("[data-module-tab][aria-selected=true]", (e) => e.dataset.moduleTab)) === "piano-keys"), "its tab, under its new key, stays selected");
await sleep(1500);
const openFile = await page.$eval("[data-testid=code-path]", (e) => e.textContent);
check(openFile === "tracks/keys.ts", `the editor stays on the file just saved: ${openFile}`);
await page.screenshot({ path: `${ROOT}/fix-rename.png` });

await browser.close();
done(errors);
