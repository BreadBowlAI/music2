// Make it a kit (KIT-22 to KIT-24), as a musician does it: first-song's Hats track, made to play the
// drum machine by an outside edit, offers Make it a kit in its track menu, with its words, and on the
// drum machine's card. Making it one is one undo step: the track is the kit's bus, a Drum track goes
// to it, the bus is selected and Beat shows the kit's rows; the toast says what it keeps. Undo puts
// the drum machine back, offered again. The lint, and a picture.
//
//   node e2e/make-kit.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, PROJ, read, ROOT, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, state, toasts, key, view } = await open(1440, 900);
const has = async (sel) => !!(await page.$(sel));
const KEEPS = "Same drums, pitches, levels and lengths; each hit's noise comes out a little different";
const hats = read("tracks/hats.ts");
const machine = hats.replace(/drums\.drum\(lane\.notes\("hits"\), \{[^}]*\}\)/, 'drums.kit(lane.notes("hits"))');
check(machine !== hats && machine.includes("drums.kit("), "first-song's Hats, written to play the drum machine");
fs.writeFileSync(`${PROJ}/tracks/hats.ts`, machine);
check(await until(async () => JSON.stringify((await state()).drum_machines) === '["hats"]', 15000), "the state names it as a drum machine track");

// ---------------------------------------------------------------- its card and its menu offer it

await page.click('[data-module-tab="hats"]');
await view("graph");
const card = '[data-module="hats"] [data-make-kit="hats"]';
check(await until(() => has(card), 15000), "the drum machine's card has Make it a kit");
check((await page.$eval(card, (e) => e.title)).includes(KEEPS), "its tooltip says what it keeps");
await page.click('[data-track-menu="hats"]');
await page.waitForSelector("[data-testid=track-menu]");
const row = "[data-testid=track-menu] [data-testid=make-kit]";
check(await has(row), "the Hats track's menu has Make it a kit");
check((await page.$eval(row, (e) => e.textContent)).includes(KEEPS), "with its words");
await page.evaluate(lint, "[data-testid=track-menu]").then((bad) => check(bad.length === 0, `the lint finds nothing in the menu${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`));

// ---------------------------------------------------------------- making it one

const rev = (await state()).view.revision;
await page.click(row);
const kit = async () => (await state()).view.drum_kits?.[0];
check(await until(async () => (await kit())?.bus === "hats", 15000), "the Hats track is a kit's bus");
check(JSON.stringify((await kit()).rows.map((r) => r.track)) === '["hats-2"]', `with a Drum track going to it: ${JSON.stringify((await kit())?.rows)}`);
check((await state()).view.revision === rev + 1, "one revision");
check(read("tracks/hats.ts").includes('bus("hats"') && read("tracks/hats.ts").includes("gain(input)"), `its file is the bus's: ${read("tracks/hats.ts").match(/export default [^\n]*/)?.[0]}`);
check(await until(async () => (await toasts()).some((t) => t.includes("Made Hats a kit: Hats 2, going to it.") && t.includes(KEEPS))), `the toast says what it made and keeps: ${(await toasts()).join(" | ")}`);
check((await state()).undo_name?.name === "Make Hats a kit", `one undo step, "Make Hats a kit": ${(await state()).undo_name?.name}`);
check(JSON.stringify((await state()).drum_machines) === "[]", "no drum machine track is left");

// ---------------------------------------------------------------- Beat shows the kit

check(await until(() => has('[data-module-tab="hats"][aria-selected=true]')), "the bus is selected");
check(await until(() => has('[data-view-tab="beat"]')), "the dock has a Beat tab");
await view("beat");
check(await until(() => has('[data-testid=beat-row][data-row="hats-2"]')), "Beat shows the kit's row");
await page.screenshot({ path: `${ROOT}/make-kit.png` });

// ---------------------------------------------------------------- one undo puts it back

await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => read("tracks/hats.ts") === machine), "⌘Z puts the drum machine back");
check(await until(async () => JSON.stringify((await state()).drum_machines) === '["hats"]'), "offered again");
check(!fs.existsSync(`${PROJ}/tracks/hats-2.ts`), "its Drum track gone");

done(errors);
