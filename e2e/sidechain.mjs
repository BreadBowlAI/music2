// Sidechain from the card (CARD-35, CARD-36, PKG-19): a Duck added from + after the bass's sound
// where the song reads it (`bass: bass.out`) listens to nothing and says None; Listens to lists the
// song's other tracks and None, and picking Kick writes `kick.env` in its call, one undo step
// ("Bass's duck listens to Kick"); ⌘Z puts the call back as it was added. What the Duck does to the
// bass's meter, played, is sidechain-dip.mjs's: it needs the meters' 33 ms tick.
//
// On first-song (the `fix` kind), its bass made a steady tone first (its lanes kept, unplayed), and
// its Duck taken out of song.ts.
//
// As the `chain` kind (E2E_DEVICE=chain) it runs on Device's chain (taste.md DV-11) whole: the + after a
// track's sound in the song is its row's there, and every check holds.
//
//   node e2e/sidechain.mjs <root> <home> <project>     (the `fix` or `chain` kind)
import { check, done, open, read, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

write(
  "tracks/bass.ts",
  `import { track } from "@m2/core";
import { osc } from "std/dsp";
import { scale } from "std/math";

// A steady tone (its lanes kept, unplayed), as sidechain-dip.mjs's.
export default track("bass", { name: "Bass" }, ({ lane }) => {
  void [lane.notes("notes"), lane.control("cutoff", "hz", 500), lane.mask("drop")];
  return scale(osc.sine(440), 0.25);
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
// On the chain run, that + is the Bass row's, beside the mix (taste.md DV-11).
if (process.env.E2E_DEVICE === "chain") check(!!(await page.$(`[data-testid=chain] [data-branch="Bass"] ${bass.replace('[data-module="song"] ', "")}`)), "on the chain, it is the Bass row's");

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

// ---------------------------------------------------------------- undo
await key("Meta+z");
check(await until(() => read("song.ts").replace('import { duck } from "std/fx";\n', "") === added), "⌘Z takes the kick out of the call again");
check(await until(async () => (await listening()) === "Listens to:None"), `and the row says None: ${await listening()}`);

done(errors);
