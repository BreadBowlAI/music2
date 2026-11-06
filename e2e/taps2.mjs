// Looks in the app (TAP-10 to TAP-16): while the song plays, this window asks for looks at the
// Duck in first-song's song.ts (the bass, ducked whenever the kick hits) and at the Master, and
// each arrives on this window's socket beside the meters: the duck's meter, how far it turns the
// bass down, which moves with the kick; the spectrum of what goes into it (the bass, loudest low
// down); the levels of what goes in and what comes out; and the Master's ceiling's meter. A meter
// the duck doesn't give is refused in words, and a stopped look stops coming. Two scopes of one
// call on one card share one ask (TAP-16): folding the card away from one doesn't stop the other.
//
//   node e2e/taps2.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { page, errors, api, state, view, posts } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// The page's looks, as they arrive (CDP), and the window's client id, from its socket's address.
const looked = [];
let client = null;
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketCreated", ({ url }) => {
  client = new URL(url).searchParams.get("client") ?? client;
});
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "looks") looked.push(m);
  } catch {}
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid=timeline] [data-track="kick"]', { timeout: 10000 });
const playing = async () => (await api("/status")).body.playing;

const { session, view: graphView } = await state();
const song = graphView.modules.find((m) => m.key === "song");
const duck = song?.nodes.find((n) => n.label === "duck");
const master = song?.nodes.find((n) => n.label === "master");
check(!!duck && !!master && !!client, `the Duck and the Master are boxes of the song (${duck?.id}, ${master?.id}), and this window has a client (${client})`);

const ask = (node, kind, at, on = true) => post("/look", { session, client, module: "song", node: node?.id, kind, at, on });
/** This window's looks of `kind` at `at` of `node` since message `from`. */
const seen = (from, node, kind, at) => looked.slice(from).flatMap((m) => m.looks.filter((s) => s.node === node?.id && s.kind === kind && s.at === at));

const meter = await ask(duck, "meter", "reduction");
check(meter.status === 200 && meter.body.look?.unit === "db", `the duck's meter is asked for: its unit is dB (${JSON.stringify(meter.body)})`);
const spectrum = await ask(duck, "spectrum", "in");
const hz = spectrum.body.look?.hz ?? [];
check(spectrum.status === 200 && hz.length === 120 && hz[0] === 20, `the spectrum of what goes into it: 120 bands from 20 Hz (${hz.length}, ${hz[0]})`);
const levels = [await ask(duck, "level", "in"), await ask(duck, "level", "out")];
check(levels.every((l) => l.status === 200 && l.body.look?.channels >= 1), `and the levels of what goes in and comes out (${levels.map((l) => l.body.look?.channels)})`);
const ceiling = await ask(master, "meter", "reduction");
check(ceiling.status === 200, `the Master's ceiling's meter is asked for too (${JSON.stringify(ceiling.body).slice(0, 120)})`);
const no = await ask(duck, "meter", "gain");
check(no.status >= 400 && no.body.error === "duck gives no meter gain: its meter is reduction.", `a meter it doesn't give is refused in words: "${no.body.error}"`);

