// Applies server patches (see crates/server/src/patch.rs) immutably. Anything not on a
// patched path keeps its object identity, so React.memo can skip unchanged tracks/clips.

type Seg = string | number;
export type Op =
  | { op: "set"; path: Seg[]; value: unknown }
  | { op: "del"; path: Seg[] }
  | { op: "ids"; path: Seg[]; ids: number[]; add: Record<string, unknown> };

function update(node: any, path: Seg[], i: number, op: Op): any {
  if (op.op === "del" && i === path.length - 1) {
    const { [path[i] as string]: _, ...rest } = node;
    return rest;
  }
  if (i === path.length) {
    if (op.op === "set") return op.value;
    if (op.op === "ids") {
      const byId = new Map<number, any>((node ?? []).map((x: any) => [x.id, x]));
      return op.ids.map((id) => op.add[String(id)] ?? byId.get(id));
    }
    return node;
  }
  const key = path[i];
  const copy = Array.isArray(node) ? [...node] : { ...(node ?? {}) };
  (copy as any)[key] = update(node?.[key], path, i + 1, op);
  return copy;
}

export function applyPatch<T>(root: T, ops: Op[]): T {
  return ops.reduce((acc, op) => update(acc, op.path, 0, op), root as any);
}
