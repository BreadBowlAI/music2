// Data problems in the diagnostic model (DIAG-1, DIAG-4) on a fresh copy of the converted first-song:
// a lane file that doesn't read keeps playing its last accepted data, with an Error naming the file
// and the field (FMT-9); clips without ids are a Notice whose fix the app applies (FMT-12); a file
// under data/ that isn't one is a Notice (FMT-7).   node run.mjs s4-problems fix
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, toasts } = await open();
const rows = () => page.$$eval("[data-testid=attention] [data-problem]", (els) => els.map((e) => ({ code: e.dataset.problem, severity: e.dataset.severity, src: e.dataset.src, text: e.innerText })));
const clips = (lane) => page.$$eval(`[data-lane="${lane}"] [data-clip]`, (els) => els.length).catch(() => 0);
const issue = (track) => page.$eval(`[data-track="${track}"] [data-testid=track-issues]`, (e) => ({ severity: e.dataset.severity, title: e.title })).catch(() => null);

// FMT-9: a lane file with an unknown field, written while the session runs.
const hats = read("data/hats/hits.json");
const n = await clips("hats.hits");
write("data/hats/hits.json", hats.replace('"start": 0,', '"start": 0, "swing": 0.2,'));
check(await until(async () => (await rows()).some((r) => r.code === "lane.invalid")), "a lane file with an unknown field is lane.invalid in the strip");
const bad = (await rows()).find((r) => r.code === "lane.invalid");
check(bad.severity === "error" && /data\/hats\/hits\.json/.test(bad.text) && /swing/.test(bad.text), `an Error naming the file and the field: ${bad.text.slice(0, 160)}`);
check((await clips("hats.hits")) === n, "the lane keeps its last accepted clips");
const mark = await issue("hats");
check(mark?.severity === "error" && /swing/.test(mark.title), "the hats track is marked, the message on hover");
check(!(await page.$("[data-testid=compile-banner]")), "and the song still plays: no compile banner");
await page.screenshot({ path: `${ROOT}/s4-problems-invalid.png` });
write("data/hats/hits.json", hats);
check(await until(async () => !(await rows()).some((r) => r.code === "lane.invalid")), "fixed by hand, the problem goes");

// FMT-12: clips without ids are a Notice; its fix gives them ids, one undoable edit.
const keys = JSON.parse(read("data/keys/chords.json"));
write("data/keys/chords.json", JSON.stringify({ clips: keys.clips.map(({ id, ...c }) => c) }));
check(await until(async () => (await rows()).some((r) => r.code === "clip.anonymous")), "clips without ids: clip.anonymous");
const anon = (await rows()).find((r) => r.code === "clip.anonymous");
check(anon.severity === "notice", "a Notice");
const fixButton = '[data-testid=attention] [data-problem="clip.anonymous"] [data-fix="assign-clip-ids"]';
check(!!(await page.$(fixButton)), "with its fix a click away");
check((await clips("keys.chords")) === keys.clips.length, "the clips still play and show");
await page.screenshot({ path: `${ROOT}/s4-problems-anon.png` });
await page.click(fixButton);
check(await until(() => JSON.parse(read("data/keys/chords.json")).clips.every((c) => /^[0-9a-z]{8}$/.test(c.id))), "the fix gives every clip an id");
check(await until(async () => !(await rows()).some((r) => r.code === "clip.anonymous")), "and the notice goes");
check(await until(async () => (await toasts()).some((t) => t.startsWith("Fixed data/keys/chords.json"))), "a toast says what was fixed");
await page.click('button[aria-label="Undo"]');
check(await until(() => JSON.parse(read("data/keys/chords.json")).clips.every((c) => c.id === undefined)), "undo takes the ids back");

// FMT-7: a file under data/ that isn't one music2 reads.
write("data/Kick.notes.json", "{}");
// It makes no revision of its own (PLANE-3): it shows with the next one.
write("song.ts", `${read("song.ts")}// a comment\n`);
check(await until(async () => (await rows()).some((r) => r.code === "file.unknown")), "an unknown file under data/ is file.unknown");
check((await rows()).find((r) => r.code === "file.unknown").severity === "notice", "a Notice, after the others");
const order = (await rows()).map((r) => r.severity);
check(JSON.stringify(order) === JSON.stringify([...order].sort((a, b) => ["error", "warning", "notice"].indexOf(a) - ["error", "warning", "notice"].indexOf(b))), `worst first: ${order}`);
await browser.close();
done(errors);
