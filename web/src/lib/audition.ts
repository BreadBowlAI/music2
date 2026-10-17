// Audition (8b.6, MON-7): hear notes as you press, drag or draw them in Notes, and the keyboard's
// keys while you hold them. Each note this window holds has a handle; the server plays it on the
// lane named and lets it go on its Off. Nothing here is written: there is nothing to undo.
//
// Everything held is let go on pointer up (the views say when), and here: the window losing focus
// or being hidden, the toggle going off, ⌘. (Stop All Sound), Space with no pointer down, and the
// view going away (`attach`'s stop). When the socket closes the server lets go of the window's
// notes itself, so they are only forgotten here.

import { create } from "zustand";
import { CLOSED, on, send } from "./events";
import type { AuditionIn, AuditionOut } from "./graph8b";
import { setLocal, useLocalStore } from "./local";
import { onMenu } from "./native";
import { useStore } from "./store";

declare module "./graph8b" {
  interface AuditionOut {
    /** A take records these Ons (lane B): the keyboard piano's. */
    record?: boolean;
  }
}

/** What a key on the keyboard plays at (Q17): the server's own default. Notes play at theirs. */
export const KEY_VELOCITY = 0.8;
/** A painted note sounds for its length, or this long if that is shorter (ms). */
export const PAINT_MS = 250;
/** While anything is held, the server hears from this window this often, or lets go after 10 s. */
const KEEPALIVE_MS = 2000;
/** A lane the server said was silent stays so until a strike on it goes unanswered this long. */
const SILENT_CHECK_MS = 600;

/** A note this window holds: its lane and its pitch. */
export interface Held {
  module: string;
  lane: string;
  pitch: number;
}

interface Audition {
  /** The toggle (NT-2): on unless turned off for this song. */
  on: boolean;
  /** What this window holds now, by handle. A new map at each change, so views redraw. */
  held: ReadonlyMap<number, Held>;
  /** The lanes the server said nothing plays, as `module.lane`. */
  silent: ReadonlySet<string>;
}

export const useAudition = create<Audition>(() => ({ on: true, held: new Map(), silent: new Set() }));

// The toggle is kept with the song's view settings (`local.audition`, polish Q16): off stays off
// when the song opens again. Another song open: its own.
useLocalStore.subscribe((s, before) => {
  if (s.values.audition === before.values.audition) return;
  const on = s.values.audition !== false;
  if (on === useAudition.getState().on) return;
  useAudition.setState({ on });
  if (!on) releaseAll();
});

let nextHandle = 0;
let keepalive: ReturnType<typeof setInterval> | null = null;
const checks = new Map<string, ReturnType<typeof setTimeout>>();

const laneKey = (module: string, lane: string) => `${module}.${lane}`;

function setHeld(held: Map<number, Held>) {
  useAudition.setState({ held });
  if (held.size && !keepalive) {
    keepalive = setInterval(() => {
      const handles = [...useAudition.getState().held.keys()];
      if (handles.length) say({ keep: handles });
    }, KEEPALIVE_MS);
  } else if (!held.size && keepalive) {
    clearInterval(keepalive);
    keepalive = null;
  }
}

/** Send one audition message. False when the socket isn't open: nothing sounds, nothing is said (the status pill says "Reconnecting…"). */
const say = (msg: Omit<AuditionOut, "t">) => send({ t: "audition", ...msg });

function setSilent(key: string, silent: boolean) {
  const now = useAudition.getState().silent;
  if (now.has(key) === silent) return;
  const next = new Set(now);
  if (silent) next.add(key);
  else next.delete(key);
  useAudition.setState({ silent: next });
}

/**
 * A strike on a lane the server called silent: it may play now (an edit gave it a reader). It stays
 * silent if the server says so again; else the hint goes once that answer is overdue.
 */
function recheck(key: string) {
  if (!useAudition.getState().silent.has(key) || checks.has(key)) return;
  checks.set(
    key,
    setTimeout(() => {
      checks.delete(key);
      setSilent(key, false);
    }, SILENT_CHECK_MS),
  );
}

/**
 * Play `pitch` on lane `module.lane` until `release`. The handle, or null when nothing was sent (the
 * toggle is off, or no connection). `always`: whatever the toggle says (the keyboard piano plays an
 * instrument; the toggle is about hearing the notes you edit).
 */
export function strike(module: string, lane: string, pitch: number, velocity = KEY_VELOCITY, { always = false, record = false } = {}): number | null {
  if (!always && !useAudition.getState().on) return null;
  const h = ++nextHandle;
  // `record`: a take keeps it (the keyboard piano's; lane B). Notes' own clicks never are.
  if (!say({ module, lane, on: [{ h, pitch, velocity: clampVelocity(velocity) }], ...(record ? { record } : {}) })) return null;
  recheck(laneKey(module, lane));
  setHeld(new Map(useAudition.getState().held).set(h, { module, lane, pitch }));
  return h;
}

