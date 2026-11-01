// The beat grid (KIT-10 to KIT-15): first-song's Kick and Hats sent to a Group bus are a drum kit,
// and selecting one shows the Beat tab beside Notes, a row per drum. A click on a step writes a hit
// on that row's own lane (the timeline's lane shows it), one undo step; a drag along a row writes
// several as one; a click on a hit takes it away. The Hats' notes are off the grid (a tenth of a
// beat long): shown where they are, and left alone by an edit beside them. A velocity drag sets a
// hit's velocity, one undo step. + Drum ▸ Clap adds a row going to the bus, whose steps play (its
// meter moves). A track in no kit shows Notes in Beat's place. The lint, and a picture.
//
//   node e2e/beat-grid.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, PROJ, read, ROOT, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, toasts, key, box, show } = await open(1440, 900);
const has = async (sel) => !!(await page.$(sel));
const { session } = await state();
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify({ session, ...body }) });
const revision = async () => (await state()).view.revision;
const step = async (combo) => {
  const rev = await revision();
  await key(combo);
  return until(async () => (await revision()) !== rev);
};
const kits = async () => (await state()).view.kits ?? [];
/** Track `track`'s hits lane as the state's timeline has it: each note in song time, `[start, length, pitch, velocity]`. */
const lane = async (track) => {
  const t = (await state()).timeline.tracks.find((x) => x.module === track);
  const l = t?.lanes.find((x) => x.key === "hits");
  return (l?.clips ?? []).flatMap((c) => c.notes.map((n) => [c.start + n.start, n.length, n.pitch, n.velocity])).sort((a, b) => a[0] - b[0]);
};
const row = (track) => `[data-testid=beat-row][data-row="${track}"]`;
const cell = (track, beat) => `${row(track)} [data-step="${beat}"]`;
const on = (track, beat) => page.$eval(cell(track, beat), (e) => e.dataset.on === "true").catch(() => null);
const clickCell = async (track, beat) => {
  const b = await box(cell(track, beat));
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
};
const rects = (track) => page.$$eval(`[data-testid=timeline] [data-lane="${track}.hits"] [data-clip] rect`, (els) => els.length).catch(() => -1);
// The server's meters messages, as the page gets them: each track's peaks (linear).
const meters = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") meters.push(m.tracks ?? {});
  } catch {}
});

// ---------------------------------------------------------------- a kit, and its tab

check(!(await has('[data-view-tab="beat"]')), "with no kit, the dock has no Beat tab");
await post("/mix/bus", { kind: "group", label: "Drums" });
await post("/mix/route", { channel: "kick", to: "drums" });
await post("/mix/route", { channel: "hats", to: "drums" });
const k = await until(async () => (await kits()).length === 1 && (await kits())[0].rows.length === 2);
const kit = (await kits())[0];
check(k && kit.bus === "drums" && kit.rows.map((r) => `${r.track}:${r.sound}`).join() === "kick:kick,hats:hats", `the Kick and the Hats sent to a Drums bus are a kit (${JSON.stringify(kit)})`);
await show("kick");
check(await until(() => has('[data-view-tab="beat"]')), "with the Kick selected, the dock has a Beat tab");
const tabs = await page.$$eval("[data-view-tab]", (els) => els.map((e) => e.dataset.viewTab));
check(tabs.join() === "graph,notes,beat,mix,code", `beside Notes (${tabs})`);
await page.click('[data-view-tab="beat"]');
check(await until(() => has("[data-testid=beat]")), "Beat shows the beat grid");
const rows = await page.$$eval("[data-testid=beat-row-name]", (els) => els.map((e) => `${e.dataset.row}:${e.textContent}`));
check(rows.join() === "kick:Kick,hats:Hats", `a row per drum, in the timeline's order, named (${rows})`);
check((await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === "kick", "the velocity lane shows the selected Kick's row");
const named = () => page.$eval('[data-testid=beat-row-name][data-row="hats"]', (e) => e.textContent).catch(() => "");
await post("/label", { module: "hats", label: "Top" });
check(await until(async () => (await named()) === "TopHats"), `a row named apart from its sound says its sound too (${await named()})`);
await post("/label", { module: "hats", label: "Hats" });
await until(async () => (await named()) === "Hats");
const kicks = await lane("kick");
check(await on("kick", 0) && !(await on("kick", 0.5)), "the Kick's hits are on: beat 1 on, the step after off");
const offGrid = await page.$$eval(`${row("hats")} [data-off-grid]`, (els) => els.map((e) => Number(e.dataset.offGrid)));
check(offGrid.length > 0 && offGrid[0] === 0.5 && (await on("hats", 0.5)), `the Hats' notes, a tenth of a beat long, show where they are, off the grid (${offGrid.slice(0, 4)}…)`);

// ---------------------------------------------------------------- a click is a hit, one undo step

const hats = await lane("hats");
const minis = await rects("kick");
let rev = await revision();
await clickCell("kick", 0.5);
check(await until(async () => (await lane("kick")).length === kicks.length + 1), "a click on a step writes a hit");
const added = (await lane("kick")).find((n) => n[0] === 0.5);
check(added && added[1] === 0.25 && added[2] === kicks[0][2] && added[3] === 0.8, `on the Kick's lane at its step, a step long, at the row's pitch, velocity 0.8 (${JSON.stringify(added)})`);
check((await revision()) === rev + 1, "one revision");
check(await until(async () => (await rects("kick")) === minis + 1), `the timeline's Kick lane shows it (${minis} → ${await rects("kick")} notes)`);
check(JSON.stringify(await lane("hats")) === JSON.stringify(hats), "the Hats' off-grid notes are left alone");
check(await until(() => on("kick", 0.5)), "its step is on");
check((await step("Meta+z")) && (await until(async () => (await lane("kick")).length === kicks.length)), "one undo takes it back");
check(await until(async () => !(await on("kick", 0.5))), "and the step is off again");

// ---------------------------------------------------------------- a drag along a row is one step

rev = await revision();
const a = await box(cell("kick", 1.25)), b = await box(cell("kick", 1.75));
await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
await page.mouse.down();
await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 10 });
await page.mouse.up();
check(await until(async () => (await lane("kick")).length === kicks.length + 3), "a drag along the row paints a hit in each step it crosses");
check((await revision()) === rev + 1, "as one revision");
check((await step("Meta+z")) && (await until(async () => (await lane("kick")).length === kicks.length)), "one undo takes the three back");

