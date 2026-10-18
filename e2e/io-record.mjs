// Recording audio in the app (the io plan's slice 3). The run's server has an injected sound input
// (run.mjs gives it M2_AUDIO_INPUT=inject and M2_AUDIO_INPUTS="Test mic,USB mic"):
//   - Settings ▸ Sound input lists "Same device as the output", then the inputs; USB mic chosen is
//     kept in settings.json; "Hear yourself while recording" is off, says to use headphones, and is
//     kept when ticked, across a reload;
//   - + Track ▸ Audio track; R counts in (4 … 1), then the take grows on the audio lane as a red
//     clip with an input level bar; a tone injected moves the bar; the take hears the input (Settings
//     said so); R ends it: one clip "Audio take.wav" with its waveform, selected, "Undo recording on
//     Audio"; ⌘Z takes it back and the file stays in assets/;
//   - the Song row selected, R says which tracks record; the inject API answers only with
//     M2_AUDIO_INPUT=inject; a read-only window can't record (423); the lint.
//
//   node e2e/io-record.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { spawn } from "node:child_process";
import { HEADLESS } from "./env.mjs";
import { lint } from "./lintlib.mjs";
import { HOME, PROJ, ROOT, check, done, open, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, box, key, toasts } = await open(1440, 900);
const injects = process.env.M2_AUDIO_INPUT === "inject";
const settings = () => {
  try {
    return JSON.parse(fs.readFileSync(`${HOME}/.music2/settings.json`, "utf8"));
  } catch {
    return {};
  }
};
const record = async () => (await state()).record ?? null;
const status = async () => (await api("/status")).body;
const inject = (body) => api("/input/inject", { method: "POST", body: JSON.stringify(body) });
const blur = () => page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
const press = async (k) => {
  await blur();
  await page.keyboard.press(k);
};
const button = () => page.$eval("[data-testid=record]", (e) => ({ phase: e.dataset.phase, text: e.textContent ?? "" })).catch(() => null);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const openSettings = async () => {
  await blur();
  await key("Meta+,");
  return until(() => page.$("[data-testid=settings-dialog]"), 3000);
};
const closeSettings = async () => {
  await page.click("[data-testid=settings-done]");
  await until(async () => !(await page.$("[data-testid=settings-dialog]")), 2000);
};
const inputLabel = () => page.$eval("[data-testid=settings-input]", (e) => e.textContent).catch(() => null);
const hear = () => page.$eval("[data-testid=settings-hear]", (e) => e.checked).catch(() => null);

// ---------------------------------------------------------------- Settings ▸ Sound input (D13, D21)

check(await openSettings(), "⌘, opens Settings");
check((await inputLabel()) === "Same device as the output", `Sound input: the output's own device by default (${await inputLabel()})`);
await page.click("[data-testid=settings-input]").catch(() => {});
await until(() => page.$("[data-testid=settings-input-menu]"), 2000);
const rows = await page.$$eval("[data-testid=settings-input-menu] [role=menuitemradio]", (els) => els.map((e) => e.textContent));
check(rows.join("|") === "Same device as the output|Test mic|USB mic", `its menu lists the inputs there are: ${rows.join(" | ")}`);
await page.click('[data-testid=settings-input-menu] [data-choice="USB mic"]').catch(() => {});
check(await until(() => settings().input?.device === "USB mic" && settings().input?.name === "USB mic", 3000), `USB mic chosen, kept: ${JSON.stringify(settings().input)}`);
const note = await page.$eval("[data-testid=settings-hear-note]", (e) => e.textContent).catch(() => "");
check((await hear()) === false && note === "Use headphones: speakers feed back into the microphone.", `Hear yourself is off, and says to use headphones (${await hear()}, ${note})`);
await linted("[data-testid=settings-dialog]", "Settings with Sound input");
await page.click("[data-testid=settings-hear]").catch(() => {});
check(await until(() => settings().input?.monitor === true, 3000), `ticked, kept: ${JSON.stringify(settings().input)}`);
await closeSettings();
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
await openSettings();
check(await until(async () => (await inputLabel()) === "USB mic" && (await hear()) === true, 3000), `a reload keeps both (${await inputLabel()}, ${await hear()})`);
await page.screenshot({ path: `${ROOT}/io-record-settings.png` });
await closeSettings();

// ---------------------------------------------------------------- R on the Song row, and the inject API

await page.click('[data-module-tab="song"] [data-testid=track-label]');
await press("r");
check(
  await until(async () => (await toasts()).some((t) => t.includes("Select a notes track or an audio track to record on it.")), 2000),
  `nothing selected, R says which tracks record: ${JSON.stringify(await toasts())}`,
);
const early = await inject({ samples: [0.5] });
check(
  injects ? early.status === 400 && early.body.error === "Nothing is recording from the sound input." : early.status === 404,
  `the inject API answers only with M2_AUDIO_INPUT=inject, and says when nothing records (${early.status} ${JSON.stringify(early.body)})`,
);

// ---------------------------------------------------------------- + Track ▸ Audio track; R (D14, D19, D20)

