// Sidechain's dip (CARD-35, CARD-36), apart from sidechain.mjs: on first-song with a steady bass, a
// Duck around `bass.out` that listens to nothing plays the bass as it was, its meter still; made to
// listen to the kick (`kick.env` in its call, written as an outside edit), the bass's meter dips
// each time the kick's meter rises. Each meters message holds the most since the one before, so
// that needs messages shorter than the kick's dip: the meters' 33 ms tick, a sleep. So it is tagged
// `sleeps` (left out of GitHub's Mac job, whose tick comes every 70 to 250 ms: since the silent
// player keeps the wall clock's pace there, a message holds that much sound, a kick and the gap
// after it, and reads the bass's loudest). sidechain.mjs adds the Duck and picks Kick by the card.
//
//   node e2e/sidechain-dip.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";

// The bass as sidechain.mjs makes it: a steady 440 Hz sine, whose peak every block holds.
write(
  "tracks/bass.ts",
  `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { scale } from "std/math";

// A steady tone (its lanes kept, unplayed), so its meter shows only what a Duck does to it.
export default track("bass", { name: "Bass" }, ({ lane }) => {
  void [lane.notes("notes"), lane.control("cutoff", "hz", 500), lane.mask("drop")];
  return scale(osc.sine(440), 0.25);
});
`,
);
// Its Duck listening to nothing, as + after its sound adds it (sidechain.mjs).
const was = read("song.ts");
const none = was.replace("bass: duck(bass.out, kick.env, { depth: db(-9) }),", "bass: duck(bass.out, { depth: -8 }),");
write("song.ts", none);
check(none.includes("bass: duck(bass.out, { depth: -8 }),") && none.includes('import { duck } from "std/fx";'), "the song's bass goes through a Duck that listens to nothing");

const { page, errors, api, show } = await open(1440, 900);
await until(async () => !!(await page.$('[data-module="song"]')), 9000);
await show("song");
const duck = '[data-module="song"] [data-node="duck"]';
const listening = () => page.$eval(`${duck} [data-listens]`, (e) => e.textContent).catch(() => null);
check(await until(async () => (await listening()) === "Listens to:None"), `its card says it listens to nothing: ${await listening()}`);

// The server's meters messages, as the page gets them: each track's peak (linear).
const meters = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") meters.push(m.tracks ?? {});
  } catch {}
});
const peak = (v) => (Array.isArray(v) ? Math.max(...v.map(Math.abs)) : Math.abs(Number(v ?? 0)));
const db = (x) => 20 * Math.log10(Math.max(x, 1e-6));
const playing = async () => (await api("/status")).body.playing;
/**
 * Play for `ms` from the start: the bass's level (dB) where the kick's meter is up (over −20 dB)
 * and where it is down (under −40 dB), each the mean of the meters messages, and how far the bass
 * moved (dB, its loudest less its quietest, the first half second left out).
 */
const heard = async (ms) => {
  await page.mouse.click(10, 10);
  await page.keyboard.press("Space");
  await until(playing, 3000);
  await sleep(500);
  const from = meters.length;
  await sleep(ms);
  await page.keyboard.press("Space");
  await until(async () => !(await playing()), 3000);
  const got = meters.slice(from).filter((m) => "bass" in m);
  const mean = (xs) => xs.reduce((a, b) => a + b, 0) / Math.max(xs.length, 1);
  const up = got.filter((m) => db(peak(m.kick)) > -20).map((m) => db(peak(m.bass)));
  const down = got.filter((m) => db(peak(m.kick)) < -40).map((m) => db(peak(m.bass)));
  const all = got.map((m) => db(peak(m.bass)));
  return { up: mean(up), down: mean(down), n: [up.length, down.length], moved: Math.max(...all) - Math.min(...all) };
};
const said = (h) => `bass ${h.up.toFixed(1)} dB with the kick up, ${h.down.toFixed(1)} dB with it down (${h.n.join("/")} reads), moving ${h.moved.toFixed(1)} dB`;

const alone = await heard(3000);
check(alone.n[0] > 3 && alone.n[1] > 3 && alone.moved < 1.5, `with None it plays the bass as it was: ${said(alone)}`);

// ---------------------------------------------------------------- listening to the kick
write("song.ts", none.replace("bass: duck(bass.out, { depth: -8 }),", "bass: duck(bass.out, kick.env, { depth: -8 }),"));
check(await until(async () => (await listening()) === "Listens to:Kick", 9000), `written to listen to the kick, its card says Kick: ${await listening()}`);
const ducked = await heard(3000);
// Each message's peak is the loudest of its window, so a dip shows less than its depth (-8 dB at the
// kick's loudest): the bass moves where it didn't, lower while the kick's meter is up.
check(ducked.n[0] > 3 && ducked.n[1] > 3 && ducked.moved > 2 && ducked.down - ducked.up > 1, `the bass dips while the kick plays: ${said(ducked)}`);

done(errors);
