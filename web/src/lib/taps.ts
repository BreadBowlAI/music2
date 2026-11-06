// Looks (TAP-12, TAP-16): what this window asks the server to look at in what plays. A box's scope,
// spectrum, level or one of its sound's meters, at its output (`out`), its input (`in`), or a meter
// by its name. One mechanism for every look: it asks only while what shows it is on screen in a
// window that shows, asks again every few seconds (the server keeps an ask 10 s), and stops when it
// goes. What it sees comes in this window's `scopes` message, beside the meters.

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
  why?: string;
}

/** A scopes message: each look this window asked for, what it saw. */
interface ScopesIn {
  t: "scopes";
  session?: number;
  scopes: Seen[];
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

/**
 * Ask for `look` while `looking` (TAP-16): again every few seconds, and a stop when it goes. Each
 * read it sees goes to `seen`; why it can't show (the ask refused, or the message's `why`) goes to
 * `why`, and null once it shows again.
 */
export function useLook(look: Look, looking: boolean, seen: (s: Seen) => void, why: (words: string | null) => void): void {
  const to = useRef({ seen, why });
  to.current = { seen, why };
  const { session, module, node, kind, at } = look;
  useEffect(() => {
    if (!looking) return;
    let alive = true;
    const ask = (on: boolean) =>
      graphApi.scope(session, module, node, on, kind, at).then(
        () => alive && on && to.current.why(null),
        (e) => alive && on && to.current.why(said(e)),
      );
    void ask(true);
    const again = setInterval(() => void ask(true), ASK_MS);
    const off = on<ScopesIn>("scopes", (msg) => {
      const mine = msg.scopes.find((s) => s.module === module && s.node === node && (s.kind ?? "scope") === kind && (s.at ?? "out") === at);
      if (!mine) return;
      if (mine.why) return to.current.why(mine.why);
      to.current.seen(mine);
    });
    return () => {
      alive = false;
      clearInterval(again);
      off();
      void ask(false).catch(() => {});
    };
  }, [session, module, node, kind, at, looking]);
}
