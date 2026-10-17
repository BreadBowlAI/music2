// 8b.3 The Mix view in the app (design-8b §5.5, §5.7; taste.md §3.5, MX-1, MX-2, MX-5; lead
// decision T6): the strips in track order with Master pinned right; the fader law; a drag that
// writes a channel's left-out level into its entry and nothing else, with no toast, and its undo;
// Shift for fine; a typed level; Alt-click for 0 dB; the foot for silence; the wheel (one edit, and
// a sideways scroll moves the strips, not the fader); the keys; a channel's first pan, written as
// the drag passes 4 px and undone in two steps; the master's fader; no level slider in the track
// headers; selection and the strip's menu; a track in no channel; and a read-only window.
//
//   node e2e/s8-mix.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { spawn } from "node:child_process";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { page, errors, posts, api, state, toasts, key, box, view } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const sent = (path) => posts.filter((p) => p.url === `/api/graph/${path}`).length;
const undo = async () => {
  const rev = (await state()).view.revision;
  await key("Meta+z");
  return until(async () => (await state()).view.revision !== rev);
};
const orig = { song: read("song.ts"), keys: read("tracks/keys.ts") };
const songIs = (text) => until(() => read("song.ts") === text);
/** The song's code with one mixer entry (`kick: kick.out,`) written as `to`. */
const entry = (from, to) => orig.song.replace(from, to);

// The fader law (design-8b §5.5): 0 dB at 75% of the travel, +12 dB at the top, the square law
// below 0 dB down to −60 dB, which sits just above the 4 px foot.
const FOOT = 4;
const at = (db) => (db >= 0 ? Math.min(1, 0.75 + (0.25 * db) / 12) : db <= -60 ? 0 : 0.75 * ((db + 60) / 60) ** 2);
const dbAt = (p) => Math.round((p >= 0.75 ? (12 * (p - 0.75)) / 0.25 : -60 * (1 - Math.sqrt(Math.max(0, p) / 0.75))) * 10) / 10 || 0;

const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
const fader = (id) => `${strip(id)} [data-fader]`;
/** A strip's fader as drawn: its value, whether the code leaves it out, its level, and where its thumb sits on the travel (0..1). */
const faderOf = (id) =>
  page.$eval(fader(id), (e, FOOT) => {
    const travel = e.querySelector("[data-testid=fader-travel]").getBoundingClientRect();
    const thumb = e.querySelector("[data-testid=fader-thumb]").getBoundingClientRect();
    const levelEl = e.querySelector("[data-testid=fader-level]");
    return {
      value: Number(e.dataset.value),
      dflt: e.dataset.default === "true",
      level: levelEl?.textContent ?? null,
      muted: levelEl ? getComputedStyle(levelEl).color === "rgb(101, 105, 111)" : null,
      title: levelEl?.title ?? "",
      p: (travel.bottom - FOOT - (thumb.top + thumb.height / 2)) / (travel.height - FOOT),
      room: travel.height - FOOT,
      editable: !!e.dataset.knobId,
    };
  }, FOOT);
const pan = (id) => `${strip(id)} [data-knob$=".pan"]`;
const panOf = (id) => page.$eval(pan(id), (e) => ({ value: Number(e.dataset.value), dflt: e.dataset.default === "true", text: e.querySelector(".tabular-nums")?.textContent }));
/** Press on `sel`'s thumb (a fader) or its middle, move `dy` px up (down when negative), and let go; `during` runs while it is held. */
const dragBy = async (sel, dy, { shift = false, during } = {}) => {
  const b = await box(sel);
  const x = b.x + b.width / 2, y = b.y + b.height / 2;
  if (shift) await page.keyboard.down("Shift");
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y - dy, { steps: 10 });
  const seen = during ? await during() : null;
  await page.mouse.up();
  if (shift) await page.keyboard.up("Shift");
  return seen;
};
const thumb = (id) => `${fader(id)} [data-testid=fader-thumb]`;
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);

// ---------------------------------------------------------------- the view (⌥3)

