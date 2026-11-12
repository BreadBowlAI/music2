// Where playback is, as the last 200 ms poll said (taste.md VIEW-8): a store read by selectors, so
// the poll renders only what shows the status (the top bar), never the whole window. The playheads
// move every frame by themselves, from the clock (TL-9).

import { create } from "zustand";
import type { GraphStatus } from "./graph";

export const useStatus = create<GraphStatus>(() => ({ playing: false, seconds: 0, beat: 0, loop: null }));

/** The status now, outside React. */
export const statusNow = () => useStatus.getState();
