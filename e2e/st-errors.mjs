// Errors in plain words (the studio plan's D2, Q45, Q46): the compile banner, the Code view's
// problem list and Needs attention lead with a plain sentence and a fix, and point at your own
// line; the problem's own words, its code and the place inside std are under Details.
//
//   node e2e/st-errors.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";

const { browser, page, errors, state, view, status } = await open(1440, 900);
const bannerItems = () => page.$$eval("[data-testid=compile-banner] li", (els) => els.map((e) => e.innerText.replace(/\s+/g, " ").trim()));
const clean = async (sel, what) => {
  const bad = [...new Set([...(await page.evaluate(lint, sel)), ...(await page.evaluate(tips, sel))])];
  check(bad.length === 0, `${what}: the taste lint and the tooltip lint find nothing${bad.length ? ` (${bad.slice(0, 6).join("; ")})` : ""}`);
};
const settled = () => until(async () => !(await page.$("[data-status]")), 10000);
const closePopover = async () => {
  if (await page.$("[data-testid=status-popover]")) await page.keyboard.press("Escape");
  await sleep(150);
};

// ---------------------------------------------------------------- a level written as a cutoff

const bass0 = read("tracks/bass.ts");
const bass = bass0
  .replace('import { hz, khz, track } from "@m2/core";', 'import { db, hz, khz, track } from "@m2/core";\nimport { lowpass } from "std/filter";')
  .replace("return bass(lane.notes(\"notes\"), { cutoff: blend(cutoff, wobble, drop) });", "return lowpass(bass(lane.notes(\"notes\"), { cutoff: blend(cutoff, wobble, drop) }), db(-3));");
const line = bass.split("\n").findIndex((l) => l.includes("lowpass(bass(")) + 1;
write("tracks/bass.ts", bass);
check(await until(async () => !!(await page.$("[data-status~=compile]")), 10000), "a level as a cutoff: the code doesn't compile");
const d = (await state()).diagnostics?.[0] ?? {};
check(
  d.file === "tracks/bass.ts" && d.line === line && d.plain === "Cutoff wants a frequency (Hz), but gets a level (dB)." && d.fix === "Write it in Hz, such as hz(800).",
  `the server places it on your own line (${d.file}:${d.line}, line ${line} wanted) in plain words: ${d.plain} ${d.fix}`,
);
check(d.message === "input `cutoff` expects control<hz>, got control<db>" && d.code === "code.check" && /^std\/src\/filter\.ts:\d+:\d+$/.test(d.inner ?? ""), `with the original beside them: ${d.message} · ${d.code} · ${d.inner}`);
await status();
await page.waitForSelector("[data-testid=compile-banner]");
const [item] = await bannerItems();
check(
  item?.startsWith(`Cutoff wants a frequency (Hz), but gets a level (dB). tracks/bass.ts, line ${line}. Write it in Hz, such as hz(800).`),
  `the banner says it plainly, where, and the fix: ${item}`,
);
check(!item?.includes("expects control<hz>") && !item?.includes("std/src"), "the technical words aren't on show");
const weight = await page.$eval("[data-testid=compile-banner] li[data-plain] span", (e) => getComputedStyle(e).fontWeight);
check(Number(weight) >= 600, `the sentence leads, in bold (${weight})`);
await page.click("[data-testid=compile-banner] li[data-plain] summary");
const details = await page.$eval("[data-testid=compile-banner] li[data-plain] details", (e) => (e.open ? e.innerText.replace(/\s+/g, " ") : ""));
check(
  details.includes("input `cutoff` expects control<hz>, got control<db>") && details.includes("code.check") && /std\/src\/filter\.ts:\d+:\d+/.test(details),
  `Details holds the original, its code and the place inside std: ${details}`,
);
await clean("[data-testid=status-popover]", "the status popover with plain words and Details open");
// Show in code: bass.ts, the line marked.
await page.click("[data-testid=compile-banner] button");
check(await until(async () => (await page.$eval("[data-testid=code-path]", (e) => e.textContent).catch(() => "")) === "tracks/bass.ts"), "Show in code opens tracks/bass.ts");
const marked = async () =>
  page.evaluate(() => {
    const sq = document.querySelector("[data-testid=code] .squiggly-error");
    const top = sq?.parentElement?.style.top;
    return top ? ([...document.querySelectorAll("[data-testid=code] .view-line")].find((l) => l.style.top === top)?.textContent ?? "") : "";
  });
check(await until(async () => (await marked()).includes("lowpass(bass(")), `the editor marks your own line: "${(await marked()).trim()}"`);
const row = await page.$eval("[data-testid=problems] [data-plain]", (e) => e.querySelector("button").innerText.replace(/\s+/g, " ")).catch(() => "");
check(
  row.startsWith(`tracks/bass.ts:${line} Cutoff wants a frequency (Hz), but gets a level (dB). Write it in Hz, such as hz(800).`),
  `the Code view's list says it plainly, at your line: ${row}`,
);
check(!!(await page.$("[data-testid=problems] [data-plain] details")), "with the original under Details");
await clean("[data-testid=problems]", "the Code view's problem list");
write("tracks/bass.ts", bass0);
check(await settled(), "put back, it compiles");
await view("graph");

// ---------------------------------------------------------------- a name misspelt