await key("Alt+3");
check(await until(async () => !!(await page.$('[data-view-tab="mix"][aria-selected="true"] ')) && !!(await page.$(strip("master")))), "⌥3 shows the Mix view");
const strips = await page.$$eval("[data-testid=mix] [data-strip]", (els) =>
  els.map((e) => {
    const r = e.getBoundingClientRect();
    return { id: e.dataset.strip, name: e.querySelector("[data-testid=strip-name]")?.textContent, w: Math.round(r.width), left: r.left, right: r.right, bar: getComputedStyle(e.firstElementChild).backgroundColor };
  }),
);
check(same(strips.map((s) => [s.id, s.name]), [["kick", "Kick"], ["hats", "Hats"], ["bass", "Bass"], ["keys", "Keys"], ["master", "Master"]]), `a strip per track in track order, then Master: ${strips.map((s) => s.name)}`);
const mixBox = await box("[data-testid=mix]");
const master = strips.at(-1), keysStrip = strips.at(-2);
check(strips.slice(0, -1).every((s) => s.w === 76) && master.w === 96, `strips are 76 px, Master 96 (${strips.map((s) => s.w)})`);
check(Math.abs(master.right - (mixBox.x + mixBox.width)) < 1 && master.left - keysStrip.right >= 12, `Master is pinned right, at least 12 px after the strips (${Math.round(master.left - keysStrip.right)} px)`);
check(master.bar === "rgb(154, 159, 166)" && new Set(strips.slice(0, -1).map((s) => s.bar)).size === 4 && !strips.slice(0, -1).some((s) => s.bar === master.bar), `each track's strip has its colour; Master is graphite (${strips.map((s) => s.bar).join(", ")})`);
await page.screenshot({ path: `${ROOT}/s8-mix.png` });

const kick0 = await faderOf("kick"), hats0 = await faderOf("hats");
check(kick0.value === 0 && kick0.dflt && kick0.level === "0.0" && kick0.muted && kick0.title.includes("Not written in your code yet"), `Kick's level, left out of its entry, reads 0.0 in grey and says why (${JSON.stringify(kick0)})`);
check(hats0.value === -6 && !hats0.dflt && hats0.level === "−6.0" && !hats0.muted, `Hats' reads −6.0, as written (${hats0.level})`);
check(Math.abs(kick0.p - 0.75) < 0.01 && Math.abs(hats0.p - at(-6)) < 0.01, `0 dB sits at 75% of the travel, −6 dB at 61% (${kick0.p.toFixed(3)}, ${hats0.p.toFixed(3)})`);
const pans = [await panOf("kick"), await panOf("hats"), await panOf("keys")];
check(same(pans.map((p) => p.text), ["C", "R 30", "L 25"]) && pans[0].dflt && !pans[1].dflt, `pans read C (in grey: not written), R 30 and L 25: ${pans.map((p) => p.text)}`);

// ---------------------------------------------------------------- a fader drag (MX-2, design-8b §5.2)

const room = kick0.room;
const during = await dragBy(thumb("kick"), -30, {
  during: async () => ({ leases: (await state()).leases?.length ?? 0, song: read("song.ts"), bubble: await page.$eval("[data-testid=value-bubble]", (e) => e.textContent).catch(() => null) }),
});
const want = dbAt(0.75 - 30 / room);
check(during.leases === 1 && during.song === orig.song && during.bubble === `${want < 0 ? "−" : ""}${Math.abs(want).toFixed(1)} dB`, `while it is held it is heard (a lease), shows ${during.bubble}, and writes nothing yet`);
const wrapped = /kick: \{ in: kick\.out, db: db\((-?[\d.]+)\) \},/;
check(await until(() => wrapped.test(read("song.ts"))), `let go, it writes Kick's level into its entry: ${read("song.ts").match(/kick: .*,/)?.[0]}`);
const written = Number(read("song.ts").match(wrapped)?.[1]);
check(written === want && read("song.ts").replace(wrapped, "kick: kick.out,") === orig.song, `${written} dB, 30 px down the fader's law (${want}), and nothing else in the file changed`);
await sleep(300);
check(sent("set") === 1 && (await toasts()).length === 0, `one write, and no toast (${sent("set")} writes; ${await toasts()})`);
check(await until(async () => { const f = await faderOf("kick"); return f.value === written && !f.dflt && !f.muted; }), "the fader shows it, as written");
check(await until(async () => (await undoTitle()) === "Undo Kick volume (⌘Z)"), `Undo names it: ${await undoTitle()}`);
await undo();
check(await songIs(orig.song), "undo puts `kick: kick.out` back");

// Shift: a tenth as far.
await dragBy(thumb("hats"), 40, { shift: true });
const fine = dbAt(at(-6) + 4 / room);
check(await songIs(entry("db: db(-6)", `db: db(${fine})`)), `Shift-drag 40 px up moves it as 4 px do: −6 to ${fine}`);
await undo();
await songIs(orig.song);

