// The beat grid (KIT-10 to KIT-15), on a kit made as a musician makes one: + Track ▸ Drum kit ▸
// Basic kit, under first-song's own Kick and Hats (so its tracks are Kick 2, Snare and Hats 2), with a
// beat an agent writes (the Kick 2 on every beat, the Hats 2 on the offbeats, a tenth of a beat
// long). Selecting a kit's track shows the Beat tab beside Notes, a row per drum. A click on a step
// writes a hit on that row's own lane (the timeline's lane shows it), one undo step; a drag along a
// row writes several as one; a click on a hit takes it away; two quick clicks land in order. The
// short hats start on their steps, so they are on the grid; an agent's note between two steps is a
// mark where it starts. A velocity drag sets a hit's velocity, one undo step. + Drum ▸ Clap adds a
// row going to the bus, whose steps play (its meter moves). A bus's row says how tracks reach it; a
// track in no kit shows Notes in Beat's place, and ⌥5 there says why. The lint, and a picture.
//
//   node e2e/beat-grid.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, PROJ, read, ROOT, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, api, state, caughtUp, hold, toasts, key, box, show } = await open(1440, 900);
const has = async (sel) => !!(await page.$(sel));
const { session } = await state();
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify({ session, ...body }) });
const revision = async () => (await state()).view.revision;
const step = async (combo) => {
  const rev = await revision();
  await key(combo);
  return until(async () => (await revision()) !== rev);
};
const kits = async () => (await state()).view.drum_kits ?? [];
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
const marks = (track) => page.$$eval(`${row(track)} [data-off-grid]`, (els) => els.map((e) => Number(e.dataset.offGrid)));
const busWords = (bus = "drums") => page.$eval(`[data-testid=timeline] [data-track="${bus}"] [data-testid=bus-lane]`, (e) => e.textContent).catch(() => null);
const undoName = async () => (await state()).undo_name?.name;
/** Clip `id` of track `track`'s hits lane with `change` made to its notes, as one `notes` gesture (an agent's edit). */
const reNote = async (track, id, change) => {
  const c = (await state()).timeline.tracks.find((t) => t.module === track).lanes.find((l) => l.key === "hits").clips.find((x) => x.id === id);
  return post("/clips", { gesture: { op: "notes", clip: { module: track, lane: "hits", id }, notes: change(c.notes) } });
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
/** The kit's tracks: + Track names them apart from first-song's own Kick and Hats. */
const KICK = "kick-2", HATS = "hats-2";

// ---------------------------------------------------------------- a Group bus's words

check(!(await has('[data-view-tab="beat"]')), "with no kit, the dock has no Beat tab");
await post("/mix/bus", { kind: "group", label: "Room" });
const group = "Plays the tracks whose output is set to it, and what tracks send it. Set them in Mix (⌥3).";
check(await until(async () => (await busWords("room")) === group), `a Group bus's row says how tracks reach it: by their output, or a send (${await busWords("room")})`);
await step("Meta+z");

// ---------------------------------------------------------------- + Track ▸ Drum kit ▸ Basic kit, and its tab

await show("hats");
await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => null);
await page.keyboard.type("basic kit");
await sleep(150);
await page.keyboard.press("Enter");
const k = await until(async () => (await kits()).length === 1 && (await kits())[0].rows.length === 3);
const kit = (await kits())[0];
check(
  k && kit.bus === "drums" && kit.rows.map((r) => `${r.track}:${r.sound}`).join() === `${KICK}:kick,snare:snare,${HATS}:hats`,
  `+ Track ▸ Drum kit ▸ Basic kit makes a kit: a Drums bus, Kick 2, Snare and Hats 2 (${JSON.stringify(kit)})`,
);
const kitWords = "The Drums kit: Kick 2, Snare and Hats 2 play into it. Write their beat in Beat (⌥5).";
check(await until(async () => (await busWords()) === kitWords), `the kit's bus says so in its row, and how to open Beat (${await busWords()})`);
// A beat, as an agent writes one.
const beat = async (module, id, notes) => post("/lane", { revision: await revision(), module, lane: "hits", clips: [{ id, start: 0, length: 32, notes }] });
await beat(KICK, "kickbeat", Array.from({ length: 32 }, (_, i) => ({ pitch: 36, start: i, length: 0.25, velocity: 0.8 })));
await beat(HATS, "hatsbeat", Array.from({ length: 32 }, (_, i) => ({ pitch: 42, start: i + 0.5, length: 0.1, velocity: 0.7 })));
check(await until(async () => (await lane(KICK)).length === 32 && (await lane(HATS)).length === 32), "an agent writes a beat into the kit's lanes");
// The page didn't write it: the grid below reads it once the page shows it.
await caughtUp();
await show("kick");
check(await until(() => has('[data-module-tab="kick"][aria-selected=true]')) && !(await has('[data-view-tab="beat"]')), "first-song's own Kick, on the master, is in no kit: no Beat tab");
await show("bass");
const shown = () => page.$$eval("[data-view-tab]", (els) => els.map((e) => `${e.dataset.viewTab}${e.getAttribute("aria-selected") === "true" ? "*" : ""}`).join());
const before = await shown();
await key("Alt+5");
check(
  await until(async () => (await toasts()).some((t) => t.includes("Beat shows a drum kit: select a Drum track or its bus."))),
  `⌥5 with the Bass, in no kit, opens nothing and says why (${await toasts()})`,
);
check((await shown()) === before && !before.includes("beat"), `the dock stays as it was (${await shown()})`);
await show(KICK);
check(await until(() => has('[data-view-tab="beat"]')), "with the Kick 2 selected, the dock has a Beat tab");
const tabs = await page.$$eval("[data-view-tab]", (els) => els.map((e) => e.dataset.viewTab));
check(tabs.join() === "graph,notes,beat,mix,code", `beside Notes (${tabs})`);
await page.click('[data-view-tab="beat"]');
check(await until(() => has("[data-testid=beat]")), "Beat shows the beat grid");
const rows = await page.$$eval("[data-testid=beat-row-name]", (els) => els.map((e) => `${e.dataset.row}:${e.textContent}`));
check(rows.join() === `${KICK}:Kick 2Kick,snare:Snare,${HATS}:Hats 2Hats`, `a row per drum, in the timeline's order, each named, with its sound where the name doesn't say it (${rows})`);
check((await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === KICK, "the velocity lane shows the selected Kick 2's row");
const kicks = await lane(KICK);
check(await on(KICK, 0) && !(await on(KICK, 0.5)), "the Kick 2's hits are on: beat 1 on, the step after off");
check((await marks(HATS)).length === 0 && (await on(HATS, 0.5)), `the Hats 2's notes, a tenth of a beat long, start on their steps: on the grid, with no marks (${(await marks(HATS)).slice(0, 4)})`);
check((await page.$eval(row(KICK), (e) => e.title)).includes("⌘1 and ⌘2 make the steps shorter or longer"), "a row's tooltip says what ⌘1 and ⌘2 do");
// An agent's hats note between two steps: off the grid, a mark where it starts.
await reNote(HATS, "hatsbeat", (notes) => [...notes, { pitch: 42, start: 2.85, length: 0.1, velocity: 0.7 }]);
check(await until(async () => (await marks(HATS)).join() === "2.85" && (await on(HATS, 2.75))), `a note between two steps is off the grid: a mark where it starts, its step on (${await marks(HATS)})`);
const said = await page.$eval(`${row(HATS)} [data-off-grid]`, (e) => e.title).catch(() => "");
check(said === "It starts a little after 1.3.4 and is 0.1 beat long. The grid leaves it where it is; click its step to take it away.", `its tooltip says where, by bar and beat (${said})`);

// ---------------------------------------------------------------- a click is a hit, one undo step

const hats = await lane(HATS);
const minis = await rects(KICK);
let rev = await revision();
await clickCell(KICK, 0.5);
check(await until(async () => (await lane(KICK)).length === kicks.length + 1), "a click on a step writes a hit");
const added = (await lane(KICK)).find((n) => n[0] === 0.5);
check(added && added[1] === 0.25 && added[2] === kicks[0][2] && added[3] === 0.8, `on the Kick's lane at its step, a step long, at the row's pitch, velocity 0.8 (${JSON.stringify(added)})`);
check((await revision()) === rev + 1, "one revision");
check(await until(async () => (await undoName()) === "adding a Kick 2 hit"), `named "adding a Kick 2 hit" (${await undoName()})`);
check(await until(async () => (await rects(KICK)) === minis + 1), `the timeline's Kick lane shows it (${minis} → ${await rects(KICK)} notes)`);
check(JSON.stringify(await lane(HATS)) === JSON.stringify(hats), "the Hats' notes are left alone");
check(await until(() => on(KICK, 0.5)), "its step is on");
check((await step("Meta+z")) && (await until(async () => (await lane(KICK)).length === kicks.length)), "one undo takes it back");
check(await until(async () => !(await on(KICK, 0.5))), "and the step is off again");

// ---------------------------------------------------------------- a drag along a row is one step

rev = await revision();
const a = await box(cell(KICK, 1.25)), b = await box(cell(KICK, 1.75));
await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
await page.mouse.down();
await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 10 });
await page.mouse.up();
check(await until(async () => (await lane(KICK)).length === kicks.length + 3), "a drag along the row paints a hit in each step it crosses");
check((await revision()) === rev + 1, "as one revision");
check(await until(async () => (await undoName()) === "adding 3 Kick 2 hits"), `named "adding 3 Kick 2 hits" (${await undoName()})`);
check((await step("Meta+z")) && (await until(async () => (await lane(KICK)).length === kicks.length)), "one undo takes the three back");

