// The gate every gesture of the window goes through (AGENT-2), on a page that hears of each change
// late: its reads of the song's state held back 400 ms (fixlib's `late`). A gesture drawn before the
// window shows its own last change waits for it; then it goes from what the window shows when only
// the window's own changes that moved no ids came since, from what it was drawn on when a change
// made elsewhere came in between (the server refuses it if that touched its target, naming who), and
// not at all after a change of its own that moved ids ("The song changed while you pressed. Try
// again."). The review's probes of `own-edits`, each a check here:
//  - a Reverb's size dragged right after a second Reverb was added after it (which moves every id
//    inside the chain): not sent, said, and neither Reverb changes; dragged again, the one dragged;
//  - a pan turned right after ⌘Z took the window's own new track back: not sent, said, never refused
//    by the server nor left waiting; turned again, it turns;
//  - a take's count-in cancelled, then a pan turned: it lands at once, nothing waits for the take;
//  - 8 quick clicks on the Beat grid: 8 hits, none given up;
//  - two quick track moves, the second before the page shows the first: both kept;
//  - an agent sets the pan the window turned a moment before, and the window turns it again before
//    it shows its first turn: refused, naming the agent, the agent's value kept; the same for a clip.
// node own-gate.mjs <root> <home> <project>
import { createHash } from "node:crypto";
import { check, done, open, read, sess, sleep, until } from "./fixlib.mjs";