// A typed level: the design's own example (design-8b §5.2).
const level = `${fader("kick")} [data-testid=fader-level]`;
await page.click(level, { count: 2 });
const typeIn = `${fader("kick")} [data-testid=fader-type-in]`;
check(await until(async () => !!(await page.$(typeIn))) && (await page.$eval(typeIn, (e) => e.value === "0.0" && e.selectionStart === 0 && e.selectionEnd === 3)), "a double-click on the level opens it to type, all selected");
await page.keyboard.type("loud");
await page.keyboard.press("Enter");
const refused = await page.$eval(`${fader("kick")} [role=alert]`, (e) => e.textContent).catch(() => null);
check(refused === "Type a level, like −6 or −inf" && read("song.ts") === orig.song, `"loud" is refused in words, and nothing is written: ${refused}`);
await page.click(typeIn, { count: 3 });
await page.keyboard.type("-3");
await page.keyboard.press("Enter");
check(await songIs(entry("kick: kick.out,", "kick: { in: kick.out, db: db(-3) },")), `typing −3 writes kick: { in: kick.out, db: db(-3) } (${read("song.ts").match(/kick: .*,/)?.[0]})`);
await undo();
await songIs(orig.song);

// Alt-click: 0 dB.
const hb = await box(`${fader("hats")} [data-testid=fader-travel]`);
await page.keyboard.down("Alt");
await page.mouse.click(hb.x + hb.width / 2, hb.y + 10);
await page.keyboard.up("Alt");
check(await songIs(entry("db: db(-6)", "db: db(0)")), "Alt-click puts Hats at 0 dB");
await undo();
await songIs(orig.song);

// The foot: −∞.
await dragBy(thumb("hats"), -120);
check(await songIs(entry("db: db(-6)", "db: db(-96)")), "dragged into the foot, it writes −96 dB: std's silence");
check(await until(async () => (await faderOf("hats")).level === "−∞"), `and reads −∞ (${(await faderOf("hats")).level})`);
await undo();
await songIs(orig.song);

// The wheel: up and down only, 0.1 dB a notch, one edit (lead decision T6).
const sets = sent("set");
const kb = await box(thumb("keys"));
await page.mouse.move(kb.x + kb.width / 2, kb.y + kb.height / 2);
for (let i = 0; i < 3; i++) await page.mouse.wheel({ deltaY: -40 });
check(await songIs(entry("db: db(-5)", "db: db(-4.7)")), "three notches up move Keys from −5 to −4.7 dB");
await sleep(700);
check(sent("set") === sets + 1, `as one write (${sent("set") - sets})`);
await undo();
check(await songIs(orig.song), "and one undo");
await page.mouse.wheel({ deltaX: 120 });
await sleep(700);
check(sent("set") === sets + 1 && (await faderOf("keys")).value === -5, "a sideways scroll over a fader doesn't move it");

// The keys: ↑ when focused, a notch below −60 dB is −∞.
const sets1 = sent("set");
await page.focus(thumb("kick"));
await key("ArrowUp");
await key("ArrowUp");
check(await songIs(entry("kick: kick.out,", "kick: { in: kick.out, db: db(0.2) },")) && sent("set") === sets1 + 1, "↑ twice on a focused fader: +0.2 dB, as one write");
await undo();
await songIs(orig.song);
await page.click(level, { count: 2 });
await page.waitForSelector(typeIn);
await page.keyboard.type("-60");
await page.keyboard.press("Enter");
await songIs(entry("kick: kick.out,", "kick: { in: kick.out, db: db(-60) },"));
const kt = await box(thumb("kick"));
await page.mouse.move(kt.x + kt.width / 2, kt.y + kt.height / 2);
await page.mouse.wheel({ deltaY: 40 });
check(await songIs(entry("kick: kick.out,", "kick: { in: kick.out, db: db(-96) },")), "one notch down from −60 dB is −∞");
await undo();
await undo();
await songIs(orig.song);
// Tab: in a strip, its pan, then its fader, then the next strip's pan.
await page.$eval(`${pan("kick")} svg`, (e) => e.focus());
const focused = [];
for (let i = 0; i < 2; i++) {
  await key("Tab");
  focused.push(await page.evaluate(() => { const e = document.activeElement; return `${e?.closest("[data-strip]")?.dataset.strip}:${e?.closest("[data-knob]")?.dataset.knob}`; }));
}
check(same(focused, ["kick:kick.db", "hats:hats.pan"]), `Tab goes from Kick's pan to its fader, then to Hats' pan: ${focused}`);

// ---------------------------------------------------------------- the first pan (design-8b §5.3)

