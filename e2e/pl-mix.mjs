// Polish A1 and A2: M, S and a meter on every Mix strip (taste.md §2.8, §2.9, §3.5; design-8b §2.3,
// §6.7; the polish plan's Q1 to Q4, X23, X36). M and S on a track's strip, centred, as in its
// header and the same state; a click, ⌥-click on S, a drag across strips; Space still plays after a
// click; "still drives Duck"; none on Master. A strip of two tracks ("Kick + Hats"): M and S act on
// both, a dash when they differ, ⌥-click solos exactly the two. The strip menu's Mute in song, one
// undo step, and M on a strip muted in the song opening that menu. Then the meters: two bars on
// every track strip and on Master, the size each is; while first-song plays, Hats' strip meter reads
// as its header meter does (the same tap, before the fader), also with its fader at −30 dB; the
// floor after a stop; a clip dot when Hats goes over the top, cleared by a click, and none on
// Master. A PC page's words, and the lint in each new state.
//
//   node e2e/pl-mix.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, ROOT, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, state, toasts, key, box } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const orig = { song: read("song.ts"), kick: read("tracks/kick.ts"), hats: read("tracks/hats.ts") };
const FLOOR = -60;

const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
const sm = (id) => `${strip(id)} [data-strip-mute]`;
const ss = (id) => `${strip(id)} [data-strip-solo]`;
const pressed = (sel) => page.$eval(sel, (e) => e.getAttribute("aria-pressed")).catch(() => null);
const overlays = async () => (await state()).overlays;
/** Wait until the server's sets are `mute` and `solo` (sorted). */
const settled = (mute, solo) => until(async () => { const o = await overlays(); return same(o.mute, mute) && same(o.solo, solo); }, 5000);
const clear = async () => {
  await post("/overlay", { session: (await state()).session, clear: true });
  await settled([], []);
  await until(async () => (await pressed(sm("kick"))) === "false");
};
const mid = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });
const playing = async () => (await api("/status")).body.playing;
const playSong = async () => {
  if (!(await playing())) await page.keyboard.press("Space");
  return until(playing, 3000);
};
const stopSong = async () => {
  if (await playing()) await page.keyboard.press("Space");
  return until(async () => !(await playing()), 3000);
};
const undo = async () => {
  const rev = (await state()).view.revision;
  await key("Meta+z");
  return until(async () => (await state()).view.revision !== rev);
};
/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const menuRows = (sel) => page.waitForSelector(sel, { timeout: 2000 }).then(() => page.$$eval(`${sel} [role=menuitem]`, (els) => els.map((e) => e.textContent))).catch(() => []);
const openStripMenu = async (id) => {
  await page.click(`${strip(id)} [data-testid=strip-name]`, { button: "right" });
  return menuRows("[data-testid=strip-menu]");
};

// ---------------------------------------------------------------- M and S on the strips (A1)

await key("Alt+3");
await until(async () => !!(await page.$(strip("master"))));
const has = await page.$$eval("[data-testid=mix] [data-strip]", (els) => els.map((e) => [e.dataset.strip, !!e.querySelector("[data-strip-mute]"), !!e.querySelector("[data-strip-solo]")]));
check(has.filter(([id]) => id !== "master").every(([, m, s]) => m && s) && has.length === 5, `the Kick strip has M and S, as every track's strip does (${JSON.stringify(has)})`);
check(!(await page.$(`${strip("master")} [data-strip-mute], ${strip("master")} [data-strip-solo]`)), "Master has no M or S (MON-1)");
const placed = await page.$eval(strip("kick"), (e) => {
  const s = e.getBoundingClientRect();
  const m = e.querySelector("[data-strip-mute]").getBoundingClientRect(), o = e.querySelector("[data-strip-solo]").getBoundingClientRect();
  return { mw: m.width, mh: m.height, sw: o.width, gap: o.left - m.right, centre: (m.left + o.right) / 2 - (s.left + s.width / 2), text: e.querySelector("[data-strip-mute]").textContent };
});
check(placed.mw === 20 && placed.mh === 20 && placed.sw === 20 && placed.gap === 2 && Math.abs(placed.centre) <= 1 && placed.text === "M", `20 px each, 2 px apart, centred in the strip (${JSON.stringify(placed)})`);

