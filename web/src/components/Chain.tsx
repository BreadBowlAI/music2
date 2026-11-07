// Device (taste.md §3.5): a sound that is a chain as its cards left to right, with no ports and no
// cables (CARD-42), else its patch. Each chain card is a frame (CARD-43 to CARD-47): a header with
// its three sizes, its face drawn big, its tabs (its groups, then All), one tab's values in a grid,
// and the code it writes. The sizes, the tabs and the Chain · Patch choice are the window's own.

import clsx from "clsx";
import { ChevronRight, Maximize2, Minus, Square } from "lucide-react";
import { useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { controlLabel, display, lockReason, moduleName, picks, type CardRow, type GraphState, type ModuleView, type ViewNode } from "../lib/graph";
import { ALL, chainOf, frameOf, knobsOf, packOf, type Branch, type FrameTab } from "../lib/frame";
import { useCards, type CardSize } from "../lib/folded";
import { useNotice } from "../lib/notice";
import { opt } from "../lib/native";
import { trackColors } from "../lib/palette";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { ViewRow } from "./CardViews";
import { DockSlot } from "./Dock";
import { CardTools, Fed, InputRow, KeyLine, ListensRow, Locked, MixerCard, NodeGraph, PluginRow, Ports, stableKeys, titleOf, Tracks, VoicesLine } from "./NodeGraph";
import { SettingRow, shown } from "./Settings";
import { Segment } from "./ui";

/** The widths of a chain card (CARD-44), in px: Folded, and Normal without and with a picture on its face; Expanded is twice Normal. */
const FOLDED_W = 240;
const NORMAL_W = 380;
const PICTURE_W = 460;
/** A knob's size on the face, and on a tab (CARD-45). */
const FACE_KNOB = 36;
const TAB_KNOB = 28;
/** Expanded draws its face bigger (CARD-44): its knobs this size. */
const BIG_KNOB = 44;
/** How tall a picture is on a Normal card's face, one or two of them, so the card fits the dock's first height (CARD-43); Expanded draws each at its own size. */
const FACE_PICTURE_H = [104, 76];

/**
 * Device: the chain where the sound is one and the window wants it, else the patch (CARD-42). The
 * Chain · Patch switch is always in its tools; Chain is greyed, saying why, where the sound isn't a
 * chain. The patch of a sound that is a chain says Chain is there (review F15).
 */
export function Device({ graph, module }: { graph: GraphState; module: string }) {
  const found = graph.view.modules.find((m) => m.key === module);
  const of = useMemo(() => (found ? chainOf(found) : null), [found]);
  const chain = of && "line" in of ? of : null;
  const want = useShell((s) => s.device);
  const why = of && "why" in of ? `${of.why} Showing its patch.` : null;
  const hint = chain && want === "patch" ? "This sound is a chain: Chain shows it as cards." : null;
  return (
    <>
      {found && (
        <DockSlot at="tools">
          <div role="radiogroup" aria-label="Show the sound as" className="shrink-0 h-ctl-sm flex items-center gap-0.5 p-0.5 rounded-sm bg-hover">
            {(["chain", "patch"] as const).map((d) => (
              <Segment
                key={d}
                role="radio"
                aria-checked={(chain ? want : "patch") === d}
                data-device={d}
                on={(chain ? want : "patch") === d}
                disabled={d === "chain" && !chain}
                title={d === "chain" ? (chain ? "The sound's cards, left to right" : (why ?? "This sound has no chain")) : "The sound's patch: its parts and their cables"}
                onClick={() => useShell.getState().setDevice(d)}
              >
                {d === "chain" ? "Chain" : "Patch"}
              </Segment>
            ))}
          </div>
        </DockSlot>
      )}
      {(why || hint) && (
        <DockSlot at="title">
          <span data-testid={why ? "device-parts" : "device-chain-hint"}>{why ?? hint}</span>
        </DockSlot>
      )}
      {chain && found && want === "chain" ? <Chain graph={graph} module={found} chain={chain} /> : <NodeGraph graph={graph} module={module} />}
    </>
  );
}

/**
 * The chain: its cards left to right, at full size, scrolling sideways (taste.md §3.5). A mix (the
 * Song) has a row for each track's sound it reads, that sound's effects after it, then its line.
 */
function Chain({ graph, module, chain }: { graph: GraphState; module: ModuleView; chain: { line: ViewNode[]; branches: Branch[] } }) {
  const locked = useStore((s) => lockReason(graph, s.connected));
  const tracks = useMemo(() => {
    const by = new Map(graph.view.modules.filter((m) => m.track).map((m) => [m.key, moduleName(m)]));
    return (key: string) => by.get(key);
  }, [graph.view.modules]);
  // What drives each input no cable names, for the "← Blend" its row reads (CARD-42).
  const fed = useMemo(() => {
    const named = new Map<string, string>();
    for (const e of module.port_edges) {
      const from = module.nodes.find((n) => n.id === e.from);
      if (from) named.set(`${e.to}<${e.to_port}`, titleOf(from, graph.effects));
    }
    return named;
  }, [module, graph.effects]);
  // The Song's mixer is one compact card (MX-6), its levels and pans the Mix view's.
  const mixer = useMemo(() => {
    if (module.key !== "song" || graph.mix.mixer !== "ok") return null;
    const node = chain.line.find((n) => n.component === "std:mixer");
    return node ? { id: node.id, channels: new Set(graph.mix.strips.flatMap((s) => (s.channel ? [s.channel] : []))).size } : null;
  }, [module.key, chain, graph.mix]);
  const keys = useMemo(() => stableKeys(module.nodes, graph.leases), [module, graph.leases]);
  const frame = (n: ViewNode) => (
    <Frame
      key={keys.node(n.id)}
      graph={graph}
      module={module.key}
      node={n}
      nodeKey={keys.node(n.id)}
      knobKey={keys.knob}
      editable={!locked}
      mixer={mixer?.id === n.id ? mixer.channels : null}
    />
  );
  return (
    <Locked.Provider value={locked}>
      <Tracks.Provider value={tracks}>
        <MixerCard.Provider value={mixer}>
          <Ports.Provider value={false}>
            <Fed.Provider value={fed}>
              <div className="flex-1 min-h-0 overflow-auto p-3" data-testid="chain">
                <section className="w-max flex items-start gap-3" data-module={module.key}>
                  {chain.branches.length > 0 && (
                    <div className="flex flex-col gap-2" data-branches>
                      {chain.branches.map((b) => (
                        <div key={b.from.id} className="flex items-start gap-3" data-branch={titleOf(b.from, graph.effects)}>
                          <TrackSound node={b.from} module={module.key} graph={graph} editable={!locked} />
                          {b.cards.map(frame)}
                        </div>
                      ))}
                    </div>
                  )}
                  {chain.line.map(frame)}
                </section>
              </div>
            </Fed.Provider>
          </Ports.Provider>
        </MixerCard.Provider>
      </Tracks.Provider>
    </Locked.Provider>
  );
}

/**
 * A track's sound where a mix reads it (`Bass.out` in the song): its name, with its colour, and the +
 * that adds an effect after it there (CARD-36), so its row reads "Bass, then its effects".
 */
function TrackSound({ node, module, graph, editable }: { node: ViewNode; module: string; graph: GraphState; editable: boolean }) {
  const titled = titleOf(node, graph.effects);
  const track = graph.view.modules.find((m) => m.track && moduleName(m) === titled)?.key;
  const colour = track ? trackColors(graph.timeline, track) : null;
  return (
    <div
      className="shrink-0 w-[152px] h-9 pl-3 pr-1.5 rounded-md bg-raised ring-1 ring-line flex items-center gap-2 min-w-0"
      data-node={node.label}
      data-node-id={node.id}
      title={`${titled}'s sound, as ${module === "song" ? "the song" : "this sound"} reads it`}
    >
      {colour && <span aria-hidden className="size-2 shrink-0 rounded-xs" style={{ background: colour.bar }} />}
      <span data-sound-title className="min-w-0 flex-1 text-sm font-semibold text-fg truncate">
        {titled}
      </span>
      <CardTools node={node} module={module} titled={titled} session={graph.session} revision={graph.view.revision} editable={editable} effects={graph.effects} mixing={false} />
    </div>
  );
}

const SIZES: { size: CardSize; Icon: typeof Minus; says: string }[] = [
  { size: "folded", Icon: Minus, says: "Fold" },
  { size: "normal", Icon: Square, says: "Normal size" },
  { size: "expanded", Icon: Maximize2, says: "Expand" },
];

/** One chain card: header, face, tabs and the code line (CARD-43). */
function Frame({
  graph,
  module,
  node,
  nodeKey,
  knobKey,
  editable,
  mixer,
}: {
  graph: GraphState;
  module: string;
  node: ViewNode;
  nodeKey: string;
  knobKey: (id: string) => string;
  editable: boolean;
  /** The Song's mixer as its compact card: how many channels it has (MX-6). */
  mixer: number | null;
}) {
  const titled = titleOf(node, graph.effects);
  const off = !!node.bypassed;
  const size = useCards((s) => s.sizes[node.id] ?? "normal");
  const picked = useCards((s) => s.tabs[node.id]);
  const frame = useMemo(() => frameOf(node), [node]);
  const tab = frame.tabs.find((t) => t.name === picked) ?? frame.tabs[0];
  const pack = packOf(node);
  const recordings = (node.settings ?? []).filter((x) => x.kind === "recording" && !x.readonly && shown(x));
  // An instrument (it takes notes) wears its track's colour along its top (taste.md §3.5).
  const instrument = node.inputs.some((i) => i.kind === "notes");
  const colour = instrument ? trackColors(graph.timeline, module) : null;
  const faceHasPicture = frame.face.some((r) => "view" in r);
  const normal = faceHasPicture ? PICTURE_W : NORMAL_W;
  const width = size === "folded" ? FOLDED_W : size === "expanded" ? normal * 2 : normal;
  const row = (r: CardRow, cell: number | undefined, ided = false, height?: number) => (
    <RowOf key={key(r)} row={r} node={node} module={module} titled={titled} nodeKey={nodeKey} knobKey={knobKey} graph={graph} editable={editable} cell={cell} ided={ided} height={height} />
  );
  const grid = (rows: CardRow[], cell: number) => <div className="grid grid-cols-[repeat(auto-fill,64px)] justify-between gap-y-1 px-3 py-1">{rows.map((r) => row(r, cell))}</div>;
  // The face: its pictures beside its values, so a Normal card fits the dock's first height (review
  // F12); Expanded draws its values bigger, under its pictures (CARD-44).
  const pictures = frame.face.filter((r) => "view" in r);
  const values = frame.face.filter((r) => !("view" in r));
  const face =
    size === "expanded" ? (
      <div data-face>
        {pictures.map((r) => row(r, undefined))}
        {grid(values, BIG_KNOB)}
      </div>
    ) : pictures.length && values.length ? (
      <div data-face className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-start">
        <div className="pt-1">{pictures.map((r) => row(r, undefined, false, FACE_PICTURE_H[Math.min(pictures.length, 2) - 1]))}</div>
        {grid(values, FACE_KNOB)}
      </div>
    ) : (
      <div data-face>{grid(frame.face, FACE_KNOB)}</div>
    );
  // All: every value in one list, as the patch's card shows it, each with its id in its tooltip.
  const list = (rows: CardRow[]) => <div className="py-1">{rows.map((r) => row(r, undefined, true))}</div>;
  const body = (t: FrameTab) => (t.name === ALL ? list(t.rows) : grid(t.rows, TAB_KNOB));
  // "Listens to" (CARD-35), a connection the chain still shows: what its input hears, after the face.
  const heard = node.listens?.port ? node.inputs.find((p) => p.id === node.listens!.port) : undefined;
  const listens = node.listens ? <ListensRow node={node} input={heard} filled={false} editable={editable} /> : null;
  return (
    <div
      className="group/card shrink-0 rounded-md bg-raised ring-1 ring-line flex flex-col overflow-hidden"
      style={{ width }}
      data-node={node.label}
      data-node-id={node.id}
      data-frame={size}
      data-bypassed={off || undefined}
      // A card with one recording takes a file dropped anywhere on it, as the patch's card does (CARD-5).
      data-recording-call={recordings.length === 1 ? node.id : undefined}
      data-recording-setting={recordings.length === 1 ? recordings[0].slot : undefined}
      data-recording-title={recordings.length === 1 ? titled : undefined}
      title={node.file ? `${node.file}:${node.line}` : undefined}
    >
      {colour && <div data-colour-bar className="h-[3px] shrink-0" style={{ background: colour.bar }} />}
      <div className="h-9 shrink-0 pl-3 pr-1.5 flex items-center gap-2 min-w-0 border-b border-line">
        <div className="min-w-[64px] flex-1 flex items-baseline gap-1.5">
          <span data-node-title className={clsx("text-sm font-semibold truncate", off ? "text-fg-muted" : "text-fg")} title={titled !== node.label ? node.label : undefined}>
            {off ? `${titled} · Off` : titled}
          </span>
          {pack && size !== "folded" && (
            <span data-pack className="shrink-0 text-xs text-fg-faint">
              {pack}
            </span>
          )}
        </div>
        <div className="shrink-0 flex items-center" role="group" aria-label={`${titled}'s size`}>
          {SIZES.map(({ size: to, Icon, says }) => (
            <button
              key={to}
              data-size-to={to}
              aria-pressed={size === to}
              aria-label={`${says} ${titled}`}
              title={`${says}: this window only, the song is unchanged`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => useCards.getState().size(node.id, to)}
              className={clsx(
                "size-ctl-xs grid place-items-center rounded-sm cursor-pointer transition-colors duration-80 ease-linear",
                size === to ? "bg-hover text-fg" : "text-fg-faint hover:text-fg hover:bg-hover",
              )}
            >
              <Icon size={12} absoluteStrokeWidth strokeWidth={1.5} />
            </button>
          ))}
        </div>
        {/* Folded, its header is its name and its sizes only, so the name always shows (review F3). */}
        {size !== "folded" && (
          <CardTools node={node} module={module} titled={titled} session={graph.session} revision={graph.view.revision} editable={editable} effects={graph.effects} mixing={mixer != null} />
        )}
      </div>
      {node.voices && size !== "folded" && <VoicesLine words={node.voices} />}
      {node.reads_key && size !== "folded" && <KeyLine />}
      {node.plugin && size !== "folded" && <PluginRow node={node.plugin} title={titled} session={graph.session} />}
      <div data-node-body className={clsx("flex flex-col min-w-0", off && "opacity-50")}>
        {mixer != null ? (
          <div className="h-9 px-3 flex items-center gap-2 text-xs min-w-0">
            <span className="text-fg-muted truncate">
              {mixer} {mixer === 1 ? "channel" : "channels"}
            </span>
            <button
              data-testid="open-mix"
              className="ml-auto shrink-0 font-medium text-accent hover:text-accent-strong cursor-pointer"
              title="Levels and pans are in the Mix view"
              onClick={() => useShell.getState().setView("mix")}
            >
              Open Mix {opt("3")}
            </button>
          </div>
        ) : size === "folded" ? (
          <Sentence node={node} rows={frame.face} />
        ) : (
          <>
            {face}
            {listens}
            {size === "normal" && tab && (
              <>
                <TabBar node={node} titled={titled} tabs={frame.tabs} shown={tab.name} drawn={[...knobsOf(node, frame.face), ...knobsOf(node, tab.rows)]} />
                <div role="tabpanel" data-tab-panel={tab.name} aria-label={tab.name}>
                  {body(tab)}
                </div>
              </>
            )}
            {size === "expanded" && frame.tabs.length > 0 && (
              <>
                <div className="flex flex-wrap border-t border-line">
                  {frame.tabs
                    .filter((t) => t.name !== ALL)
                    .map((t) => (
                      <section key={t.name} data-tab-panel={t.name} aria-label={t.name} className="min-w-[200px] flex-1 border-r border-line last:border-r-0">
                        <div className="h-6 px-3 flex items-end text-xs font-semibold text-fg-muted">{t.name}</div>
                        {grid(t.rows, TAB_KNOB)}
                      </section>
                    ))}
                </div>
                {/* All, behind a disclosure; open when it is the only tab. */}
                <details data-all className="group/all border-t border-line" open={frame.tabs.length === 1 || undefined}>
                  <summary className="h-7 px-3 flex items-center gap-1 text-xs font-semibold text-fg-muted cursor-pointer list-none [&::-webkit-details-marker]:hidden hover:text-fg">
                    <ChevronRight size={12} className="shrink-0 transition-transform duration-140 ease-out group-open/all:rotate-90" />
                    All values
                  </summary>
                  {list(frame.tabs.find((t) => t.name === ALL)!.rows)}
                </details>
              </>
            )}
          </>
        )}
      </div>
      {node.code && size !== "folded" && (
        // The whole call, wrapped as the mockup draws it; past three lines the rest is in its tooltip (review F10).
        <div data-code-line data-code className="mt-auto px-3 py-1.5 border-t border-line font-mono text-xs text-fg-muted whitespace-pre-wrap break-words line-clamp-3" title={node.code}>
          {node.code}
        </div>
      )}
    </div>
  );
}

const key = (r: CardRow) => ("input" in r ? `input:${r.input}` : "setting" in r ? `setting:${r.setting}` : `view:${r.view}`);

/** One row of a chain card, by what it names: an input, a setting or a view (CARD-45). */
function RowOf({
  row,
  node,
  module,
  titled,
  nodeKey,
  knobKey,
  graph,
  editable,
  cell,
  ided,
  height,
}: {
  row: CardRow;
  node: ViewNode;
  module: string;
  titled: string;
  nodeKey: string;
  knobKey: (id: string) => string;
  graph: GraphState;
  editable: boolean;
  cell?: number;
  ided: boolean;
  /** A view's height, in place of its own: a picture on a Normal face. */
  height?: number;
}) {
  const locked = useStore((s) => lockReason(graph, s.connected));
  const tracks = useContext(Tracks);
  const wide = (el: React.ReactNode) => (cell ? <div className="col-span-full min-w-0">{el}</div> : el);
  if ("input" in row) {
    const port = node.inputs.find((p) => p.id === row.input);
    if (!port) return null;
    return (
      <InputRow
        nodeKey={nodeKey}
        knobKey={knobKey}
        node={node}
        owner={titled}
        input={port}
        filled={false}
        session={graph.session}
        revision={graph.view.revision}
        editable={editable}
        cell={cell}
        ided={ided}
      />
    );
  }
  if ("setting" in row) {
    const x = node.settings?.find((s) => s.id === row.setting);
    if (!x || !shown(x)) return null;
    return wide(
      <SettingRow
        x={x}
        owner={titled}
        card={titled}
        call={node.id}
        session={graph.session}
        revision={graph.view.revision}
        editable={editable}
        locked={locked}
        tracks={tracks}
        ided={ided}
      />,
    );
  }
  const v = node.views?.find((x) => x.name === row.view);
  if (!v) return null;
  return wide(<ViewRow view={v} height={height} node={node} module={module} session={graph.session} revision={graph.view.revision} editable={editable} locked={locked} owner={titled} />);
}

/**
 * A Folded card's one sentence (CARD-44): its face's values, "Cutoff 2.4 kHz · Attack 5 ms". It
 * stands for every knob it hides (CARD-47), and a click opens the card to Normal.
 */
function Sentence({ node, rows }: { node: ViewNode; rows: CardRow[] }) {
  const hides = node.knobs.map((k) => k.id);
  const glows = useNotice((s) => hides.some((id) => !!s.glow?.knobs.has(id)));
  // The words of the face's first picture, once the server worked them out (CARD-63), else its values.
  const first = rows.flatMap((r) => ("view" in r ? (node.views ?? []).filter((v) => v.name === r.view && v.kind === "picture") : []))[0];
  const words = first?.kind === "picture" ? first.words : null;
  const values = rows
    .flatMap((r) => {
      if (!("input" in r)) return [];
      const p = node.inputs.find((x) => x.id === r.input);
      const k = p?.knob ? node.knobs.find((x) => x.id === p.knob) : undefined;
      if (!p || !k) return [];
      const value = picks(k) ? (k.options?.find((o) => o.value === k.value)?.label ?? String(k.value)) : display(k, k.value);
      // Named as its knob is, by the one rule (a level is Volume, never "Db"; review S7).
      return [`${controlLabel(k)} ${value}`];
    })
    .slice(0, 4)
    .join(" · ");
  const said = words ?? values;
  return (
    <button
      data-sentence
      data-hides={hides.join(" ") || undefined}
      data-glow={glows || undefined}
      className="h-9 px-3 flex items-center text-left text-xs text-fg-muted hover:text-fg cursor-pointer min-w-0"
      title="Open it to Normal size"
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => useCards.getState().size(node.id, "normal")}
    >
      <span className="truncate">{said || "Open it to see its values"}</span>
    </button>
  );
}

