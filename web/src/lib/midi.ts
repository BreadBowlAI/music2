// MIDI keyboards (8b.10): the server hears them; this window tells it what they play, and shows
// which are plugged in (components/MidiStatus.tsx).
//
// What plays is this window's selected track's notes lane (lib/playTarget.ts, Q4), sent on the
// WebSocket as `{"t": "midi", "module", "lane"}` whenever it changes, and again on each (re)connect.
// The last window to say wins; with nothing selected it says `module: null`, and keys play nothing.
// The server tells every window the keyboards and the target (`{"t": "midi", "midi": …}`, and the
// state's `midi` when the song loads).

import { create } from "zustand";
import { CLOSED, on, send } from "./events";
import { currentTarget } from "./playTarget";
import { useShell } from "./shell";
import { useStore } from "./store";

/** The keyboards plugged in, by name, and the lane they play. */
export interface MidiState {
  devices: string[];
  /** What keys play; `kit`, a kit's bus, when they play a drum kit by sound (KIT-25). */
  target: { module: string; lane: string; kit?: string } | null;
  /** Keyboards turned off in Settings (studio lane B): they neither play nor record. */
  off?: string[];
}

declare module "./graph" {
  interface GraphState {
    /** MIDI keyboards (8b.10). */
    midi?: MidiState;
  }
}

export const useMidi = create<MidiState>(() => ({ devices: [], target: null, off: [] }));

/** What this window last told the server on this connection, with the song it was for. */
let told: string | null = null;

function tell() {
  const { connected, graph } = useStore.getState();
  if (!connected || !graph) {
    told = null;
    return;
  }
  const t = currentTarget();
  const msg = { t: "midi", module: t?.module ?? null, lane: t?.lane ?? null };
  const key = `${graph.session} ${msg.module} ${msg.lane}`;
  if (key !== told && send(msg)) told = key;
}

// The state's `midi` each time the song is fetched: a window that just loaded, or a new song.
let seen: MidiState | undefined;
useStore.subscribe((s) => {
  const m = s.graph?.midi;
  if (m && m !== seen) {
    seen = m;
    useMidi.setState({ devices: m.devices, target: m.target, off: m.off ?? [] });
  }
  tell();
});
useShell.subscribe(tell);
on<{ midi: MidiState }>("midi", (msg) => useMidi.setState({ devices: msg.midi.devices, target: msg.midi.target, off: msg.midi.off ?? [] }));
// A socket that closed forgets what it was told: the next one is told again.
on(CLOSED, () => {
  told = null;
});