await page.click(sm("kick"));
check(await settled(["kick"], []), "M on the Kick strip mutes Kick (overlays.mute is [kick])");
check(await until(async () => (await pressed('[data-testid=timeline] [data-mute="kick"]')) === "true" && (await pressed(sm("kick"))) === "true"), "and the header's M lights: the same state");
await page.mouse.move(700, 300);
const kickM = () => page.$eval(sm("kick"), (e) => ({ title: e.title, bg: getComputedStyle(e).backgroundColor }));
check(await until(async () => { const m = await kickM(); return m.title.includes("still drives Duck") && m.bg === "rgb(76, 80, 86)"; }, 2000), `a muted Kick's strip M is gray-700 and says it still drives the Duck (${JSON.stringify(await kickM())})`);
check(await playSong(), "Space plays after a click on M: the button doesn't keep the focus");
check(await stopSong(), "and Space stops");
await page.click('[data-testid=timeline] [data-mute="kick"]');
check(await settled([], []) && (await until(async () => (await pressed(sm("kick"))) === "false")), "the header's M unmutes it, and the strip's M goes off");

await page.click(ss("kick"));
await page.click(ss("hats"));
check(await settled([], ["hats", "kick"]), "S on two strips solos both");
await page.keyboard.down("Alt");
await page.click(ss("bass"));
await page.keyboard.up("Alt");
check(await settled([], ["bass"]), "⌥-click on the Bass strip's S solos only Bass");
check((await pressed(ss("kick"))) === "false" && (await until(async () => (await pressed('[data-testid=timeline] [data-solo="bass"]')) === "true")), "Kick's S goes off; Bass's header S lights");
await clear();

// A drag across three strips' M mutes the three.
const [k, h, b] = [await box(sm("kick")), await box(sm("hats")), await box(sm("bass"))];
await page.mouse.move(mid(k).x, mid(k).y);
await page.mouse.down();
await page.mouse.move(mid(h).x, mid(h).y, { steps: 6 });
await page.mouse.move(mid(b).x, mid(b).y, { steps: 6 });
await page.mouse.up();
check(await settled(["bass", "hats", "kick"], []), "a drag across three strips' M mutes the three");
await linted("[data-testid=mix]", "the Mix view with mutes on");
await page.screenshot({ path: `${ROOT}/pl-mix-mutes.png` });
await clear();

// ---------------------------------------------------------------- Mute in song from the strip's menu

const rows = await openStripMenu("kick");
check(rows.some((r) => r.startsWith("Mute in song")), `the Kick strip's menu has Mute in song (${rows})`);
await page.click("[data-testid=strip-menu] [data-testid=mute-in-song]");
check(await until(() => /muted: true/.test(read("tracks/kick.ts"))), `it writes muted: true into kick.ts (${read("tracks/kick.ts").split("\n").find((l) => l.includes("track("))})`);
check(await until(async () => (await toasts()).some((t) => t.includes("Kick muted in the song"))), `a toast says so, with Undo (${await toasts()})`);
check(await until(async () => (await page.$eval(sm("kick"), (e) => e.dataset.inSong === "true" && e.getAttribute("aria-pressed") === "true").catch(() => false))), "the strip's M lights");
await page.click(sm("kick"));
const inSongRows = await menuRows("[data-testid=strip-menu]");
check(inSongRows[0] === "Unmute in song", `M on it opens the strip's menu, Unmute in song first (${inSongRows})`);
await key("Escape");
await until(async () => !(await page.$("[data-testid=strip-menu]")));
await undo();
check(await until(() => read("tracks/kick.ts") === orig.kick), "Undo puts kick.ts back, byte for byte");

// ---------------------------------------------------------------- a strip of two tracks (Q4)

