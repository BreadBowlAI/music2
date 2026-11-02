// 8b.7 Heard while dragging (design-8b §9.3, WRITE-22a): while the song plays, a clip, an
// automation point or a note dragged is heard where it is, through a lane lease: one begin, an
// update a frame, one commit when let go, which is the one write and the one undo step. Mid-drag
// the server lists the lease and nothing is written. Esc puts it back (a cancel, no write, and the
// selection stays). A window that closes mid-drag ends its lease. A clip changed elsewhere mid-drag
// ends it, and the window says why. Stopped, a drag writes on release as before, with no lease. The
// timeline mid-drag passes the taste lint. The tempo field dragged (RENDER-27a): accent while held,
// heard as it goes (the status's tempo follows), written once by the lease's commit when let go,
// one undo step; Esc puts it back. And a PC page: Esc puts a drag back there too.
//
//   node e2e/s8-leases.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, read, sleep, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, posts, api, state, caughtUp, toasts, key, box } = await open(1440, 900);
const PPB = 24;
const bassClip = '[data-lane="bass.notes"] [data-clip]';
const notesOf = () => JSON.parse(read("data/bass/notes.json")).clips;
const revision = async () => (await state()).view.revision;
const status = async () => (await api("/status")).body;
const leases = async () => (await api("/leases")).body;
const undoNow = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
/** The lease requests and the clip writes this page made since `from`, by what each was. */
const sent = (from) =>
  posts.slice(from).flatMap((p) => {
    const m = /^\/api\/graph\/(?:lane\/lease\/(\w+)|(clips))$/.exec(p.url);
    return m ? [{ what: m[1] ?? m[2], body: JSON.parse(p.body || "{}") }] : [];
  });
const count = (list, what) => list.filter((s) => s.what === what).length;
const transport = (action) => api("/transport", { method: "POST", body: JSON.stringify({ action }) });
/** Play from the start, and wait until the page's clock says so (it polls the status). */
const play = async () => {
  await api("/seek", { method: "POST", body: JSON.stringify({ beat: 0 }) });
  await transport("play");
  await until(async () => (await status()).playing);
  await sleep(500);
};
/** Press at `from`, move by `dx`,`dy` in `steps` moves a frame or two apart, and leave the button down. */
const hold = async (from, dx, dy = 0, steps = 10) => {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(from.x + (dx * i) / steps, from.y + (dy * i) / steps);
    await sleep(40);
  }
};

await page.waitForSelector(bassClip);
const id = notesOf()[0].id;
const start0 = notesOf()[0].start;

// ---------------------------------------------------------------- a clip dragged while playing
await play();
let n = posts.length;
const rev0 = await revision();
const file0 = read("data/bass/notes.json");
const c = await box(bassClip);
const grab = { x: c.x + 30, y: c.y + c.height / 2 };
await hold(grab, 4 * PPB);
await sleep(300);
const mid = sent(n);
const listed = await leases();
check(listed.some((l) => l.kind === "lane" && l.lanes?.some((x) => x.module === "bass" && x.lane === "notes")), `mid-drag the server lists a lane lease on Bass's notes: ${JSON.stringify(listed.map((l) => l.kind))}`);
check(count(mid, "begin") === 1 && count(mid, "update") >= 3, `one begin, an update a frame (${count(mid, "begin")} begin, ${count(mid, "update")} updates)`);
check(mid.find((s) => s.what === "begin")?.body.gesture?.op === "move" && mid.at(-1)?.body.gesture?.by === 4, `each the clip's move, the last by a bar: ${JSON.stringify(mid.at(-1)?.body.gesture)}`);
check((await revision()) === rev0 && read("data/bass/notes.json") === file0, "nothing written mid-drag: the same revision, the same file");
const bad = await page.evaluate(lint, "[data-testid=timeline]");
check(bad.length === 0, `the timeline mid-drag passes the taste lint ${bad.slice(0, 3).join(" | ")}`);
await page.mouse.up();
check(await until(() => notesOf()[0].start === start0 + 4), `let go, the clip is a bar later: ${notesOf()[0].start}`);
await until(() => count(sent(n), "commit") > 0, 3000);
const after = sent(n);
check(count(after, "commit") === 1 && count(after, "clips") === 0, `written by one commit of its lease, no other write (${after.map((s) => s.what).join(" ")})`);
check(await until(async () => (await revision()) === rev0 + 1), `one revision more: ${await revision()}`);
check(notesOf()[0].id === id, "the clip keeps its id");
check(await until(async () => (await undoNow()).startsWith("Undo moving a Bass clip")), `one undo step, named: ${await undoNow()}`);
check(await until(async () => (await leases()).length === 0), "the lease ended");

