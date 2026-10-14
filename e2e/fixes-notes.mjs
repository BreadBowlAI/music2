// The notes-side fixes: the open clip follows its clip or closes when it is gone, piano-roll writes
// name the revision their notes came from, the roll is read only while the code is broken and a
// refused edit leaves nothing stale selected, and Enter in a field doesn't jump playback.
//   node fixes-notes.mjs <root> <home> <project>
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, posts, api, state, toasts, key, box, drag } = await open();
const FILE = "data/bass/notes.json";
const PPB = 24;
const spans = () => JSON.parse(read(FILE)).clips.map((c) => [c.start, c.length, c.notes.length]);
const header = () => page.$eval("[data-testid=notes] header", (e) => e.textContent).catch(() => null);
const rollOpen = async () => !!(await page.$("[data-testid=notes] header"));
const viewTab = () => page.$eval("[data-view-tab][aria-selected=true]", (e) => e.dataset.viewTab);
const lane = async () => box('[data-lane="bass.notes"] > div:last-child');
const atBeat = async (beat) => {
  const l = await lane();
  return { x: l.x + beat * PPB, y: l.y + l.height / 2 };
};
const undoButton = async () => {
  await page.click('button[aria-label="Undo"]');
};

// ---------------------------------------------------------------- the open clip follows its clip, or closes

let p = await atBeat(37);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[36,4,0]]"), "double-clicking empty space at bar 10 makes a clip");
check(await until(async () => /· bar 10/.test(await header())), `and opens it: ${await header()}`);
p = await atBeat(45);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[36,4,0],[44,4,0]]"), "another at bar 12");
check(await until(async () => /· bar 12/.test(await header())), `which opens: ${await header()}`);

// Clip 2 open, deleted from the timeline: the roll closes rather than showing clip 3 in its place.
let c = await box('[data-lane="bass.notes"] :nth-child(2 of [data-clip])');
await page.mouse.click(c.x + 20, c.y + c.height / 2, { count: 2 });
check(await until(async () => /· bar 10/.test(await header())), "clip 2 opens again");
await page.mouse.click(c.x + 20, c.y + c.height / 2);
await page.keyboard.press("Delete");
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[44,4,0]]"), "Delete removes it");
check(await until(async () => !(await rollOpen()) && (await viewTab()) === "graph"), "and the roll closes, back to the graph");
check(!(await page.$('[data-lane="bass.notes"] [data-clip][data-open]')), "no clip is outlined as open");
await undoButton();
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[36,4,0],[44,4,0]]"), "Undo brings it back");

// A clip made and opened, then undone: the roll closes rather than open another.
p = await atBeat(48.5);
await page.mouse.click(p.x, p.y, { count: 2 });
check(await until(async () => /· bar 13/.test(await header())), "a clip made at bar 13 opens");
await undoButton();
check(await until(() => spans().length === 3), "Undo takes it away");
check(await until(async () => !(await rollOpen())), `and the roll closes (${await header()})`);

// A clip with a note, moved in the timeline and back by undo: the roll follows it both ways.
c = await box('[data-lane="bass.notes"] :nth-child(3 of [data-clip])');
await page.mouse.click(c.x + 20, c.y + c.height / 2, { count: 2 });
check(await until(async () => /· bar 12/.test(await header())), "clip 3 opens");
await page.waitForSelector("[data-testid=roll-grid] > div.border-l-2");
await sleep(200);
await page.$eval("[data-testid=roll-scroller]", (el) => { el.scrollTop = 26 + (127 - 60) * 16 - el.clientHeight / 2; });
await sleep(100);
/** The roll's pixels per beat: where the clip's end is drawn, over its length. */
const rollPpb = () =>
  page.evaluate(() => {
    const g = document.querySelector("[data-testid=roll-grid]").getBoundingClientRect();
    const end = document.querySelector("[data-testid=roll-grid] > div.border-l-2").getBoundingClientRect();
    const bars = Number(document.querySelector('[data-testid=notes] input[aria-label="Clip length in bars"]').value);
    return (end.left - g.left) / (bars * 4);
  });
const grid = await box("[data-testid=roll-grid]");
const ppb = await rollPpb();
await page.mouse.click(grid.x + 1.1 * ppb, grid.y + (127 - 60) * 16 + 8);
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[36,4,0],[44,4,1]]"), "a note added to it");
c = await box('[data-lane="bass.notes"] :nth-child(3 of [data-clip])');
await drag({ x: c.x + 20, y: c.y + c.height / 2 }, { x: c.x + 20 - 12 * PPB, y: c.y + c.height / 2 }, 16);
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[32,4,1],[36,4,0]]"), "dragged to bar 9, it sorts second");
check(await until(async () => /· bar 9/.test(await header())), `the roll follows it: ${await header()}`);
await undoButton();
check(await until(() => JSON.stringify(spans()) === "[[0,32,48],[36,4,0],[44,4,1]]"), "Undo puts it back at bar 12");
check(await until(async () => /· bar 12/.test(await header()) && (await page.$$("[data-testid=notes] [data-note]")).length === 1), `the roll follows it back: ${await header()}`);
await page.screenshot({ path: `${ROOT}/fix-roll-follow.png` });