write(
  "song.ts",
  orig.song
    .replace('import { master, mixer } from "std/mix";', 'import { sum } from "std/math";\nimport { master, mixer } from "std/mix";')
    .replace("      kick: kick.out,\n      hats: { in: hats.out, db: db(-6), pan: 0.3 },\n", "      drums: sum(kick.out, hats.out),\n"),
);
check(await until(async () => (await page.$eval(`${strip("drums")} [data-testid=strip-name]`, (e) => e.textContent).catch(() => null)) === "Kick + Hats", 10000), "a channel of Kick and Hats is one strip, Kick + Hats");
await page.click(sm("drums"));
check(await settled(["hats", "kick"], []), "its M mutes both");
await page.click('[data-testid=timeline] [data-mute="hats"]');
await settled(["kick"], []);
const mixed = await until(async () => (await pressed(sm("drums"))) === "mixed");
const dash = await page.$eval(sm("drums"), (e) => e.textContent).catch(() => null);
check(mixed && dash === "–", `with Hats unmuted in its header, the strip's M is mixed, a dash (${await pressed(sm("drums"))}, ${dash})`);
await page.click(sm("drums"));
check(await settled(["hats", "kick"], []), "a click on the dash mutes both");
await page.click(ss("bass"));
await settled(["hats", "kick"], ["bass"]);
await page.keyboard.down("Alt");
await page.click(ss("drums"));
await page.keyboard.up("Alt");
check(await settled(["hats", "kick"], ["hats", "kick"]), "⌥-click on its S solos exactly Kick and Hats");
check((await pressed(ss("drums"))) === "true" && (await pressed(ss("bass"))) === "false", "its S lights; Bass's goes off");
await page.screenshot({ path: `${ROOT}/pl-mix-shared.png` });
await clear();
write("song.ts", orig.song);
await until(async () => !!(await page.$(strip("kick"))) && !(await page.$(strip("drums"))), 10000);

// ---------------------------------------------------------------- the meters (A2)

const meterOf = (id) => `${strip(id)} [role=meter]`;
const sizes = await page.$$eval("[data-testid=mix] [data-strip]", (els) =>
  els.map((e) => {
    const m = e.querySelector("[role=meter]");
    const t = e.querySelector("[data-testid=fader-travel]")?.getBoundingClientRect();
    const r = m?.getBoundingClientRect();
    return { id: e.dataset.strip, w: r?.width ?? 0, h: r ? Math.round(r.height) : 0, travel: t ? Math.round(t.height) : 0, label: m?.getAttribute("aria-label") };
  }),
);
check(sizes.find((s) => s.id === "hats")?.w === 7 && sizes.every((s) => s.w > 0), `the Hats strip has a meter, as every strip does (${sizes.map((s) => `${s.id}:${s.w}`).join(" ")})`);
check(sizes.filter((s) => s.id !== "master").every((s) => s.w === 7) && sizes.find((s) => s.id === "master").w === 11, "two bars: 2 × 3 px on a track's strip, 2 × 5 px on Master, 1 px between");
check(sizes.every((s) => s.travel - s.h <= 4 && s.h > 40), `each is the fader's travel tall (${sizes.map((s) => `${s.h}/${s.travel}`).join(" ")})`);
const titles = { hats: await page.$eval(meterOf("hats"), (e) => e.title).catch(() => ""), master: await page.$eval(meterOf("master"), (e) => e.title).catch(() => "") };
check(titles.hats === "Hats's level. Its fader doesn't change this meter." && titles.master === "The song's level", `their tooltips: "${titles.hats}", "${titles.master}"`);

/** The Hats strip's meter and the Hats header's, read at the same moment. */
const pair = () =>
  page.evaluate(() => {
    const read = (sel) => Number(document.querySelector(sel)?.getAttribute("aria-valuenow"));
    return { strip: read('[data-strip="hats"] [role=meter]'), head: read('[data-testid=timeline] [data-track="hats"] [role=meter]') };
  });
/**
 * Pairs read 50 ms apart: 10, then on until one is `loud`, for 12 s at most. first-song's hats are
 * 45 ms hits, which the meter shows for a frame or two: ten reads can all land between them, and
 * reads 150 ms apart once took 37 to find one over −24 dB.
 */
