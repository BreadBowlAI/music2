// Drum kits (KIT-2, KIT-5): + Track's Drums are Drum and FM metal, and no drum machine; the kits the
// packs name are a group of their own, "Drum kit (several tracks)", right after the Drums, each row
// saying its tracks, its foot what it writes. Typing "basic kit" and Enter adds a Drums bus and the
// kit's tracks (Kick 2, Snare, Hats 2, as first-song has a Kick and Hats) below the selected track,
// each a Drum going to the bus, the first selected, with a toast and one undo step named for the
// kit; one undo takes it all back, a redo puts it again. A beat in its lanes plays: in the Mix view
// the Drums bus's meter moves, and its tracks go to it. The lint.
//
//   node e2e/kits.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, toasts, key } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const exists = (f) => fs.existsSync(`${PROJ}/${f}`);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const shown = () => page.$$eval("[data-testid=timeline] [data-track]", (els) => els.filter((e) => !e.closest("[data-song-row]")).map((e) => e.dataset.track));
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 5000);
const revision = async () => (await state()).view.revision;
const FLOOR = -60;
const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
/** The loudest strip `id`'s meter reads over `ms`, in dB. */
const loudest = async (id, ms) => {
  let top = FLOOR;
  const t = Date.now();
  while (Date.now() - t < ms) {
    const v = await page.$eval(`${strip(id)} [role=meter]`, (e) => e.getAttribute("aria-valuenow")).catch(() => null);
    if (v !== null && Number(v) > top) top = Number(v);
    await sleep(60);
  }
  return top;
};
const song = read("song.ts");

await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');
await page.click('[data-module-tab="hats"]');
await until(() => page.$eval('[data-track="hats"]', (e) => e.dataset.selected === "true"));

// ---------------------------------------------------------------- + Track's list (KIT-2, KIT-5)

await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => null);
const groups = await page.$$eval("[data-testid=instrument-list] [role=group]", (els) =>
  els.map((g) => ({ name: g.getAttribute("aria-label"), rows: [...g.querySelectorAll("[role=option]")].map((o) => o.dataset.instrument) })),
);
const drums = groups.find((g) => g.name === "Drums");
check(same(drums?.rows, ["@m2/drums:drum", "@m2/synths:fm-metal"]), `+ Track's Drums are Drum and FM metal, no drum machine: ${JSON.stringify(drums?.rows)}`);
const kits = groups[1];
check(
  kits?.name === "Drum kit (several tracks)" && same(kits.rows, ["kit:@m2/drums:drum/808 kit", "kit:@m2/drums:drum/909 kit", "kit:@m2/drums:drum/Basic kit"]),
  `the kits are a group of their own, right after the Drums: ${JSON.stringify(kits)}`,
);
const said = await page.$eval('[data-instrument="kit:@m2/drums:drum/Basic kit"]', (e) => e.textContent).catch(() => "");
check(said === "Basic kitKick, Snare, Hats and a Drums bus", `each row says its tracks: ${said}`);
const bad = await page.evaluate(lint, "[data-testid=instrument-list]");
check(bad.length === 0, `the lint finds nothing in the list${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
await page.keyboard.type("basic kit");
await sleep(150);
const rows = await page.$$eval("[data-testid=instrument-list] [role=option]", (els) => els.map((e) => e.dataset.instrument));
const foot = await page.$eval("[data-testid=instrument-code]", (e) => e.textContent).catch(() => "");
check(
  same(rows, ["kit:@m2/drums:drum/Basic kit"]) && foot === "tracks/drums.ts · bus(…), and a track for each drum going to it",
  `typing "basic kit" finds it, and its foot says what it writes: ${rows} · ${foot}`,
);

// ---------------------------------------------------------------- adding it: one undo step

const rev0 = await revision();
await page.keyboard.press("Enter");
check(await until(() => exists("tracks/drums.ts") && exists("tracks/hats-2.ts")), "Enter adds the kit: its bus's file and its tracks'");
check(
  read("tracks/drums.ts").includes('bus("drums", { name: "Drums" }, ({ input }) => gain(input, { db: -6 }))') &&
    read("tracks/kick-2.ts").includes('track("kick-2", { name: "Kick 2" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "kick" }))'),
  `a Drums bus, and a Drum playing each drum: ${read("tracks/kick-2.ts").split("\n").at(-2)}`,
);
check(
  ['"kick-2": { in: kick2.out, to: "drums" }', 'snare: { in: snare.out, to: "drums" }', '"hats-2": { in: hats2.out, to: "drums" }'].every((c) => read("song.ts").includes(c)),
  "each track's channel goes to the bus",
);
check(await until(async () => same(await shown(), ["kick", "hats", "drums", "kick-2", "snare", "hats-2", "bass", "keys"])), `below Hats, together, the bus first: ${await shown()}`);
check(await until(() => page.$eval('[data-track="kick-2"]', (e) => e.dataset.selected === "true").catch(() => false)), "its first track is selected");
check(await toasted("Added the Basic kit: Kick 2, Snare, Hats 2, going to Drums"), `a toast says what it added (${await toasts()})`);
check((await revision()) === rev0 + 1 && (await until(async () => (await state()).undo_name?.name === "adding the Basic kit")), `one revision, one undo step: ${(await state()).undo_name?.name}`);
await key("Meta+z");
check(await until(() => read("song.ts") === song && !exists("tracks/drums.ts") && !exists("tracks/snare.ts")), "one undo takes it all back");
await key("Meta+Shift+z");
check(await until(() => exists("tracks/drums.ts") && exists("tracks/snare.ts")), "a redo puts it again");

// ---------------------------------------------------------------- it plays (the Mix view)

const st = await state();
const outputs = Object.fromEntries((st.mix?.strips ?? []).map((s) => [s.channel, s.output]));
check(same([outputs["kick-2"], outputs.snare, outputs["hats-2"]], ["drums", "drums", "drums"]), `in the Mix view its tracks go to the Drums bus: ${JSON.stringify(outputs)}`);
for (const [module, pitch] of [["kick-2", 36], ["snare", 38], ["hats-2", 42]]) {
  const now = await state();
  const notes = [0, 1, 2, 3].map((i) => ({ pitch, start: i, length: 0.25, velocity: 1 }));
  await post("/lane", { session: now.session, revision: now.view.revision, module, lane: "hits", clips: [{ id: `beat${module.replace("-", "")}`.padEnd(8, "0").slice(0, 8), start: 0, length: 4, notes }] });
}
await key("Alt+3");
await until(() => page.$(strip("drums")).then(Boolean));
await page.keyboard.press("Space");
check(await until(async () => (await api("/status")).body.playing, 3000), "the song plays");
const heard = await loudest("drums", 4000);
check(heard > FLOOR, `the Drums bus's meter moves: the kit is heard through it (${heard} dB)`);
await page.keyboard.press("Space");

done(errors);