await page.keyboard.press("Escape");
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list] input");
await page.keyboard.type("audio");
await page.keyboard.press("Enter");
const tracks = async () => (await state()).timeline.tracks.filter((t) => t.track).map((t) => t.module);
check(await until(async () => (await tracks()).includes("audio"), 5000), `+ Track ▸ Audio track adds one: ${await tracks()}`);
await until(async () => (await page.$('[data-module-tab="audio"][aria-selected=true]')) !== null, 2000);
const before = (await state()).view.revision;
await press("r");
check(await until(async () => (await record())?.phase === "count", 3000), `R counts in on the audio track (${JSON.stringify(await record())} ${JSON.stringify(await toasts())})`);
const r0 = await record();
check(r0?.kind === "audio" && r0.module === "audio" && r0.lane === "clips" && r0.hear === true, `an audio take, heard as Settings says (${JSON.stringify(r0)})`);
check(await until(async () => (await button())?.text === "4", 1500), `the button says 4 (${(await button())?.text})`);
check(await until(async () => (await button())?.text === "1", 3000), `then counts down to 1 (${(await button())?.text})`);
check(await until(async () => (await status()).playing && (await record())?.phase === "take", 3000), "a bar later the song plays and the take records");
const take = "[data-lane=\"audio.clips\"] [data-testid=take]";
check(await until(() => page.$(take), 2000), "the take shows in the audio track's lane");
const shown = await page.$eval(take, (e) => ({ kind: e.dataset.kind, text: e.textContent, outline: getComputedStyle(e).outlineColor })).catch(() => null);
check(shown?.kind === "audio" && shown.text === "Recording" && shown.outline === "rgb(175, 61, 54)", `a red clip that says Recording (${JSON.stringify(shown)})`);
const level = () => page.$eval(`${take} [data-testid=take-level]`, (e) => Number(e.getAttribute("aria-valuenow"))).catch(() => null);
check(await until(async () => (await level()) === -60, 2000), `its input level bar reads silence (${await level()} dB)`);
const tone = await inject({ tone: { hz: 440, seconds: 1.5, gain: 0.5 } });
check(tone.status === 200, `a tone is injected (${tone.status} ${JSON.stringify(tone.body)})`);
check(await until(async () => (await level()) === -6, 2000), `the bar shows the tone's level, −6 dB (${await level()} dB)`);
const w1 = (await box(take))?.width ?? 0;
await sleep(500);
const w2 = (await box(take))?.width ?? 0;
check(w2 > w1 + 10, `the take grows to the playhead (${w1.toFixed(0)} → ${w2.toFixed(0)} px)`);
await linted("[data-lane=\"audio.clips\"]", "the audio lane while it records");
await page.screenshot({ path: `${ROOT}/io-record-take.png`, clip: { x: 0, y: 0, width: 1440, height: 520 } });
await sleep(1200);
await press("r");
const clips = () => {
  try {
    return JSON.parse(fs.readFileSync(`${PROJ}/data/audio/clips.json`, "utf8")).clips;
  } catch {
    return [];
  }
};
check(await until(() => clips().length === 1, 6000), `R ends it: one clip in the lane file (${JSON.stringify(clips())})`);
const c = clips()[0];
check(c?.asset === "Audio take.wav" && fs.existsSync(`${PROJ}/assets/Audio take.wav`), `named "Audio take.wav", in assets/ (${c?.asset})`);
check((await status()).playing, "the song plays on");
await press("Space");
check(await until(() => page.$(`[data-clip="${c?.id}"][data-picked]`), 3000), "the new clip is selected");
const inked = await until(
  () =>
    page.$eval(`[data-clip="${c?.id}"] canvas`, (cv) => {
      const d = cv.getContext("2d").getImageData(0, 0, cv.width, cv.height).data;
      let ink = 0;
      for (let i = 3; i < d.length; i += 4) if (d[i] > 0) ink++;
      return ink > 200;
    }).catch(() => false),
  4000,
);
check(inked, "it draws its waveform");
check((await state()).view.revision >= before + 1, "written as an edit");
check(await until(async () => (await undoTitle()).startsWith("Undo recording on Audio"), 3000), `Undo names it: ${await undoTitle()}`);
check((await button())?.phase === "off" && !(await page.$(take)), "the button rests and the growing take is gone");
await page.screenshot({ path: `${ROOT}/io-record-clip.png`, clip: { x: 0, y: 0, width: 1440, height: 520 } });
await key("Meta+z");
check(!!c && (await until(() => clips().length === 0, 3000)), "⌘Z takes it back");
check(fs.existsSync(`${PROJ}/assets/Audio take.wav`), "the file stays (music2 never deletes an asset)");

// ---------------------------------------------------------------- a read-only window can't record (WRITE-17)

const roHome = `${ROOT}/ro-home`;
fs.mkdirSync(roHome, { recursive: true });
const roFile = `${ROOT}/ro-session.json`;
const ro = spawn(HEADLESS, [PROJ, roFile, `ro${Date.now()}`], { env: { ...process.env, HOME: roHome, M2_AUDIO: "none", M2_MIDI: "none" }, stdio: "ignore" });
process.on("exit", () => ro.exitCode === null && ro.kill("SIGKILL"));
await until(async () => fs.existsSync(roFile), 30000);
await sleep(300);
const rs = JSON.parse(fs.readFileSync(roFile, "utf8"));
const roApi = async (path, body) => {
  const r = await fetch(`${rs.url}/api/${path}`, body === undefined ? { headers: { "x-m2-token": rs.token } } : { method: "POST", headers: { "x-m2-token": rs.token, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
let roGraph = null;
await until(async () => (roGraph = (await roApi("graph")).json)?.session !== undefined, 15000);
check(!!roGraph?.read_only, `a second window opens the song read-only (${JSON.stringify(roGraph?.read_only ?? null)})`);
const refused = await roApi("graph/record/start", { session: roGraph?.session, module: "audio", lane: "clips" });
check(refused.status === 423 && /nothing was changed/.test(refused.json?.error ?? ""), `R there is refused, 423 (${refused.status} ${refused.json?.error})`);
check((await roApi("graph")).json?.record == null, "and nothing records");
ro.kill("SIGTERM");

done(errors);
