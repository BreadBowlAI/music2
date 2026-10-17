// The tooltip lint (the studio plan's Q43, D19): run in the page (`page.evaluate(tips, sel)`), it
// lists each control under `sel` that needs a tooltip and has none, or whose tooltip only repeats
// the words on it. One line per control, as `what at <element> "its words"`.
//
// A tooltip is asked of:
//   - a button with no words of its own (an icon), whatever its role;
//   - a toggle (`aria-pressed`), a tab, a switch, a slider, a knob or fader (`[data-knob]`), a spin
//     button;
//   - any control named in `keys` (selector → its key as `chord()` spells it): it has a key, so its
//     tooltip names it ("Loop (C)").
// Not asked of: a button whose own words say its action and that has no key ("Done", "Cancel"), nor
// a choice of a segmented control whose words name it ("Minor");
// rows of a menu or a list (`[role=menu] *`, `[role=listbox] *`, `[role=option]`, menu items),
// whose words are the action; the code editor's insides; anything hidden.
// A control's tooltip is its own `title`, or that of an ancestor holding no other control (a knob's
// box). A tooltip never just repeats the control's words: "Export (⇧⌘E)", not "Export".
export function tips(sel = "body", keys = {}) {
  const CONTROL = "button, [role=button], [aria-pressed], [role=tab], [role=switch], [role=radio], [role=slider], [role=spinbutton], [data-knob], select";
  const SKIP = "[role=menu], [role=listbox], [role=option], [role=menuitem], [role=menuitemcheckbox], [role=menuitemradio], .monaco-editor";
  const bad = [];
  const roots = [...document.querySelectorAll(sel)];
  const all = [...new Set(roots.flatMap((r) => [...(r.matches(CONTROL) ? [r] : []), ...r.querySelectorAll(CONTROL)]))];
  const seen = (e) => {
    if (!e.getClientRects().length) return false;
    const s = getComputedStyle(e);
    return s.visibility !== "hidden" && s.opacity !== "0";
  };
  /** The words on a control: its text, but no words of an SVG icon. */
  const words = (e) =>
    [...e.querySelectorAll("*"), e]
      .flatMap((n) => [...n.childNodes])
      .filter((n) => n.nodeType === 3 && !n.parentElement.closest("svg"))
      .map((n) => n.textContent)
      .join(" ")
      .replace(/\s+/g, " ")
      .trim();
  const name = (e) => {
    const id = e.dataset?.testid ? `[data-testid=${e.dataset.testid}]` : "";
    const data = [...e.attributes].filter((a) => a.name.startsWith("data-") && a.name !== "data-testid").slice(0, 2).map((a) => `[${a.name}${a.value ? `=${a.value.slice(0, 20)}` : ""}]`).join("");
    const role = e.getAttribute("role") ? `[role=${e.getAttribute("role")}]` : "";
    const label = e.getAttribute("aria-label") ? ` "${e.getAttribute("aria-label").slice(0, 32)}"` : words(e) ? ` "${words(e).slice(0, 32)}"` : "";
    return `<${e.tagName.toLowerCase()}${id}${role}${data}>${label}`;
  };
  /** The control's own title, or the nearest ancestor's that holds no other control. */
  const tooltip = (e) => {
    for (let n = e; n && n !== document.body; n = n.parentElement) {
      if (n !== e && [...n.querySelectorAll(CONTROL)].some((c) => c !== e && !c.contains(e) && !e.contains(c) && seen(c))) return "";
      const t = n.getAttribute("title");
      if (t !== null) return t.trim();
    }
    return "";
  };
  const plain = (s) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
  const keyed = Object.entries(keys);
  for (const e of all) {
    if (!seen(e) || e.closest(SKIP)) continue;
    // A knob's or fader's box and the slider in it are one control: the box speaks for it.
    if (e.matches("[role=slider]") && e.parentElement?.closest("[data-knob]")) continue;
    const text = words(e);
    const key = keyed.find(([s]) => e.matches(s))?.[1];
    const kind = e.matches("[aria-pressed], [role=tab], [role=switch], [role=slider], [role=spinbutton], [data-knob], select");
    // A knob's or fader's tooltip may sit on its box or round the slider in it (where the pointer is).
    const inner = e.matches("[data-knob]") ? e.querySelector("[role=slider]") : null;
    const t = tooltip(e) || (inner ? tooltip(inner) : "");
    if (!t && (!text || kind || key)) bad.push(`no tooltip at ${name(e)}`);
    else if (t && text && plain(t) === plain(text) && !key) bad.push(`a tooltip that repeats its words ("${t}") at ${name(e)}`);
    else if (key && !t.includes(key)) bad.push(`a tooltip without its key ${key} ("${t}") at ${name(e)}`);
    if (bad.length >= 400) break;
  }
  return bad;
}