const { page, errors, toasts, key, box, view, show, state, api, caughtUp, hold } = await open(1440, 900, { late: 400 });
/** Every answer that refused something, as it came. */
const refusals = [];
page.on("response", (r) => r.status() >= 400 && /\/api\//.test(r.url()) && refusals.push(`${r.request().method()} ${r.url().replace(/^.*\/api/, "/api")} ${r.status()}`));
/** Every toast shown since `watch()`, each once. */
let seen = [];
let watching = 0;
const watch = () => {
  seen = [];
  clearInterval(watching);
  watching = setInterval(async () => {
    for (const t of await toasts().catch(() => [])) if (!seen.includes(t)) seen.push(t);
  }, 40);
};
const MOVED = "The song changed while you pressed. Try again.";
const said = () => `toasts: ${JSON.stringify(seen)}, refused: ${JSON.stringify(refusals)}`;
/** Drag the knob (or fader) `sel` up by `dy` pixels and let go: nothing waited for. */
const turn = async (sel, dy) => {
  const grip = `${sel} svg, ${sel} [data-testid=fader-thumb]`;
  await page.$eval(grip, (e) => e.scrollIntoView({ block: "center", inline: "center" }));
  const b = await box(grip);
  const cx = b.x + b.width / 2, cy = b.y + b.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  for (let i = 1; i <= 6; i++) await page.mouse.move(cx, cy - (i * dy) / 6);
  await page.mouse.up();
};
/** A request of an agent's, "Claude", with the tools key. */
const agent = async (path, body) => {
  const res = await fetch(`${sess.url}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { "x-m2-token": sess.token, "x-m2-as": "Claude", "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
/** Hold the page's own POSTs to `path` back `ms` before they reach the server: a gesture pressed meanwhile is pressed before the change is made. */
const holdPost = (path, ms = 600) => hold((r) => (r.method() === "POST" && new URL(r.url()).pathname === path ? ms : 0));
const settle = async () => {
  await caughtUp();
  await until(async () => !(await toasts()).length, 9000);
};

// ---------------------------------------------------------------- a pan right after a count-in cancelled
await view("mix");
const hatsPan = '[data-testid=mix] [data-knob="hats.pan"]';
await page.waitForSelector(hatsPan);
const pan = () => /hats: \{ in: hats\.out, db: db\(-6\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];
const record = async () => (await state()).record ?? null;
const inject = (body) => api("/midi/inject", { method: "POST", body: JSON.stringify(body) });
await page.click('[data-module-tab="bass"] [data-testid=track-label]');
await inject({ device: "Test Keys", plugged: true });
await sleep(300);
await page.$eval("[data-testid=timeline]", (e) => e.focus()).catch(() => {});
await page.keyboard.press("r");
check(await until(async () => (await record())?.phase === "count", 2000), "R counts in");
await page.keyboard.press("Escape");
check(await until(async () => (await record()) === null, 2000), "Esc cancels the count");
// Once the page has heard the take is over.
await sleep(300);
watch();
const t0 = Date.now();
await turn(hatsPan, 30);
const landed = await until(() => pan() === "0.5", 6000);
check(landed && Date.now() - t0 < 1500, `a pan turned right after lands at once: ${pan()} after ${Date.now() - t0} ms`);
check(!seen.some((t) => /Nothing was changed/.test(t)), `and says nothing: ${said()}`);

// ---------------------------------------------------------------- a drag right after a second Reverb
await settle();
await view("graph");
await show("keys");
const reverbs = () => read("tracks/keys.ts").trim().split("\n").pop();
const inner = () => /reverb\(pluck\(lane\.notes\("chords"\)\), \{ size: ([\d.]+)/.exec(reverbs())?.[1];
const outer = () => /\), \{ size: ([\d.]+), damping: [\d.]+, mix: [\d.]+ \}\)\);$/.exec(reverbs())?.[1];
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector('[data-effect="reverb"]');
await page.click('[data-effect="reverb"]');
check(await until(() => read("tracks/keys.ts").includes("reverb(pluck(")), "a first Reverb after the keys' pluck");
const size = '[data-module="keys"] [data-node="reverb"] [data-knob="size"]';
await page.waitForSelector(`${size} svg`);
await settle();
let n = refusals.length;
watch();
let off = await holdPost("/api/graph/effect/add");
await page.click('[data-module="keys"] button[aria-label="Add an effect after reverb"]');
await page.waitForSelector('[data-effect="reverb"]');
await page.click('[data-effect="reverb"]');
// At once, while the add is on its way and the page draws one Reverb: its size, turned up.
await turn(size, 40);
check(await until(() => (reverbs().match(/reverb\(/g) ?? []).length === 2), `a second Reverb comes: ${reverbs()}`);
await off();
check(await until(() => seen.includes(MOVED)), `the drag pressed before the page showed it isn't sent, and says so: ${said()}`);
await sleep(1000);
check(inner() === "0.6" && outer() === "0.6", `neither Reverb's size changed: ${inner()}, ${outer()}`);
check(refusals.length === n, `nothing was sent to be refused: ${said()}`);
await settle();
const innerSize = '[data-knob-id^="tracks/keys.ts#2/"][data-knob-id$="size"]';
check(!!(await page.$(innerSize)), "the Reverb dragged is the inner call now");
await turn(innerSize, 40);
check(await until(() => inner() !== "0.6"), `turned again, the Reverb dragged changes: ${inner()}`);
check(outer() === "0.6", `and the one just added doesn't: ${outer()}`);

// ---------------------------------------------------------------- a pan right after ⌘Z of a new track
await view("mix");
await page.waitForSelector(hatsPan);
const s0 = await state();
const drum = s0.instruments.find((i) => i.component === "@m2/drums:drum")?.component ?? s0.instruments[0].component;
const added = await api("/track/add", { method: "POST", body: JSON.stringify({ session: s0.session, component: drum }) });
check(added.status === 200, `a track added: ${added.status}`);
await settle();
const song1 = read("song.ts");
n = refusals.length;
watch();
off = await holdPost("/api/graph/undo");
const undone = page.waitForResponse((r) => r.url().endsWith("/api/graph/undo"));
await page.mouse.click(10, 10);
await key("Meta+z");
// At once, while the ⌘Z is on its way: the pan, turned up.
await turn(hatsPan, 30);
const u = await (await undone).json();
check(typeof u.revision === "number" && u.moved === true, `⌘Z of the track answers the revision it made, which moved ids: ${JSON.stringify({ revision: u.revision, moved: u.moved, file: u.file })}`);
check(await until(() => seen.includes(MOVED)), `the pan turned at once isn't sent, and says so: ${said()}`);
check(await until(() => read("song.ts") !== song1), "the track is taken back");
check(pan() === "0.5" && refusals.length === n && !seen.some((t) => /Nothing was changed/.test(t)), `nothing refused, nothing left waiting: ${pan()}, ${said()}`);
await off();
await settle();
await turn(hatsPan, 30);
check(await until(() => pan() === "0.7"), `turned again, it turns: ${pan()}`);

// ---------------------------------------------------------------- two quick track moves
await settle();
await view("graph");
const shownOrder = () => page.$$eval("[data-testid=timeline] [data-track]", (els) => els.filter((e) => !e.closest("[data-song-row]")).map((e) => e.dataset.track));
const order0 = await shownOrder();
const dragTo = async (m, onto) => {
  const from = await box(`[data-module-tab="${m}"]`);
  const to = await box(`[data-module-tab="${onto}"]`);
  await page.mouse.move(from.x + 60, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + 60, from.y + from.height / 2 - 10, { steps: 3 });
  await page.mouse.move(to.x + 60, to.y + 4, { steps: 10 });
  await page.mouse.up();
};
const [first, second, last] = [order0[0], order0[1], order0.at(-1)];
n = refusals.length;
// The last track to the top; then at once, before the first move is even written, the second.
off = await holdPost("/api/graph/layout");
await dragTo(last, first);
await sleep(100);
await dragTo(second, first);
check(await until(async () => {
  const o = await shownOrder();
  return o[0] === second && o[1] === last;
}, 6000), `both moves kept: ${second}, then ${last}, on top (${(await shownOrder()).join()})`);
await off();
check(refusals.length === n, `nothing refused: ${said()}`);

// ---------------------------------------------------------------- 8 quick clicks on the Beat grid
await settle();
const KICK = "kick-2";
await show("hats");
await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => null);
await page.keyboard.type("basic kit");
await sleep(150);
await page.keyboard.press("Enter");
check(await until(async () => ((await state()).view.drum_kits ?? []).length === 1), "a kit");
await settle();
await show(KICK);
await until(async () => !!(await page.$('[data-view-tab="beat"]')));
await page.click('[data-view-tab="beat"]');
await until(async () => !!(await page.$("[data-testid=beat]")));
await settle();
const hits = async () => {
  const t = (await state()).timeline.tracks.find((x) => x.module === KICK);
  return (t?.lanes.find((x) => x.key === "hits")?.clips ?? []).reduce((all, c) => all + c.notes.length, 0);
};
const hits0 = await hits();
const cells = [];
for (let i = 0; i < 8; i++) cells.push(await box(`[data-testid=beat-row][data-row="${KICK}"] [data-step="${i}"]`));
watch();
for (const b of cells) {
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
  await sleep(120);
}
check(await until(async () => (await hits()) - hits0 === 8, 12000), `8 clicks, 8 hits: ${(await hits()) - hits0}`);
check(!seen.some((t) => /Nothing was changed/.test(t)), `none given up: ${said()}`);

// ---------------------------------------------------------------- an agent's change in between
await settle();
await view("mix");
await page.waitForSelector(hatsPan);
watch();
n = refusals.length;
const set1 = page.waitForResponse((r) => r.url().endsWith("/api/graph/set"));
await turn(hatsPan, 30);
await set1;
const turned = pan();
const s1 = (await agent("/api/graph")).body;
const panId = s1.view.modules.flatMap((m) => m.nodes).flatMap((x) => x.knobs).find((k) => k.label === "hats.pan").id;
const theirs = await agent("/api/graph/set", { session: s1.session, revision: s1.view.revision, id: panId, gesture: "agent", value: -0.8 });
check(theirs.status === 200 && pan() === "-0.8", `an agent sets the pan the window just turned (${turned}): ${theirs.status} ${pan()}`);
// At once, the page still drawing the pan before its own turn.
await turn(hatsPan, 30);
check(await until(() => seen.some((t) => t.includes("the agent Claude changed it"))), `the window's turn is refused, naming the agent: ${said()}`);
await sleep(800);
check(pan() === "-0.8", `the agent's value is kept: ${pan()}`);

await settle();
await view("graph");
const hatsClip = () => JSON.parse(read("data/hats/hits.json")).clips[0];
const clip = '[data-lane="hats.hits"] [data-clip]';
await page.$eval(clip, (e) => e.scrollIntoView({ block: "center", inline: "start" }));
const PPB = 24;
const edge = async (x, y, dx) => {
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 8 });
  await page.mouse.up();
};
const b = await box(clip);
watch();
const trimmed = page.waitForResponse((r) => r.url().endsWith("/api/graph/clips"));
await edge(b.x + b.width - 3, b.y + b.height / 2, -4 * PPB);
await trimmed;
const length = hatsClip().length;
// An agent puts one more hit in that clip.
const text = read("data/hats/hits.json");
const theirsClip = { ...hatsClip(), notes: [...hatsClip().notes, { pitch: hatsClip().notes[0].pitch, start: 0.5, length: 0.25 }] };
const tx = {
  id: "own-gate-clip",
  base: { "data/hats/hits.json": `sha256:${createHash("sha256").update(text).digest("hex")}` },
  ops: [{ op: "clips", lane: ["hats", "hits"], put: [theirsClip], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["hats", "hits"]], modules: [] } },
};
const committed = await agent("/api/tx/commit", { session: s1.session, tx });
const kept = read("data/hats/hits.json");
check(committed.status === 200 && kept !== text, `an agent puts a hit in the clip the window just trimmed (${length} beats): ${committed.status}`);
// At once, the page still drawing the clip before its own trim: its left edge.
await edge(b.x + 3, b.y + b.height / 2, 4 * PPB);
check(await until(() => seen.some((t) => t.includes("the agent Claude changed"))), `the window's drag is refused, naming the agent: ${said()}`);
await sleep(800);
check(read("data/hats/hits.json") === kept, `the agent's clip is kept: ${JSON.stringify(hatsClip()).slice(0, 80)}`);
clearInterval(watching);
done(errors);
