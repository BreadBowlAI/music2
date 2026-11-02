// Where a gesture goes from (AGENT-2), the second review's probes of `own-edits`, on a page whose
// reads of the song's state are held back 400 ms (fixlib's `late`). A gesture goes from the
// window's newest own revision in the run of its own changes right after the one it was drawn at,
// says it to the server, and is refused only when a change after it touched its target, naming
// who; after a change of its own since that moved ids in a file it targets, it isn't sent.
//  - an outside save that rode along with the window's own pan: the keys pan, drawn before, is
//    refused, naming outside the app, and the outside value is kept (P3);
//  - a Reverb added on Keys doesn't hold back a pan in song.ts; the window's own pan, then an
//    agent's change in tracks/keys.ts, then its pan again: written (P5, P6); a fader's first move
//    writes a `db()` call, which moves song.ts's ids: a pan pressed at once isn't sent, and says so;
//  - two Reverbs, Move right on the inner one, and its size dragged at once: not sent, said,
//    neither Reverb changed; ⌘Z of the move, and a drag at once: the same (P1);
//  - an agent's hit, then its undo: the window's drag drawn with the hit is refused naming the
//    agent, not the app (P7);
//  - an agent's marker while a marker is dragged is kept; two quick marker moves of the window's
//    own, a state read between, are both kept (P4);
//  - bars deleted right after an agent inserted bars before them: refused, naming the agent, and
//    nothing deleted (P2).
// node own-base.mjs <root> <home> <project>
import { createHash } from "node:crypto";
import { check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

const { page, errors, toasts, key, box, view, show, state, api, caughtUp, hold, drag } = await open(1440, 900, { late: 400 });
const refusals = [];
page.on("response", (r) => r.status() >= 400 && /\/api\//.test(r.url()) && refusals.push(`${r.request().method()} ${r.url().replace(/^.*\/api/, "/api")} ${r.status()}`));
let seen = [];
const watcher = setInterval(async () => {
  for (const t of await toasts().catch(() => [])) if (!seen.includes(t)) seen.push(t);
}, 40);
const MOVED = "The song changed while you pressed. Try again.";
const said = () => `toasts: ${JSON.stringify(seen)}, refused: ${JSON.stringify(refusals)}`;
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
const holdPost = (path, ms = 600) => hold((r) => (r.method() === "POST" && new URL(r.url()).pathname === path ? ms : 0));
const agent = async (path, body) => {
  const res = await fetch(`${sess.url}${path}`, {
    method: "POST",
    headers: { "x-m2-token": sess.token, "x-m2-as": "Claude", "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
const settle = async () => {
  await caughtUp();
  await sleep(800);
  await until(async () => !(await toasts()).length, 9000);
  seen = [];
  refusals.length = 0;
};
const hatsPan = '[data-testid=mix] [data-knob="hats.pan"]';
const keysPan = '[data-testid=mix] [data-knob="keys.pan"]';
const pan = () => /hats: \{ in: hats\.out, db: db\(-6\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];
const keysPanNow = () => /keys: \{ in: keys\.out, db: db\(-5\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];

// ---------------------------------------------------------------- an outside save rides along (P3)
await view("mix");
await page.waitForSelector(hatsPan);
await settle();
const set1 = page.waitForResponse((r) => r.url().endsWith("/api/graph/set"));
write("song.ts", read("song.ts").replace("pan: -0.25", "pan: 0.6"));
// At once: the hats pan, whose commit takes the outside save in with it.
await turn(hatsPan, 30);
const mixed = await (await set1).json();
check(mixed.mixed === true, `the window's commit says it took in a change made elsewhere: ${JSON.stringify({ mixed: mixed.mixed, revision: mixed.revision })}`);
// At once: the keys pan, drawn at -0.25 before both.
await turn(keysPan, 30);
check(await until(() => seen.some((t) => t.includes("changed outside the app"))), `the keys pan, drawn before, is refused, naming outside the app: ${said()}`);
await sleep(800);
check(keysPanNow() === "0.6", `the outside save's keys pan is kept: ${keysPanNow()}`);

// ---------------------------------------------------------------- moved per file, and the base (P5, P6)
await settle();
await view("graph");
await show("keys");
let off = await holdPost("/api/graph/effect/add", 900);
await page.click('[data-module="keys"] button[aria-label="Add an effect after pluck"]');
await page.waitForSelector('[data-effect="reverb"]');
await page.click('[data-effect="reverb"]');
await view("mix");
await page.waitForSelector(hatsPan);
let before = pan();
await turn(hatsPan, 30);
check(await until(() => read("tracks/keys.ts").includes("reverb(pluck(")), "a Reverb added on Keys");
await off();
check(await until(() => pan() !== before), `a pan in song.ts pressed right after it is written: ${before} -> ${pan()}`);
check(!seen.includes(MOVED), `and not held back: ${said()}`);

await settle();
const set2 = page.waitForResponse((r) => r.url().endsWith("/api/graph/set"));
await turn(hatsPan, 30);
await set2;
const first = pan();
const s1 = await state();
const pluck = s1.view.modules.find((m) => m.key === "keys").nodes.flatMap((n) => n.knobs).find((k) => k.slot === "brightness");
const theirs = await agent("/api/graph/set", { session: s1.session, revision: s1.view.revision, id: pluck.id, gesture: "agent", value: 3000 });
check(theirs.status === 200, `an agent sets the keys' brightness, in tracks/keys.ts: ${theirs.status}`);
// At once: the pan again, before the window shows its first turn.
await turn(hatsPan, 30);
check(await until(() => pan() !== first), `the pan turned again is written, never refused for the window's own turn: ${first} -> ${pan()}`);
check(refusals.length === 0, `nothing refused: ${said()}`);

await settle();
before = pan();
off = await holdPost("/api/graph/set", 700);
await turn('[data-testid=mix] [data-knob="kick.db"]', 30);
// Down this time: the pan is near its top.
await turn(hatsPan, -30);
check(await until(() => /kick: \{ in: kick\.out, db: db\(/.test(read("song.ts"))), `the kick's first volume is written as a db() call, a new call: ${/kick: [^\n]*/.exec(read("song.ts"))?.[0]}`);
await off();
check(await until(() => seen.includes(MOVED)), `so the pan pressed at once in song.ts isn't sent, and says so: ${said()}`);
check(pan() === before, `the pan is as it was: ${pan()}`);
await settle();
await turn(hatsPan, -30);
check(await until(() => pan() !== before), `turned again, it turns: ${pan()}`);

// ---------------------------------------------------------------- two Reverbs trade places (P1)
await settle();
await view("graph");
await show("keys");
await page.click('[data-module="keys"] button[aria-label="Add an effect after reverb"]');
await page.waitForSelector('[data-effect="reverb"]');
await page.click('[data-effect="reverb"]');
check(await until(() => (read("tracks/keys.ts").match(/reverb\(/g) ?? []).length === 2), "a second Reverb");
await settle();
// The inner one (the first written) made different, by a save outside.
write("tracks/keys.ts", read("tracks/keys.ts").replace("{ size: 0.6", "{ size: 0.3"));
const inner = '[data-knob-id="tracks/keys.ts#2/1.size"]';
check(await until(async () => (await page.$eval(inner, (e) => e.textContent).catch(() => "")).includes("0.3"), 8000), "the inner Reverb's size is drawn at 0.3");
await settle();
const sizes = () => [...read("tracks/keys.ts").matchAll(/size: ([\d.]+)/g)].map((m) => m[1]).join(", ");
const moveRight = async () => {
  await page.$eval(inner, (e) => e.closest("[data-node]").querySelector("[data-effect-menu]").click());
  await until(async () => !!(await page.$("[data-testid=effect-menu]")), 3000);
  await page.evaluate(() => [...document.querySelectorAll("[data-testid=effect-menu] [role=menuitem]")].find((x) => x.textContent === "Move right")?.click());
};
off = await holdPost("/api/graph/effect/move");
await moveRight();
// At once: the inner Reverb's size (0.3 drawn), up.
await turn(inner, 40);
check(await until(() => sizes() === "0.6, 0.3"), `Move right: the Reverbs trade places (${sizes()})`);
await off();
check(await until(() => seen.includes(MOVED)), `the size dragged at once isn't sent, and says so: ${said()}`);
await sleep(800);
check(sizes() === "0.6, 0.3", `neither Reverb changed: ${sizes()}`);
await settle();
off = await holdPost("/api/graph/undo");
await page.mouse.click(10, 10);
await key("Meta+z");
await turn(inner, 40);
check(await until(() => sizes() === "0.3, 0.6"), `⌘Z of the move: the Reverbs trade back (${sizes()})`);
await off();
check(await until(() => seen.includes(MOVED)), `a drag at once isn't sent, and says so: ${said()}`);
await sleep(800);
check(sizes() === "0.3, 0.6", `neither Reverb changed: ${sizes()}`);

// ---------------------------------------------------------------- an agent's undo, by the agent (P7)
await settle();
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
const text = read("data/hats/hits.json");
const s2 = await state();
const hit = { ...hatsClip(), notes: [...hatsClip().notes, { pitch: hatsClip().notes[0].pitch, start: 0.5, length: 0.25 }] };
const tx = {
  id: "own-base-clip",
  base: { "data/hats/hits.json": `sha256:${createHash("sha256").update(text).digest("hex")}` },
  ops: [{ op: "clips", lane: ["hats", "hits"], put: [hit], delete: [] }],
  expect: { confined: { ranges: {}, lanes: [["hats", "hits"]], modules: [] } },
};
check((await agent("/api/tx/commit", { session: s2.session, tx })).status === 200, "an agent puts a hit in the hats clip");
await settle();
const b = await box(clip);
check((await agent("/api/graph/undo", { session: s2.session })).status === 200 && read("data/hats/hits.json") === text, "and takes it back");
// At once: the clip's edge, as drawn with the agent's hit.
await edge(b.x + 3, b.y + b.height / 2, 4 * PPB);
check(await until(() => seen.some((t) => /the agent Claude changed/.test(t))), `the drag is refused, naming the agent who undid: ${said()}`);
check(!seen.some((t) => /changed in the app/.test(t)), `not the app: ${said()}`);
check(read("data/hats/hits.json") === text, "nothing written");

// ---------------------------------------------------------------- markers (P4)
await settle();
const markers = () => {
  try {
    return JSON.parse(read("layout.json")).markers ?? [];
  } catch {
    return [];
  }
};
const flag = (name) => box(`[data-marker-name="${name}"]`);
const s3 = await state();
await api("/markers", { method: "POST", body: JSON.stringify({ session: s3.session, markers: [{ beat: 0, name: "Intro" }, { beat: 16, name: "Verse" }] }) });
check(await until(async () => !!(await page.$('[data-marker-name="Verse"]'))), "two markers drawn");
await settle();
const mpb = await page.evaluate(() => {
  const a = document.querySelector('[data-marker-name="Intro"]'), z = document.querySelector('[data-marker-name="Verse"]');
  return (z.getBoundingClientRect().left - a.getBoundingClientRect().left) / 16;
});
let v = await flag("Verse");
await page.mouse.move(v.x + 2, v.y + v.height / 2);
await page.mouse.down();
await page.mouse.move(v.x + 2 + 2 * mpb, v.y + v.height / 2, { steps: 4 });
await agent("/api/graph/markers", { session: s3.session, markers: [{ beat: 0, name: "Intro" }, { beat: 16, name: "Verse" }, { beat: 32, name: "Chorus" }] });
check(await until(async () => !!(await page.$('[data-marker-name="Chorus"]')), 4000), "the agent's Chorus is drawn while Verse is dragged");
await page.mouse.move(v.x + 2 + 8 * mpb, v.y + v.height / 2, { steps: 6 });
await page.mouse.up();
check(await until(() => markers().some((m) => m.name === "Verse" && m.beat === 24)), `Verse let go at beat 24: ${JSON.stringify(markers())}`);
check(markers().some((m) => m.name === "Chorus"), `the agent's Chorus is kept: ${JSON.stringify(markers())}`);
await settle();
await api("/markers", { method: "POST", body: JSON.stringify({ session: s3.session, markers: [{ beat: 0, name: "Intro" }, { beat: 16, name: "Verse" }] }) });
await until(async () => !(await page.$('[data-marker-name="Chorus"]')));
await settle();
off = await holdPost("/api/graph/markers");
const dragFlag = async (name, beats) => {
  const f = await flag(name);
  await page.mouse.move(f.x + 2, f.y + f.height / 2);
  await page.mouse.down();
  await page.mouse.move(f.x + 2 + beats * mpb, f.y + f.height / 2, { steps: 6 });
  await page.mouse.up();
};
await dragFlag("Intro", 8);
// A state read between: an agent's unrelated change.
const s4 = await state();
await agent("/api/graph/set", { session: s4.session, revision: s4.view.revision, id: pluck.id, gesture: "agent", value: 2000 });
await sleep(150);
await dragFlag("Verse", 8);
check(await until(() => markers().some((m) => m.name === "Intro" && m.beat === 8) && markers().some((m) => m.name === "Verse" && m.beat === 24), 6000), `two quick moves, both kept: ${JSON.stringify(markers())}`);
await off();

// ---------------------------------------------------------------- bars after an agent's insert (P2)
await settle();
const spans = (lane) => JSON.parse(read(`data/${lane}.json`)).clips.map((c) => `${c.start}+${c.length}`).join(" ");
const r = await box("[data-testid=ruler]");
await drag({ x: r.x + 16.3 * PPB, y: r.y + 7 }, { x: r.x + 31.6 * PPB, y: r.y + 7 });
check(await until(async () => !!(await page.$("[data-testid=section]"))), "bars 5–8 selected");
await settle();
const s5 = await state();
check((await agent("/api/graph/sections", { session: s5.session, gesture: { op: "insert-bars", at: 0, bars: 4 } })).status === 200, "an agent inserts 4 bars at bar 1");
const inserted = spans("kick/hits");
// At once: the window still draws bars 5–8 as before the insert.
await key("Meta+Shift+Backspace");
check(await until(() => seen.some((t) => /the agent Claude changed/.test(t))), `the delete, drawn before the insert, is refused, naming the agent: ${said()}`);
await sleep(800);
check(spans("kick/hits") === inserted, `nothing deleted: ${spans("kick/hits")}`);

clearInterval(watcher);
done(errors);
