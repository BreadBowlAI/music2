// Record (studio lane B, Q17 to Q28): the top bar's Record button after Play (28 px, its tooltip,
// everything to its right 32 px further); its right-click menu and its four checks (Punch and
// Overdub since track recording: MON-10, MON-12), kept in this machine's settings.json; R stopped
// counts in one bar (the button says 4 … 1), then plays and records; keys injected as a MIDI
// keyboard's make a growing take in the lane; Space stops both and writes one clip over whole bars,
// selected, "Undo recording on Bass", ⌘Z takes it back; the keyboard piano records, Notes' own
// clicks don't; snapped to the grid; while looping the first pass is written at the wrap, and the
// song plays on and records the next pass (MON-11); R while playing records from now; Esc and Space
// while counting cancel and nothing plays; nothing played says so; a PC page's R; with the code
// broken R says so and starts nothing (the review's R1); Settings' MIDI keyboards
// (one turned off plays and records nothing, the mark says so, one unplugged says "(not connected)",
// one plugged in before the window opened has its row on the start screen: the review's F3);
// the lint; and on a server of the suite's own that plays into a file, the count's four clicks a beat
// apart, the first louder, and the song's first beat a bar after the first click.
//
// Keys come as a MIDI keyboard's: `/api/graph/midi/inject` (run.mjs gives the server M2_MIDI=inject).
//
//   node e2e/st-record.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { HEADLESS, REPO } from "./env.mjs";
import { exe } from "./chrome.mjs";
import { lint } from "./lintlib.mjs";
import { HOME, PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { page, errors, api, state, caughtUp, box, key } = await open(1440, 900);
const DANGER = "rgb(175, 61, 54)";
const DANGER_SOFT = "rgb(255, 233, 230)";
const OLD = "db9f9t5s";

const button = (p = page) =>
  p.$eval("[data-testid=record]", (e) => ({ phase: e.dataset.phase, text: e.textContent ?? "", title: e.title, bg: getComputedStyle(e).backgroundColor, color: getComputedStyle(e).color })).catch(() => null);
const status = async () => (await api("/status")).body;
const record = async () => (await state()).record ?? null;
const inject = (body) => api("/midi/inject", { method: "POST", body: JSON.stringify(body) });
const midi = (bytes) => inject({ device: "Test Keys", bytes });
const settings = () => {
  try {
    return JSON.parse(fs.readFileSync(`${HOME}/.music2/settings.json`, "utf8"));
  } catch {
    return {};
  }
};
/** The bass lane's clips as written, the first song's own left out. */
const takes = () => JSON.parse(fs.readFileSync(`${PROJ}/data/bass/notes.json`, "utf8")).clips.filter((c) => c.id !== OLD);
const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
const blur = () => page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
const press = async (k) => {
  await blur();
  await page.keyboard.press(k);
};
/** Each row's words (its name, then its line of detail, if any), ✓ when ticked; "—" a separator. */
const menuRows = () =>
  page.$$eval("[data-testid=record-menu] [role^=menuitem], [data-testid=record-menu] [role=separator]", (els) =>
    els.map((e) => {
      if (e.getAttribute("role") === "separator") return "—";
      const words = [...e.querySelectorAll("span")].filter((s) => s.childElementCount === 0 && s.textContent).map((s) => s.textContent);
      return `${words.join(": ")}${e.getAttribute("aria-checked") === "true" ? " ✓" : ""}`;
    }),
  );
const openMenu = async () => {
  const b = await box("[data-testid=record]");
  await page.mouse.click(b.x + 14, b.y + 14, { button: "right" });
  return until(() => page.$("[data-testid=record-menu]"), 2000);
};
const pick = async (id) => {
  await openMenu();
  await page.click(`[data-testid=record-menu] [data-menu-item=${id}]`);
  await until(async () => !(await page.$("[data-testid=record-menu]")), 2000);
};
/** Stopped, and no take: what each part starts from. */
const settled = () => until(async () => !(await status()).playing && (await record()) === null && (await button())?.phase === "off", 5000);
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);

// ---------------------------------------------------------------- the button (Q17)

