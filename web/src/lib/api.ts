// Thin client for the Rust engine. Every edit is `exec(command, params)`.

import { token } from "./native";

// The desktop app gives the engine a per-launch token; every call must carry it.
const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function call<T = any>(path: string, body?: unknown, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api/${path}`, {
    method: body === undefined && !init?.body ? "GET" : "POST",
    headers: init?.body ? auth : { "content-type": "application/json", ...auth },
    body: init?.body ?? (body === undefined ? undefined : JSON.stringify(body)),
    ...init,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

// Edits, undo-group markers and undo/redo form ONE ordered stream, like a command log:
// each waits for the previous to finish, so the engine sees them in the order the user
// made them. (Concurrent HTTP requests could otherwise be handled out of order.)
let queue: Promise<unknown> = Promise.resolve();
function ordered<T>(f: () => Promise<T>): Promise<T> {
  const p = queue.then(f, f);
  queue = p.catch(() => {});
  return p;
}

export const api = {
  state: () => call("state"),
  exec: (cmd: string, params: Record<string, unknown> = {}) => ordered(() => call("exec", { cmd, params })),
  steps: (label: string, steps: [string, unknown][]) => ordered(() => call("steps", { label, steps })),
  group: (action: "begin" | "end", label = "") => ordered(() => call("group", { action, label })),
  undo: () => ordered(() => call("undo", {})),
  redo: () => ordered(() => call("redo", {})),
  transport: (action: "play" | "stop" | "toggle" | "seek" | "panic", tick = 0) => call("transport", { action, tick }),
  loop: (on: boolean) => call("loop", { on }),
  note: (track: number, pitch: number, on: boolean) => call("note", { track, pitch, on }),
  newSong: () => call("song/new", {}),
  open: (path: string) => call("song/open", { path }),
  save: (title?: string) => call<{ path: string }>("song/save", { title }),
  exportWav: () => call<{ path: string }>("song/export", {}),
  importPath: (path: string, bar?: number) => call("import-path", { path, bar }),
  library: () => call<{ dir: string; songs: { name: string; path: string; modified: number }[] }>("library"),
  reveal: (path: string) => call("reveal", { path }),
  settings: () => call<{ plugins_dir: string; library: string }>("settings"),
  plugin: (id: string) => call(`plugin?id=${encodeURIComponent(id)}`),
  peaks: (asset: number) => call<{ peaks: number[] }>(`peaks/${asset}`),
};

export function connect(onMessage: (msg: any) => void, onStatus: (connected: boolean) => void) {
  let ws: WebSocket | null = null;
  let closed = false;
  const open = () => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    ws = new WebSocket(`${proto}://${location.host}/api/ws${token ? `?token=${token}` : ""}`);
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
