// The cards folded to their first group (CARD-25), by node id: this window's, for the open song.
// A knob a folded card hides is still a knob to the rest of the app: Show on an agent's pointer at
// it unfolds its card (AGENT-6), and the "N more" row stands for it (its glow, AGENT-5).

import { create } from "zustand";
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

/** Unfold the card that holds knob `knob`, when a folded card hides it: Show's way to it. */
export function unfoldFor(knob: string) {
  const { ids, open } = useFolded.getState();
  if (!ids.size) return;
  for (const m of useStore.getState().graph?.view.modules ?? []) {
    const card = m.nodes.find((n) => ids.has(n.id) && n.knobs.some((k) => k.id === knob));
    if (card) return open(card.id);
  }
}
