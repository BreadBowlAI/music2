// 8b.1 Tracks in the app (design-8b §3, taste.md §3.4, TL-2, TL-3): + Track and its instrument
// list, what an add writes byte for byte, colours that never shift, rename, duplicate, delete with
// its dialog and cascade, reorder by dragging, recolour, an outside edit of layout.json, one that
// doesn't read and Reset layout, the Empty template's Add a track, and a read-only window.
//
//   node e2e/s8-tracks.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { spawn } from "node:child_process";
import fs from "node:fs";
import { PROJ, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

const { page, errors, api, state, toasts, key, box, status } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const exists = (f) => fs.existsSync(`${PROJ}/${f}`);
const readOr = (f) => (exists(f) ? read(f) : null);
const snapshot = (files) => Object.fromEntries(files.map((f) => [f, readOr(f)]));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
/** The timeline's tracks top to bottom, as the page draws them. */
const shown = () => page.$$eval("[data-testid=timeline] [data-track]", (els) => els.filter((e) => !e.closest("[data-song-row]")).map((e) => e.dataset.track));
/** The tracks as the server says them, with their colours by name. */
const rows = async () => (await state()).timeline.tracks.filter((t) => t.track).map((t) => [t.module, t.colour]);
/** A track's colour bar as drawn. */
const bar = (m) => page.$eval(`[data-track="${m}"] .absolute.left-0.inset-y-0`, (e) => getComputedStyle(e).backgroundColor).catch(() => null);
const rgb = (hex) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ")})`;
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 5000);
const undoToast = (text) =>
  page.evaluate((text) => {
    const t = [...document.querySelectorAll("[data-testid=toast]")].find((e) => e.textContent.includes(text));
    const b = t && [...t.querySelectorAll("button")].find((b) => b.textContent === "Undo");
    b?.click();
    return !!b;
  }, text);
const select = async (m) => {
  await page.click(`[data-module-tab="${m}"]`);
  await until(() => page.$eval(`[data-track="${m}"]`, (e) => e.dataset.selected === "true"));
};
/** A row of the open header menu, by its words. */
const menuRow = (words) =>
  page.evaluate((words) => {
    const b = [...document.querySelectorAll("[data-testid=track-menu] [role=menuitem]")].find((e) => e.textContent.startsWith(words));
    b?.click();
    return !!b;
  }, words);
const openMenu = async (m) => {
  await page.hover(`[data-module-tab="${m}"]`);
  await page.click(`[data-track-menu="${m}"]`);
  return !!(await page.waitForSelector("[data-testid=track-menu]", { timeout: 2000 }).catch(() => null));
};
const revision = async () => (await state()).view.revision;
const orig = snapshot(["song.ts", "tracks/hats.ts", "data/hats/hits.json", "tracks/kick.ts", "tracks/bass.ts"]);

await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');

// ---------------------------------------------------------------- order and colour (D8, taste.md §2.7)

check(same(await shown(), ["kick", "hats", "bass", "keys"]), `tracks show in the mixer's order, not import order: ${await shown()}`);
check(same(await rows(), [["kick", "rose"], ["hats", "teal"], ["bass", "amber"], ["keys", "indigo"]]), "and take the palette's default order by place, by name");
check((await bar("kick")) === rgb("#d48e98") && (await bar("hats")) === rgb("#61b6ab"), "drawn in the palette's tones");
check(!exists("layout.json"), "nothing is written for that");

// ---------------------------------------------------------------- + Track (TL-3)