// ---------------------------------------------------------------- Esc puts it back
// A drag is heard (its lease begins) once the page shows the clip as the move left it: the lease
// names the clip by the hash the state gives it. The page shows the move first.
await caughtUp();
n = posts.length;
const rev1 = await revision();
const c1 = await box(bassClip);
await hold({ x: c1.x + 30, y: c1.y + c1.height / 2 }, 2 * PPB, 0, 6);
await sleep(200);
await key("Escape");
await sleep(150);
await page.mouse.up();
await until(() => count(sent(n), "cancel") > 0, 3000);
await sleep(300);
const esc = sent(n);
check(count(esc, "begin") === 1 && count(esc, "cancel") === 1 && count(esc, "commit") === 0 && count(esc, "clips") === 0, `Esc mid-drag: a cancel, no write (${esc.map((s) => s.what).join(" ")})`);
check((await revision()) === rev1 && notesOf()[0].start === start0 + 4, "no revision; the clip where it was");
check(Math.abs((await box(bassClip)).x - c1.x) < 1, `drawn where it was: ${(await box(bassClip)).x} (was ${c1.x})`);
check(await until(async () => (await leases()).length === 0), "the lease gone");
check(!!(await page.$(`${bassClip}[data-picked]`)), "the clip stays selected: Esc was the drag's alone");

// ---------------------------------------------------------------- a clip changed elsewhere mid-drag
n = posts.length;
const c2 = await box(bassClip);
await hold({ x: c2.x + 30, y: c2.y + c2.height / 2 }, 2 * PPB, 0, 6);
const outside = notesOf();
outside[0].notes[0].velocity = 0.5;
write("data/bass/notes.json", JSON.stringify({ clips: outside }, null, 2));
check(await until(async () => (await toasts()).some((t) => t.includes("Bass changed elsewhere: your drag was dropped.")), 6000), `a clip changed elsewhere mid-drag: the window says why (${(await toasts()).join(" | ")})`);
await page.mouse.up();
await sleep(400);
check(count(sent(n), "commit") === 0 && count(sent(n), "clips") === 0 && (await leases()).length === 0, "and nothing is written on release");
check(notesOf()[0].notes[0].velocity === 0.5 && notesOf()[0].start === start0 + 4, "the outside write stands");

// ---------------------------------------------------------------- a window that closes mid-drag
const two = await open(1440, 900);
await two.page.waitForSelector(bassClip);
await sleep(300);
const c3 = await two.box(bassClip);
await two.page.mouse.move(c3.x + 30, c3.y + c3.height / 2);
await two.page.mouse.down();
for (let i = 1; i <= 6; i++) {
  await two.page.mouse.move(c3.x + 30 + i * 8, c3.y + c3.height / 2);
  await sleep(40);
}
check(await until(async () => (await leases()).length === 1, 3000), "a second window's drag holds a lease");
await two.browser.close();
const t0 = Date.now();
check(await until(async () => (await leases()).length === 0, 1000), `it closes mid-drag: the lease is gone in ${Date.now() - t0} ms`);
check(notesOf()[0].start === start0 + 4, "and nothing was written");

// ---------------------------------------------------------------- stopped: no lease, one write on release
await transport("stop");
await until(async () => !(await status()).playing);
await sleep(400);
n = posts.length;
const c4 = await box(bassClip);
await hold({ x: c4.x + 30, y: c4.y + c4.height / 2 }, -4 * PPB);
await page.mouse.up();
check(await until(() => notesOf()[0].start === start0), `stopped, a drag still moves the clip: ${notesOf()[0].start}`);
await until(() => count(sent(n), "clips") > 0, 3000);
const stopped = sent(n);
check(count(stopped, "begin") === 0 && count(stopped, "clips") === 1, `with no lease: one clips write (${stopped.map((s) => s.what).join(" ")})`);

