// The server knows each revision's window, and beats moved are a change too (AGENT-2): the third
// check of `own-edits`, on a page whose reads of the song's state are held back 400 ms.
//  - the window's pan, its set held while an agent's change in tracks/keys.ts lands first, then the
//    pan again at once: both turns land, never refused for the window's own change (Q3);
//  - ⌘Z of the window's own pan takes in an outside save of the keys pan: its answer says `mixed`,
//    and the keys pan drawn before it is refused, naming outside, the outside value kept (Q1);
//  - ⌘I inserts bars (held a moment), and the Beat grid clicked at once, as drawn: not sent, said,
//    no hit lands in the bars inserted; an agent's bars, then a click at once: refused, naming the
//    agent, no hit (Q2).
// node own-window.mjs <root> <home> <project>
import { check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

const { page, errors, toasts, key, box, view, show, state, caughtUp, hold, drag } = await open(1440, 900, { late: 400 });
let seen = [];
const watcher = setInterval(async () => {
  for (const t of await toasts().catch(() => [])) if (!seen.includes(t)) seen.push(t);
}, 40);
const MOVED = "The song changed while you pressed. Try again.";
const said = () => `toasts: ${JSON.stringify(seen)}`;
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
const holdPost = (path, ms = 500) => hold((r) => (r.method() === "POST" && new URL(r.url()).pathname === path ? ms : 0));
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
};
const hatsPan = '[data-testid=mix] [data-knob="hats.pan"]';
const keysPan = '[data-testid=mix] [data-knob="keys.pan"]';
const pan = () => /hats: \{ in: hats\.out, db: db\(-6\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];
const keysPanNow = () => /keys: \{ in: keys\.out, db: db\(-5\), pan: (-?[\d.]+) \}/.exec(read("song.ts"))?.[1];

// ---------------------------------------------------------------- an agent's change lands first (Q3)
await view("mix");
await page.waitForSelector(hatsPan);
await settle();
const s0 = await state();
const pluck = s0.view.modules.find((m) => m.key === "keys").nodes.flatMap((n) => n.knobs).find((k) => k.slot === "brightness");
let off = await holdPost("/api/graph/set");
await turn(hatsPan, 30);
// While the window's commit is on its way, the agent's change in another file lands first.
const theirs = await agent("/api/graph/set", { session: s0.session, revision: s0.view.revision, id: pluck.id, gesture: "agent", value: 3000 });
check(theirs.status === 200, `an agent sets the keys' brightness first: ${theirs.status}`);
// At once: the pan again, drawn before both.
await turn(hatsPan, 30);
await sleep(600);
await off();
check(await until(() => pan() === "0.7"), `both of the window's turns land: 0.3 → 0.5 → 0.7 (${pan()})`);
check(!seen.some((t) => /Nothing was changed|changed in the app/.test(t)), `never refused for its own change: ${said()}`);

// ---------------------------------------------------------------- ⌘Z takes an outside save in (Q1)
await settle();
const undone = page.waitForResponse((r) => r.url().endsWith("/api/graph/undo"));
write("song.ts", read("song.ts").replace("pan: -0.25", "pan: 0.6"));
await page.mouse.click(10, 10);
await key("Meta+z");
const u = await (await undone).json();
check(u.mixed === true && pan() === "0.5", `⌘Z of the window's pan took the outside save in: it says mixed (${JSON.stringify({ mixed: u.mixed, revision: u.revision })}, pan ${pan()})`);
// At once: the keys pan, drawn at -0.25 before both.
await turn(keysPan, 30);
check(await until(() => seen.some((t) => t.includes("changed outside the app"))), `the keys pan, drawn before, is refused, naming outside: ${said()}`);
await sleep(800);
check(keysPanNow() === "0.6", `the outside save's keys pan is kept: ${keysPanNow()}`);

// ---------------------------------------------------------------- bars moved, and the Beat grid (Q2)
await settle();
const KICK = "kick-2";
await view("graph");
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
const hitsAt = async () => {
  const t = (await state()).timeline.tracks.find((x) => x.module === KICK);
  return (t?.lanes.find((x) => x.key === "hits")?.clips ?? []).flatMap((c) => c.notes.map((n) => c.start + n.start));
};
const hats = () => JSON.parse(read("data/hats/hits.json")).clips.map((c) => `${c.start}+${c.length}`).join(" ");
const PPB = 24;
const r = await box("[data-testid=ruler]");
await drag({ x: r.x + 0.3 * PPB, y: r.y + 7 }, { x: r.x + 15.6 * PPB, y: r.y + 7 });
check(await until(async () => !!(await page.$("[data-testid=section]"))), "bars 1–4 selected");
await settle();
let cell = await box(`[data-testid=beat-row][data-row="${KICK}"] [data-step="1"]`);
const hats0 = hats();
off = await holdPost("/api/graph/sections");
await key("Meta+i");
// At once: kick-2's step at beat 1, as drawn.
await page.mouse.click(cell.x + cell.width / 2, cell.y + cell.height / 2);
check(await until(() => hats() !== hats0), `⌘I inserts bars: hats ${hats0} → ${hats()}`);
await off();
check(await until(() => seen.includes(MOVED)), `the step clicked at once, drawn before the bars moved, isn't sent, and says so: ${said()}`);
await sleep(800);
check(!(await hitsAt()).includes(1), `no hit lands in the bars inserted: ${JSON.stringify(await hitsAt())}`);

await settle();
cell = await box(`[data-testid=beat-row][data-row="${KICK}"] [data-step="1"]`);
const s1 = await state();
check((await agent("/api/graph/sections", { session: s1.session, gesture: { op: "insert-bars", at: 0, bars: 4 } })).status === 200, "an agent inserts 4 bars at bar 1");
// At once: the same step, as drawn before the agent's bars.
await page.mouse.click(cell.x + cell.width / 2, cell.y + cell.height / 2);
check(await until(() => seen.some((t) => /the agent Claude/.test(t))), `the step clicked at once is refused, naming the agent: ${said()}`);
await sleep(800);
check(!(await hitsAt()).includes(1), `no hit lands in the agent's bars: ${JSON.stringify(await hitsAt())}`);

clearInterval(watcher);
done(errors);