const keys0 = read("tracks/keys.ts");
write("tracks/keys.ts", keys0.replace("pluck(lane", "pluk(lane"));
check(await until(async () => !!(await page.$("[data-status~=compile]")), 10000), "pluk(…): the code doesn't compile");
await status();
await page.waitForSelector("[data-testid=compile-banner] li[data-plain]");
const [pluk] = await bannerItems();
check(
  pluk?.startsWith("`pluk` isn't known in tracks/keys.ts.".replace(/`/g, "")) && pluk.includes('Did you mean pluck? Import it: import { pluck } from "@m2/synths".'),
  `it says the name isn't known, and proposes pluck and its import: ${pluk}`,
);
check((await page.$$eval("[data-testid=compile-banner] li[data-plain] code[data-code]", (els) => els.map((e) => e.textContent))).includes('import { pluck } from "@m2/synths"'), "the import is set as code, to copy");
await closePopover();
write("tracks/keys.ts", keys0);
check(await settled(), "put back, it compiles");

// ---------------------------------------------------------------- a typo

write("tracks/keys.ts", keys0.replace('notes("chords")));', 'notes("chords"));'));
check(await until(async () => !!(await page.$("[data-status~=compile]")), 10000), "a bracket missing: the code doesn't compile");
await status();
await page.waitForSelector("[data-testid=compile-banner] li[data-plain]");
const [typo] = await bannerItems();
check(typo?.startsWith("There is a typo on line 4 of tracks/keys.ts. Check the brackets, commas and quotes on that line and the one before."), `a typo names its line: ${typo}`);
await closePopover();
write("tracks/keys.ts", keys0);
check(await settled(), "put back, it compiles");

// ---------------------------------------------------------------- a plugin that isn't installed

write("tracks/keys.ts", keys0.replace("import { track }", "import { plugin, track }").replace('pluck(lane.notes("chords"))', 'plugin("clap:org.example.not-installed", { notes: lane.notes("chords") })'));
check(await until(async () => !!(await page.$("[data-status~=problems]")), 10000), "a plugin that isn't installed: it plays, and something needs attention");
await status();
await page.waitForSelector('[data-testid=attention] [data-problem="plugin.missing"]');
const plugin = await page.$eval('[data-testid=attention] [data-problem="plugin.missing"]', (e) => ({
  message: e.querySelector("[data-testid=problem-message]")?.innerText,
  fix: e.querySelector("[data-testid=problem-fix]")?.innerText,
  details: e.querySelector("details")?.textContent ?? "",
}));
check(
  plugin.message === "Keys plays a plugin that isn't installed on this computer, so it is silent." && plugin.fix === "Install it, or pick another instrument: right-click Keys, then Change instrument….",
  `Needs attention says so plainly, with the fix: ${plugin.message} | ${plugin.fix}`,
);
check(plugin.details.includes("org.example.not-installed") && plugin.details.includes("plugin.missing"), `the original under Details: ${plugin.details.slice(0, 160)}`);
const mark = await page.$eval('[data-track="keys"] [data-testid=track-issues]', (e) => e.title).catch(() => "");
check(mark.startsWith("Keys plays a plugin that isn't installed on this computer, so it is silent."), `the track's mark says it plainly too: ${mark.split("\n")[0]}`);
await clean("[data-testid=status-popover]", "Needs attention with plain words");
await closePopover();
write("tracks/keys.ts", keys0);
check(await settled(), "put back, nothing needs attention");

// ---------------------------------------------------------------- a sound file that isn't there

const hats0 = read("tracks/hats.ts");
write("data/hats/loop.json", JSON.stringify({ clips: [{ id: "a", start: 0, length: 4, asset: "loop.wav" }] }));
write(
  "tracks/hats.ts",
  hats0.replace('import { drums } from "@m2/drums";', 'import { drums } from "@m2/drums";\nimport { sum } from "std/math";').replace(/drums\.drum\(lane\.notes\("hits"\), \{[^}]*\}\)/, 'sum($&, lane.audio("loop"))'),
);
check(await until(async () => (await state()).problems?.some((p) => p.code === "asset.missing"), 10000), "a clip whose sound file isn't in assets/");
await status();
await page.waitForSelector('[data-testid=attention] [data-problem="asset.missing"]');
const asset = await page.$eval('[data-testid=attention] [data-problem="asset.missing"]', (e) => ({
  message: e.querySelector("[data-testid=problem-message]")?.innerText,
  fix: e.querySelector("[data-testid=problem-fix]")?.innerText,
}));
check(
  asset.message === "loop.wav isn't in the song's assets folder, so its clip plays nothing." && asset.fix === "Put the file back, or delete the clip.",
  `Needs attention says the file is missing, plainly: ${asset.message} | ${asset.fix}`,
);
await closePopover();
write("tracks/hats.ts", hats0);
fs.rmSync(`${PROJ}/data/hats/loop.json`);
check(await settled(), "put back, nothing needs attention");

// ---------------------------------------------------------------- a problem no rule knows

const song0 = read("song.ts");
write("song.ts", `${song0}\nthrow new Error("my own words");\n`);
check(await until(async () => !!(await page.$("[data-status~=compile]")), 10000), "an error of the song's own: the code doesn't compile");
await status();
await page.waitForSelector("[data-testid=compile-banner] li");
const [own] = await bannerItems();
check(own?.includes("my own words") && !(await page.$("[data-testid=compile-banner] li[data-plain]")), `one no rule knows keeps its own words: ${own}`);
await closePopover();
write("song.ts", song0);
check(await settled(), "put back, it compiles");

await browser.close();
done(errors);
