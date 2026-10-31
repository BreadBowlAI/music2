// Upgrade it (AGENT-14, FMT-25), on a fresh copy of examples/first-song whose files are then made
// first-song's as it was before wave 3 (apps/m2/tests/old-songs/first-song-std-0.8: std 0.8,
// @m2/core 1, the simple kick and hat of @m2/synths 1), as a copy of an old song would be. The song
// stops building, and the status pill says first that it was made for an older music2; its popover
// offers Upgrade it. That writes the song as m2 upgrade does, one write, one undo step named
// Upgrade: the kick One drum's (today's file byte for byte), the hat One drum's hats, the manifest's
// lines and the lock today's. The song builds again and the pill says no more of it; a message says
// what changed, with Undo. ⌘Z puts every file back byte for byte. The popover linted.
//
//   node e2e/upgrade.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, read, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const OLD = new URL("../apps/m2/tests/old-songs/first-song-std-0.8/", import.meta.url);
const old = (f) => fs.readFileSync(new URL(f, OLD), "utf8");
// The files first-song had then that it has otherwise now.
const FILES = ["music2.toml", "music2.lock", "tracks/hats.ts", "tracks/kick.ts"];
const today = Object.fromEntries(FILES.map((f) => [f, read(f)]));

const { page, errors, posts, state, status, key, toasts } = await open(1440, 900);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title).catch(() => null);
const pill = () => page.$eval("[data-status]", (e) => ({ ids: e.dataset.status ?? "", text: e.textContent ?? "" })).catch(() => ({ ids: "", text: "" }));

// ---------------------------------------------------------------- made for an older music2

for (const f of FILES) write(f, old(f));
check(await until(async () => (await state()).upgrade === true, 15000), "the state says the song was made for an older music2");
check(await until(async () => (await pill()).ids.split(" ")[0] === "upgrade"), `the status pill says so first: ${(await pill()).ids}`);
check((await pill()).text.includes("Made for an older music2"), `in words: ${(await pill()).text}`);
await status();
check(await until(async () => !!(await page.$("[data-testid=upgrade] [data-testid=upgrade-run]"))), "its popover offers Upgrade it");
const said = await page.$eval("[data-testid=upgrade]", (e) => e.textContent ?? "");
check(said.includes("This song was made for an older music2") && said.includes("Undo takes it back"), `and says what it does: ${said}`);
const bad = await page.evaluate(lint, "[data-testid=status-popover]");
check(bad.length === 0, `lint: the status popover${bad.length ? `: ${bad.slice(0, 3).join(" | ")}` : ""}`);

// ---------------------------------------------------------------- Upgrade it

const n = posts.length;
await page.click("[data-testid=upgrade-run]");
check(await until(() => read("tracks/kick.ts") === today["tracks/kick.ts"], 15000), "Upgrade it writes the kick as One drum's: today's file, byte for byte");
check(read("tracks/hats.ts").includes('drums.drum(lane.notes("hits"), { drum: "hats" })'), "the hat is One drum's hats");
const manifest = read("music2.toml");
check(['"@m2/core" = "2"', 'std = "0.9"', '"@m2/synths" = "2"', '"@m2/drums" = "2"'].every((l) => manifest.includes(`${l}\n`)), `music2.toml asks for this music2: ${manifest}`);
check(read("music2.lock") === today["music2.lock"], "music2.lock locks today's packs");
check(posts.slice(n).filter((p) => p.url === "/api/graph/upgrade").length === 1, "one write");
check(
  await until(async () => {
    const s = await state();
    return s.upgrade === false && s.view.draft_errors.length === 0;
  }),
  "the song builds again",
);
check(await until(async () => !(await pill()).ids.includes("upgrade")), `the status pill says no more of it: ${(await pill()).ids}`);
check(await until(async () => (await undoTitle()) === "Undo Upgrade (⌘Z)"), `one undo step, Upgrade: ${await undoTitle()}`);
check(await until(async () => (await toasts()).some((t) => t.includes("Upgraded to this music2: changed 4 files") && t.includes("Undo"))), `a message says what changed, with Undo: ${await toasts()}`);

// ---------------------------------------------------------------- ⌘Z

await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => FILES.every((f) => read(f) === old(f)), 15000), "⌘Z puts every file back, byte for byte");

done(errors);
