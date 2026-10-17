// The taste lint (taste.md §0) as a function, for suites that lint states of their own: run in the
// page (`page.evaluate(lint)`), it lists everything under `sel` outside the design system.
export function lint(sel = "body") {
  const SIZES = new Set([11, 12, 13, 15]);
  const RADII = new Set([0, 2, 4, 8]);
  const bad = [];
  const name = (e) => {
    const id = e.dataset?.testid ? `[data-testid=${e.dataset.testid}]` : "";
    const cls = typeof e.className === "string" && e.className ? `.${e.className.trim().split(/\s+/).slice(0, 3).join(".")}` : "";
    const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join(" ").slice(0, 24);
    return `<${e.tagName.toLowerCase()}${id}${cls}>${text ? ` "${text}"` : ""}`;
  };
  const say = (e, what) => bad.length < 400 && bad.push(`${what} at ${name(e)}`);
  for (const e of [...document.querySelectorAll(sel)].flatMap((r) => [r, ...r.querySelectorAll("*")])) {
    if (!e.getClientRects().length) continue;
    const s = getComputedStyle(e);
    const inEditor = !!e.closest(".monaco-editor");
    const texty = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) || /^(INPUT|TEXTAREA|SELECT)$/.test(e.tagName);
    if (texty && !inEditor && e.namespaceURI === "http://www.w3.org/1999/xhtml") {
      const px = parseFloat(s.fontSize);
      if (!SIZES.has(px)) say(e, `font-size ${s.fontSize}`);
    }
    if (texty && /mono|menlo|courier|sf mono/i.test(s.fontFamily) && !e.closest("[data-code]")) say(e, `mono font ${s.fontFamily.split(",")[0]}`);
    for (const corner of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"]) {
      for (const v of s[corner].split(" ")) {
        const r = parseFloat(v);
        if (v.endsWith("%") ? r !== 0 && r < 50 : !RADII.has(r) && r < 9999) {
          say(e, `border-radius ${s[corner]}`);
          break;
        }
      }
    }
    for (const pseudo of [null, "::before", "::after"]) {
      const p = pseudo ? getComputedStyle(e, pseudo) : s;
      if (/gradient/.test(p.backgroundImage)) say(e, `gradient ${pseudo ?? ""}${p.backgroundImage.slice(0, 40)}`);
      if (p.backdropFilter && p.backdropFilter !== "none") say(e, `backdrop-filter ${pseudo ?? ""}${p.backdropFilter}`);
    }
  }
  return bad;
}