const b0 = await button();
const play = await box("[data-testid=play]");
const rec = await box("[data-testid=record]");
const loop = await box("[data-testid=loop-toggle]");
check(
  !!rec && Math.abs(rec.x - (play.x + play.width + 4)) < 1 && rec.width === 28 && rec.height === 28 && Math.abs(loop.x - (rec.x + 32)) < 1,
  `Record sits right after Play, 28 px, and Loop moved 32 px on (play ${play.x}+${play.width}, record ${rec?.x} ${rec?.width}×${rec?.height}, loop ${loop.x})`,
);
check(b0?.title === "Record (R) · right-click for options", `its tooltip says what it does, its key and its menu: ${b0?.title}`);
check(b0?.phase === "off" && b0.color === DANGER && b0.bg === "rgba(0, 0, 0, 0)", `at rest a ghost with a red circle (${JSON.stringify(b0)})`);

// ---------------------------------------------------------------- its menu, kept on this machine (Q18)

check(await openMenu(), "a right-click opens its menu");
const rows0 = await menuRows();
check(
  rows0.join("|") ===
    "Count in first (one bar of clicks) ✓|Snap notes to the grid|" +
      "Punch in and out: Record only between the loop's edges, and replace what was there.|" +
      "Overdub: Add the notes you play to the clip under them.|—|MIDI keyboards…|Sound input…",
  `its rows: count in (ticked), snap, Punch and Overdub (not), each saying what it does, a line, MIDI keyboards, Sound input (${rows0.join(" | ")})`,
);
const bad0 = await page.evaluate(lint, "[data-testid=record-menu]");
check(bad0.length === 0, `the lint finds nothing in the menu${bad0.length ? `: ${bad0.slice(0, 6).join(" | ")}` : ""}`);
await page.keyboard.press("Escape");
await pick("quantize");
check(await until(() => settings().record?.quantize === true && settings().record?.count_in === true, 3000), `Snap kept in settings.json: ${JSON.stringify(settings().record)}`);
await openMenu();
check((await menuRows())[1] === "Snap notes to the grid ✓", "the menu shows it ticked");
await page.click("[data-testid=record-menu] [data-menu-item=quantize]");
check(await until(() => settings().record === undefined, 3000), `untick it: back to the defaults, left out of the file (${JSON.stringify(settings())})`);
await pick("keyboards");
check(await until(() => page.$("[data-testid=settings-dialog]"), 2000), "MIDI keyboards… opens Settings");
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=settings-dialog]")), 2000);

// ---------------------------------------------------------------- R: a count-in, then a take (Q19 to Q25)

