// What a node in the graph draws, and where (taste.md DV-12 to DV-14, the graph note §4 and §5):
// worked out from the view alone, once for the layout and the drawing both, so a wire's end is
// always where its row is drawn. A call's node is collapsed or expanded; a place a sound comes from
// or goes to is a pill; any other node shows all its values.

import { VIEW_H } from "../components/CardViews";
import { settingHeight, shown } from "../components/Settings";
import { picks, type CardRow, type CardView, type InputPort, type Setting, type ViewNode } from "./graph";

/** A collapsed node's width, and a pill's (a lane, an import, an export). */
export const NODE_W = 216;
export const SLIM_W = 152;
/** An expanded node's columns: each group one, wrapping after four (DV-12). */
export const COL_W = 176;
export const MAX_COLS = 4;
/** A pill: its name and its one output or input. */
export const PILL_H = 32;
/** A header line of the label, and the header's room above and below it (one line: 24 px). */
const HEAD_LINE = 16;
const HEAD_PAD = 8;
/** The lines under the header: how an instrument plays, the key it follows, a plugin's window. */
export const LINE_H = 24;
export const PLUGIN_H = 28;
/** An input or an output on a row of its own. */
export const ROW_H = 28;
/** A bar, a menu or a switch; a segmented choice over its label; a row naming code. */
export const VALUE_H = 28;
export const SEG_H = 48;
export const EXPR_H = 48;
/** A column's title. */
export const GROUP_H = 24;
/** Above and below a block of rows. */
export const PAD = 6;
/** A picture collapsed, and expanded (DV-19). */
export const COLLAPSED_PICTURE_H = 48;
export const EXPANDED_PICTURE_H = 132;
/** The code line's text and its room. */
const CODE_LINE = 14;
const CODE_PAD = 12;
/** How many of its face's values a collapsed node shows (DV-12). */
const FACE_VALUES = 4;

/** A connection: a row with its input's shape on the node's left edge (DV-12). */
export type Conn = { t: "input"; port: InputPort } | { t: "listens" };
/** A value, drawn where values are: a bar (or a menu or a switch), a row naming a lane or the code, a setting, a view. */
export type Value =
  | { t: "knob"; port: InputPort }
  | { t: "lane"; port: InputPort }
  | { t: "expr"; port: InputPort }
  | { t: "setting"; x: Setting }
  | { t: "view"; v: CardView };
/** An expanded node's column: a group, More, or Setup. */
export interface Column { name: string; items: Value[]; h: number }

export interface Shape {
  w: number;
  h: number;
  /** A call's node: it has two sizes. */
  sized: boolean;
  expanded: boolean;
  /** A lane, the time, an import or an export: its name and its one port. */
  pill: boolean;
  /** The header's height (DV-12: 24 px, more when the label wraps). */
  head: number;
  /** The lines under the header, together. */
  lines: number;
  conns: Conn[];
  /** Outputs beyond the rows, each on a row of its own. */
  fills: string[];
  ins: Map<string, number>;
  outs: Map<string, number>;
  /** Collapsed: its face's first view; expanded: its face's views, across the top. */
  views: CardView[];
  viewH: number;
  /** Collapsed: up to four of its face's values; a node with no sizes: all of them. */
  values: Value[];
  columns: Column[];
  /** Each row of columns, its height. */
  bands: number[];
  /** The values a collapsed node hides: how many, and the knobs among them, which +N stands for (DV-16). */
  hidden: number;
  hides: string[];
  /** The code line's height, expanded. */
  code: number;
}

/** What a node's header holds beside its label, for the room the label has. */
export interface HeadRoom {
  /** The label, as the header says it. */
  title: string;
  /** Icon buttons: ⏻, +, ⋯, ⤢. */
  buttons: number;
  /** Words beside the label: +N, "per note", its pack, Presets. */
  words: string[];
}

/** Whether `p` of `n` is a connection: a sound or notes coming in, a value a wire drives, the input a "Listens to" row stands for. */
export function isConn(n: ViewNode, p: InputPort) {
  if (p.knob) return false;
  if (p.kind === "audio" || p.kind === "notes" || p.inlet || n.listens?.port === p.id) return true;
  if (p.readonly != null || p.lane) return false;
  return true;
}

