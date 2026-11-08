// Looks (TAP-12, TAP-16): what this window asks the server to look at in what plays. A box's scope,
// spectrum, level or one of its sound's meters, at its output (`out`), its input (`in`), or a meter
// by its name. One mechanism for every look: it asks only while what shows it is on screen in a
// window that shows, asks again every few seconds (the server keeps an ask 10 s), and stops when it
// goes. Parts of the window that show the same look share one ask: it starts with the first and
// stops with the last. What it sees comes in this window's `looks` message, beside the meters.

import { useEffect, useRef, useState, type RefObject } from "react";
import { on } from "./events";
import { graphApi } from "./graph";

export type LookKind = "scope" | "spectrum" | "level" | "meter";

/** A look at a box: its module, its id in the view, its kind, and its point. */
export interface Look {
  session: number;
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
  /** A meter's unit, from the answer to its ask: what a window draws a switch by (CARD-73). */
  unit?: string;
}

const shared = new Map<string, Shared>();

/** Show `look` to `seen` and `why` until the function it gives back is called: the first to show it asks, the last to go stops. */
function join(look: Look, seen: (s: Seen) => void, why: (words: string | null) => void): () => void {
  const { session, module, node, kind, at } = look;
  const key = JSON.stringify([session, module, node, kind, at]);
  let s = shared.get(key);
  if (!s) {
    const one: Shared = { seen: new Set(), why: new Set(), stop: () => {} };
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
    void ask(true);
    const again = setInterval(() => void ask(true), ASK_MS);
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
      off();
      void ask(false).catch(() => {});
    };
    shared.set(key, one);
    s = one;
  }
  const mine = s;
  mine.seen.add(seen);
  mine.why.add(why);
  return () => {
    mine.seen.delete(seen);
    mine.why.delete(why);
    if (mine.seen.size === 0) {
      shared.delete(key);
      mine.stop();
    }
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
  const { session, module, node, kind, at } = look;
  useEffect(() => {
    if (!looking) return;
    return join(
      { session, module, node, kind, at },
      (s) => to.current.seen(s),
      (w) => to.current.why(w),
    );
  }, [session, module, node, kind, at, looking]);
}