await page.click('[data-module-tab="bass"] [data-testid=track-label]');
await inject({ device: "Test Keys", plugged: true });
const before = (await state()).view.revision;
await press("r");
check(await until(async () => (await record())?.phase === "count", 2000), `R: the server counts in (${JSON.stringify(await record())})`);
await until(async () => (await button())?.bg === DANGER_SOFT, 1000);
const counting = await button();
check(counting?.phase === "count" && counting.text === "4" && counting.bg === DANGER_SOFT, `the button says 4 on danger-soft (${JSON.stringify(counting)})`);
check(await until(async () => (await button())?.text === "1", 3000), `then counts down to 1 (${(await button())?.text})`);
check(await until(async () => (await status()).playing && (await record())?.phase === "take", 3000), "a bar later the song plays and the take records");
await until(async () => (await button())?.bg === DANGER, 2000);
const recording = await button();
check(recording?.phase === "take" && recording.bg === DANGER && recording.color === "rgb(255, 255, 255)", `recording: a white circle on danger (${JSON.stringify(recording)})`);
await sleep(400);
await midi([0x90, 45, 100]);
await sleep(500);
await midi([0x80, 45, 0]);
await sleep(300);
await midi([0x90, 52, 80]);
check(await until(async () => Number(await page.$eval("[data-testid=take]", (e) => e.dataset.notes).catch(() => 0)) === 2, 3000), "the take grows in Bass's lane with the notes played");
const w1 = (await box("[data-testid=take]"))?.width ?? 0;
await sleep(500);
const w2 = (await box("[data-testid=take]"))?.width ?? 0;
check(w2 > w1 + 10, `it grows to the playhead (${w1.toFixed(0)} → ${w2.toFixed(0)} px)`);
const label = await page.$eval("[data-testid=take]", (e) => ({ text: e.textContent, outline: getComputedStyle(e).outlineColor, bg: getComputedStyle(e).backgroundColor })).catch(() => null);
check(label?.text === "Recording" && label.outline === DANGER && label.bg === DANGER_SOFT, `it says Recording, a danger outline round a danger-soft body (${JSON.stringify(label)})`);
const bad1 = await page.evaluate(lint, "header");
check(bad1.length === 0, `the lint finds nothing in the top bar while recording${bad1.length ? `: ${bad1.slice(0, 6).join(" | ")}` : ""}`);
await page.screenshot({ path: `${ROOT}/st-record-take.png`, clip: { x: 0, y: 0, width: 1440, height: 400 } });
await press("Space");
check(await until(async () => !(await status()).playing, 3000), "Space stops the song");
check(await until(() => takes().length === 1, 5000), `one new clip in the lane file (${takes().length})`);
const t1 = takes()[0];
check(t1 && t1.start % 4 === 0 && t1.length % 4 === 0 && t1.length >= 4, `over whole bars (${t1?.start} for ${t1?.length})`);
check(t1?.notes.length === 2 && t1.notes[0].pitch === "A2" && Math.abs(t1.notes[0].velocity - 100 / 127) < 1e-9, `with the notes played, at their velocity (${JSON.stringify(t1?.notes)})`);
check((await state()).view.revision === before + 1, "one Tx");
check(await until(() => page.$(`[data-clip="${t1?.id}"][data-picked]`), 3000), "the new clip is selected");
check(await until(async () => (await undoTitle()).startsWith("Undo recording on Bass"), 3000), `Undo names it: ${await undoTitle()}`);
check((await button())?.phase === "off" && !(await page.$("[data-testid=take]")), "the button rests and the growing take is gone");
await key("Meta+z");
check(await until(() => takes().length === 0, 3000), "⌘Z takes it back");

// ---------------------------------------------------------------- the keyboard piano records; Notes' clicks don't (Q21)

await settled();
await pick("count_in");
check(await until(() => settings().record?.count_in === false, 3000), `count-in off, kept: ${JSON.stringify(settings().record)}`);
await key("Meta+k");
await until(() => page.$("[data-testid=keyboard-piano]"), 2000);
await press("r");
check(await until(async () => (await record())?.phase === "take" && (await status()).playing, 2000), "R with no count-in records at once");
await sleep(300);
await blur();
await page.keyboard.down("a");
check(
  await until(async () => (await page.$eval("[data-testid=piano-plays]", (e) => e.textContent).catch(() => "")) === "Plays Bass · recording", 2000),
  `the piano's card says it records (${await page.$eval("[data-testid=piano-plays]", (e) => e.textContent).catch(() => "")})`,
);
await sleep(300);
await page.keyboard.up("a");
await sleep(200);
const unwritten = (await state()).view.revision;
await press("r");
check(await until(() => takes().length === 1, 5000), "R again ends the take and writes it");
check((await status()).playing, "R ends recording; the song plays on");
check(takes()[0]?.notes.map((n) => n.pitch).join() === "C4", `the piano's A key recorded (${JSON.stringify(takes()[0]?.notes)})`);
await press("Space");
await key("Meta+k");
// ⌘Z takes back the step the page shows: the take's, once the page shows it (the take is on disk
// before its revision is published).
await caughtUp(unwritten + 1);
await key("Meta+z");
await until(() => takes().length === 0, 3000);

// Notes' own keys: heard, not recorded.
await settled();
await page.click(`[data-clip="${OLD}"]`, { count: 2 });
await until(() => page.$("[data-testid=notes] [data-key]"), 3000);
await press("r");
await until(async () => (await record())?.phase === "take", 2000);
const k = await page.$eval("[data-testid=notes] [data-key]", (e) => {
  const r = e.getBoundingClientRect();
  return { x: r.left + 46, y: r.top + r.height / 2 };
});
await page.mouse.move(k.x, k.y);
await page.mouse.down();
await sleep(300);
await page.mouse.up();
await press("r");
check(await until(async () => (await toasts()).some((t) => t.includes("Nothing was played, so nothing was recorded.")), 3000), `a click on Notes' keys isn't recorded: the toast says nothing was (${JSON.stringify(await toasts())})`);
check(takes().length === 0, "and nothing is written");
await press("Space");

