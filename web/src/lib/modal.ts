import { useEffect, useRef, type RefObject } from "react";

/** What Tab goes round in a dialog, in the order the page draws it. */
const CONTROLS = "button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled])";

/**
 * The keys of a modal dialog (taste.md §2.14), the same in every one (the export question, the
 * delete question, Save As, Rename Song):
 * - Return presses the button with the focus ring. In a text field or a check box, or with the
 *   focus on nothing, it runs `onDefault` (the dialog's main button). On a Mac too: the main button
 *   may write the song, so it runs only when it is the one shown focused or the field is.
 * - Space on a button or a check box presses it; in a text field it types a space; anywhere else
 *   it does nothing.
 * - Tab and ⇧Tab go round every control in the dialog, never behind it.
 * - Esc runs `onEscape`.
 * - No key reaches the page behind (Space would play, M would mute, Return would go home).
 * `root` is the dialog's box. `active` false (the dialog isn't shown) listens to nothing.
 */
export function useModalKeys(root: RefObject<HTMLElement | null>, opts: { onEscape: () => void; onDefault: () => void; active?: boolean }) {
  const latest = useRef(opts);
  latest.current = opts;
  const active = opts.active ?? true;
  useEffect(() => {
    if (!active) return;
    const key = (e: KeyboardEvent) => {
      const box = root.current;
      if (!box) return;
      // Modal: no key reaches the page. The browser's own action (typing in a field) still happens.
      e.stopPropagation();
      const controls = [...box.querySelectorAll<HTMLElement>(CONTROLS)].filter((c) => c.getClientRects().length);
      const focus = document.activeElement;
      const at = controls.indexOf(focus as HTMLElement);
      if (e.key === "Escape") latest.current.onEscape();
      else if (e.key === "Tab") {
        const n = controls.length;
        controls[at < 0 ? (e.shiftKey ? n - 1 : 0) : (at + (e.shiftKey ? n - 1 : 1)) % n]?.focus();
      } else if (e.key === "Enter") {
        if (e.isComposing) return;
        if (at >= 0 && focus instanceof HTMLButtonElement) focus.click();
        else latest.current.onDefault();
      } else if (e.key === " ") {
        // A button or a box: the browser presses it. A field: it types. Elsewhere: nothing.
        if (at >= 0) return;
      } else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [active, root]);
}
