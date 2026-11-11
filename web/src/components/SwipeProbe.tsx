import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { chord, mod } from "../lib/native";
import { closeProbe, counts, go, listen, report, startAgain, STEPS, useProbe } from "../lib/probe";
import { Button, keepFocus } from "./ui";

/** How often its counts change. */
const EVERY = 250;
/** Its foot, clear of the Graph's zoom bar (32 px from the dock's foot, 28 tall) and 8 px over it. */
const ABOVE_ZOOM_BAR = 68;

const ROUNDS = {
  A: { title: "Let the app scroll as it does today", more: "music2 only listens." },
  B: { title: "Cancel the swipe", more: "music2 stops the browser's own scrolling here and moves the view itself, as the camera will." },
};

/**
 * The swipe probe's panel (taste.md VIEW-10), from the command palette: plain steps for two rounds
 * of swipes over the timeline and the Graph, what it has heard, and Copy report. At the window's
 * lower right, above the Graph's zoom bar: clear of the Graph's top left, where its nodes start. It
 * listens only while a round's steps show; closed, it removes every listener and keeps what it heard.
 */
export default function SwipeProbe() {
  const { open, page, round, step } = useProbe();
  useEffect(() => (open && page === "steps" ? listen(round) : undefined), [open, page, round]);

  // Its counts, written into its text a few times a second: a swipe redraws nothing.
  const said = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const show = () => {
      const el = said.current;
      if (!el) return;
      const c = counts();
      const words = `Heard ${c.wheels} swipe events, ${c.scrolls} scrolls and ${c.gestures} pinch events.`;
      if (el.textContent !== words) el.textContent = words;
      for (const [k, v] of Object.entries(c)) el.dataset[k] = String(v);
    };
    show();
    const t = window.setInterval(show, EVERY);
    return () => window.clearInterval(t);
  }, [open, page]);

  // Copy report: copied, or (where the clipboard refuses) shown in a field for ⌘C.
  const [copied, setCopied] = useState<"no" | "yes" | "field">("no");
  const [text, setText] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const copy = () => {
    const t = report();
    setText(t);
    const shown = () => setCopied("field");
    try {
      navigator.clipboard.writeText(t).then(() => setCopied("yes"), shown);
    } catch {
      shown();
    }
  };
  useEffect(() => {
    if (copied === "yes") {
      const t = window.setTimeout(() => setCopied("no"), 2000);
      return () => window.clearTimeout(t);
    }
    if (copied === "field") {
      field.current?.focus();
      field.current?.select();
    }
  }, [copied]);
  useEffect(() => setCopied("no"), [open]);

  if (!open) return null;
  const s = STEPS[step];
  const asks = page === "steps" && round === "B" && s.swipe;
  return (
    <div
      role="dialog"
      aria-label="Swipe probe"
      data-testid="swipe-probe"
      className="fixed right-3 z-[45] w-[320px] flex flex-col gap-2 p-3 rounded-md bg-raised shadow-pop text-sm text-fg cursor-default"
      style={{ bottom: ABOVE_ZOOM_BAR }}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        e.preventDefault();
        e.stopPropagation();
        closeProbe();
      }}
    >
      <div className="flex items-center gap-2">
        <span className="flex-1 font-semibold text-fg-strong">Swipe probe</span>
        <button
          type="button"
          data-probe="close"
          onMouseDown={keepFocus}
          onClick={closeProbe}
          title="Close the probe. What it heard is kept until you start again"
          aria-label="Close"
          className="size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer"
        >
          <X size={14} />
        </button>
      </div>

      {page === "intro" && (
        <>
          <p>It measures what your trackpad sends to music2, so the timeline and the Graph can move as your fingers do. It takes about five minutes, and it changes nothing in your song.</p>
          <p className="text-fg-muted">Stop the song first. It works best on a big song: in Terminal, in the music2 folder, type</p>
          <div data-code className="px-2 py-1 rounded-sm bg-chrome font-mono text-xs text-fg select-text cursor-text">
            node e2e/big-song.mjs ~/Music/big-song
          </div>
          <p className="text-fg-muted">then open that folder ({chord("O")}).</p>
          <div className="flex justify-end">
            <Button variant="primary" data-probe="start" onMouseDown={keepFocus} onClick={() => useProbe.setState({ page: "steps", round: "A", step: 0 })}>
              Start
            </Button>
          </div>
        </>
      )}

      {page === "steps" && (
        <>
          <div className="text-xs text-fg-muted">
            Round {round} of 2 · Step {step + 1} of {STEPS.length}
          </div>
          <div>
            <div className="font-semibold">{ROUNDS[round].title}</div>
            <div className="text-xs text-fg-muted">{ROUNDS[round].more}</div>
          </div>
          <p>
            <span className="font-semibold">Over the {s.view}:</span> {s.how}
          </p>
          {s.view === "Graph" && <p className="text-xs text-fg-muted">If the Graph fits in its box, zoom it in first ({mod.cmd} + scroll over it), so it has room to move.</p>}
          {asks && <p className="font-medium">Did a scrollbar show while the view moved?</p>}
          <div className="flex items-center gap-2">
            <Button variant="secondary" data-probe="back" onMouseDown={keepFocus} onClick={() => go(-1)}>
              Back
            </Button>
            <span className="flex-1" />
            {asks ? (
              <>
                <Button variant="secondary" data-probe="yes" onMouseDown={keepFocus} onClick={() => go(1, "Yes")}>
                  Yes
                </Button>
                <Button variant="secondary" data-probe="no" onMouseDown={keepFocus} onClick={() => go(1, "No")}>
                  No
                </Button>
              </>
            ) : (
              <Button variant="primary" data-probe="next" onMouseDown={keepFocus} onClick={() => go(1)}>
                Next
              </Button>
            )}
          </div>
        </>
      )}

      {page === "done" && (
        <>
          <p>Done. Copy the report, then paste it back in your chat.</p>
          <div className="flex justify-end gap-2">
            <Button variant="secondary" data-probe="again" onMouseDown={keepFocus} onClick={startAgain}>
              Start again
            </Button>
          </div>
        </>
      )}

      {copied === "field" && (
        <>
          <textarea
            ref={field}
            data-testid="probe-report"
            data-code
            readOnly
            value={text}
            aria-label="The report"
            className="h-32 p-2 rounded-sm border border-line-strong bg-raised font-mono text-xs text-fg resize-none outline-none"
          />
          <p className="text-xs text-fg-muted">The clipboard said no: press {chord("C")} to copy the report.</p>
        </>
      )}

      <div className="flex items-center gap-2 pt-2 border-t border-line">
        <div ref={said} data-testid="probe-counts" className="flex-1 text-xs text-fg-muted" />
        <Button variant={page === "done" ? "primary" : "ghost"} size="sm" data-probe="copy" onMouseDown={keepFocus} onClick={copy}>
          {copied === "yes" ? "Copied" : "Copy report"}
        </Button>
      </div>
    </div>
  );
}
