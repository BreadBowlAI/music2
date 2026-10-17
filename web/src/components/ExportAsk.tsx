import { useEffect } from "react";
import { lockReason } from "../lib/graph";
import { exportNow, muteThenExport, useExportAsk } from "../lib/exporting";
import { is, listed } from "../lib/overlays";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/**
 * The question before an export while mutes or solos change what you hear (EX-3, design-8b §6.6):
 * the file has every track, as the song says.
 * - Muted only while listening: [Export anyway] and the main [Mute Kick in the song, then export],
 *   which names the lasting change (one undo step). When that can't be done now (read-only, the
 *   code doesn't compile), [Cancel] and [Export anyway], and the reason in one line (M8).
 * - A solo (with mutes or not): [Cancel] and [Export anyway]: muting every other track in the song
 *   would be a strange thing to offer.
 * The main button has the focus, so Return presses it. Return and Space press the button with the
 * focus ring, wherever Tab moved it. That holds on a Mac too, where an alert's Return is always its
 * default button: here the main button writes the song's code, so it runs only when it is the one
 * shown focused. Tab goes round the buttons, and no key reaches the page behind (Space would play,
 * M would mute). Esc cancels. There is no "Don't ask again": it shows only while a mute or solo
 * changes what you hear, and a setting wouldn't outlive the app's launch (Q7).
 */
export default function ExportAsk() {
  const ask = useExportAsk((s) => s.ask);
  const graph = useStore((s) => s.graph);
  const connected = useStore((s) => s.connected);
  const close = () => useExportAsk.setState({ ask: null });
  const solo = !!ask?.soloed.length;
  // Why mute in the song can't be done now, in words.
  const why = !graph ? null : graph.read_only ? "this song is open in another window" : graph.view.draft_errors.length ? "the code doesn't compile" : lockReason(graph, connected) ? "the connection is gone" : null;
  const mute = !solo && !why;
  const anyway = () => {
    close();
    void exportNow();
  };
  const muting = () => {
    if (!ask) return;
    close();
    void muteThenExport(ask.muted, ask.mutedNames);
  };
  useEffect(() => {
    if (!ask) return;
    const key = (e: KeyboardEvent) => {
      // The question is modal: no key reaches the page behind it.
      e.stopPropagation();
      const buttons = [...document.querySelectorAll<HTMLButtonElement>("[data-testid=export-ask] button")];
      const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const n = buttons.length;
      if (e.key === "Escape") close();
      else if (e.key === "Enter") at >= 0 ? buttons[at].click() : (mute ? muting : anyway)();
      else if (e.key === "Tab") buttons[at < 0 ? (e.shiftKey ? n - 1 : 0) : (at + (e.shiftKey ? n - 1 : 1)) % n]?.focus();
      // Space on a button: the browser presses it. Anywhere else it does nothing.
      else if (e.key !== " " || at >= 0) return;
      e.preventDefault();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  });
  if (!ask) return null;
  const { mutedNames: m, soloedNames: s } = ask;
  const title = solo ? (m.length ? `${listed(m)} ${is(m)} muted and only ${listed(s)} ${is(s)} soloed` : `Only ${listed(s)} ${is(s)} soloed`) : `${listed(m)} ${is(m)} muted only while you listen`;
  const body = solo ? "The file will have every track." : m.length > 1 ? "They will be in the file." : "It will be in the file.";
  const muteLabel = `Mute ${m.length > 1 ? `${m.length} tracks` : m[0]} in the song, then export`;
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div role="alertdialog" aria-modal aria-labelledby="export-ask-title" data-testid="export-ask" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="export-ask-title" className="text-lg font-semibold text-fg-strong">
          {title}
        </h2>
        <p className="mt-2 text-base text-fg-muted">{body}</p>
        {!solo && why && <p className="mt-2 text-sm text-fg-muted" data-testid="export-ask-why">{`Can't mute in the song now: ${why}.`}</p>}
        <div className="mt-6 flex justify-end gap-2">
          {mute ? (
            <>
              <Button variant="secondary" size="lg" data-testid="export-anyway" onClick={anyway}>
                Export anyway
              </Button>
              <Button variant="primary" size="lg" autoFocus data-testid="export-mute" onClick={muting}>
                {muteLabel}
              </Button>
            </>
          ) : (
            <>
              <Button variant="secondary" size="lg" data-testid="export-cancel" onClick={close}>
                Cancel
              </Button>
              <Button variant="primary" size="lg" autoFocus data-testid="export-anyway" onClick={anyway}>
                Export anyway
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