// ---------------------------------------------------------------- two clicks keep their order

// The first click's write is held back on its way: the second, which takes the hit away, must
// still land after it.
let held = false;
const off = await hold((r) => (!held && r.method() === "POST" && r.url().endsWith("/api/graph/clips") ? ((held = true), 700) : 0));
await clickCell(KICK, 0.25);
await clickCell(KICK, 0.25);
await until(async () => (await undoName()) === "removing a Kick 2 hit", 5000);
await sleep(300);
await off();
check(held && !(await lane(KICK)).some((n) => n[0] === 0.25) && !(await on(KICK, 0.25)), `on then off, the first held back on its way, lands as on then off: no hit (${JSON.stringify((await lane(KICK)).filter((n) => n[0] < 0.5))})`);
await step("Meta+z");
await step("Meta+z");
await until(async () => (await lane(KICK)).length === kicks.length);

// ---------------------------------------------------------------- a click on a hit takes it away

await clickCell(HATS, 2.75);
check(await until(async () => (await lane(HATS)).length === hats.length - 1 && !(await lane(HATS)).some((n) => n[0] === 2.85)), "a click on an off-grid hit's step takes it away, as any hit");
check((await step("Meta+z")) && (await until(async () => JSON.stringify(await lane(HATS)) === JSON.stringify(hats))), "one undo puts it back where it was");