await select("hats");
const add = await page.$("[data-testid=add-track]");
check(!!add && !(await add.evaluate((b) => b.disabled)) && (await add.evaluate((b) => b.textContent)) === "Track", "+ Track sits in the corner");
await add.click();
const list = await page.waitForSelector("[data-testid=instrument-list]", { timeout: 2000 }).catch(() => null);
const groups = await page.$$eval("[data-testid=instrument-list] [role=group]", (els) => els.map((e) => e.getAttribute("aria-label")));
const options = await page.$$eval("[data-testid=instrument-list] [role=option]", (els) => els.map((e) => e.dataset.instrument));
// music2's ready-made sounds (T3): the drum kit, One drum and FM metal; the instruments; the two with every knob.
// The last group is Audio: an audio track (studio lane A's Q8).
check(!!list && same(groups, ["Drums", "Instruments", "Sampler", "Sound design", "Audio"]) && same(options, ["@m2/drums:drum-kit", "@m2/drums:drum", "@m2/synths:fm-metal", "@m2/synths:fm-keys", "@m2/synths:fm-bell", "@m2/synths:synth-supersaw", "@m2/synths:synth-lead", "@m2/synths:synth-pad", "@m2/synths:synth-pluck", "@m2/synths:synth-sub", "@m2/synths:synth-bass", "@m2/synths:synth-strings", "std:sampler", "@m2/synths:fm-custom", "@m2/synths:synth-custom", "audio:track"]), `it opens the instrument list, drums first: ${options}`);
check((await page.$eval("[data-testid=instrument-list] [data-active]", (e) => e.dataset.instrument)) === "@m2/drums:drum-kit", "in a new window, on its first row");
await page.keyboard.type("plu");
await sleep(150);
check(same(await page.$$eval("[data-testid=instrument-list] [role=option]", (els) => els.map((e) => e.dataset.instrument)), ["@m2/synths:synth-pluck"]), "typing filters it by name");
check((await page.$eval("[data-testid=instrument-list] [data-code]", (e) => e.textContent)) === 'tracks/synth-pluck.ts · synth.pluck(lane.notes("notes"))', "its foot shows the code it writes");
await page.screenshot({ path: `${process.argv[2]}/s8-list.png` });
const rev0 = await revision();
await page.keyboard.press("Enter");
check(await until(() => exists("tracks/synth-pluck.ts")), "Enter adds it");
check(
  read("tracks/synth-pluck.ts") ===
    'import { track } from "@m2/core";\nimport { synth } from "@m2/synths";\n\nexport default track("synth-pluck", { name: "Synth pluck" }, ({ lane }) => synth.pluck(lane.notes("notes")));\n',
  "tracks/synth-pluck.ts, exactly",
);
const song1 = orig["song.ts"]
  .replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport synthPluck from "./tracks/synth-pluck";')
  .replace("      keys: { in: keys.out, db: db(-5), pan: -0.25 },\n", "      keys: { in: keys.out, db: db(-5), pan: -0.25 },\n      \"synth-pluck\": synthPluck.out,\n");
check(read("song.ts") === song1, "song.ts gets its import after the other tracks' and its channel last, in the object's style");
check(same(JSON.parse(read("data/synth-pluck/notes.json")), { clips: [] }) && read("data/synth-pluck/notes.json").endsWith("\n"), "an empty notes lane file, canonical");
const layout1 = JSON.parse(read("layout.json"));
check(
  same(layout1.order, ["kick", "hats", "synth-pluck", "bass", "keys"]) &&
    same(Object.fromEntries(Object.entries(layout1.tracks).map(([k, v]) => [k, v.colour])), { kick: "rose", hats: "teal", "synth-pluck": "green", bass: "amber", keys: "indigo" }),
  "layout.json puts it below the selected track in the first colour no track uses, and freezes every other",
);
check(await until(async () => same(await shown(), ["kick", "hats", "synth-pluck", "bass", "keys"])), "its header shows below Hats");
check(
  (await page.$eval('[data-track="synth-pluck"]', (e) => e.dataset.selected)) === "true" && !!(await page.$('[data-track="synth-pluck"] [data-lane].lit')),
  "selected and lit",
);
check((await bar("synth-pluck")) === rgb("#7cb48a"), "in its colour");
check(!(await page.$("[data-testid=track-rename]")) && (await toasted("Added Synth pluck")), "no rename opens; the toast says it was added");
check((await revision()) === rev0 + 1, "one revision");
check((await bar("kick")) === rgb("#d48e98") && (await bar("hats")) === rgb("#61b6ab") && (await bar("bass")) === rgb("#c69c68") && (await bar("keys")) === rgb("#98a0da"), "every other colour is unchanged");
await page.screenshot({ path: `${process.argv[2]}/s8-added.png` });
await key(" ");
check(await until(async () => (await api("/status")).body.playing), "Space plays at once (nothing took the key)");
await key(" ");
await until(async () => !(await api("/status")).body.playing);