/**
 * The tab bar (CARD-43): each tab its name, a dot for its `on…` switch, and the knobs it hides when
 * not shown (CARD-47). ← → pick the previous and the next (CARD-46). Too wide, it scrolls, faded at
 * an edge that hides tabs.
 */
function TabBar({ node, titled, tabs, shown, drawn }: { node: ViewNode; titled: string; tabs: FrameTab[]; shown: string; drawn: string[] }) {
  const bar = useRef<HTMLDivElement>(null);
  const [fade, setFade] = useState({ left: false, right: false });
  const measure = () => {
    const el = bar.current;
    if (!el) return;
    const left = el.scrollLeft > 1, right = el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
    setFade((f) => (f.left === left && f.right === right ? f : { left, right }));
  };
  useLayoutEffect(measure, [tabs]);
  useEffect(() => {
    const el = bar.current;
    if (!el) return;
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  const pick = (name: string, focus: boolean) => {
    useCards.getState().tab(node.id, name);
    if (focus) requestAnimationFrame(() => bar.current?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(name)}"]`)?.focus());
  };
  const mask =
    fade.left || fade.right
      ? `linear-gradient(to right, ${fade.left ? "transparent" : "black"}, black 24px, black calc(100% - 24px), ${fade.right ? "transparent" : "black"})`
      : undefined;
  return (
    <div
      ref={bar}
      role="tablist"
      aria-label={`${titled}'s tabs`}
      data-tabs={node.id}
      data-fade={[fade.left && "left", fade.right && "right"].filter(Boolean).join(" ") || undefined}
      // A track of tabs, the one shown raised, as the mockup and a segmented control draw it (taste.md §2.9).
      className="mx-3 my-1 p-0.5 rounded-sm bg-hover flex items-center gap-0.5 overflow-x-auto [scrollbar-width:none]"
      style={{ maskImage: mask, WebkitMaskImage: mask }}
      onScroll={measure}
    >
      {tabs.map((t, i) => (
        <Tab
          key={t.name}
          node={node}
          tab={t}
          drawn={drawn}
          on={t.name === shown}
          onPick={() => pick(t.name, false)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
            e.preventDefault();
            const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
            pick(next.name, true);
          }}
        />
      ))}
    </div>
  );
}

function Tab({ node, tab, drawn, on, onPick, onKeyDown }: { node: ViewNode; tab: FrameTab; drawn: string[]; on: boolean; onPick: () => void; onKeyDown: (e: React.KeyboardEvent) => void }) {
  // The knobs only this tab draws, while it isn't shown: it stands for them (CARD-47), but not for
  // one drawn in sight now, on the face or the tab shown (review F8).
  const hides = on ? [] : knobsOf(node, tab.rows).filter((id) => !drawn.includes(id));
  const glows = useNotice((s) => hides.some((id) => !!s.glow?.knobs.has(id)));
  // A switch named on… among its values: a dot on the tab, filled when on (the EQ's bands).
  const power = tab.name === ALL ? undefined : tab.rows.flatMap((r) => ("input" in r ? [node.inputs.find((p) => p.id === r.input)] : [])).find((p) => p?.slot && /^on($|[A-Z0-9_])/.test(p.slot.split(".").pop()!));
  const lit = power?.knob ? node.knobs.find((k) => k.id === power.knob) : undefined;
  const dot = lit && lit.kind === "switch" ? lit.value === true : undefined;
  return (
    <button
      role="tab"
      aria-selected={on}
      tabIndex={on ? 0 : -1}
      data-tab={tab.name}
      data-hides={hides.join(" ") || undefined}
      data-glow={glows || undefined}
      data-on={dot === undefined ? undefined : String(dot)}
      title={tab.name === ALL ? "Every value, each with its id for m2 set (← → for the next tab)" : `${tab.name}${dot === undefined ? "" : dot ? ": on" : ": off"} (← → for the next tab)`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
      onKeyDown={onKeyDown}
      className={clsx(
        "shrink-0 h-5 px-2 flex items-center gap-1.5 rounded-sm text-xs font-medium cursor-pointer transition-colors duration-80 ease-linear",
        on ? "bg-raised text-fg shadow-chip" : "text-fg-muted hover:text-fg",
      )}
    >
      {dot !== undefined && <span aria-hidden className={clsx("size-1.5 rounded-full", dot ? "bg-fg" : "ring-1 ring-inset ring-fg-faint")} />}
      {tab.name}
    </button>
  );
}