// ---------------------------------------------------------------- writes name what their notes came from

c = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(c.x + 60, c.y + c.height / 2, { count: 2 });
check(await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length === 48), "the first clip opens with its 48 notes");
const first = await page.$('[data-testid=notes] [data-note="0"]');
await first.evaluate((e) => e.scrollIntoView({ block: "center", inline: "center" }));
const n = await first.boundingBox();
const before = JSON.parse(read(FILE)).clips[0].notes[0];
// Mid-drag, the lane changes on disk (an agent, another editor).
await page.mouse.move(n.x + 3, n.y + n.height / 2);
await page.mouse.down();
await page.mouse.move(n.x + 3 + 60, n.y + n.height / 2 - 32, { steps: 8 });
const external = JSON.parse(read(FILE));
external.clips[0].notes.at(-1).velocity = 0.3;
const revision = (await state()).view.revision;
const text = `${JSON.stringify(external, null, 1)}\n`;
write(FILE, text);
check(await until(async () => (await state()).view.revision > revision), "the change on disk is taken in during the drag");
await sleep(400);
posts.length = 0;
await page.mouse.move(n.x + 3 + 64, n.y + n.height / 2 - 32, { steps: 2 });
await page.mouse.up();
check(await until(async () => (await toasts()).some((t) => /changed since/.test(t))), `the drag's write is refused: ${(await toasts()).at(-1)}`);
check(posts.some((p) => p.url === "/api/graph/lane" && JSON.parse(p.body).revision === revision), "it named the revision the drag started from");
check(read(FILE) === text, "the change on disk is kept");
check(JSON.stringify(JSON.parse(read(FILE)).clips[0].notes[0]) === JSON.stringify(before), "the note didn't move");
check(await until(async () => (await page.$$("[data-testid=notes] [data-note][data-selected]")).length === 0), "nothing stays selected after the refusal");

// ---------------------------------------------------------------- read only while the code is broken

const bass = read("tracks/bass.ts");
write("tracks/bass.ts", `${bass}\n(`);
check(await until(() => page.$("[data-testid=notes-locked]").then(Boolean)), "while the code doesn't compile the roll says it is read only");
check(await page.$eval('[data-testid=notes] input[aria-label="Clip length in bars"]', (e) => e.disabled), "its length can't be changed");
posts.length = 0;
await page.$eval("[data-testid=roll-scroller]", (el) => { el.scrollTop = 26 + (127 - 72) * 16 - el.clientHeight / 2; el.scrollLeft = 0; });
await sleep(100);
const g2 = await box("[data-testid=roll-grid]");
const ppb2 = await rollPpb();
await page.mouse.click(g2.x + 1.1 * ppb2, g2.y + (127 - 72) * 16 + 8);
await sleep(500);
check(!posts.some((p) => p.url === "/api/graph/lane"), "a click on the grid writes nothing");
check((await page.$$("[data-testid=notes] [data-note]")).length === 48, "and adds no note");
check((await page.$$("[data-testid=notes] [data-note][data-selected]")).length === 0, "nor selects one");
const onNote = await page.$('[data-testid=notes] [data-note="5"]');
await onNote.evaluate((e) => e.scrollIntoView({ block: "center", inline: "center" }));
const nb = await onNote.boundingBox();
await drag({ x: nb.x + 3, y: nb.y + nb.height / 2 }, { x: nb.x + 63, y: nb.y + nb.height / 2 - 32 }, 8);
await page.keyboard.press("Delete");
await sleep(500);
check(!posts.some((p) => p.url === "/api/graph/lane"), "dragging a note or pressing Delete writes nothing either");
await page.screenshot({ path: `${ROOT}/fix-roll-locked.png` });
write("tracks/bass.ts", bass);
check(await until(async () => !(await page.$("[data-testid=notes-locked]"))), "once it compiles, the roll is editable again");

// ---------------------------------------------------------------- Enter in the length field while playing

await api("/transport", { method: "POST", body: JSON.stringify({ action: "play" }) });
check(await until(async () => (await api("/status")).body.seconds > 1.2, 6000), "playing");
posts.length = 0;
await page.click('[data-testid=notes] input[aria-label="Clip length in bars"]');
await page.keyboard.type("9");
await page.keyboard.press("Enter");
check(await until(() => JSON.parse(read(FILE)).clips[0].length === 36), "Enter in the length field makes the clip 9 bars");
await sleep(300);
const status = (await api("/status")).body;
check(!posts.some((p) => p.url === "/api/graph/seek"), `and doesn't seek (${posts.map((p) => p.url).join(", ")})`);
check(status.playing && status.seconds > 1.2, `playback went on: ${status.seconds.toFixed(2)} s`);
await api("/transport", { method: "POST", body: JSON.stringify({ action: "stop" }) });

await browser.close();
done(errors);
