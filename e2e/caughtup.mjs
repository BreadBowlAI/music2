// caughtUp: a wait until the page shows the revision the server plays. fixlib's open() gives it; a
// suite that opens its own page makes it with `following`.
//
// A gesture goes from the revision the page shows (`[data-revision]`), and the server refuses one from
// an older revision whose target has changed since ("the code changed around this control …", a clip's
// 409, an undo step that isn't the newest). The server writes a change's files before it publishes its
// revision, and the page reads it a moment later, so a file on disk doesn't mean the page has it.

/**
 * Follow `page`'s requests to the song, and give `caughtUp(at)`: until the page's requests are
 * answered (each is, once its revision is published), and the page shows the revision the server
 * plays: at least the newest those answers named, and at least `at`. `state()` is the server's state
 * (`GET /api/graph`); a change made elsewhere (a file written, `m2`) is the server's once that state
 * shows it, or pass its revision as `at`. A wait, not a check: only if they never meet does it
 * `check(false, …)`, saying both. Follow from the start: a request already on its way isn't counted.
 */
export function following(page, state, check, ms = 8000) {
  const toSong = (r) => r.method() !== "GET" && /\/api\/graph\//.test(r.url());
  const onTheirWay = new Set();
  const made = new Map();
  page.on("request", (r) => toSong(r) && onTheirWay.add(r));
  page.on("requestfailed", (r) => onTheirWay.delete(r));
  page.on("requestfinished", async (r) => {
    if (!onTheirWay.has(r)) return;
    try {
      const { revision } = (await r.response()?.json().catch(() => null)) ?? {};
      let session = Number(new URL(r.url()).searchParams.get("session")) || undefined;
      try {
        session ??= JSON.parse(r.postData() ?? "").session;
      } catch {}
      if (typeof revision === "number" && session !== undefined) made.set(session, Math.max(made.get(session) ?? 0, revision));
    } finally {
      onTheirWay.delete(r);
    }
  });
  return async (at = 0) => {
    let server = -1, shown = -1, want = at;
    const t = Date.now();
    while (Date.now() - t < ms) {
      if (onTheirWay.size === 0) {
        const s = await state();
        server = s?.view?.revision ?? -1;
        want = Math.max(at, made.get(s?.session) ?? 0);
        shown = await page.$eval("[data-revision]", (e) => Number(e.dataset.revision)).catch(() => -1);
        if (shown === server && shown >= want) return true;
      }
      await new Promise((r) => setTimeout(r, 80));
    }
    const way = onTheirWay.size ? `, ${onTheirWay.size} of its requests on their way` : "";
    check(false, `the page shows the revision the server plays${want ? `, ${want} or later` : ""}: it shows ${shown}, the server ${server}${way}`);
    return false;
  };
}
