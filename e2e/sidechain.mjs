// Sidechain from the card (CARD-35, CARD-36, PKG-19): a Duck added from + after the bass's sound
// where the song reads it (`bass: bass.out`) listens to nothing, says None, and plays the bass as
// it was; Listens to lists the song's other tracks and None, and picking Kick writes `kick.env` in
// its call, one undo step ("Bass's duck listens to Kick"). Played, the bass's meter then dips each
// time the kick's meter rises, as it didn't before; ⌘Z puts the call back as it was added.
//
// On first-song (the `fix` kind), its bass made a steady tone first (its lanes kept, unplayed), so
// its meter shows only what the Duck does, and its Duck taken out of song.ts.
//
//   node e2e/sidechain.mjs <root> <home> <project>
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

write(
  "tracks/bass.ts",
  `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { scale } from "std/math";

// A steady tone (its lanes kept, unplayed), so its meter shows only what a Duck does to it.
export default track("bass", { name: "Bass" }, ({ lane }) => {
  void [lane.notes("notes"), lane.control("cutoff", "hz", 500), lane.mask("drop")];
  return scale(osc.saw(55), 0.25);
});
`,
);
const was = read("song.ts");
write("song.ts", was.replace('import { duck } from "std/fx";\n', "").replace("bass: duck(bass.out, kick.env, { depth: db(-9) }),", "bass: bass.out,"));
const plain = read("song.ts");
check(plain.includes("bass: bass.out,") && !plain.includes("duck("), "the song reads the bass's sound plainly, with no Duck");

const { page, errors, api, show, key } = await open(1440, 900);
const revision = async () => (await api("")).body.view?.revision;
// Wait for the outside edits to be the song the page shows.
await until(async () => !!(await page.$('[data-module="song"]')), 9000);
await show("song");
const bass = '[data-module="song"] [data-node="Bass.out"]';
check(await until(async () => !!(await page.$(`${bass} button[aria-label="Add an effect after Bass.out"]`)), 9000), "the bass's sound in the song has a + (CARD-36)");

// ---------------------------------------------------------------- add a Duck from +
await page.click(`${bass} button[aria-label="Add an effect after Bass.out"]`);
check(await until(async () => !!(await page.$('[data-effect="duck"]')), 3000), "the effect browser offers Duck");
await page.click('[data-effect="duck"]');
const added = plain.replace("bass: bass.out,", "bass: duck(bass.out, { depth: -8 }),");
check(await until(() => read("song.ts").includes("bass: duck(bass.out, { depth: -8 }),") && read("song.ts").includes('import { duck } from "std/fx";')), "Duck goes around bass.out, with its import");
check(read("song.ts").replace('import { duck } from "std/fx";\n', "") === added, "and nothing else changes");
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

// ---------------------------------------------------------------- pick Kick
await page.click(`${duck} [data-testid=listens-to]`).catch(() => {});
await until(async () => !!(await page.$("[data-testid=listens-menu]")), 3000);
const choices = await page.$$eval("[data-testid=listens-menu] [role=menuitem]", (r) => r.map((x) => x.textContent));
check(choices.join() === "Hats,Keys,Kick,None", `Listens to lists the song's other tracks, then None: ${choices.join()}`);
const bad = await page.evaluate(lint, `${duck} [data-listens], [data-testid=listens-menu]`);
check(bad.length === 0, `lint: the row and its menu${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
const before = await revision();
await page.click('[data-testid=listens-menu] [data-track="kick"]').catch(() => {});
const keyed = added.replace("bass: duck(bass.out, { depth: -8 }),", "bass: duck(bass.out, kick.env, { depth: -8 }),");
check(await until(() => read("song.ts").replace('import { duck } from "std/fx";\n', "") === keyed), "picking Kick writes kick.env in the Duck's call, and nothing else");
check(await until(async () => (await listening()) === "Listens to:Kick"), `the row says Kick: ${await listening()}`);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (b) => b.title);
check(await until(async () => /^Undo Bass's duck listens to Kick \(/.test(await undoTitle())), `one undo step: ${await undoTitle()}`);
check((await revision()) === before + 1, "one revision");

const ducked = await heard(3000);
// Each message's peak is the loudest of its window, so a dip shows less than its depth (-8 dB at the
// kick's loudest): the bass moves where it didn't, lower while the kick's meter is up.
check(ducked.n[0] > 3 && ducked.n[1] > 3 && ducked.moved > 2 && ducked.down - ducked.up > 1, `the bass dips while the kick plays: ${said(ducked)}`);

// ---------------------------------------------------------------- undo
await key("Meta+z");
check(await until(() => read("song.ts").replace('import { duck } from "std/fx";\n', "") === added), "⌘Z takes the kick out of the call again");
check(await until(async () => (await listening()) === "Listens to:None"), `and the row says None: ${await listening()}`);

done(errors);