// ---------------------------------------------------------------- snapped, and the first pass of a loop (Q26, Q27, MON-11)

await settled();
await pick("quantize");
await api("/loop", { method: "POST", body: JSON.stringify({ start: 0, end: 4 }) });
await api("/seek", { method: "POST", body: JSON.stringify({ beat: 0 }) });
const unlooped = (await state()).view.revision;
await press("r");
await until(async () => (await record())?.phase === "take", 2000);
check((await record())?.until === 4, "looping bar 1: each pass of the take ends at the loop's end");
await sleep(560);
await midi([0x90, 50, 100]);
await sleep(150);
await midi([0x80, 50, 0]);
check(await until(() => takes().length === 1, 5000), "at the wrap the first pass is written");
// MON-11: the take goes round the loop until it stops, a pass at a time.
check(
  await until(async () => (await status()).playing && (await record())?.phase === "take" && (await record())?.pass === 1, 1000),
  `the loop plays on, recording its second pass (${JSON.stringify(await record())})`,
);
const t3 = takes()[0];
check(t3?.start === 0 && t3.length === 4 && t3.notes.length === 1 && Math.abs((t3.notes[0].start * 4) % 1) < 1e-9, `one bar, its note's start on a sixteenth (${JSON.stringify(t3)})`);
await press("Space");
await api("/loop", { method: "POST", body: JSON.stringify({ on: false }) });
// The pass written at the wrap is the server's, not an answer to the page: the page shows it first.
await caughtUp(unlooped + 1);
await key("Meta+z");
await until(() => takes().length === 0, 3000);
await pick("quantize");

// ---------------------------------------------------------------- R while playing (Q19)

await settled();
await pick("count_in");
await api("/seek", { method: "POST", body: JSON.stringify({ beat: 0 }) });
await press("Space");
await until(async () => (await status()).beat > 5, 5000);
await press("r");
check(await until(async () => (await record())?.phase === "take", 2000), "R while playing records at once, with no count");
check((await record())?.start === 4, `from the bar it is in (${(await record())?.start})`);
await press("r");
await until(async () => (await record()) === null, 3000);
await press("Space");

// ---------------------------------------------------------------- Esc and Space while counting (Q19, D6)

await settled();
await press("r");
await until(async () => (await record())?.phase === "count", 2000);
await press("Escape");
check(await until(async () => (await record()) === null && (await button())?.phase === "off", 2000), "Esc while counting cancels");
await sleep(2600);
check(!(await status()).playing, "and nothing plays");
await press("r");
await until(async () => (await record())?.phase === "count", 2000);
await press("Space");
await sleep(2600);
check(!(await status()).playing && (await record()) === null, "Space while counting cancels too: nothing plays");
check(takes().length === 0, "nothing was written");

// ---------------------------------------------------------------- a PC page's R

const pc = await open(1440, 900, { pc: true });
await pc.page.click('[data-module-tab="bass"] [data-testid=track-label]');
await pc.page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
await pc.page.keyboard.press("r");
check(await until(async () => (await record())?.phase === "count", 2000), "R on a PC page counts in too");
await pc.page.keyboard.press("Escape");
await until(async () => (await record()) === null, 2000);
await pc.browser.close();
errors.push(...pc.errors);

// ---------------------------------------------------------------- the code broken: R says so (the review's R1)

await settled();
const code = read("song.ts");
write("song.ts", `${code}\n(\n`);
await until(async () => (await state()).view.draft_errors.length > 0, 5000);
await press("r");
check(
  await until(async () => (await toasts()).some((t) => t.includes("Fix the code first: it doesn't compile right now.")), 2000),
  `with the code broken, R says to fix it first (${JSON.stringify(await toasts())})`,
);
await sleep(300);
check((await record()) === null && !(await status()).playing, "and nothing counts in or plays");
if (await record()) await press("Escape");
write("song.ts", code);
await until(async () => (await state()).view.draft_errors.length === 0, 5000);

