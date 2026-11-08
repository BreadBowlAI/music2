// The graph's own state (taste.md DV-12 to DV-16, DV-18): which nodes are expanded, which one is
// selected, and which group a picture's dot lit. The window's alone: none of it is the song's, none
// is saved or sent. Nodes are named by their key (CARD-25), which an edit never moves to another
// node, so an effect added above an expanded node leaves it expanded.
//
// A value a collapsed node hides is still a knob to the rest of the app: its node's +N stands for
// it (`data-hides`), its glow lights +N (AGENT-5), and Show on an agent's pointer at it expands the
// node (`expandFor`, AGENT-6).

import { create } from "zustand";
import { useStore } from "./store";

interface Expanded {
  /** The expanded nodes, by key. */
  keys: ReadonlySet<string>;
  /** The selected node, by key: its ring, Delete's target (DV-15). */
  selected: string | null;
  /** Nodes are moving aside, for 280 ms after one expands or collapses (DV-13). */
  moving: boolean;
  /** A group a picture's dot lit (DV-18): its node's key and the group's name. */
  lit: { node: string; group: string } | null;
  /** Expand `key`, or collapse it. */
  toggle: (key: string) => void;
  expand: (key: string) => void;
  collapseAll: () => void;
  select: (key: string | null) => void;
  /** Light `group` of node `key`, expanding it: a dot whose `opens` names it was picked. */
  light: (key: string, group: string) => void;
  /** A new song: every node collapsed, nothing selected. */
  reset: () => void;
}

let still: ReturnType<typeof setTimeout> | null = null;
/** The expanded set changed: the nodes move to their new places over 280 ms (DV-13). */
const moved = (set: (p: Partial<Expanded>) => void) => {
  if (still) clearTimeout(still);
  still = setTimeout(() => set({ moving: false }), 320);
  return { moving: true };
};

export const useExpanded = create<Expanded>((set) => ({
  keys: new Set(),
  selected: null,
  moving: false,
  lit: null,
  toggle: (key) =>
    set((s) => {
      const keys = new Set(s.keys);
      if (!keys.delete(key)) keys.add(key);
      return { keys, lit: keys.has(key) ? s.lit : s.lit?.node === key ? null : s.lit, ...moved(set) };
    }),
  expand: (key) => set((s) => (s.keys.has(key) ? s : { keys: new Set([...s.keys, key]), ...moved(set) })),
  collapseAll: () => set((s) => (s.keys.size ? { keys: new Set(), lit: null, ...moved(set) } : { lit: null })),
  select: (selected) => set((s) => (s.selected === selected ? s : { selected })),
  light: (node, group) => set((s) => (s.keys.has(node) ? { lit: { node, group } } : { keys: new Set([...s.keys, node]), lit: { node, group }, ...moved(set) })),
  reset: () => set({ keys: new Set(), selected: null, lit: null }),
}));

/** Show's way to knob `knob` (AGENT-6, DV-16): expand the node that holds it, wherever it is. */
export function expandFor(knob: string) {
  for (const m of useStore.getState().graph?.view.modules ?? []) {
    const node = m.nodes.find((n) => n.knobs.some((k) => k.id === knob));
    if (node) return useExpanded.getState().expand(node.key ?? node.id);
  }
}

// A new song starts with every node collapsed: its keys are its own (CARD-25).
useStore.subscribe((s, was) => {
  if (s.graph?.session !== was.graph?.session) useExpanded.getState().reset();
});