/** Input `p` of `n` as a value, or null when it is a connection. */
function valueOf(n: ViewNode, p: InputPort): Value | null {
  if (p.knob) return { t: "knob", port: p };
  if (isConn(n, p)) return null;
  if (p.readonly != null) return { t: "expr", port: p };
  return { t: "lane", port: p };
}

/** A row of a group or the face as a value, or null: a connection, or a setting not shown. */
function rowValue(n: ViewNode, r: CardRow): Value | null {
  if ("input" in r) {
    const p = n.inputs.find((x) => x.id === r.input);
    return p ? valueOf(n, p) : null;
  }
  if ("setting" in r) {
    const x = n.settings?.find((s) => s.id === r.setting);
    return x && shown(x) ? { t: "setting", x } : null;
  }
  const v = n.views?.find((x) => x.name === r.view);
  return v ? { t: "view", v } : null;
}

/** Every value of `n` as rows, in its order: its groups' rows, or its inputs, settings and views. */
export function valueRows(n: ViewNode): CardRow[] {
  if (n.groups?.length) return n.groups.flatMap((g) => g.rows);
  return [
    ...n.inputs.map((p): CardRow => ({ input: p.id })),
    ...(n.settings ?? []).map((x): CardRow => ({ setting: x.id })),
    ...(n.views ?? []).map((v): CardRow => ({ view: v.name })),
  ];
}

const same = (a: Value, b: Value) =>
  a.t === b.t && ("port" in a ? "port" in b && a.port.id === b.port.id : a.t === "setting" ? b.t === "setting" && a.x.id === b.x.id : a.t === "view" && b.t === "view" && a.v.name === b.v.name);
const knobOf = (v: Value) => ("port" in v ? v.port.knob : undefined);

/** The face (CARD-41, DV-12): the descriptor's, else the first group, else every value. */
export function faceOf(n: ViewNode): CardRow[] {
  if (n.face?.length) return n.face;
  if (n.groups?.length) return n.groups[0].rows;
  return valueRows(n);
}

/** A value's height in a column or a row of its own. */
function valueHeight(n: ViewNode, v: Value, expanded: boolean): number {
  switch (v.t) {
    case "knob": {
      const k = n.knobs.find((x) => x.id === v.port.knob);
      return expanded && k && picks(k) && k.kind === "choice" && (k.options?.length ?? 0) <= 5 ? SEG_H : VALUE_H;
    }
    case "lane":
      return VALUE_H;
    case "expr":
      return EXPR_H;
    case "setting":
      return settingHeight(v.x);
    case "view":
      return v.v.kind === "picture" ? (expanded ? EXPANDED_PICTURE_H : COLLAPSED_PICTURE_H) : VIEW_H[v.v.kind];
  }
}

/** How tall a collapsed node's values are: bars two to a row, a row naming a source across it. */
export function gridRows(values: Value[]): Value[][] {
  const rows: Value[][] = [];
  for (const v of values) {
    const last = rows.at(-1);
    if (v.t === "knob" && last?.length === 1 && last[0].t === "knob") last.push(v);
    else rows.push([v]);
  }
  return rows;
}

/** A canvas to measure words with, as the page draws them. */
let pen: CanvasRenderingContext2D | null = null;
let family = "";
function measure(text: string, font: string) {
  if (typeof document === "undefined") return text.length * 7;
  pen ??= document.createElement("canvas").getContext("2d");
  if (!pen) return text.length * 7;
  family ||= getComputedStyle(document.body).fontFamily || "sans-serif";
  pen.font = font.replace("{sans}", family);
  return pen.measureText(text).width;
}

/** How many lines `text` takes in `width` px at `font`, broken at its spaces, as the page wraps it. */
function linesOf(text: string, width: number, font: string) {
  if (width <= 0) return 3;
  const space = measure(" ", font);
  let lines = 1, at = 0;
  for (const word of text.split(/\s+/).filter(Boolean)) {
    let w = measure(word, font);
    // A word wider than the line breaks where it must.
    while (w > width) {
      if (at > 0) lines++;
      w -= width;
      at = 0;
      lines++;
    }
    if (at > 0 && at + space + w > width) {
      lines++;
      at = w;
    } else at = at > 0 ? at + space + w : w;
  }
  return lines;
}

