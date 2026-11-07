// The card's frame in Device's chain (taste.md DV-11 to DV-16), worked out from the view alone: which
// calls make a sound's chain, and what each card's face and tabs hold.

import type { CardRow, ModuleView, ViewNode } from "./graph";

/** A tab of a chain card: one of its groups, or All. */
export interface FrameTab { name: string; rows: CardRow[] }

/** What a chain card draws (taste.md DV-12): its face, and its tabs, All last (none when the face holds every value). */
export interface FrameOf { face: CardRow[]; tabs: FrameTab[] }

/** The tab every card with more than its face has: every value in one list. */
export const ALL = "All";

/**
 * Whether input `p` of `n` is a value the card shows, not a connection: a knob, a value a cable, a
 * lane or the code drives, or a declared control. Notes and audio coming in are connections, and
 * so is what a "Listens to" row stands for (CARD-35).
 */
function isValue(n: ViewNode, p: ViewNode["inputs"][number]) {
  // Sound and notes coming in are connections, however they are written: by a const's name too (review F2).
  if (p.kind === "audio" || p.kind === "notes" || n.listens?.port === p.id) return false;
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
 * A chain card's face and tabs (taste.md DV-12): the face its sound names, else its first group, else
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

/** A row of a mix's chain (taste.md DV-11): a track's sound the module reads, then its effects in order. */
export interface Branch { from: ViewNode; cards: ViewNode[] }

/**
 * What Device's chain shows of sound `m` (taste.md DV-11): `line`, its calls that make audio, in the order
 * the sound flows; and for a mix, `branches`, a row for each track's sound it reads before the line,
 * with that sound's effects (a song: each channel's, then its mixer and master). `why` says, in a
 * beginner's words, why there is no chain. Null when it has no calls that make audio, heard or not.
 */
export type ChainOf = { line: ViewNode[]; branches: Branch[] } | { why: string };

export function chainOf(m: ModuleView): ChainOf | null {
  const byId = new Map(m.nodes.map((n) => [n.id, n]));
  const audio = m.port_edges.filter((e) => e.kind === "audio" && e.from !== e.to);
  // A call whose sound reaches no output isn't heard: it is no part of the sound, and no card (review F15).
  const heard = new Set<string>();
  const reaches = (id: string, seen = new Set<string>()): boolean => {
    if (byId.get(id)?.kind === "export") return true;
    if (seen.has(id)) return false;
    seen.add(id);
    return audio.some((e) => e.from === id && reaches(e.to, seen));
  };
  const all = m.nodes.filter((n) => n.kind === "call" && n.audio);
  if (!all.length) return null;
  for (const n of all) if (reaches(n.id)) heard.add(n.id);
  const cards = all.filter((n) => heard.has(n.id));
  if (!cards.length) return null;
  const title = (n: ViewNode) => n.title ?? n.label;
  const parts = { why: "This sound is built from parts." };
  // What feeds each card's sound: its audio coming in, but what a "Listens to" row stands for.
  const feeds = (n: ViewNode) => [...new Set(audio.filter((e) => e.to === n.id && e.to_port !== n.listens?.port).map((e) => e.from))].flatMap((id) => byId.get(id) ?? []);
  const next = new Map<string, string>();
  for (const n of cards) {
    const to = [...new Set(audio.filter((e) => e.from === n.id && heard.has(e.to)).map((e) => e.to))];
    if (to.length > 1) return parts;
    if (to.length) next.set(n.id, to[0]);
  }
  const merges = cards.filter((n) => feeds(n).length > 1);
  if (merges.length > 1) return { why: "This sound mixes sounds in more than one place." };
  // The line: from where the sounds meet (or the first card) on to the output.
  const lineFrom = (first: ViewNode) => {
    const line: ViewNode[] = [];
    for (let at: string | undefined = first.id; at != null && line.length <= cards.length; at = next.get(at)) line.push(byId.get(at)!);
    return line;
  };
  const isCard = (n: ViewNode) => heard.has(n.id);
  let start: ViewNode;
  const branches: Branch[] = [];
  if (merges.length === 1) {
    start = merges[0];
    for (const f of feeds(start)) {
      const before: ViewNode[] = [];
      let at = f;
      while (isCard(at)) {
        before.unshift(at);
        const up = feeds(at);
        if (up.length > 1) return parts;
        if (!up.length) break;
        at = up[0];
      }
      // Only tracks' sounds meet in a chain (a song's mix); two sounds made here are mixed.
      if (at.kind !== "import") return { why: `${feeds(start).map(title).join(" and ")} are mixed here.` };
      branches.push({ from: at, cards: before });
    }
  } else {
    const firsts = cards.filter((n) => !cards.some((c) => next.get(c.id) === n.id));
    if (firsts.length !== 1) return parts;
    start = firsts[0];
    // A track's sound read here, before the line, is a row too: its + adds an effect after it (CARD-36).
    for (const f of feeds(start)) if (f.kind === "import") branches.push({ from: f, cards: [] });
  }
  const line = lineFrom(start);
  if (line.length + branches.reduce((a, b) => a + b.cards.length, 0) !== cards.length) return parts;
  return { line, branches };
}

/** The package a card's sound is from (taste.md DV-12): `@m2/mixing`, `std`, or none for the song's own and a plugin. */
export function packOf(n: ViewNode): string | null {
  if (!n.component || n.plugin) return null;
  const at = n.component.lastIndexOf(":");
  return at > 0 ? n.component.slice(0, at) : null;
}