// ---------------------------------------------------------------- Settings' MIDI keyboards (Q28)

const settingsOpen = async () => {
  await blur();
  await key("Meta+,");
  return until(() => page.$("[data-testid=settings-dialog]"), 3000);
};
const settingsClose = async () => {
  await page.keyboard.press("Escape");
  await until(async () => !(await page.$("[data-testid=settings-dialog]")), 2000);
};
const kbRows = () => page.$$eval("[data-keyboard]", (els) => els.map((e) => `${e.textContent}${e.querySelector("input")?.checked ? " ✓" : ""}`));
const live = async () => (await status()).live;
const mark = () => page.$eval("[data-testid=midi-status]", (e) => e.title).catch(() => null);
await settled();
check(await settingsOpen(), "⌘, opens Settings");
check((await kbRows()).join("|") === "Test KeysPlays ✓", `a row for the keyboard plugged in, Plays ticked (${(await kbRows()).join(" | ")})`);
await page.click("[data-keyboard='Test Keys'] input");
check(await until(() => JSON.stringify(settings().midi?.off) === '["Test Keys"]', 3000), `unticked: kept in settings.json (${JSON.stringify(settings().midi)})`);
const bad2 = await page.evaluate(lint, "[data-testid=settings-dialog]");
check(bad2.length === 0, `the lint finds nothing in Settings${bad2.length ? `: ${bad2.slice(0, 6).join(" | ")}` : ""}`);
await settingsClose();
check(await until(async () => (await mark()) === "Test Keys is turned off in Settings", 3000), `the top bar's mark says it is off: ${await mark()}`);
await midi([0x90, 60, 100]);
await sleep(400);
check((await live()) === 0, "turned off, a key plays nothing");
await midi([0x80, 60, 0]);
await until(async () => !(await toasts()).some((t) => t.includes("Nothing was played")), 10000);
await press("Space");
await until(async () => (await status()).playing, 3000);
await press("r");
await until(async () => (await record())?.phase === "take", 2000);
await midi([0x90, 60, 100]);
await sleep(300);
await midi([0x80, 60, 0]);
await press("r");
check(await until(async () => (await toasts()).filter((t) => t.includes("Nothing was played")).length > 0 && takes().length === 0, 3000), "and records nothing");
await press("Space");
await inject({ device: "Test Keys", plugged: false });
await settingsOpen();
check(await until(async () => (await kbRows()).join("|") === "Test Keys (not connected)Plays", 3000), `unplugged, its row says so (${(await kbRows()).join(" | ")})`);
await inject({ device: "Test Keys", plugged: true });
await until(async () => (await kbRows()).join("|") === "Test KeysPlays", 3000);
await page.click("[data-keyboard='Test Keys'] input");
check(await until(() => settings().midi === undefined, 3000), `ticked again: left out of the file (${JSON.stringify(settings())})`);
await settingsClose();
await midi([0x90, 60, 100]);
check(await until(async () => (await live()) === 1, 3000), "on again, a key plays");
await midi([0x80, 60, 0]);
await inject({ device: "Test Keys", plugged: false });
await settingsOpen();
check(await until(() => page.$("[data-testid=settings-midi-none]"), 3000), "none plugged in and none off: “No MIDI keyboard is plugged in.”");
await settingsClose();
// The start screen, in a window opened after the keyboard was plugged in.
await inject({ device: "Test Keys", plugged: true });
await api("/close", { method: "POST", body: "{}" });
await page.reload({ waitUntil: "domcontentloaded" });
check(await until(() => page.$("[data-testid=start]"), 5000), "the song closed: the start screen");
check(await settingsOpen(), "⌘, opens Settings there");
check(await until(async () => (await kbRows()).join("|") === "Test KeysPlays ✓", 3000), `the keyboard plugged in has its row (${(await kbRows()).join(" | ")})`);
await settingsClose();

await heard();
done(errors);