/**
 * Move held note `h` to `pitch`: an Off for the old pitch and an On for the new, in one message.
 * The new handle, or null when `h` isn't held any more (⌘. or a blur let it go) or nothing was sent.
 */
export function retune(h: number | null, pitch: number, velocity = KEY_VELOCITY): number | null {
  const held = useAudition.getState().held;
  const was = h === null ? undefined : held.get(h);
  if (h === null || !was) return null;
  if (was.pitch === pitch) return h;
  const next = new Map(held);
  next.delete(h);
  const h2 = ++nextHandle;
  const sent = say({ module: was.module, lane: was.lane, off: [h], on: [{ h: h2, pitch, velocity: clampVelocity(velocity) }] });
  if (sent) next.set(h2, { ...was, pitch });
  setHeld(next);
  return sent ? h2 : null;
}

/** Let go of held note `h`; nothing if it isn't held. */
export function release(h: number | null) {
  const held = useAudition.getState().held;
  if (h === null || !held.has(h)) return;
  const next = new Map(held);
  next.delete(h);
  setHeld(next);
  say({ off: [h] });
}

/** Play `pitch` for `ms` (a painted note). */
export function pulse(module: string, lane: string, pitch: number, velocity: number, ms: number) {
  const h = strike(module, lane, pitch, velocity);
  if (h !== null) setTimeout(() => release(h), Math.max(30, ms));
}

/** Let go of everything this window holds. */
export function releaseAll() {
  const handles = [...useAudition.getState().held.keys()];
  if (!handles.length) return;
  setHeld(new Map());
  say({ off: handles });
}

/** Turn the toggle on or off; off lets go of everything held. */
export function setAudition(on: boolean) {
  setLocal("audition", on ? null : false);
  useAudition.setState({ on });
  if (!on) releaseAll();
}

const clampVelocity = (v: number) => Math.min(1, Math.max(0, Number.isFinite(v) ? v : KEY_VELOCITY));

let attached = 0;
let detach: (() => void) | null = null;

/**
 * Start listening for what ends held notes and for the server's answers, while a view that sounds
 * notes is open. Returns how to stop, which also lets go of everything held.
 */
export function attach(): () => void {
  if (attached++ === 0) detach = listen();
  let done = false;
  return () => {
    if (done) return;
    done = true;
    releaseAll();
    if (--attached === 0) {
      detach?.();
      detach = null;
    }
  };
}

function listen() {
  // Pointers down anywhere: Space lets go only with none down (a note held by a hand plays on).
  const down = new Set<number>();
  const press = (e: PointerEvent) => void down.add(e.pointerId);
  const lift = (e: PointerEvent) => void down.delete(e.pointerId);
  const key = (e: KeyboardEvent) => {
    // ⌘. (Ctrl+. off the Mac): Stop All Sound ends every live note on the server; forget them here.
    if ((e.metaKey || e.ctrlKey) && e.key === ".") releaseAll();
    // Space with no pointer down: anything held now is stuck.
    else if (e.code === "Space" && !e.repeat && down.size === 0) releaseAll();
  };
  const blur = () => {
    down.clear();
    releaseAll();
  };
  const hidden = () => document.visibilityState === "hidden" && blur();
  window.addEventListener("pointerdown", press, true);
  window.addEventListener("pointerup", lift, true);
  window.addEventListener("pointercancel", lift, true);
  window.addEventListener("keydown", key, true);
  window.addEventListener("blur", blur);
  document.addEventListener("visibilitychange", hidden);
  // In the app ⌘. is the menu bar's.
  const offMenu = onMenu((id) => id === "panic" && releaseAll());
  const offAnswer = on<AuditionIn>("audition", (msg) => {
    if (msg.error) return useStore.getState().toast(msg.error);
    if (msg.silent && msg.module && msg.lane) {
      const k = laneKey(msg.module, msg.lane);
      clearTimeout(checks.get(k));
      checks.delete(k);
      setSilent(k, true);
    }
  });
  // The server let go of this window's notes when its socket closed.
  const offClosed = on(CLOSED, () => setHeld(new Map()));
  return () => {
    window.removeEventListener("pointerdown", press, true);
    window.removeEventListener("pointerup", lift, true);
    window.removeEventListener("pointercancel", lift, true);
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("blur", blur);
    document.removeEventListener("visibilitychange", hidden);
    offMenu();
    offAnswer();
    offClosed();
  };
}

/** Lane `module.lane` played nothing when this window last struck it. */
export const useSilent = (module: string, lane: string) => useAudition((s) => s.silent.has(laneKey(module, lane)));
