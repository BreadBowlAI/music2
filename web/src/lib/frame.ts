// The card's frame in Device's chain (CARD-42 to CARD-47), worked out from the view alone: which
// calls make a sound's chain, and what each card's face and tabs hold.

import type { CardRow, ModuleView, ViewNode } from "./graph";

/** A tab of a chain card: one of its groups, or All. */
export interface FrameTab { name: string; rows: CardRow[] }

/** What a chain card draws (CARD-43): its face, and its tabs, All last (none when the face holds every value). */
export interface FrameOf { face: CardRow[]; tabs: FrameTab[] }

/** The tab every card with more than its face has: every value in one list. */
export const ALL = "All";

/**
 * Whether input `p` of `n` is a value the card shows, not a connection: a knob, a value a cable, a
 * lane or the code drives, or a declared control. Notes and audio coming in are connections, and
 * so is what a "Listens to" row stands for (CARD-35).
 */
function isValue(n: ViewNode, p: ViewNode["inputs"][number]) {
  if (n.listens?.port === p.id) return false;
  return p.slot != null || p.knob != null || p.inlet != null || p.readonly != null || p.lane != null || p.kind === "control";
}

/** Every value of card `n` as rows, in the card's order: its groups' rows, or its inputs, settings and views. */
export function valueRows(n: ViewNode): CardRow[] {
  if (n.groups?.length) return n.groups.flatMap((g) => g.rows);
  return [
    ...n.inputs.filter((p) => isValue(n, p)).map((p): CardRow => ({ input: p.id })),
    ...(n.settings ?? []).map((x): CardRow => ({ setting: x.id })),
    ...(n.views ?? []).map((v): CardRow => ({ view: v.name })),
  ];
}

const same = (a: CardRow, b: CardRow) =>
  ("input" in a && "input" in b && a.input === b.input) ||
  ("setting" in a && "setting" in b && a.setting === b.setting) ||
  ("view" in a && "view" in b && a.view === b.view);

/**
 * A chain card's face and tabs (CARD-43): the face its sound names, else its first group, else
 * every value; the tabs, each group that isn't the face, then All, when any value isn't on the face.
 */
export function frameOf(n: ViewNode): FrameOf {
  const all = valueRows(n);
  const groups = n.groups ?? [];
  const named = !!n.face?.length;
  const face = named ? n.face! : groups.length ? groups[0].rows : all;
  if (all.every((r) => face.some((f) => same(f, r)))) return { face, tabs: [] };
  const tabs = (named ? groups : groups.slice(1)).map((g) => ({ name: g.name, rows: g.rows }));
  return { face, tabs: [...tabs, { name: ALL, rows: all }] };
}

/** The knob ids that rows of `n` draw. */
export function knobsOf(n: ViewNode, rows: CardRow[]): string[] {
  return rows.flatMap((r) => ("input" in r ? (n.inputs.find((p) => p.id === r.input)?.knob ?? []) : []));
}

/**
 * The calls of sound `m` that Device's chain shows (CARD-42), in the order its sound flows: the
 * calls that make audio, when they form one line, each feeding the next. Null when they don't (two
 * sounds mixed, a sound built from parts), or when it has none.
 */
export function chainOf(m: ModuleView): ViewNode[] | null {
  const cards = m.nodes.filter((n) => n.kind === "call" && n.audio);
  if (!cards.length) return null;
  const ids = new Set(cards.map((n) => n.id));
  const links = new Set(m.port_edges.filter((e) => e.kind === "audio" && e.from !== e.to && ids.has(e.from) && ids.has(e.to)).map((e) => `${e.from}>${e.to}`));
  const next = new Map<string, string>();
  const fed = new Set<string>();
  for (const l of links) {
    const [from, to] = l.split(">");
    if (next.has(from) || fed.has(to)) return null;
    next.set(from, to);
    fed.add(to);
  }
  if (links.size !== cards.length - 1) return null;
  const first = cards.find((n) => !fed.has(n.id));
  const line: ViewNode[] = [];
  for (let at = first?.id; at != null && line.length <= cards.length; at = next.get(at)) line.push(cards.find((n) => n.id === at)!);
  return line.length === cards.length ? line : null;
}

/** The package a card's sound is from (CARD-43): `@m2/mixing`, `std`, or none for the song's own and a plugin. */
export function packOf(n: ViewNode): string | null {
  if (!n.component || n.plugin) return null;
  const at = n.component.lastIndexOf(":");
  return at > 0 ? n.component.slice(0, at) : null;
}