/** A server of the suite's own that plays into a file, the metronome on: the count-in's clicks, then the song's first beat. */
async function heard() {
  const root = `${ROOT}/capture`;
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(`${root}/home/.music2`, { recursive: true });
  fs.cpSync(`${REPO}/templates/empty`, `${root}/proj`, { recursive: true });
  // Its Beat track, with no clips: silent, so what plays is the clicks.
  const song = fs.readFileSync(`${root}/proj/song.ts`, "utf8").replace("// const beat", "const beat").replace("// beat: beat.out, // drums", "beat: beat.out,");
  fs.writeFileSync(`${root}/proj/song.ts`, song);
  const capture = `${root}/played.f32`;
  const sessFile = `${root}/home/.music2/session.json`;
  const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "none", M2_AUDIO_CAPTURE: capture, HOME: `${root}/home` };
  const server = spawn(HEADLESS, [`${root}/proj`, sessFile, `c${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${root}/headless.log`, "w")] });
  const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
  try {
    if (!(await until(() => fs.existsSync(sessFile), 20_000))) return check(false, "the capture server started");
    await sleep(300);
    const s2 = JSON.parse(fs.readFileSync(sessFile, "utf8"));
    const call = (p, path, body) =>
      p.evaluate(
        async (token, path, body) => {
          const r = await fetch(`/api/graph${path}`, { method: body ? "POST" : "GET", headers: { "x-m2-token": token, "content-type": "application/json" }, body: body && JSON.stringify(body) });
          return r.json().catch(() => ({}));
        },
        s2.page_token,
        path,
        body,
      );
    const p2 = await browser.newPage();
    await p2.setViewport({ width: 1440, height: 900 });
    await p2.evaluateOnNewDocument((token) => {
      window.__M2__ = { native: false, token, platform: "macos" };
    }, s2.page_token);
    await p2.goto(s2.url, { waitUntil: "domcontentloaded" });
    await p2.waitForSelector("[data-testid=metronome-toggle]", { timeout: 10000 });
    const g = await call(p2, "");
    const track = g.timeline.tracks.find((t) => t.track && t.lanes.some((l) => l.kind === "notes"));
    const lane = track.lanes.find((l) => l.kind === "notes").key;
    await p2.click("[data-testid=metronome-toggle]");
    await sleep(400);
    await until(() => fs.existsSync(capture), 5000);
    const frames = () => Math.floor(fs.statSync(capture).size / 8);
    const left = (a, b) => {
      const buf = fs.readFileSync(capture);
      const x = new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.length / 4));
      const out = [];
      for (let i = a; i < Math.min(b, x.length / 2); i++) out.push(x[2 * i]);
      return out;
    };
    const blips = (x) => {
      const found = [];
      let quiet = 4800;
      for (let i = 0; i < x.length; i++) {
        if (Math.abs(x[i]) > 0.01 && quiet >= 4800) {
          let peak = 0;
          for (let j = i; j < Math.min(i + 1440, x.length); j++) peak = Math.max(peak, Math.abs(x[j]));
          found.push({ at: i, peak });
        }
        quiet = Math.abs(x[i]) < 1e-4 ? quiet + 1 : 0;
      }
      return found;
    };
    const f0 = frames();
    await call(p2, "/record/start", { session: g.session, module: track.module, lane, count_in: true });
    await sleep(3200);
    await call(p2, "/record/stop", { session: g.session });
    await call(p2, "/transport", { action: "stop" });
    await sleep(200);
    const bpm = g.timeline.tempo?.[0]?.bpm ?? 120;
    const beat = Math.round((60 / bpm) * 48000);
    const found = blips(left(f0, frames()));
    const gaps = found.slice(1, 5).map((b, i) => b.at - found[i].at);
    check(
      found.length >= 5 && gaps.every((d) => Math.abs(d - beat) <= 1) && found[0].peak > 0.2 && found.slice(1, 4).every((b) => b.peak < 0.2),
      `four clicks a beat apart (${beat} frames), the first louder, then the song's first beat a bar after the first (${found.slice(0, 6).map((b) => `${b.at}:${b.peak.toFixed(3)}`).join(" ")})`,
    );
    check(found.length >= 5 && found[4].peak > 0.2, "the song's downbeat clicks a bar later, accented");
  } finally {
    await browser.close();
    server.kill();
  }
}
