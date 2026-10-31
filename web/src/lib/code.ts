// The code editor's side of the API: the project's TypeScript files, the sources its
// imports resolve to, and saving a file over the text it was based on.

import type { GraphState } from "./graph";
import { token } from "./native";

/**
 * A file of the accepted revision. `draft` is set when the disk says otherwise (saved, but
 * not compiling): that text, or null if the file is gone. A file only on disk has
 * `accepted: false`.
 */
export interface ProjectFile { path: string; text: string; draft?: string | null; accepted?: false }

export interface ProjectFiles {
  revision: number;
  files: ProjectFile[];
  /** The file each module (by key) is defined in, when the compiler says. */
  modules: { key: string; label?: string; file: string | null }[];
  /** Text left unsaved when the song last closed, offered back (WRITE-27); `changed`: its file changed since, or is gone. */
  left?: Left[];
}

/** A draft the song opened with: a file's text that wasn't saved (WRITE-27). */
export interface Left { file: string; text: string; changed: boolean }

/** What the file says on disk, which a save must be based on; null if it is gone. */
export const onDisk = (f: ProjectFile) => (f.draft === undefined ? f.text : f.draft);

/** A source an import resolves to, by its path under `node_modules` (`std/fx.ts`). */
export interface TypeFile { path: string; text: string }

/** A save: the new state, with the undo step it made (AGENT-2), or what the file says now if it changed since it was read. */
export type Saved = { ok: true; graph: GraphState; step?: number } | { ok: false; current: string };

const headers: Record<string, string> = { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) };

async function request<T>(method: string, path: string, body?: unknown): Promise<{ status: number; data: T & { error?: string } }> {
  const res = await fetch(`/api/graph${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: res.status, data: await res.json().catch(() => ({})) };
}

async function get<T>(path: string): Promise<T> {
  const { status, data } = await request<T>("GET", path);
  if (status >= 400) throw new Error(data.error ?? `HTTP ${status}`);
  return data;
}

export const codeApi = {
  files: () => get<ProjectFiles>("/files"),
  types: () => get<{ files: TypeFile[] }>("/types"),
  save: async (session: number, path: string, base: string, text: string): Promise<Saved> => {
    const { status, data } = await request<GraphState & { current?: string }>("PUT", "/file", { session, path, base, text });
    if (status === 409 && typeof data.current === "string") return { ok: false, current: data.current };
    if (status >= 400) throw new Error(data.error ?? `HTTP ${status}`);
    return { ok: true, graph: data, step: (data as { step?: number }).step };
  },
  /** Keep a file's unsaved text in the song's `.music2/drafts/`, so a crash doesn't lose it (WRITE-27); null: nothing unsaved now. */
  draft: async (session: number, file: string, base: string, text: string | null) => {
    const { status, data } = await request<object>("POST", "/draft", { session, file, base, text });
    if (status >= 400) throw new Error(data.error ?? `HTTP ${status}`);
  },
};

/** `song.ts` first, then `tracks/`, then the rest; alphabetical within each. */
export function fileOrder(a: string, b: string) {
  const rank = (p: string) => (p === "song.ts" ? 0 : p.startsWith("tracks/") && !p.slice(7).includes("/") ? 1 : 2);
  return rank(a) - rank(b) || a.localeCompare(b);
}

/**
 * The file the module with key `module` is written in: where the compiler says, else the first
 * file (`song.ts`). Never guessed from the key or the label, neither of which names a file (ID-10).
 */
export function moduleFile(paths: string[], modules: ProjectFiles["modules"], module: string): string | undefined {
  const said = modules.find((m) => m.key === module)?.file;
  if (said && paths.includes(said)) return said;
  return [...paths].sort(fileOrder)[0];
}