// ---------------------------------------------------------------- an automation point dragged while playing
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"] [data-clip] [data-point="1"]');
await play();
const cutoff0 = read("data/bass/cutoff.json");
const pointsOf = () => JSON.parse(read("data/bass/cutoff.json")).clips[0].points;
n = posts.length;
const rev2 = await revision();
const p1 = await box('[data-lane="bass.cutoff"] [data-clip] [data-point="1"]');
await hold({ x: p1.x + p1.width / 2, y: p1.y + p1.height / 2 }, -4 * PPB, 6, 10);
await sleep(300);
const pts = sent(n);
check(count(pts, "begin") === 1 && count(pts, "update") >= 3 && pts[0].body.gesture?.op === "points", `a point dragged while playing: a begin of its points, an update a frame (${count(pts, "begin")}, ${count(pts, "update")})`);
check(read("data/bass/cutoff.json") === cutoff0 && (await revision()) === rev2, "nothing written mid-drag");
await page.mouse.up();
check(await until(() => pointsOf()[1]?.[0] === 12), `let go, the point is 4 beats earlier: ${JSON.stringify(pointsOf())}`);
await until(() => count(sent(n), "commit") > 0, 3000);
const pts2 = sent(n);
check(count(pts2, "commit") === 1 && count(pts2, "clips") === 0 && (await until(async () => (await revision()) === rev2 + 1)), `one commit, one revision (${pts2.map((s) => s.what).join(" ")})`);
check(await until(async () => (await undoNow()).startsWith("Undo moving a Cutoff point")), `Undo names it: ${await undoNow()}`);

// ---------------------------------------------------------------- a note dragged in Notes while playing
const cb = await box(bassClip);
await transport("stop");
await page.mouse.click(cb.x + 20, cb.y + cb.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=roll-grid] [data-note]");
await play();
n = posts.length;
const rev3 = await revision();
const notes0 = notesOf()[0].notes;
const nb = await box('[data-testid=roll-grid] [data-note="0"]');
await hold({ x: nb.x + Math.min(6, nb.width / 3), y: nb.y + nb.height / 2 }, 60, 0, 10);
await sleep(300);
const roll = sent(n);
check(count(roll, "begin") === 1 && count(roll, "update") >= 3 && roll[0].body.gesture?.op === "notes", `a note dragged while playing: a begin of the clip's notes, an update a frame (${count(roll, "begin")}, ${count(roll, "update")})`);
check((await revision()) === rev3 && JSON.stringify(notesOf()[0].notes) === JSON.stringify(notes0), "nothing written mid-drag");
await page.mouse.up();
check(await until(() => JSON.stringify(notesOf()[0].notes) !== JSON.stringify(notes0)), "let go, the note moves");
// The page's request events can come after the server answered: wait for the commit's.
await until(() => count(sent(n), "commit") > 0, 3000);
const roll2 = sent(n);
check(count(roll2, "commit") === 1 && roll2.find((s) => s.what === "commit")?.body.gesture?.op === "notes", `written by one commit with op "notes" (${roll2.map((s) => s.what).join(" ")})`);
check(!posts.slice(n).some((p) => p.url === "/api/graph/lane"), "not by the roll's lane write");
check(await until(async () => (await revision()) === rev3 + 1), "one revision");
check(await until(async () => (await undoNow()).startsWith("Undo Bass notes")), `the undo step is "Bass notes": ${await undoNow()}`);
await transport("stop");

// ---------------------------------------------------------------- the tempo field dragged (RENDER-27a)
const tempoOf = () => JSON.parse(read("data/arrangement.json")).tempo[0].bpm;
const tempo0 = tempoOf();
const number = "[data-testid=tempo-field] span.tabular-nums";
/** The number's colour, and the accent's, as the page computes them. */
const colours = () =>
  page.evaluate((sel) => {
    const probe = document.createElement("span");
    probe.style.color = "var(--color-accent)";
    document.body.append(probe);
    const accent = getComputedStyle(probe).color;
    probe.remove();
    return { number: getComputedStyle(document.querySelector(sel)).color, accent };
  }, number);
