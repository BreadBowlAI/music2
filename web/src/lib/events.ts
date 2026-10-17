// The window's socket to the server. It hears toasts, `graph` when the open project changed,
// exports, meters and audition's answers, each by its type (`on`). It also sends: audition's
// notes (`send`).

import { clientId } from "./graph";
import { token } from "./native";

/** The type a listener hears when the socket closes: what it was told until then is old news. */
export const CLOSED = "close";

type Listener = (msg: any) => void;
const listeners = new Map<string, Set<Listener>>();
let socket: WebSocket | null = null;

/** Hear the server's messages of type `t` (`{"t": t, …}`), or `CLOSED`. Returns how to stop. */
export function on<T = any>(t: string, fn: (msg: T) => void): () => void {
  let set = listeners.get(t);
  if (!set) listeners.set(t, (set = new Set()));
  set.add(fn);
  return () => void set.delete(fn);
}

const emit = (msg: { t?: unknown }) => {
  if (typeof msg?.t === "string") for (const fn of [...(listeners.get(msg.t) ?? [])]) fn(msg);
};

/** Send `msg` on this window's socket. False when it isn't open: nothing is kept to send later. */
export function send(msg: object): boolean {
  if (socket?.readyState !== WebSocket.OPEN) return false;
  socket.send(JSON.stringify(msg));
  return true;
}

/** Open the socket, and open it again 800 ms after it closes, until the returned function is called. */
export function connect(onStatus: (connected: boolean) => void) {
  let closed = false;
  let mine: WebSocket | null = null;
  const open = () => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    // The window's id: when this socket closes, the server ends the knob drags and the notes it
    // left open.
    const ws = new WebSocket(`${proto}://${location.host}/api/ws?client=${encodeURIComponent(clientId)}${token ? `&token=${token}` : ""}`);
    socket = mine = ws;
    ws.onopen = () => onStatus(true);
    ws.onmessage = (e) => emit(JSON.parse(e.data));
    ws.onclose = () => {
      if (socket === ws) {
        socket = null;
        emit({ t: CLOSED });
      }
      onStatus(false);
      if (!closed) setTimeout(open, 800);
    };
  };
  open();
  return () => {
    closed = true;
    mine?.close();
  };
}

/** A lane an edit just made here (automate), as `module.lane`: the timeline scrolls to it and lights it. */
export const LANE_MADE = "m2:lane-made";
export const laneMade = (module: string, lane: string) => window.dispatchEvent(new CustomEvent(LANE_MADE, { detail: `${module}.${lane}` }));

/** A track an edit just made here (+ Track, Duplicate), as `module.lane` of its own row: the timeline scrolls to it and lights it, its automation left folded. */
export const TRACK_MADE = "m2:track-made";
export const trackMade = (module: string, lane: string) => window.dispatchEvent(new CustomEvent(TRACK_MADE, { detail: `${module}.${lane}` }));
