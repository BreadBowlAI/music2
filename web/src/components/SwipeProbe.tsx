import clsx from "clsx";
import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { chord } from "../lib/native";
import { asks, closeProbe, counts, enter, go, listen, MODELS, putBack, RATES, report, startAgain, STEPS, useProbe, type Mac } from "../lib/probe";
import { Button, keepFocus } from "./ui";

/** How often its counts change. */
const EVERY = 250;
/** Its foot, clear of the Graph's zoom bar (32 px from the dock's foot, 28 tall) and 8 px over it. */
const ABOVE_ZOOM_BAR = 68;

const ROUNDS = {
  A: { title: "Let the app scroll as it does today", more: "music2 only listens." },
  B: { title: "Cancel the swipe", more: "music2 stops the window's own scrolling here and moves the view itself, as the camera will." },
};
const WHERE = {
  timeline: "Over the timeline (the tracks at the top):",
  Graph: "Over the Graph (the panel at the bottom), on its dotted background, not over a box:",
};

/**
 * The swipe probe's panel (taste.md VIEW-10), from the command palette: what it is, a few questions
 * about the Mac, then plain steps for two rounds of swipes over the timeline and the Graph, what it
 * has heard, and Copy report. At the window's lower right, above the Graph's zoom bar: clear of the
 * Graph's top left, where its nodes start. It listens only while a round's steps show, makes room
 * for each step's view, and puts back what it changed when it stops.
 */
export default function SwipeProbe() {
  const { open, page, round, step, room } = useProbe();
  const listening = open && page === "steps";
  useEffect(() => (listening ? listen(round) : undefined), [listening, round]);
  useEffect(() => (listening ? () => putBack() : undefined), [listening]);
  useEffect(() => void (listening && enter(round, step)), [listening, round, step]);

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
  // A report shown for ⌘C goes when the step changes: it would say less than the probe has heard.
  useEffect(() => setCopied("no"), [open, page, round, step]);

  if (!open) return null;
  const s = STEPS[step];
  const at = `${round}${step + 1}`;
  const asking = page === "steps" && asks(s);
  return (
    <div
      role="dialog"
      aria-label="Swipe probe"
      data-testid="swipe-probe"
      data-at={page === "steps" ? at : undefined}
      data-room={page === "steps" ? room : undefined}
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
          <p>It measures what your trackpad sends to music2, so the timeline and the Graph can move as your fingers do. It takes about ten minutes, and it changes nothing in your song.</p>
          <p className="text-fg-muted">Stop the song first. Any song works; a big one shows more. If you build music2 from its code, you can make a big test song: in Terminal, in the music2 folder, type</p>
          <div data-code className="px-2 py-1 rounded-sm bg-chrome font-mono text-xs text-fg select-text cursor-text">
            node e2e/big-song.mjs ~/Music/big-song
          </div>
          <p className="text-fg-muted">then open that folder ({chord("O")}). Otherwise use your biggest song.</p>
          <div className="flex justify-end">
            <Button variant="primary" data-probe="start" onMouseDown={keepFocus} onClick={() => useProbe.setState({ page: "mac" })}>
              Start
            </Button>
          </div>
        </>
      )}

      {page === "mac" && <AboutMac />}

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
            <span className="font-semibold">{WHERE[s.view]}</span> {s.how}
          </p>
          {!room.startsWith(`${at} `) && <p className="text-xs text-fg-muted">Making room for it to move…</p>}
          {asking && (
            <p className="font-medium">
              Did the scrollbar show while the view moved?
              {step === 0 && <span className="font-normal text-fg-muted"> The scrollbar is the thin bar at the right or bottom edge that shows where you are.</span>}
            </p>
          )}
          <div className="flex items-center gap-2">
            <Button variant="secondary" data-probe="back" onMouseDown={keepFocus} onClick={() => go(-1)}>
              Back
            </Button>
            <span className="flex-1" />
            {asking ? (
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

/**
 * About the Mac: what the page can't see (its user agent always says macOS 10.15, and the web view
 * may hold the page at 60 frames a second on a 120 Hz screen). Each answer may be left out.
 */
function AboutMac() {
  const mac = useProbe((s) => s.mac);
  const set = (p: Partial<Mac>) => useProbe.setState((s) => ({ mac: { ...s.mac, ...p } }));
  const field = "h-ctl px-2 rounded-sm border border-line-strong bg-raised text-sm text-fg outline-none focus:border-accent placeholder:text-fg-off";
  return (
    <>
      <div>
        <div className="font-semibold">About your Mac</div>
        <div className="text-xs text-fg-muted">The app can't see these, and they decide how smooth it can be.</div>
      </div>
      <Choices label="Which Mac is it?" options={MODELS} value={mac.model} data="data-mac-model" onPick={(model) => set({ model })} />
      <input data-mac="more" value={mac.more} onChange={(e) => set({ more: e.target.value })} placeholder="Its name or year, if you like" aria-label="More about the Mac" spellCheck={false} className={field} />
      <label className="flex flex-col gap-1">
        <span className="font-medium">
          Which macOS? <span className="font-normal text-fg-muted">Apple menu ▸ About This Mac shows it.</span>
        </span>
        <input data-mac="macos" value={mac.macos} onChange={(e) => set({ macos: e.target.value })} placeholder="15.1" spellCheck={false} className={field} />
      </label>
      <Choices label="Its screen's refresh rate? System Settings ▸ Displays ▸ Refresh rate shows it." options={RATES} value={mac.rate} data="data-mac-rate" onPick={(rate) => set({ rate })} />
      <div className="flex items-center gap-2">
        <Button variant="secondary" data-probe="back" onMouseDown={keepFocus} onClick={() => useProbe.setState({ page: "intro" })}>
          Back
        </Button>
        <span className="flex-1" />
        <Button variant="primary" data-probe="next" onMouseDown={keepFocus} onClick={() => useProbe.setState({ page: "steps", round: "A", step: 0 })}>
          Next
        </Button>
      </div>
    </>
  );
}

/** One answer of a few, as a column of radio rows. */
function Choices({ label, options, value, data, onPick }: { label: string; options: string[]; value: string | null; data: string; onPick: (v: string) => void }) {
  return (
    <div role="radiogroup" aria-label={label} className="flex flex-col">
      <span className="mb-1 font-medium">{label}</span>
      {options.map((o) => (
        <button
          key={o}
          type="button"
          role="radio"
          aria-checked={value === o}
          {...{ [data]: o }}
          onMouseDown={keepFocus}
          onClick={() => onPick(o)}
          className={clsx("h-ctl-sm px-2 rounded-sm flex items-center gap-2 text-left cursor-pointer", value === o ? "bg-accent-soft text-accent-strong" : "hover:bg-hover")}
        >
          <span className={clsx("size-2.5 shrink-0 rounded-full border", value === o ? "border-accent bg-accent" : "border-line-strong")} />
          <span className="truncate">{o}</span>
        </button>
      ))}
    </div>
  );
}