// ---------------------------------------------------------------- a click on a hit takes it away

await clickCell("hats", 0.5);
check(await until(async () => (await lane("hats")).length === hats.length - 1 && !(await lane("hats")).some((n) => n[0] === 0.5)), "a click on an off-grid hit takes it away, as any hit");
check((await step("Meta+z")) && (await until(async () => JSON.stringify(await lane("hats")) === JSON.stringify(hats))), "one undo puts it back where it was");

// ---------------------------------------------------------------- velocity

await page.click('[data-testid=beat-row-name][data-row="kick"]');
check(await until(async () => (await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === "kick"), "a click on a row's name selects it: the velocity lane shows it");
const bar = `[data-testid=beat-velocity] [data-start="1"]`;
const v = await box(bar);
rev = await revision();
await page.mouse.move(v.x + v.width / 2, v.y + v.height - 12);
await page.mouse.down();
await page.mouse.move(v.x + v.width / 2, v.y + v.height - 16, { steps: 4 });
await page.mouse.up();
const soft = await until(async () => (await lane("kick")).find((n) => n[0] === 1)?.[3] < 0.5);
check(soft && (await revision()) === rev + 1, `a drag down on a hit's velocity bar makes it softer, one revision (${(await lane("kick")).find((n) => n[0] === 1)?.[3]})`);
check((await step("Meta+z")) && (await until(async () => (await lane("kick")).find((n) => n[0] === 1)?.[3] === 0.8)), "one undo puts its velocity back");

// ---------------------------------------------------------------- + Drum, heard

const song = read("song.ts");
await page.click("[data-testid=add-drum]");
await page.waitForSelector("[data-testid=add-drum-menu]");
const sounds = await page.$$eval("[data-testid=add-drum-menu] [data-sound]", (els) => els.map((e) => e.dataset.sound));
check(sounds.join() === "kick,snare,clap,hats,tom,rim,cowbell,cymbal,shaker", `+ Drum offers the Drum's sounds (${sounds})`);
await page.click('[data-testid=add-drum-menu] [data-sound="clap"]');
check(await until(() => fs.existsSync(`${PROJ}/tracks/clap.ts`) && read("song.ts").includes('clap: { in: clap.out, to: "drums" }')), "Clap adds a track whose channel goes to the Drums bus");
check(read("tracks/clap.ts").includes('drums.drum(lane.notes("hits"), { drum: "clap" })'), "playing the clap");
check(await until(() => has(row("clap"))), "and a Clap row, last");
for (const beat of [1, 3, 5, 7]) await clickCell("clap", beat);
check(await until(async () => (await lane("clap")).length === 4), `four clicks on its steps write four hits (${JSON.stringify(await lane("clap"))})`);
check((await lane("clap")).every((n) => n[2] === 39), "each at the clap's own note, D#2");
const from = meters.length;
await page.keyboard.press("Space");
const heard = await until(() => meters.slice(from).some((m) => Math.max(0, ...(m.clap ?? [])) > 0), 6000);
check(heard, "played, the Clap's meter moves: its steps are heard");
await page.keyboard.press("Space");
await until(async () => !(await api("/status")).body.playing, 3000);
for (let i = 0; i < 4; i++) await step("Meta+z");
check(await until(async () => (await lane("clap")).length === 0), "four undos take the four hits back, one each");
await step("Meta+z");
check(await until(async () => !(await has(row("clap"))) && !fs.existsSync(`${PROJ}/tracks/clap.ts`) && read("song.ts") === song), "a fifth takes the Clap back: its row, its file and its channel");

// ---------------------------------------------------------------- the lint, and a track in no kit

await page.evaluate(lint, "[data-testid=dock]").then((bad) => check(bad.length === 0, `the lint finds nothing in the beat grid${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`));
await page.screenshot({ path: `${ROOT}/beat-grid.png` });
await show("bass");
check(await until(async () => !(await has('[data-view-tab="beat"]')) && (await has("[data-testid=notes]"))), "the Bass, in no kit: no Beat tab, and Notes in its place");
await show("hats");
check(await until(() => has("[data-testid=beat]")), "the Hats selected again: Beat again");
await show("drums");
check(
  await until(async () => (await has("[data-testid=beat]")) && (await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === "kick"),
  "the Drums bus selected: Beat too, its velocity lane the kit's first row",
);
check(!(await toasts()).some((t) => /error|failed/i.test(t)), `no toast said something went wrong (${await toasts()})`);
await sleep(50);

done(errors);