// ---------------------------------------------------------------- a note a millionth before a step

await reNote(KICK, "kickbeat", (notes) => notes.map((n) => (n.start === 1 ? { ...n, start: 0.999999 } : n)));
check(
  await until(async () => (await lane(KICK)).some((n) => n[0] === 0.999999) && (await on(KICK, 1)) && !(await on(KICK, 0.75)) && (await marks(KICK)).length === 0),
  "a Kick note a millionth before beat 2 is in beat 2's step, on the grid",
);
await clickCell(KICK, 1);
check(await until(async () => !(await lane(KICK)).some((n) => n[0] === 0.999999)), "a click there takes it away: the server's step is the grid's");
await step("Meta+z");
await step("Meta+z");
check(await until(async () => (await lane(KICK)).some((n) => n[0] === 1) && !(await lane(KICK)).some((n) => n[0] === 0.999999)), "two undos put the Kick back on beat 2");

// ---------------------------------------------------------------- velocity

await page.click(`[data-testid=beat-row-name][data-row="${KICK}"]`);
check(await until(async () => (await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === KICK), "a click on a row's name selects it: the velocity lane shows it");
const bar = `[data-testid=beat-velocity] [data-start="1"]`;
const v = await box(bar);
rev = await revision();
await page.mouse.move(v.x + v.width / 2, v.y + v.height - 12);
await page.mouse.down();
await page.mouse.move(v.x + v.width / 2, v.y + v.height - 16, { steps: 4 });
await page.mouse.up();
const soft = await until(async () => (await lane(KICK)).find((n) => n[0] === 1)?.[3] < 0.5);
check(soft && (await revision()) === rev + 1, `a drag down on a hit's velocity bar makes it softer, one revision (${(await lane(KICK)).find((n) => n[0] === 1)?.[3]})`);
check((await step("Meta+z")) && (await until(async () => (await lane(KICK)).find((n) => n[0] === 1)?.[3] === 0.8)), "one undo puts its velocity back");

// ---------------------------------------------------------------- another step: still on the grid

await key("Meta+2");
check(await until(async () => (await page.$eval("[data-testid=beat-grid]", (e) => e.dataset.grid)) === "1/8"), "⌘2: the steps are eighths");
check((await marks(KICK)).length === 0 && (await on(KICK, 0)) && (await on(KICK, 1)), `the Kick's hits start on eighths too: on the grid, with no marks (${(await marks(KICK)).slice(0, 4)})`);
await key("Meta+1");
check(await until(async () => (await page.$eval("[data-testid=beat-grid]", (e) => e.dataset.grid)) === "1/16"), "⌘1: sixteenths again");

// ---------------------------------------------------------------- + Drum, heard

const song = read("song.ts");
await page.click("[data-testid=add-drum]");
await page.waitForSelector("[data-testid=add-drum-menu]");
const sounds = await page.$$eval("[data-testid=add-drum-menu] [data-sound]", (els) => els.map((e) => e.dataset.sound));
check(
  sounds.join() === "kick,kick 2,snare,snare 2,clap,hats,tom,rim,claves,cowbell,cymbal,cymbal 2,ride,ride 2,shaker,shaker 2,tambourine",
  `+ Drum offers the Drum's sounds, the drum machine's every drum (KIT-20) (${sounds})`,
);
await page.click('[data-testid=add-drum-menu] [data-sound="clap"]');
check(await until(() => fs.existsSync(`${PROJ}/tracks/clap.ts`) && read("song.ts").includes('clap: { in: clap.out, to: "drums" }')), "Clap adds a track whose channel goes to the Drums bus");
check(read("tracks/clap.ts").includes('drums.drum(lane.notes("hits"), { drum: "clap" })'), "playing the clap");
check(await until(() => has(row("clap"))), "and a Clap row, last");
check(await until(async () => (await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === "clap"), "the Clap selected: the velocity lane its row");
check(await until(async () => (await busWords()) === "The Drums kit: Kick 2, Snare, Hats 2 and Clap play into it. Write their beat in Beat (⌥5)."), `the bus's row names it too (${await busWords()})`);
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

// ---------------------------------------------------------------- a long song draws what shows

// The Clap gone, its selection went with it: the Kick 2 again.
await show(KICK);
await until(() => has("[data-testid=beat]"));

check((await has(cell(KICK, 0))) && !(await has(cell(KICK, 32))), "the steps far out of view aren't drawn: beat 1's is, the last bar's isn't");
await page.$eval("[data-testid=beat-scroller]", (e) => (e.scrollLeft = e.scrollWidth));
check(await until(async () => (await has(cell(KICK, 32))) && (await on(KICK, 31))), "scrolled to the end, the last bars' steps are, the song's last hit on");
await page.$eval("[data-testid=beat-scroller]", (e) => (e.scrollLeft = 0));

// ---------------------------------------------------------------- the lint, and a track in no kit

await page.evaluate(lint, "[data-testid=dock]").then((bad) => check(bad.length === 0, `the lint finds nothing in the beat grid${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`));
await page.screenshot({ path: `${ROOT}/beat-grid.png` });
await show("bass");
check(await until(async () => !(await has('[data-view-tab="beat"]')) && (await has("[data-testid=notes]"))), "the Bass, in no kit: no Beat tab, and Notes in its place");
await show(HATS);
check(await until(() => has("[data-testid=beat]")), "the Hats 2 selected again: Beat again");
await show("drums");
check(
  await until(async () => (await has("[data-testid=beat]")) && (await page.$eval("[data-testid=beat-velocity]", (e) => e.dataset.row)) === KICK),
  "the Drums bus selected: Beat too, its velocity lane the kit's first row",
);
check(!(await toasts()).some((t) => /error|failed/i.test(t)), `no toast said something went wrong (${await toasts()})`);
await sleep(50);

done(errors);
