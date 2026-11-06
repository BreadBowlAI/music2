// A Drum cut by another track's hits (KIT-27), from its card: an Open hat added by + Drum to a
// Basic kit says "Cut by: None"; its list has None and the song's other Drum tracks, by name;
// picking Hats writes `cutBy: hats2.hits` into its call, imports the Hats, and has the Hats return
// their lane beside their sound, one undo step ("Open hat cut by Hats 2"), and the row says so. The
// render changes only after the closed hat: the open hat stops there (an open hat at beat 0, a closed
// one at beat 0.25). ⌘Z puts every file back, and the render with them; None takes `cutBy` out. The
// row and its list linted.
//
//   node e2e/choke.mjs <root> <home> <project> <m2 cli>     (the `export` kind: examples/first-song open)
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { PROJ, ROOT, check, done, open, read, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const M2 = process.argv[5];
const { page, errors, api, show, key } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const state = async () => (await api("")).body;
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `lint: ${what}${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);
};
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (b) => b.title).catch(() => null);

/** The song's first second, rendered by the m2 command line: its left channel, at 48 kHz. */
const render = (name) => {
  execFileSync(M2, ["render", PROJ, `${ROOT}/${name}`, "--seconds", "1"]);
  const b = fs.readFileSync(`${ROOT}/${name}`);
  const data = b.indexOf("data") + 8;
  const channels = b.readUInt16LE(22);
  const bytes = b.readUInt16LE(34) / 8;
  const out = [];
  for (let at = data; at + bytes * channels <= b.length; at += bytes * channels) out.push(bytes === 3 ? b.readIntLE(at, 3) : b.readInt16LE(at));
  return out;
};
/** The energy of `a` less `b` from sample `from` to `to`. */
const apart = (a, b, from, to) => {
  let e = 0;
  for (let i = from; i < to; i++) e += (a[i] - b[i]) ** 2;
  return e;
};

// ---------------------------------------------------------------- a Basic kit and an Open hat

const session = (await state()).session;
const made = await post("/track/kit", { session, component: "@m2/drums:drum", kit: "Basic kit" });
const drum = await post("/kit/drum", { session, bus: made.body.module, sound: "open hat" });
check(
  drum.status === 200 && (await until(() => fs.existsSync(`${PROJ}/tracks/open-hat.ts`) && fs.existsSync(`${PROJ}/tracks/hats-2.ts`))),
  `a Basic kit (Hats 2, as first-song has Hats) and an Open hat: ${drum.status}`,
);
// An open hat at beat 0, the kit's closed hat at beat 0.25.
for (const [module, pitch, start] of [["open-hat", 46, 0], ["hats-2", 42, 0.25]]) {
  const now = await state();
  const notes = [{ pitch, start, length: 0.25, velocity: 0.8 }];
  await post("/lane", { session, revision: now.view.revision, module, lane: "hits", clips: [{ id: `choke${pitch}0`, start: 0, length: 4, notes }] });
}
check(await until(() => read("data/hats-2/hits.json").includes('"start":0.25') || read("data/hats-2/hits.json").includes('"start": 0.25')), "the hits are written");
const uncut = render("uncut.wav");
const before = { open: read("tracks/open-hat.ts"), hats: read("tracks/hats-2.ts") };

// ---------------------------------------------------------------- the Cut by row

await show("open-hat");
const row = '[data-module="open-hat"] [data-cut-by]';
await until(async () => !!(await page.$(row)), 8000);
const says = () => page.$eval(row, (e) => e.textContent).catch(() => null);
check((await says()) === "Cut by:None", `the Open hat's card says what cuts it: ${await says()}`);
await page.click(`${row} [data-testid=cut-by]`).catch(() => {});
await until(async () => !!(await page.$("[data-testid=cut-by-menu]")), 3000);
const choices = await page.$$eval("[data-testid=cut-by-menu] [role=menuitem]", (r) => r.map((x) => x.textContent));
check(choices.join() === "None,Hats,Hats 2,Kick,Kick 2,Snare", `its list: None and the other Drum tracks, by name: ${choices.join()}`);
await linted(`${row}, [data-testid=cut-by-menu]`, "the Cut by row and its list");
await page.click('[data-testid=cut-by-menu] [data-track="hats-2"]').catch(() => {});
check(
  await until(() => read("tracks/open-hat.ts").includes('drums.drum(lane.notes("hits"), { drum: "open hat", cutBy: hats2.hits })')),
  `picking Hats 2 writes cutBy: ${read("tracks/open-hat.ts").split("\n").at(-2)}`,
);
check(read("tracks/open-hat.ts").includes('import hats2 from "./hats-2";'), "and imports the Hats 2 track");
check(read("tracks/hats-2.ts").includes('({ out: drums.drum(lane.notes("hits"), { drum: "hats" }), hits: lane.notes("hits") })'), "which returns its lane beside its sound");
check(await until(async () => (await says()) === "Cut by:Hats 2"), `the row says Hats 2 now: ${await says()}`);
check(await until(async () => /^Undo Open hat cut by Hats 2 \(/.test(await undoTitle())), `one undo step: ${await undoTitle()}`);

// ---------------------------------------------------------------- heard: the open hat stops

const cut = render("cut.wav");
const at = Math.floor((0.25 * 60 * 48000) / 124); // beat 0.25 at first-song's 124 bpm, 48 kHz
check(apart(cut, uncut, 0, at) === 0, "the render is the same up to the closed hat");
check(apart(cut, uncut, at + 192, at + 9600) > 0, "after it, the open hat is gone");
check(apart(cut, uncut, at + 9600, cut.length) < apart(cut, uncut, at + 192, at + 9600), "most of what changed is the open hat's ring just after the cut");

// ---------------------------------------------------------------- undo, and None

await key("Meta+z");
check(await until(() => read("tracks/open-hat.ts") === before.open && read("tracks/hats-2.ts") === before.hats), "⌘Z puts both files back");
check(await until(async () => (await says()) === "Cut by:None"), `and the row: ${await says()}`);
check(apart(render("back.wav"), uncut, 0, uncut.length) === 0, "and the render with them");
await key("Meta+Shift+z");
check(await until(async () => (await says()) === "Cut by:Hats 2"), "⌘⇧Z cuts it again");
await page.click(`${row} [data-testid=cut-by]`).catch(() => {});
await until(async () => !!(await page.$("[data-testid=cut-by-menu]")), 3000);
await page.click('[data-testid=cut-by-menu] [data-track=""]').catch(() => {});
check(await until(() => read("tracks/open-hat.ts").includes('drums.drum(lane.notes("hits"), { drum: "open hat" })')), "None takes cutBy out");
check(await until(async () => (await says()) === "Cut by:None"), "and the row says None");
done(errors);