// ⌘R renames it: only the name is written; its key, file and data stay.
await key("Meta+r");
await page.waitForSelector('[data-track="synth-pluck"] [data-testid=track-rename]', { timeout: 2000 }).catch(() => {});
await page.keyboard.type("Lead Vox");
await key("Enter");
check(await until(() => read("tracks/synth-pluck.ts").includes('{ name: "Lead Vox" }')), "⌘R, a name and Enter write the name");
check(read("song.ts") === song1 && exists("data/synth-pluck/notes.json") && !exists("tracks/lead-vox.ts"), "and only the name: the ID stays synth-pluck");

// A second ⌘T, Enter: the instrument added last, again, below the selected track.
await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 2000 }).catch(() => {});
check((await page.$eval("[data-testid=instrument-list] [data-active]", (e) => e.dataset.instrument).catch(() => null)) === "@m2/synths:synth-pluck", "⌘T opens the list on the instrument added last");
await page.keyboard.press("Enter");
check(await until(() => exists("tracks/synth-pluck-2.ts")), "Enter adds another Synth pluck, keyed synth-pluck-2");
check(await until(async () => same(await shown(), ["kick", "hats", "synth-pluck", "synth-pluck-2", "bass", "keys"])), "below Lead Vox");
check(await toasted("Added Synth pluck") && (await undoToast("Added Synth pluck")), "its toast's Undo");
check(await until(() => !exists("tracks/synth-pluck-2.ts") && !exists("data/synth-pluck-2") && !read("song.ts").includes("synthPluck2")), "takes back everything it wrote");

// ---------------------------------------------------------------- duplicate (design-8b §3.2)

await select("bass");
await key("Meta+d");
check(await until(() => exists("tracks/bass-2.ts")), "⌘D on Bass duplicates it");
check(read("song.ts").includes('"bass-2": duck(bass2.out, kick.env, { depth: db(-9) }),'), "its channel is copied with its duck");
const ids = (f) => JSON.parse(read(f)).clips.map((c) => c.id);
check(ids("data/bass-2/notes.json").length === ids("data/bass/notes.json").length && ids("data/bass-2/notes.json").every((i) => i && !ids("data/bass/notes.json").includes(i)), "its clips have new ids");
check(await until(async () => (await shown()).join() === "kick,hats,synth-pluck,bass,bass-2,keys"), "Bass 2 shows below Bass");
check((await rows()).find((r) => r[0] === "bass-2")?.[1] === "amber" && (await toasted("Duplicated Bass")), "in Bass's colour, said in a toast");
check((await page.$eval('[data-track="bass-2"] [data-testid=track-label]', (e) => e.textContent)) === "Bass 2", "named Bass 2");

// ---------------------------------------------------------------- delete (design-8b §3.3, §3.5)

const five = ["song.ts", "tracks/hats.ts", "data/hats/hits.json", "layout.json", "tracks/kick.ts"];
const beforeHats = snapshot(five);
await select("hats");
await key("Meta+Backspace");
check(await until(() => !exists("tracks/hats.ts")), "⌘⌫ deletes Hats without asking");
check(!exists("data/hats") && !read("song.ts").includes("hats") && !JSON.stringify(JSON.parse(read("layout.json"))).includes("hats"), "its data, its import, its channel and its layout entry go");
check(await toasted("Deleted Hats") && (await undoToast("Deleted Hats")), "the toast says so, with Undo");
check(await until(() => same(snapshot(five), beforeHats)), "Undo puts every file back, byte for byte");