const inserts = sent("mix/insert");
const kp = await box(`${pan("kick")} svg`);
const px = kp.x + kp.width / 2, py = kp.y + kp.height / 2;
await page.mouse.move(px, py);
await page.mouse.down();
await page.mouse.move(px, py - 3, { steps: 3 });
await sleep(400);
check(sent("mix/insert") === inserts && read("song.ts") === orig.song, "3 px into a drag of Kick's pan, nothing is written");
await page.mouse.move(px, py - 6, { steps: 2 });
check(await songIs(entry("kick: kick.out,", "kick: { in: kick.out, pan: 0 },")), "past 4 px, `pan: 0` is written into Kick's entry");
await page.mouse.move(px, py - 20, { steps: 6 });
check(await until(async () => (await state()).leases?.length === 1), "and the drag goes on, heard, as a lease on the new pan");
await page.mouse.up();
const panned = /kick: \{ in: kick\.out, pan: (-?[\d.]+) \},/;
check(await until(() => Number(read("song.ts").match(panned)?.[1]) > 0.1), `let go, its value is written: ${read("song.ts").match(/kick: .*,/)?.[0]}`);
check(sent("mix/insert") === inserts + 1 && (await toasts()).length === 0, "one insert, and no toast");
check(await until(async () => { const p = await panOf("kick"); return !p.dflt && /^R \d+$/.test(p.text); }), `Kick's pan reads ${(await panOf("kick")).text}, written`);
await undo();
check(await songIs(entry("kick: kick.out,", "kick: { in: kick.out, pan: 0 },")), "⌘Z puts the pan back at the centre");
await undo();
check(await songIs(orig.song), "a second ⌘Z takes `pan: 0` out: `kick: kick.out` again");
check(await until(async () => (await panOf("kick")).dflt), "and the pan reads C in grey again");

// ---------------------------------------------------------------- Master (MX-5)

await dragBy(thumb("master"), -20);
const mastered = /\{ db: (-[\d.]+) \}/;
check(await until(() => mastered.test(read("song.ts"))), `the master's fader writes master(…, { db: … }): ${read("song.ts").split("\n").slice(-6).join(" ")}`);
const masterDb = Number(read("song.ts").match(mastered)?.[1]);
check(masterDb === dbAt(0.75 - 20 / room), `as a plain number, on the fader's law: ${masterDb}`);
await undo();
check(await songIs(orig.song), "undone");

// ---------------------------------------------------------------- track headers (taste.md §3.4)

check(!(await page.$('[data-track="hats"] [data-knob], [data-track="hats"] [role=slider], [data-track="hats"] input[type=range]')), "a track's header has no level slider");
check((await post("/label", { session: (await state()).session, module: "hats", label: "Shakers & Hats" })).status === 200, "Hats is called Shakers & Hats");
await until(async () => (await page.$eval('[data-track="hats"] [data-testid=track-label]', (e) => e.textContent)) === "Shakers & Hats");
const fits = await page.$eval('[data-track="hats"] [data-testid=track-label]', (e) => e.scrollWidth <= e.clientWidth);
check(fits, "and its name fits its header");
check(await until(async () => (await page.$eval(`${strip("hats")} [data-testid=strip-name]`, (e) => e.title)).startsWith("Shakers & Hats")), "the strip's name, cut short, is whole in its tooltip");
await undo();

// ---------------------------------------------------------------- selection and the strip's menu (§3.7)