const idle = await colours();
check(idle.number !== idle.accent, `the tempo isn't accent while not held: ${idle.number}`);
check((await page.$eval("[data-testid=tempo-field]", (e) => e.title)).includes("Heard as you drag"), "its tooltip says it is heard as you drag");
// Stopped: the time readout follows, so it is heard as it goes, playing or not.
await until(async () => !(await status()).playing);
n = posts.length;
const rev4 = await revision();
const tf = await box("[data-testid=tempo-field]");
const bpms = new Set();
await page.mouse.move(tf.x + tf.width / 2, tf.y + tf.height / 2);
await page.mouse.down();
for (let i = 1; i <= 12; i++) {
  await page.mouse.move(tf.x + tf.width / 2, tf.y + tf.height / 2 - i * 6);
  await sleep(60);
  bpms.add((await status()).bpm);
}
const held = await colours();
check(held.number === held.accent, `held, the number is accent: ${held.number}`);
check(bpms.size >= 3, `mid-drag the status's tempo follows: ${[...bpms].join(", ")}`);
check(read("data/arrangement.json").includes(String(tempo0)) && tempoOf() === tempo0 && (await revision()) === rev4, "nothing written mid-drag");
check((await leases()).some((l) => l.kind === "tempo"), "the server lists a tempo lease");
const songbar = await page.evaluate(lint, "[data-testid=tempo]");
check(songbar.length === 0, `the song bar mid-drag passes the taste lint ${songbar.slice(0, 3).join(" | ")}`);
await page.mouse.up();
check(await until(() => tempoOf() === tempo0 + 18), `let go, written: ${tempoOf()} (was ${tempo0})`);
await until(() => posts.slice(n).some((p) => p.url === "/api/graph/tempo/lease/commit"), 3000);
const tposts = posts.slice(n).map((p) => p.url);
check(tposts.filter((u) => u === "/api/graph/tempo/lease/commit").length === 1 && !tposts.includes("/api/graph/tempo"), `once, by the lease's commit (${tposts.filter((u) => !u.endsWith("/update")).join(" ")})`);
check(await until(async () => (await revision()) === rev4 + 1) && (await until(async () => (await undoNow()).startsWith("Undo Tempo"))), `one revision, one undo step: ${await undoNow()}`);
check(await until(async () => (await status()).bpm === tempo0 + 18 && (await leases()).length === 0), "the lease ended; the written tempo plays");
check(await until(async () => (await colours()).number !== idle.accent), "the number isn't accent once let go");
// Esc puts it back (playing).
await play();
n = posts.length;
const tf2 = await box("[data-testid=tempo-field]");
await page.mouse.move(tf2.x + tf2.width / 2, tf2.y + tf2.height / 2);
await page.mouse.down();
for (let i = 1; i <= 8; i++) {
  await page.mouse.move(tf2.x + tf2.width / 2, tf2.y + tf2.height / 2 + i * 6);
  await sleep(50);
}
check(await until(async () => (await status()).bpm < tempo0 + 18, 3000), `dragged down, heard: ${(await status()).bpm}`);
await key("Escape");
await sleep(100);
await page.mouse.up();
await until(() => posts.slice(n).some((p) => p.url === "/api/graph/tempo/lease/cancel"), 3000);
const escPosts = posts.slice(n).map((p) => p.url);
check(escPosts.includes("/api/graph/tempo/lease/cancel") && !escPosts.includes("/api/graph/tempo/lease/commit"), `Esc: a cancel, no write (${escPosts.filter((u) => !u.endsWith("/update")).join(" ")})`);
check(await until(async () => (await status()).bpm === tempo0 + 18), `the written tempo plays again: ${(await status()).bpm}`);
check(Number(await page.$eval("[data-testid=tempo-field]", (e) => e.dataset.bpm)) === tempo0 + 18 && tempoOf() === tempo0 + 18, "the field and the file say it");
await transport("stop");

// ---------------------------------------------------------------- a PC page: Esc puts a drag back there too
const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector(bassClip);
await pc.api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
await sleep(700);
const k = pc.posts.length;
const c5 = await pc.box(bassClip);
await pc.page.mouse.move(c5.x + 30, c5.y + c5.height / 2);
await pc.page.mouse.down();
for (let i = 1; i <= 6; i++) {
  await pc.page.mouse.move(c5.x + 30 + i * 12, c5.y + c5.height / 2);
  await sleep(40);
}
await pc.page.keyboard.press("Escape");
await sleep(150);
await pc.page.mouse.up();
await sleep(400);
const pcSent = pc.posts.slice(k).map((p) => p.url);
check(pcSent.includes("/api/graph/lane/lease/begin") && pcSent.includes("/api/graph/lane/lease/cancel") && !pcSent.includes("/api/graph/lane/lease/commit"), `on a PC page, a drag while playing leases, and Esc cancels it: ${pcSent.join(" ")}`);
check(pc.errors.length === 0, `no console errors on the PC page ${pc.errors.slice(0, 3).join(" | ")}`);
await pc.api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });
await pc.browser.close();

await browser.close();
done(errors);
