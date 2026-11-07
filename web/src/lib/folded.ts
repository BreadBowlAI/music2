// The cards folded to their first group (CARD-25), by node id: this window's, for the open song.
// A knob a folded card hides is still a knob to the rest of the app: Show on an agent's pointer at
// it unfolds its card (AGENT-6), and the "N more" row stands for it (its glow, AGENT-5).
//
// And the chain's cards (taste.md DV-12 to DV-16): each card's size and the tab it shows. The window's
// own, as folding is: none of it is the song's. Whether Device shows the chain is the shell's.

import { create } from "zustand";
import { frameOf, knobsOf } from "./frame";
import { useStore } from "./store";

export const useFolded = create<{ ids: Set<string>; flip: (id: string) => void; open: (id: string) => void }>((set) => ({
  ids: new Set(),
  flip: (id) =>
    set((s) => {
      const ids = new Set(s.ids);
      if (!ids.delete(id)) ids.add(id);
      return { ids };
    }),
  open: (id) =>
    set((s) => {
      if (!s.ids.has(id)) return s;
      const ids = new Set(s.ids);
      ids.delete(id);
      return { ids };
    }),
}));

/** A chain card's size (taste.md DV-13). */
export type CardSize = "folded" | "normal" | "expanded";

export const useCards = create<{
  /** Each card's size, by node id; Normal when not set. */
  sizes: Record<string, CardSize>;
  /** The tab each card shows, by node id; its first when not set. */
  tabs: Record<string, string>;
  size: (id: string, size: CardSize) => void;
  tab: (id: string, tab: string) => void;
}>((set) => ({
  sizes: {},
  tabs: {},
  size: (id, size) => set((s) => ({ sizes: { ...s.sizes, [id]: size } })),
  tab: (id, tab) => set((s) => ({ tabs: { ...s.tabs, [id]: tab } })),
}));

/**
 * Show's way to knob `knob`: unfold the patch's card that hides it; and on the chain's card, open a
 * Folded card to Normal and show the tab that holds it, when its face doesn't (taste.md DV-16).
 */
export function unfoldFor(knob: string) {
  const { ids, open } = useFolded.getState();
  const cards = useCards.getState();
  for (const m of useStore.getState().graph?.view.modules ?? []) {
    const card = m.nodes.find((n) => n.knobs.some((k) => k.id === knob));
    if (!card) continue;
    if (ids.has(card.id)) open(card.id);
    const size = cards.sizes[card.id] ?? "normal";
    if (size === "folded") cards.size(card.id, "normal");
    const { face, tabs } = frameOf(card);
    if (size === "expanded" || knobsOf(card, face).includes(knob)) return;
    const tab = tabs.find((t) => knobsOf(card, t.rows).includes(knob));
    if (tab) cards.tab(card.id, tab.name);
    return;
  }
}
