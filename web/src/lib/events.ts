// Events from the server: toasts, and `graph` when the open project changed.

import { clientId } from "./graph";
import { token } from "./native";

export function connect(onMessage: (msg: any) => void, onStatus: (connected: boolean) => void) {
  let ws: WebSocket | null = null;
  let closed = false;
  const open = () => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    // The window's id: when this socket closes, the server ends the knob drags it left open.
    ws = new WebSocket(`${proto}://${location.host}/api/ws?client=${encodeURIComponent(clientId)}${token ? `&token=${token}` : ""}`);
    ws.onopen = () => onStatus(true);
    ws.onmessage = (e) => onMessage(JSON.parse(e.data));
    ws.onclose = () => {
      onStatus(false);
      if (!closed) setTimeout(open, 800);
    };
  };
  open();
  return () => {
    closed = true;
    ws?.close();
  };
}