const beforeKick = snapshot(five);
check(await openMenu("kick"), "a header's ⋯ opens its menu");
const rowsOfMenu = await page.$$eval("[data-testid=track-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
check(["Rename", "Duplicate", "Delete", "Colour", "Edit in code"].every((w) => rowsOfMenu.some((r) => r.startsWith(w))), `Rename, Duplicate, Delete, Colour, Edit in code: ${rowsOfMenu.join(" | ")}`);
await menuRow("Delete");
const dialog = await page.waitForSelector("[data-testid=delete-dialog]", { timeout: 3000 }).catch(() => null);
const said = dialog ? await dialog.evaluate((e) => e.innerText) : "";
check(!!dialog && said.includes("Delete Kick?") && said.includes("Bass and Bass 2 duck to Kick") && said.includes("those 2 Ducks"), `deleting the kick asks, naming both ducks: ${said.replace(/\n+/g, " | ")}`);
check((await page.$eval("[data-testid=delete-confirm]", (e) => e.textContent)) === "Delete Kick and 2 Ducks" && same(snapshot(five), beforeKick), "with a danger button, and nothing written yet");
await page.screenshot({ path: `${process.argv[2]}/s8-delete-dialog.png` });
await key("Enter");
check(await until(() => !exists("tracks/kick.ts")), "Enter confirms");
check(read("song.ts").includes("      bass: bass.out,\n") && read("song.ts").includes('      "bass-2": bass2.out,\n') && !read("song.ts").includes("duck("), "the ducks come off: bass: bass.out");
check(await toasted("Deleted Kick and 2 Ducks"), "the toast says what went with it");
await key("Meta+z");
check(await until(() => same(snapshot(five), beforeKick)), "⌘Z restores it, byte for byte");

// ---------------------------------------------------------------- reorder and recolour (FMT-19, PLANE-3)

await until(async () => (await shown()).join() === "kick,hats,synth-pluck,bass,bass-2,keys");
const rev1 = await revision();
const song2 = read("song.ts");
const from = await box('[data-module-tab="keys"]');
const to = await box('[data-module-tab="kick"]');
await page.mouse.move(from.x + 60, from.y + from.height / 2);
await page.mouse.down();
await page.mouse.move(from.x + 60, from.y + from.height / 2 - 10, { steps: 3 });
await page.mouse.move(to.x + 60, to.y + 4, { steps: 10 });
check(!!(await page.$("[data-testid=drop-line]")), "dragging a header shows where it lands");
await page.mouse.up();
check(await until(async () => (await shown())[0] === "keys"), "dropped at the top, Keys moves there");
check(JSON.parse(read("layout.json")).order[0] === "keys" && read("song.ts") === song2 && (await revision()) === rev1, "only layout.json is written, with no revision");
check(!(await toasts()).some((t) => /Moved|moving/i.test(t)), "and no toast: the timeline shows it");
await key("Meta+z");
check(await until(async () => (await shown()).join() === "kick,hats,synth-pluck,bass,bass-2,keys"), "⌘Z puts it back");

check(await openMenu("bass"), "the menu again");
await menuRow("Colour");
await page.waitForSelector("[data-testid=colours]", { timeout: 2000 }).catch(() => {});
check((await page.$$("[data-testid=colours] [data-colour]")).length === 12 && (await page.$eval('[data-colour="amber"]', (e) => e.getAttribute("aria-checked"))) === "true", "Colour ▸ shows twelve swatches, a check on the current one");
await page.screenshot({ path: `${process.argv[2]}/s8-colours.png` });
await page.click('[data-colour="violet"]');
check(await until(() => JSON.parse(read("layout.json")).tracks.bass.colour === "violet"), "recolouring writes the name");
check(await until(async () => (await bar("bass")) === rgb("#b197d0")) && (await bar("bass-2")) === rgb("#c69c68"), "and only that track's colour changes");

// From outside (an agent, git): the timeline follows within a second.
const outside = JSON.parse(read("layout.json"));
outside.order = ["keys", ...outside.order.filter((k) => k !== "keys")];
write("layout.json", JSON.stringify(outside));
const t0 = Date.now();
check(await until(async () => (await shown())[0] === "keys", 3000), "an outside edit to layout.json reorders the timeline");
check(Date.now() - t0 < 1500, `within a second or so (${Date.now() - t0} ms)`);

// A merge that left conflict markers: the defaults show, + Track still works, reorder refuses, Reset layout.
const conflicted = `{\n<<<<<<< HEAD\n  "order": ["kick"]\n=======\n  "order": ["bass"]\n>>>>>>> theirs\n}\n`;
write("layout.json", conflicted);
check(await until(async () => (await state()).problems.some((p) => p.code === "layout.invalid")), "a layout.json that doesn't read is a problem");
check(await until(async () => (await shown()).join() === "kick,hats,bass,keys,synth-pluck,bass-2"), "the timeline shows the default order");
await select("keys");
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list]");
await page.keyboard.type("pad");
await page.keyboard.press("Enter");
check(await until(() => exists("tracks/synth-pad.ts")) && read("layout.json") === conflicted, "+ Track still adds a track, and leaves layout.json as it is");
const dragFrom = await box('[data-module-tab="synth-pad"]');
await page.mouse.move(dragFrom.x + 60, dragFrom.y + dragFrom.height / 2);
await page.mouse.down();
await page.mouse.move(dragFrom.x + 60, dragFrom.y - 80, { steps: 10 });
await page.mouse.up();
check(await toasted("layout.json doesn't read: fix it, or reset the layout"), "a reorder refuses, in words");
check(read("layout.json") === conflicted, "and writes nothing");
await status();
const reset = await page.waitForSelector("[data-testid=status-popover] button ::-p-text(Reset layout)", { timeout: 3000 }).catch(() => null);
check(!!reset, "the status pill's popover offers Reset layout");
await reset?.click();
check(await until(() => { try { return !!JSON.parse(read("layout.json")).order; } catch { return false; } }), "which writes the default layout over it");
await page.keyboard.press("Escape");

