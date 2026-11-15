// Looks (TAP-12, TAP-16): what this window asks the server to look at in what plays. A box's scope,
// spectrum, level or one of its sound's meters, at its output (`out`), its input (`in`), or a meter
// by its name. One mechanism for every look: it asks only while what shows it is on screen in a
// window that shows, asks again every few seconds (the server keeps an ask 10 s), and stops when it
// goes. Parts of the window that show the same look share one ask: it starts with the first and
// stops with the last. What it sees comes in this window's `looks` message, beside the meters. It
// asks only by the ids of a view that shows the window's own last change (AGENT-2): the server judges
// a box's id by the code it plays now, and a change of the window's own may give that id another call.

import { useEffect, useRef, useState, type RefObject } from "react";
import { on } from "./events";
import { graphApi } from "./graph";
import { drawnAfterOwn, hearOwn } from "./history";
import { useStore } from "./store";

export type LookKind = "scope" | "spectrum" | "level" | "meter";

/** A look at a box: its module, its id in the view of `revision`, its kind, and its point. */
export interface Look {
  session: number;
  /** The revision of the view the part showing it was drawn from: its ids are that view's. */
  revision: number;
  module: string;
  node: string;
  kind: LookKind;
  /** `out`, `in`, or a meter's name (`reduction`). */
  at: string;
}

/** What a look saw at a read (TAP-12 to TAP-15), or why it saw nothing. */
export interface Seen {
  module: string;
  node: string;
  kind: LookKind;
  at: string;
  /** A scope's last frames, a list per channel. */
  frames?: number[][];
  /** A spectrum's bands, in dB, a twelfth of an octave each from 20 Hz. */
  bins?: number[];
  /** A level's peak and RMS per channel since the last read, linear. */
  peak?: number[];
  rms?: number[];
  /** A meter's number now, and the most since the last read, in its unit. */
  now?: number;
  most?: number;
  /** A meter's unit (`db`, `switch`), as the answer to its ask named it (TAP-15): a read carries it once that answer has come. */
  unit?: string;
  why?: string;
}

/** A looks message: each look this window asked for, what it saw. */
interface LooksIn {
  t: "looks";
  session?: number;
  looks: Seen[];
}

/** How often a look asks again: the server keeps an ask 10 s (TAP-8). */
export const ASK_MS = 4000;

/** An error's words. */
const said = (e: unknown) => (e instanceof Error ? e.message : String(e)).replace(/^edit rejected: /, "");

/** Whether `el` is on screen, in a window that shows: looking costs nothing otherwise (TAP-4). */
export function useLooking(el: RefObject<Element | null>): boolean {
  const [onScreen, setOnScreen] = useState(false);
  const [shown, setShown] = useState(() => document.visibilityState === "visible");
  useEffect(() => {
    const at = el.current;
    if (!at) return;
    const io = new IntersectionObserver((seen) => setOnScreen(seen.some((s) => s.isIntersecting)));
    io.observe(at);
    const vis = () => setShown(document.visibilityState === "visible");
    document.addEventListener("visibilitychange", vis);
    return () => {
      io.disconnect();
      document.removeEventListener("visibilitychange", vis);
    };
  }, [el]);
  return onScreen && shown;
}

/** One look this window asks for, shared by every part that shows it (TAP-16). */
interface Shared {
  seen: Set<(s: Seen) => void>;
  why: Set<(words: string | null) => void>;
  stop: () => void;
  /** A part showing it was drawn from revision `r`: an ask that waits for that may go. */
  drawn: (r: number) => void;
  /** A meter's unit, from the answer to its ask: what a window draws a switch by (CARD-73). */
  unit?: string;
}

const shared = new Map<string, Shared>();

/**
 * Show `look` to `seen` and `why` until `leave` is called: the first to show it asks, the last to go
 * stops. `drawn` says each revision the part showing it is drawn from after `look.revision`.
 */
function join(look: Look, seen: (s: Seen) => void, why: (words: string | null) => void): { leave: () => void; drawn: (r: number) => void } {
  const { session, module, node, kind, at } = look;
  const key = JSON.stringify([session, module, node, kind, at]);
  let s = shared.get(key);
  if (!s) {
    const one: Shared = { seen: new Set(), why: new Set(), stop: () => {}, drawn: () => {} };
    let alive = true;
    const tell = (words: string | null) => one.why.forEach((w) => w(words));
    const ask = (on: boolean) =>
      graphApi.look(session, module, node, on, kind, at).then(
        (a) => {
          if (on && a.look?.unit) one.unit = a.look.unit;
          return alive && on && tell(null);
        },
        (e) => alive && on && tell(said(e)),
      );
    // The newest revision a part showing it was drawn from: its id is that view's.
    let drawn = look.revision;
    // Asked for, and not sent yet: it goes once the window shows its own last change and a part
    // showing it was drawn from it (AGENT-2). Till then the id may name the call a change of the
    // window's own, on its way, gives it; a look whose box that change took out has stopped by then.
    let waiting = false;
    let asked = false;
    const may = () => useStore.getState().graph?.session === session && drawnAfterOwn(session, drawn);
    const keep = () => {
      if (!alive) return;
      waiting = !may();
      if (waiting) return;
      asked = true;
      void ask(true);
    };
    const woken = () => void (waiting && keep());
    const unhear = hearOwn(woken);
    one.drawn = (r) => {
      drawn = Math.max(drawn, r);
      woken();
    };
    keep();
    const again = setInterval(keep, ASK_MS);
    const off = on<LooksIn>("looks", (msg) => {
      const mine = msg.looks.find((x) => x.module === module && x.node === node && x.kind === kind && x.at === at);
      if (!mine) return;
      if (mine.why) return tell(mine.why);
      const read = one.unit ? { ...mine, unit: one.unit } : mine;
      one.seen.forEach((f) => f(read));
    });
    one.stop = () => {
      alive = false;
      clearInterval(again);
      unhear();
      off();
      // A look that never asked has nothing to stop.
      if (asked) void ask(false).catch(() => {});
    };
    shared.set(key, one);
    s = one;
  }
  const mine = s;
  mine.seen.add(seen);
  mine.why.add(why);
  mine.drawn(look.revision);
  return {
    leave: () => {
      mine.seen.delete(seen);
      mine.why.delete(why);
      if (mine.seen.size === 0) {
        shared.delete(key);
        mine.stop();
      }
    },
    drawn: (r) => mine.drawn(r),
  };
}

/**
 * Ask for `look` while `looking` (TAP-16): again every few seconds, and a stop when it goes, once
 * no other part of this window shows it. Each read it sees goes to `seen`; why it can't show (the
 * ask refused, or the message's `why`) goes to `why`, and null once it shows again.
 */
export function useLook(look: Look, looking: boolean, seen: (s: Seen) => void, why: (words: string | null) => void): void {
  const to = useRef({ seen, why });
  to.current = { seen, why };
  const { session, revision, module, node, kind, at } = look;
  const drawnFrom = useRef(revision);
  drawnFrom.current = revision;
  const part = useRef<ReturnType<typeof join> | null>(null);
  useEffect(() => {
    if (!looking) return;
    const p = (part.current = join(
      { session, revision: drawnFrom.current, module, node, kind, at },
      (s) => to.current.seen(s),
      (w) => to.current.why(w),
    ));
    return () => {
      part.current = null;
      p.leave();
    };
  }, [session, module, node, kind, at, looking]);
  // Drawn from a newer view, with the same id: an ask waiting for it may go.
  useEffect(() => part.current?.drawn(revision), [revision]);
}