/** The header's height: its label wraps rather than being cut, a line or two, three at the most (DV-12). */
function headHeight(w: number, room: HeadRoom) {
  const used = 12 + room.buttons * 22 + room.words.reduce((a, x) => a + measure(x, "500 11px {sans}") + 14, 0);
  // A little room spare, so the page's own wrapping never needs a line the layout didn't give.
  const lines = Math.min(3, linesOf(room.title, w - used - 4, "600 12px {sans}"));
  return HEAD_PAD + lines * HEAD_LINE;
}

/**
 * A lane, an import or the time with its one output, or an export with its one input: the header
 * names the signal, so the node is a pill, its name with its shape beside it.
 */
export const isPill = (n: ViewNode) =>
  n.kind !== "call" && n.inputs.length + n.outputs.length === 1 && n.inputs.every((i) => !i.knob && !i.inlet && i.readonly == null);

/**
 * What node `n` draws at its size, and where its inputs and outputs are, from its top. Each
 * connection is a row; output `i` sits on the right of the `i`th row, so a line of nodes draws
 * straight; outputs beyond them get rows of their own; a node with no rows has its first output on
 * its header. Expanding never moves a row: what changes is below them.
 */
export function shapeOf(n: ViewNode, expanded: boolean, room: (n: ViewNode, expanded: boolean, hidden: number) => HeadRoom): Shape {
  const ins = new Map<string, number>();
  const outs = new Map<string, number>();
  const base = { ins, outs, conns: [], fills: [], views: [], viewH: 0, values: [], columns: [], bands: [], hidden: 0, hides: [], code: 0, lines: 0 };
  if (isPill(n)) {
    for (const p of n.inputs) ins.set(p.id, PILL_H / 2);
    for (const p of n.outputs) outs.set(p.id, PILL_H / 2);
    return { ...base, w: SLIM_W, h: PILL_H, head: PILL_H, sized: false, expanded: false, pill: true };
  }
  const sized = n.kind === "call";
  const open = sized && expanded;
  // Its connections, in the order the code writes them; a "Listens to" whose input the call leaves
  // out stands right after the first, where its row would be (CARD-35).
  const conns: Conn[] = n.inputs.filter((p) => isConn(n, p)).map((port) => ({ t: "input", port }));
  if (n.listens && !n.listens.port) conns.splice(Math.min(1, conns.length), 0, { t: "listens" });
  // Its values: every one, each once.
  const all: Value[] = [];
  for (const r of valueRows(n)) {
    const v = rowValue(n, r);
    if (v && !all.some((x) => same(x, v))) all.push(v);
  }
  for (const x of (n.settings ?? []).filter(shown)) if (!all.some((v) => v.t === "setting" && v.x.id === x.id)) all.push({ t: "setting", x });
  const face: Value[] = [];
  for (const r of faceOf(n)) {
    const v = rowValue(n, r);
    if (v && !face.some((x) => same(x, v))) face.push(v);
  }
  // Collapsed: its face's first view of any kind, and up to four of its face's values but settings.
  const values = sized ? face.filter((v) => v.t === "knob" || v.t === "lane" || v.t === "expr").slice(0, FACE_VALUES) : all.filter((v) => v.t !== "view");
  const firstView = face.find((v): v is Extract<Value, { t: "view" }> => v.t === "view")?.v;
  const hiddenOnes = sized ? all.filter((v) => v.t !== "view" && !values.some((x) => same(x, v))) : [];
  const hidden = open ? 0 : hiddenOnes.length;
  const hides = open ? [] : hiddenOnes.flatMap((v) => knobOf(v) ?? []);
  // Expanded: the face's views across the top, then every group as a column.
  const faceViews = face.flatMap((v) => (v.t === "view" ? [v.v] : []));
  const columns = open ? columnsOf(n, face, faceViews) : [];
  const cols = Math.min(columns.length, MAX_COLS);
  const w = open ? Math.max(NODE_W, cols * COL_W) : NODE_W;
  // As tall at both sizes, so expanding never moves the end of a wire on it (DV-12).
  const head = sized
    ? Math.max(headHeight(NODE_W, room(n, false, hiddenOnes.length)), headHeight(w, room(n, true, 0)))
    : headHeight(w, room(n, false, 0));
  const lines = (n.voices ? LINE_H : 0) + (n.reads_key ? LINE_H : 0) + (n.plugin ? PLUGIN_H : 0);
  let y = head + lines;
  const rows: number[] = [];
  if (conns.length) {
    y += PAD;
    for (const c of conns) {
      if (c.t === "input") ins.set(c.port.id, y + ROW_H / 2);
      rows.push(y + ROW_H / 2);
      y += ROW_H;
    }
  }
  const fills: string[] = [];
  n.outputs.forEach((o, j) => {
    if (j < rows.length) outs.set(o.id, rows[j]);
    else if (j === 0 && !rows.length) outs.set(o.id, head / 2);
    else fills.push(o.id);
  });
  if (fills.length && !rows.length) y += PAD;
  for (const id of fills) {
    outs.set(id, y + ROW_H / 2);
    y += ROW_H;
  }
  if (conns.length || fills.length) y += PAD;
  const views = open ? faceViews : firstView && sized ? [firstView] : sized ? [] : (n.views ?? []);
  const viewH = views.length ? Math.max(...views.map((v) => valueHeight(n, { t: "view", v }, open))) : 0;
  if (views.length) y += viewH + (sized ? 0 : (views.length - 1) * COLLAPSED_PICTURE_H);
  let bands: number[] = [];
  if (open) {
    for (let i = 0; i < columns.length; i += MAX_COLS) bands.push(Math.max(...columns.slice(i, i + MAX_COLS).map((c) => c.h)));
    y += bands.reduce((a, b) => a + b, 0);
  } else if (values.length) {
    y += PAD;
    for (const row of gridRows(sized ? values : values.map((v) => v))) y += sized ? Math.max(...row.map((v) => valueHeight(n, v, false))) : 0;
    if (!sized) for (const v of values) y += valueHeight(n, v, false);
    y += PAD;
  }
  const code = open && n.code ? CODE_PAD + CODE_LINE * linesOf(n.code, w - 24, "400 11px ui-monospace, SFMono-Regular, Menlo, monospace") : 0;
  y += code;
  if (!sized) bands = [];
  return { ...base, w, h: Math.max(y, head), sized, expanded: open, pill: false, head, lines, conns, fills, views, viewH, values, columns, bands, hidden, hides, code };
}