// The first bar, looped, so the kick hits again and again.
await post("/loop", { start: 0, end: 4, on: true });
await page.keyboard.press("Space");
check(await until(playing, 3000), "Space plays the first bar, looped");
let from = looked.length;
// The next 40 messages, however fast they come (the meters' 33 ms sleep is longer on a busy Mac).
await until(() => seen(from, duck, "meter", "reduction").length >= 40, 15000);
const turns = seen(from, duck, "meter", "reduction");
const most = Math.max(...turns.map((s) => s.most ?? 0));
const least = Math.min(...turns.map((s) => s.now ?? 99));
check(turns.length >= 40 && turns.every((s) => typeof s.now === "number" && s.most >= s.now), `the duck's meter comes in each message: now, and the most since the one before (${turns.length} messages)`);
check(most > 3 && most <= 9.01 && least < most - 2, `it moves with the kick: up to ${most.toFixed(1)} dB down (its depth is 9 dB), and back to ${least.toFixed(1)}`);
const bins = seen(from, duck, "spectrum", "in").at(-1)?.bins ?? [];
const loudest = bins.indexOf(Math.max(...bins));
check(bins.length === 120 && hz[loudest] < 250, `the bass goes in: its spectrum is loudest at ${hz[loudest]?.toFixed(0)} Hz`);
const peak = (at) => Math.max(...seen(from, duck, "level", at).map((s) => s.peak?.[0] ?? 0));
const [went, came] = [peak("in"), peak("out")];
check(went > 0.05 && came > 0 && came <= went + 1e-6, `what comes out is never louder than what goes in (${went.toFixed(3)} in, ${came.toFixed(3)} out)`);
const top = seen(from, master, "meter", "reduction");
check(top.length > 10 && top.every((s) => s.now >= 0), `the Master's ceiling says how far it turns down, 0 dB or more (${top.length} messages, up to ${Math.max(...top.map((s) => s.most)).toFixed(2)} dB)`);
const others = looked.slice(from).flatMap((m) => m.looks.filter((s) => s.node !== duck.id && s.node !== master.id));
check(others.length === 0, `only what this window asked for comes (${others.length} others)`);

// Stopped, a look stops coming; the others go on.
await ask(duck, "meter", "reduction", false);
await sleep(300);
from = looked.length;
await until(() => seen(from, duck, "spectrum", "in").length >= 6, 8000);
check(seen(from, duck, "spectrum", "in").length >= 6 && seen(from, duck, "meter", "reduction").length === 0, `stopped, the meter stops coming; the spectrum goes on`);
for (const [n, kind, at] of [[duck, "spectrum", "in"], [duck, "level", "in"], [duck, "level", "out"], [master, "meter", "reduction"]]) await ask(n, kind, at, false);

// ---------------------------------------------------------------- two views of one look (TAP-16)

const TWICE = `import { component, port, track, view } from "@m2/core";
import { osc } from "std/dsp";

const twice = component(
  { id: "twice", label: "Twice", output: port.audio(), editor: { views: { a: view.scope(), b: view.scope() }, groups: { One: ["a"], Two: ["b"] } } },
  () => osc.saw(110),
);

export default track("twice", { name: "Twice" }, () => twice());
`;
write("tracks/twice.ts", TWICE);
write("song.ts", read("song.ts").replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport twice from "./tracks/twice";').replace("      kick: kick.out,", "      kick: kick.out,\n      twice: twice.out,"));
await until(async () => !!(await page.$('[data-module-tab="twice"]')), 15000);
await page.click('[data-module-tab="twice"] [data-testid=track-label]');
await view("graph");
const card = '[data-module="twice"] [data-node="twice"]';
await until(async () => (await page.$$(`${card} [data-scope]`)).length === 2, 15000);
await page.$eval(card, (e) => e.scrollIntoView({ block: "start", inline: "center" }));
const drawn = () => page.$$eval(`${card} [data-scope]`, (els) => els.map((e) => Number(e.dataset.frames ?? 0)));
check(await until(async () => (await drawn()).every((n) => n >= 3), 10000), `the card's two scopes of one call both draw while it plays (${await drawn()})`);
const stops = () => posts.filter((p) => p.url === "/api/graph/look" && JSON.parse(p.body).module === "twice" && JSON.parse(p.body).on === false).length;
const [was, stopped] = [(await drawn())[0], stops()];
await page.click(`${card} [data-fold]`);
await until(async () => (await page.$$(`${card} [data-scope]`)).length === 1, 5000);
await sleep(1500);
const now = (await drawn())[0];
await page.mouse.click(10, 10);
check(stops() === stopped && now >= was + 5, `folded away from one, the other goes on drawing, and no stop is sent (${stops() - stopped} stops; ${was} then ${now} drawings)`);
await page.keyboard.press("Space");
await until(async () => !(await playing()), 3000);

done(errors);
