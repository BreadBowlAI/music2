// Track br's slice 1 (gaps #32): the installed plugins in the side browser and + Track. The list is
// the host process's scan (HOST-3). A plugin dropped onto a track goes after its sound, with Undo;
// one picked in + Track makes a new track playing it. Needs the probe plugin: run.mjs gives this
// suite's server M2_PROBE=1 (debug builds only).
//
//   node e2e/br-plugins.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { PROJ, check, done, open, read, until } from "./fixlib.mjs";
import fs from "node:fs";

const { page, errors, toasts, show } = await open(1440, 900, { browser: true });
const kick = read("tracks/kick.ts");
const song = read("song.ts");
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 8000);
const clickIn = (sel, words) =>
  page.evaluate(
    (sel, words) => {
      const b = [...document.querySelectorAll(sel)].find((e) => e.textContent.trim() === words);
      b?.click();
      return !!b;
    },
    sel,
    words,
  );

// ---------------------------------------------------------------- the side browser's Plugins groups

const fx = "[data-browser] [data-browser-plugins=effects]";
const ins = "[data-browser] [data-browser-plugins=instruments]";
check(!!(await page.$(fx)) && !!(await page.$(ins)), "the side browser has a Plugins group under the instruments and one under the effects");
await page.click(`${fx} button[aria-expanded]`);
check(await until(() => page.$('[data-browser-item="clap:m2.probe"]')), "opened, the effects' Plugins group lists m2 probe");
check((await page.$eval('[data-browser-item="clap:m2.probe"]', (e) => e.textContent).catch(() => null)) === "m2 probe", "by its name");
await page.click(`${ins} button[aria-expanded]`);
check(await until(() => page.$('[data-browser-instrument="clap:m2.probe"]')), "the instruments' Plugins group lists it too");
check(!!(await page.$(`${fx} button[data-look-again]`)), "a Plugins group has Look again");

// ---------------------------------------------------------------- dropped onto Kick: after its sound

await page.evaluate(() => {
  const at = document.querySelector('[data-testid=timeline] [data-track="kick"]');
  const dt = new DataTransfer();
  dt.setData("application/x-m2-effect", "clap:m2.probe");
  for (const type of ["dragenter", "dragover", "drop"]) at.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
});
// Kick's sound also feeds its env (the bass ducks to it): the app asks first, as for std's effects.
if (await toasted("would reach")) await clickIn("[data-testid=toast] button", "Add anyway");
const wrapped = 'plugin("clap:m2.probe", { in: kick(lane.notes("hits")), key: "m2-probe" })';
check(await until(() => read("tracks/kick.ts").includes(`const out = ${wrapped};`)), "a drop onto Kick wraps its sound in the plugin, keyed");
check(/import \{ ms, track, plugin \} from "@m2\/core";/.test(read("tracks/kick.ts")), "and imports plugin");
check(await toasted("Added m2 probe after Kick"), "a toast says what was added, with Undo");
await show("kick");
check(await until(() => page.$$eval('[data-module="kick"] [data-node-title]', (els) => els.some((e) => e.textContent.includes("m2 probe")))), "the plugin's card shows in Kick");
await page.click('button[aria-label="Undo"]');
check(await until(() => read("tracks/kick.ts") === kick), "Undo takes it away, byte for byte");

// ---------------------------------------------------------------- + Track: a track playing it

await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 2000 }).catch(() => {});
check(await until(() => page.$('[data-testid=instrument-list] [data-instrument="clap:m2.probe"]')), "+ Track lists m2 probe under Plugins");
const groups = await page.$$eval("[data-testid=instrument-list] [role=group]", (els) => els.map((e) => e.getAttribute("aria-label")));
check(groups.at(-1) === "Plugins", `Plugins is + Track's last group: ${groups}`);
await page.click('[data-testid=instrument-list] [data-instrument="clap:m2.probe"]');
const file = `${PROJ}/tracks/m2-probe.ts`;
check(await until(() => fs.existsSync(file)), "picking it adds tracks/m2-probe.ts");
check(
  fs.existsSync(file) &&
    read("tracks/m2-probe.ts").includes('track("m2-probe", { name: "m2 probe" }, ({ lane }) => plugin("clap:m2.probe", { notes: lane.notes("notes"), key: "m2-probe" }))'),
  "a track playing the plugin from its notes lane, keyed as the track",
);
check(await toasted("Added m2 probe"), "said with its Undo");
await page.click('button[aria-label="Undo"]');
check(await until(() => !fs.existsSync(file) && read("song.ts") === song), "one Undo takes the track away");

// ---------------------------------------------------------------- Look again

const scans = [];
page.on("request", (r) => r.url().endsWith("/api/plugins/scan") && scans.push(r.method()));
await page.click(`${fx} button[data-look-again]`);
check(await until(() => scans.length === 1), "Look again scans again");
check(await until(() => page.$('[data-browser-item="clap:m2.probe"]')), "and still lists the probe");

done(errors);