await page.click(`${strip("bass")} [data-testid=strip-name]`);
check(await until(async () => !!(await page.$(`${strip("bass")}[data-selected]`)) && (await page.$eval("[data-testid=module-title]", (e) => e.textContent)) === "Bass"), "clicking a strip selects its track");
check(!!(await page.$('[data-view-tab="mix"][aria-selected="true"]')), "and the dock stays on Mix");
await page.click(`${strip("keys")} [data-testid=strip-name]`, { button: "right" });
const items = await page.waitForSelector("[data-testid=strip-menu]", { timeout: 2000 }).then(() => page.$$eval("[data-testid=strip-menu] [role=menuitem]", (els) => els.map((e) => e.textContent))).catch(() => []);
check(items.length === 3 && items[0] === "Rename" && items[1].startsWith("Colour") && items[2].startsWith("Edit in code"), `a right-click opens its menu: ${items}`);
await page.click("[data-testid=strip-menu] [data-testid=colour-row]");
await page.click('[data-testid=colours] [data-colour="green"]');
const colour = (k) => { try { return JSON.parse(read("layout.json")).tracks[k]?.colour; } catch { return null; } };
check(await until(() => colour("keys") === "green"), "Colour ▸ green recolours Keys (layout.json)");
check(await until(async () => (await page.$eval(strip("keys"), (e) => getComputedStyle(e.firstElementChild).backgroundColor)) === "rgb(124, 180, 138)"), "and its strip");
await undo();
await page.click(`${strip("keys")} [data-testid=strip-name]`, { button: "right" });
await page.waitForSelector("[data-testid=strip-menu]");
await page.click("[data-testid=strip-menu] [role=menuitem]");
const field = await page.waitForSelector(`${strip("keys")} [data-testid=track-rename]`, { timeout: 2000 }).catch(() => null);
check(!!field, "Rename turns the strip's name into a field");
await page.keyboard.type("Pads");
await page.keyboard.press("Enter");
check(await until(() => read("tracks/keys.ts").includes('{ name: "Pads" }')), "Enter writes the name into keys.ts");
await undo();
check(await until(() => read("tracks/keys.ts") === orig.keys), "undone");
await page.click(`${strip("keys")} [data-testid=strip-name]`, { button: "right" });
await page.waitForSelector("[data-testid=strip-menu]");
await page.click("[data-testid=strip-menu] [role=menuitem]:last-child");
const line = orig.song.split("\n").findIndex((l) => l.includes("keys: { in: keys.out")) + 1;
check(
  await until(async () =>
    (await page.$eval("[data-testid=code] [data-file][aria-current=true]", (e) => e.dataset.file).catch(() => null)) === "song.ts" &&
    (await page.$eval("[data-testid=code] .active-line-number", (e) => Number(e.textContent)).catch(() => null)) === line),
  `Edit in code opens song.ts at Keys' channel (line ${line})`,
);
// The code editor has the keys now, ⌥3 included (taste.md §5.3): back by the tab.
await view("mix");
await page.waitForSelector(strip("kick"));

// ---------------------------------------------------------------- a track in no channel

// The keys only drive the bass's duck (as mix_tests.rs has it), so they reach no channel.
write(
  "song.ts",
  orig.song
    .replace('import { duck } from "std/fx";', 'import { follow } from "std/dsp";\nimport { duck } from "std/fx";')
    .replace("      keys: { in: keys.out, db: db(-5), pan: -0.25 },\n", "")
    .replace("duck(bass.out, kick.env,", "duck(bass.out, follow(keys.out),"),
);
check(await until(async () => (await page.$eval(strip("keys"), (e) => e.textContent).catch(() => "")).includes("Not in the mixer")), "a track in no channel gets a strip that says so");
check(!!(await page.$(`${strip("keys")} [data-testid=no-fader] button`)) && !(await page.$(fader("keys"))), "with Edit in code, and no fader");
write("song.ts", orig.song);
await until(async () => !!(await page.$(fader("keys"))));

// ---------------------------------------------------------------- the strips scroll sideways (T6)

await page.setViewport({ width: 380, height: 900 });
await sleep(300);
const scroller = "[data-testid=mix-strips]";
const sb = await page.$eval(scroller, (e) => ({ over: e.scrollWidth > e.clientWidth, left: e.scrollLeft }));
const kt2 = await box(thumb("kick"));
await page.mouse.move(kt2.x + kt2.width / 2, kt2.y + kt2.height / 2);
const sets2 = sent("set");
await page.mouse.wheel({ deltaX: 80 });
await sleep(700);
const sa = await page.$eval(scroller, (e) => e.scrollLeft);
check(sb.over && sa > sb.left && sent("set") === sets2 && read("song.ts") === orig.song, `in a narrow dock a sideways scroll over a fader moves the strips (${sb.left} → ${sa}), not the fader`);
await page.setViewport({ width: 1440, height: 900 });
await sleep(300);

// ---------------------------------------------------------------- a read-only window (WRITE-17)

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
check(opened.status === 200 && opened.body.read_only?.why === "held", "opened while another program holds it: read-only");
await page.waitForSelector(fader("kick"), { timeout: 8000 }).catch(() => {});
const ro = await faderOf("kick").catch(() => null);
const roTitle = await page.$eval(`${fader("kick")} > div`, (e) => e.title).catch(() => "");
check(!!ro && !ro.editable && ro.level === "0.0" && roTitle.startsWith("Read only"), `the faders show their levels and say why they can't move: ${roTitle}`);
const writes = posts.length;
await dragBy(thumb("kick"), -30);
await dragBy(`${pan("kick")} svg`, 20);
await page.click(level, { count: 2 });
await sleep(500);
check(posts.slice(writes).every((p) => !/preview|set|mix\/insert/.test(p.url)) && read("song.ts") === orig.song && !(await page.$(typeIn)), "a fader drag, a pan drag and a double-click change nothing");
holder.stdin.end();
await new Promise((r) => holder.once("exit", r));

done(errors);