const samples = async (loud) => {
  const out = [];
  const t = Date.now();
  while (out.length < 10 || (Date.now() - t < 12000 && !out.some(loud))) {
    out.push(await pair());
    await sleep(50);
  }
  return out;
};
/** Reads for a check's words: how many, the first ten, and the loud ones. */
const told = (reads, loud) => `${reads.length} reads: ${reads.slice(0, 10).map((s) => `${s.strip}/${s.head}`).join(" ")}; loud: ${reads.filter(loud).map((s) => `${s.strip}/${s.head}`).join(" ") || "none"}`;
check(await playSong(), "first-song plays");
const sides = await until(async () => {
  const s = await page.$eval(meterOf("hats"), (e) => [Number(e.dataset.left), Number(e.dataset.right)]).catch(() => [FLOOR, FLOOR]);
  return s.every((v) => v > FLOOR);
}, 4000);
check(sides, "the Hats strip's two bars both read above the floor");
check(await until(async () => Number(await page.$eval(meterOf("master"), (e) => e.getAttribute("aria-valuenow")).catch(() => FLOOR)) > FLOOR, 3000), "Master's meter moves with the song's output");
const heard = (s) => s.strip > FLOOR;
const before = await samples(heard);
check(before.every((s) => Math.abs(s.strip - s.head) <= 1.5) && before.some(heard), `the Hats strip's meter reads as its header's, at every read (${told(before, heard)})`);
await page.click(`${strip("hats")} [data-testid=fader-level]`, { count: 2 });
await page.waitForSelector(`${strip("hats")} [data-testid=knob-type-in]`);
await page.keyboard.type("-30");
await page.keyboard.press("Enter");
check(await until(() => read("song.ts").includes("hats: { in: hats.out, db: db(-30), pan: 0.3 },")), "Hats' fader goes to −30 dB");
await sleep(600);
const loud = (s) => s.strip > -24;
const after = await samples(loud);
check(after.every((s) => Math.abs(s.strip - s.head) <= 1.5) && after.some(loud), `and its strip meter still reads as the header's: before the fader (${told(after, loud)})`);
await linted("[data-testid=mix]", "the Mix view playing");
await page.screenshot({ path: `${ROOT}/pl-mix-playing.png` });
check(await stopSong(), "Space stops");
const t0 = Date.now();
const quiet = await until(
  () =>
    page.$$eval("[data-testid=mix] [role=meter]", (els) =>
      els.every((e) => Number(e.getAttribute("aria-valuenow")) === -60 && Number(e.dataset.left) === -60 && Number(e.dataset.right) === -60),
    ),
  1000,
);
check(quiet, `after the stop every strip meter reads the floor within a second (${Date.now() - t0} ms)`);
await undo();
await until(() => read("song.ts") === orig.song);

// ---------------------------------------------------------------- the clip dot (Q3)

write("tracks/hats.ts", orig.hats.replace('import { hat } from "std/instruments";', 'import { hat } from "std/instruments";\nimport { scale } from "std/math";').replace('hat(lane.notes("hits"))', 'scale(hat(lane.notes("hits")), 8)'));
const rev = (await state()).view.revision;
await until(async () => (await state()).view.revision !== rev, 10000);
const dot = `${strip("hats")} [data-testid=clip-dot]`;
check(!(await page.$(dot)), "no clip dot on Hats before it goes over the top");
await playSong();
check(await until(async () => !!(await page.$(dot)), 6000), "Hats, 8 times louder, goes over the top: its strip's clip dot lights");
const dotInfo = await page.$eval(dot, (e) => {
  const d = e.firstElementChild.getBoundingClientRect(), m = e.closest("[data-strip]").querySelector("[role=meter]").getBoundingClientRect();
  return { title: e.title, colour: getComputedStyle(e.firstElementChild).backgroundColor, size: d.width, above: d.bottom <= m.top + 1, centred: Math.abs(d.left + d.width / 2 - (m.left + m.width / 2)) <= 1 };
}).catch(() => null);
check(dotInfo?.title === "Too loud: it went over the top. Click to clear." && dotInfo.colour === "rgb(175, 61, 54)" && dotInfo.size === 4 && dotInfo.above && dotInfo.centred, `a 4 px danger dot above the meter, with its words (${JSON.stringify(dotInfo)})`);
check(!(await page.$(`${strip("master")} [data-testid=clip-dot]`)), "Master has no clip dot: it ends in a limiter");
await stopSong();
check(!!(await page.$(dot)), "it stays lit after the stop");
await page.click(dot);
check(await until(async () => !(await page.$(dot))), "a click clears it");
write("tracks/hats.ts", orig.hats);
await until(() => read("tracks/hats.ts") === orig.hats);

// ---------------------------------------------------------------- a PC page's words

const pc = await open(1440, 900, { pc: true });
await pc.key("Alt+3");
await until(async () => !!(await pc.page.$(ss("hats"))));
const pcTitle = await pc.page.$eval(ss("hats"), (e) => e.title).catch(() => "");
check(pcTitle.includes("Alt-click: solo only this track"), `on a PC, S's tooltip says Alt-click (${pcTitle})`);
await pc.browser.close();

await browser.close();
done(errors);
