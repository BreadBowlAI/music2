// The keyboard piano (the next track's B2, Q7 to Q9): the computer's letter keys play the selected
// track's notes, as Logic's Musical Typing does. Keys by their place (`e.code`), so other keyboard
// layouts play the same notes. A is C, the white keys run A S D F G H J K L ; ' and the black ones
// W E T Y U O P above them, from C to the F an octave and a half up; Z and X move an octave down and
// up (A from C1 to C7). Every note at one velocity, 0.8 (`KEY_VELOCITY`).
//
// Off at every launch, never kept. While on, it owns exactly its 20 keys (the 18 notes, Z and X)
// with no ⌘, ⌃ or ⌥, outside text fields, the code editor and dialogs: their shortcuts (A, S, E, X,
// Z, K) wait until it is off. Every other key keeps its own (Space, Return, C, M, B, Q…). A note
// sounds until its key comes up; the window losing focus, the page hidden, or ⌘ pressed or let go
// (a Mac sends no key up for a letter let go while ⌘ is held) lets every note go. Nothing is written:
// there is nothing to undo. ⌘K (Ctrl+K off the Mac) turns it on and off, from anywhere.

import { create } from "zustand";
import { KEY_VELOCITY, attach, release, strike } from "./audition";
import { isMac, native } from "./native";
import { currentTarget } from "./playTarget";

/** Each note key, by `e.code`: semitones above the octave's C. */
export const NOTE_KEYS: Record<string, number> = {
  KeyA: 0, KeyW: 1, KeyS: 2, KeyE: 3, KeyD: 4, KeyF: 5, KeyT: 6, KeyG: 7, KeyY: 8, KeyH: 9, KeyU: 10, KeyJ: 11,
  KeyK: 12, KeyO: 13, KeyL: 14, KeyP: 15, Semicolon: 16, Quote: 17,
};
/** The white keys and the black keys, in order, as their caps say them. */
export const WHITE: [string, string][] = [["KeyA", "A"], ["KeyS", "S"], ["KeyD", "D"], ["KeyF", "F"], ["KeyG", "G"], ["KeyH", "H"], ["KeyJ", "J"], ["KeyK", "K"], ["KeyL", "L"], ["Semicolon", ";"], ["Quote", "'"]];
/** Each black key and the white key it sits after. */
export const BLACK: [string, string, number][] = [["KeyW", "W", 0], ["KeyE", "E", 1], ["KeyT", "T", 3], ["KeyY", "Y", 4], ["KeyU", "U", 5], ["KeyO", "O", 7], ["KeyP", "P", 8]];
const DOWN = "KeyZ", UP = "KeyX";
/** A's octave: C1 to C7. C4 is MIDI 60. */
export const LOWEST = 1, HIGHEST = 7;

/** The MIDI note key `code` plays with A in `octave`. */
export const pitchOf = (code: string, octave: number) => 12 * (octave + 1) + NOTE_KEYS[code];

interface Piano {
  on: boolean;
  /** A's octave. */
  octave: number;
  /** The keys held now, by `e.code`, and the pitch each plays: a new map at each change, so the card redraws. */
  held: ReadonlyMap<string, number>;
}

export const usePiano = create<Piano>(() => ({ on: false, octave: 4, held: new Map() }));

/** The audition handle of each key held (null: it sounds nothing, as no track was selected). */
const handles = new Map<string, number | null>();
let stop: (() => void) | null = null;

function setHeld(code: string, pitch: number | null) {
  const held = new Map(usePiano.getState().held);
  if (pitch === null) held.delete(code);
  else held.set(code, pitch);
  usePiano.setState({ held });
}

/** Let go of every note the piano holds. */
export function letGo() {
  for (const h of handles.values()) release(h);
  handles.clear();
  if (usePiano.getState().held.size) usePiano.setState({ held: new Map() });
}

/** Turn it on or off; off lets every note go. */
export function setPiano(on: boolean) {
  if (on === usePiano.getState().on) return;
  if (on) {
    // Audition's answers (a lane nothing plays), and the server letting go when the socket closes.
    stop = attach();
    listen(true);
  } else {
    listen(false);
    letGo();
    stop?.();
    stop = null;
  }
  usePiano.setState({ on });
}

export const togglePiano = () => setPiano(!usePiano.getState().on);

/** Typing goes to a field or the code editor, or a dialog is open: the keys are theirs. */
function theirs(e: KeyboardEvent) {
  const t = e.target instanceof Element ? e.target : document.activeElement;
  const a = document.activeElement;
  const field = (el: Element | null) =>
    el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement || (el instanceof HTMLElement && el.isContentEditable) || !!el?.closest(".monaco-editor");
  return field(t) || field(a) || !!document.querySelector('[aria-modal="true"]');
}

function down(e: KeyboardEvent) {
  if (e.key === "Meta") return letGo();
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const note = e.code in NOTE_KEYS;
  if (!note && e.code !== DOWN && e.code !== UP) return;
  if (theirs(e)) return;
  // The key is the piano's: no shortcut runs too.
  e.preventDefault();
  e.stopImmediatePropagation();
  if (e.repeat) return;
  if (!note) {
    const { octave } = usePiano.getState();
    usePiano.setState({ octave: Math.min(HIGHEST, Math.max(LOWEST, octave + (e.code === UP ? 1 : -1))) });
    return;
  }
  if (handles.has(e.code)) return;
  const pitch = pitchOf(e.code, usePiano.getState().octave);
  const t = currentTarget();
  // With no track to play, the key is still the piano's: it plays nothing.
  // A take records what it plays (lane B).
  handles.set(e.code, t ? strike(t.module, t.lane, pitch, KEY_VELOCITY, { always: true, record: true }) : null);
  setHeld(e.code, pitch);
}

function up(e: KeyboardEvent) {
  if (e.key === "Meta") return letGo();
  if (!handles.has(e.code)) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  release(handles.get(e.code) ?? null);
  handles.delete(e.code);
  setHeld(e.code, null);
}

const hidden = () => document.visibilityState === "hidden" && letGo();

function listen(on: boolean) {
  if (on) {
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", letGo);
    document.addEventListener("visibilitychange", hidden);
  } else {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
    window.removeEventListener("blur", letGo);
    document.removeEventListener("visibilitychange", hidden);
  }
}

// ⌘K (Ctrl+K off the Mac) turns it on and off from anywhere: a field, the code editor (whose own ⌘K
// chords give way to it), a dialog. Before the page's other keys see it. In the desktop app the menu
// bar's View ▸ Keyboard Piano has the chord, and sends `piano` (App.tsx).
window.addEventListener(
  "keydown",
  (e) => {
    if (native || e.code !== "KeyK" || e.shiftKey || e.altKey || !(isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (!e.repeat) togglePiano();
  },
  true,
);
