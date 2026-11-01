// Track br's slices 2 and 3 (gaps #26, FMT-22, HOST-8): presets for std sounds and plugins. A sound
// card has Presets: yours and music2's own, a search, Save as preset…. Loading one is one edit with
// Undo; saving writes a file in the scratch HOME's Presets folder and no song file. A plugin's card
// (the probe, M2_PROBE=1) lists yours and the plugin's own: Dark loads its state, with Undo.
//
//   node e2e/br-presets.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { HOME, PROJ, check, done, open, read, until } from "./fixlib.mjs";
import fs from "node:fs";

const { page, errors, toasts, state, api, view, caughtUp } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 8000);

// A Synth pluck track, as + Track adds one.
const added = await post("/track/add", { session: (await state()).session, component: "@m2/synths:synth-pluck" });
const module = added.body.module;
const file = `tracks/${module}.ts`;
await view("graph");
await until(async () => !!(await page.$(`[data-module-tab="${module}"]`)));
await page.click(`[data-module-tab="${module}"] [data-testid=track-label]`);
const button = `[data-module="${module}"] [data-presets]`;
check(await until(() => page.$(button)), "a Synth pluck card has Presets");
const before = read(file);

const openPresets = async () => {
  await page.click(button);
  return until(() => page.$("[data-testid=preset-popover] [data-preset-group=std] [data-preset]"));
};
check(await openPresets(), "Presets opens a list");
const groups = await page.$$eval("[data-testid=preset-popover] [role=group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.join() === "Yours,music2", `its groups are Yours, then music2 (${groups})`);
const names = await page.$$eval("[data-preset-group=std] [data-preset]", (els) => els.map((e) => e.textContent));
check(names.join() === "Glass,Soft", `music2's are Synth pluck's own, by name (${names})`);
await page.type("[data-testid=preset-popover] input[aria-label='Search presets']", "so");
check(
  await until(async () => (await page.$$eval("[data-preset-group=std] [data-preset]", (els) => els.map((e) => e.textContent))).join() === "Soft"),
  "the search keeps the ones that match",
);

// ---------------------------------------------------------------- load: one edit, with Undo

const knob = async (slot) => {
  const s = await state();
  const m = s.view.modules.find((m) => m.key === module);
  return m?.nodes.flatMap((n) => n.knobs).find((k) => k.slot === slot)?.value;
};
check((await knob("brightness")) === 4500, "the brightness knob starts at its default, 4500 Hz");
await page.click("[data-preset-group=std] [data-preset='Soft']");
check(await until(() => read(file).includes("brightness: 1800")), "loading Soft writes its settings into the call");
check(/decay: 0\.6, spread: 0\.6, velSens: 0\.6/.test(read(file)), "every control it gives");
check(await until(async () => (await knob("brightness")) === 1800), "the knob moves to 1800 Hz");
check(await toasted("Loaded Soft into Synth pluck"), "a toast says so, with Undo");
await page.click('button[aria-label="Undo"]');
check(await until(() => read(file) === before), "Undo puts the code back, byte for byte");

// ---------------------------------------------------------------- save as yours

// Presets lists and saves the call as the page shows it, and Undo changed it: the page shows that first.
await caughtUp();

const yours = `${HOME}/Music/music2/Presets/_m2_synths/synth-pluck`;
check(await openPresets(), "Presets opens again");
await page.click("[data-testid=preset-save]");
await page.type("[data-testid=preset-name]", "My Pluck");
await page.keyboard.press("Enter");
check(await toasted("Saved Synth pluck's settings as My Pluck"), "Save as preset… says it saved");
const saved = fs.existsSync(`${yours}/My Pluck.json`) ? JSON.parse(fs.readFileSync(`${yours}/My Pluck.json`, "utf8")) : null;
check(saved?.sound === "@m2/synths:synth-pluck" && saved?.values?.brightness === 4500 && saved?.m2preset === 1, `a file in the Presets folder holds every setting (${JSON.stringify(saved)})`);
check(read(file) === before, "and no song file changed");
check(await openPresets(), "Presets opens again");
check(await until(() => page.$("[data-preset-group=yours] [data-preset='My Pluck']")), "My Pluck is listed under Yours");

// The same name, in other capitals: asked first, then replaced.
await page.click("[data-testid=preset-save]");
await page.type("[data-testid=preset-name]", "my pluck");
await page.keyboard.press("Enter");
check(
  await until(async () => (await page.$eval("[data-testid=preset-popover]", (e) => e.textContent).catch(() => "")).includes("A preset called My Pluck is there. Replace it?")),
  "a name that is there asks before it replaces it",
);
await page.click("[data-testid=preset-save-confirm]");
check(await toasted("as my pluck"), "Replace saves it");
check(fs.readdirSync(yours).join() === "my pluck.json", `one file, under its new name (${fs.readdirSync(yours)})`);

// ---------------------------------------------------------------- a plugin's presets (HOST-8)

const probe = await post("/track/add", { session: (await state()).session, component: "clap:m2.probe" });
const pm = probe.body.module;
const blob = `${PROJ}/data/${pm}/${pm}.state`;
await until(async () => !!(await page.$(`[data-module-tab="${pm}"]`)));
await page.click(`[data-module-tab="${pm}"] [data-testid=track-label]`);
const probeButton = `[data-module="${pm}"] [data-presets]`;
check(await until(() => page.$(probeButton)), "the probe's card has Presets");
await page.click(probeButton);
check(await until(() => page.$("[data-preset-group=plugin] [data-preset]")), "its popover lists the plugin's own");
const probeGroups = await page.$$eval("[data-testid=preset-popover] [role=group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(probeGroups.join() === "Yours,m2 probe's", `its groups are Yours, then m2 probe's (${probeGroups})`);
const own = await page.$$eval("[data-preset-group=plugin] [data-preset]", (els) => els.map((e) => e.textContent));
check(own.join() === "Bright,Dark", `the probe offers Bright and Dark (${own})`);
check(!fs.existsSync(blob), "no state is saved yet");
await page.click("[data-preset-group=plugin] [data-preset='Dark']");
check(await toasted("Loaded Dark into m2 probe"), "Dark loads, with Undo");
check(fs.existsSync(blob) && fs.readFileSync(blob).includes("dark") && fs.existsSync(`${blob}.json`), "its state and sidecar are written");
await page.click('button[aria-label="Undo"]');
check(await until(() => !fs.existsSync(blob)), "Undo takes them away");

done(errors);