/**
 * An expanded node's columns (DV-12): each group in order, the face's values that are in no named
 * group first in the first column, values in no group last in More, and settings in no group in
 * Setup after them (DV-8). A view on the face is across the top, so in no column; a group's own
 * views sit at the top of its column. A value a wire drives is a row above, so in no column either.
 */
function columnsOf(n: ViewNode, face: Value[], faceViews: CardView[]): Column[] {
  const groups = n.groups?.length ? n.groups : [{ name: "Values", rows: valueRows(n) }];
  const named = groups.filter((g) => g.name !== "More");
  const inNamed = (v: Value) => named.some((g) => g.rows.some((r) => same(rowValue(n, r) ?? ({ t: "view", v: { name: "" } } as Value), v)));
  const lead = face.filter((v) => v.t !== "view" && v.t !== "setting" && !inNamed(v));
  const out: Column[] = [];
  const setup: Value[] = [];
  groups.forEach((g, i) => {
    const items: Value[] = [];
    const add = (v: Value | null) => v && !items.some((x) => same(x, v)) && items.push(v);
    if (i === 0) lead.forEach(add);
    const rows = g.rows.map((r) => rowValue(n, r)).filter((v): v is Value => !!v && !(v.t === "view" && faceViews.some((f) => f.name === v.v.name)));
    const rest = i === 0 ? rows : rows.filter((v) => !lead.some((x) => same(x, v)));
    // A group's own views at the top of its column.
    for (const v of rest.filter((v) => v.t === "view")) add(v);
    for (const v of rest.filter((v) => v.t !== "view")) {
      if (v.t === "setting" && (g.name === "More" || !n.groups?.length)) setup.push(v);
      else add(v);
    }
    if (items.length) out.push({ name: g.name, items, h: 0 });
  });
  // A setting no group names, when the node has groups but no More (every row named).
  for (const x of (n.settings ?? []).filter(shown)) {
    const v: Value = { t: "setting", x };
    if (!out.some((c) => c.items.some((y) => same(y, v))) && !setup.some((y) => same(y, v))) setup.push(v);
  }
  if (setup.length) out.push({ name: "Setup", items: setup, h: 0 });
  for (const c of out) c.h = GROUP_H + PAD + c.items.reduce((a, v) => a + valueHeight(n, v, true), 0) + PAD;
  return out;
}