// ---------------------------------------------------------------- read only (WRITE-17)

check((await post("/close", {})).status === 200, "the song closes");
const holder = spawn("python3", ["-c", `
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
os.ftruncate(fd, 0)
os.write(fd, sys.argv[2].replace("PID", str(os.getpid())).encode())
print("ready", flush=True)
sys.stdin.read()
`, `${PROJ}/.music2/lock`, JSON.stringify({ pid: 0, program: "agent", host: "here", host_id: "x", token: "0", since: Math.floor(Date.now() / 1000) }).replace('"pid":0', '"pid":PID')], { stdio: ["pipe", "pipe", "inherit"] });
await new Promise((r) => holder.stdout.once("data", r));
const opened = await post("/open", { path: PROJ });
check(opened.status === 200 && opened.body.read_only?.why === "held", "opened again while another program holds it: read-only");
await page.waitForSelector("[data-testid=read-only-banner]", { timeout: 6000 }).catch(() => {});
check(!!(await page.$("[data-testid=read-only-banner]")) && (await page.$eval("[data-testid=add-track]", (b) => b.disabled)), "the pill says so, and + Track is off");
const songRo = read("song.ts");
const refused = await post("/track/add", { session: opened.body.session, component: "@m2/drums:drum" });
check(refused.status === 423 && read("song.ts") === songRo && !exists("tracks/one-drum.ts"), `an add is refused, 423, and nothing is written: ${refused.body.error?.slice(0, 60)}`);
holder.stdin.end();
await new Promise((r) => holder.once("exit", r));

// ---------------------------------------------------------------- the Empty template (TL-3)

const empty = await page.evaluate(async (token) => {
  const r = await fetch("/api/project/new", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ name: "Empty one", template: "empty" }) });
  return { status: r.status, body: await r.json() };
}, sess.page_token);
check(empty.status === 200, "an Empty song opens");
const first = await page.waitForSelector("[data-testid=add-first-track]", { timeout: 8000 }).catch(() => null);
check(!!first && (await page.$eval("[data-testid=no-tracks]", (e) => e.innerText)).includes("or write one in code"), "its lanes offer Add a track, or code");
await page.screenshot({ path: `${process.argv[2]}/s8-empty.png` });
await first?.click();
await page.waitForSelector("[data-testid=instrument-list]");
await page.keyboard.type("one d");
await page.keyboard.press("Enter");
const dir = empty.body.dir;
check(await until(() => fs.existsSync(`${dir}/tracks/one-drum.ts`)), "Add a track adds One drum");
const emptySong = fs.readFileSync(`${dir}/song.ts`, "utf8");
check(emptySong.includes("import oneDrum from \"./tracks/one-drum\";") && emptySong.includes("      \"one-drum\": oneDrum.out,\n"), "into its mixer");
await page.waitForSelector('[data-lane="one-drum.hits"]', { timeout: 4000 }).catch(() => {});
const lane = await box('[data-lane="one-drum.hits"] > div:last-child');
await page.mouse.click(lane.x + 30, lane.y + lane.height / 2, { count: 2 });
check(await until(async () => (await state()).timeline.tracks.find((t) => t.module === "one-drum")?.lanes[0].clips.length === 1), "and its lane takes a clip");
await post("/transport", { action: "play" });
check(await until(async () => (await api("/status")).body.playing), "and the song plays");
await post("/transport", { action: "stop" });

done(errors);
