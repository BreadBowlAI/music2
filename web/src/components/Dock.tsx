import clsx from "clsx";
import { ChevronDown, ChevronUp, CodeXml, Drum, Piano, SlidersHorizontal, SlidersVertical } from "lucide-react";
import { createContext, memo, useContext, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { dockView, findNoteClip, kitOf, moduleName, type GraphState } from "../lib/graph";
import { opt } from "../lib/native";
import { GRAPHITE, trackColors } from "../lib/palette";
import { clampDock, useShell, VIEWS, type DockView } from "../lib/shell";
import BeatView from "./BeatView";
import { addEffectTo, dragsEffect, EFFECT_DRAG } from "./Browser";
import CodeView from "./CodeView";
import Mix from "./Mix";
import { NodeGraph } from "./NodeGraph";
import NoteView from "./NoteView";
import TrackName from "./TrackName";
import { Button, EmptyState, keepFocus, Segment } from "./ui";

/** Where a view puts its own part of the dock's header: what it shows, after the switcher, and its tools on the right. */
const Slots = createContext<{ title: HTMLElement | null; tools: HTMLElement | null }>({ title: null, tools: null });

/** Render `children` in the dock header's title (after the switcher) or tools (right) slot. */
export function DockSlot({ at, children }: { at: "title" | "tools"; children: React.ReactNode }) {
  const slot = useContext(Slots)[at];
  return slot ? createPortal(children, slot) : null;
}

const VIEW: Record<DockView, { label: string; Icon: typeof Piano; tab: string }> = {
  // The id the graph tab had: the Device view shows the track's sound (its patch, until the chain view).
  device: { label: "Device", Icon: SlidersHorizontal, tab: "graph" },
  notes: { label: "Notes", Icon: Piano, tab: "notes" },
  beat: { label: "Beat", Icon: Drum, tab: "beat" },
  mix: { label: "Mix", Icon: SlidersVertical, tab: "mix" },
  code: { label: "Code", Icon: CodeXml, tab: "code" },
};

/** The switcher's order: Beat beside Notes (KIT-10), shown only while a drum kit's track is selected. */
const TABS: DockView[] = ["device", "notes", "beat", "mix", "code"];

/**
 * The dock (taste.md §3.5): a 32px header (the selection, the view switcher, the view's tools, hide)
 * over the view. Its view is one choice for the window; what is selected decides only what it shows
 * (§3.7), and Notes shows in Beat's place while the selection is in no drum kit (KIT-10). Hidden
 * (E), its header stays, to bring it back.
 */
export default function Dock({ graph, module }: { graph: GraphState; module: string }) {
  const chosen = useShell((s) => s.view);
  const view = dockView(graph, chosen, module);
  const kit = kitOf(graph, module);
  const open = useShell((s) => s.dock);
  const saved = useShell((s) => s.height);
  // Held to the window: the lanes never get less than three rows and the Song's (§3.8).
  const [, setWindowHeight] = useState(window.innerHeight);
  useEffect(() => {
    const on = () => setWindowHeight(window.innerHeight);
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  const height = clampDock(saved);
  const [title, setTitle] = useState<HTMLElement | null>(null);
  const [tools, setTools] = useState<HTMLElement | null>(null);
  const shown = graph.view.modules.find((m) => m.key === module);
  // Before the code first compiles there are no modules, only the timeline's tracks from data/ (D2).
  const label = shown?.label ?? graph.timeline.tracks.find((t) => t.module === module)?.label ?? module;
  const color = trackColors(graph.timeline, module) ?? GRAPHITE;
  const name = moduleName({ key: module, label });
  return (
    <section id="dock" data-testid="dock" aria-label={`${VIEW[view].label}: ${name}`} className="shrink-0 flex flex-col bg-canvas">
      {/* A container, so a view's tools give up their words as the header narrows (taste.md §3.8). */}
      <div data-testid="dock-header" className="@container/dock h-dockbar shrink-0 flex items-center gap-3 pl-3 pr-1.5 bg-chrome border-b border-line">
        {/* The selection: what the dock shows. A fixed width, so the switcher after it never moves with
            the name (principle 8); a long name is cut, whole in its tooltip. A double-click renames the track. */}
        <div data-testid="dock-chip" className="w-40 shrink-0 flex items-center gap-2 min-w-0 text-sm">
          <span className="size-2 shrink-0 rounded-xs" style={{ background: color.bar }} />
          <TrackName module={module} label={label} track={!!shown?.track} testid="module-title" className="min-w-0 text-sm font-semibold text-fg" />
        </div>
        <div role="tablist" aria-label="Views" className="shrink-0 h-ctl-sm flex items-center gap-0.5 p-0.5 rounded-sm bg-hover">
          {TABS.filter((v) => v !== "beat" || kit).map((v) => {
            const { label, Icon, tab } = VIEW[v];
            return (
              <Segment
                key={v}
                role="tab"
                aria-selected={view === v}
                aria-controls="dock-body"
                data-view-tab={tab}
                on={view === v}
                title={`${label} (${opt(String(VIEWS.indexOf(v) + 1))})`}
                onClick={() => useShell.getState().setView(v)}
              >
                <Icon size={14} />
                {label}
              </Segment>
            );
          })}
        </div>
        {/* What the view shows (the clip's bars), after the switcher; the first thing to go when the header narrows. */}
        <span ref={setTitle} data-testid="dock-detail" className="min-w-0 flex items-center gap-1.5 text-sm text-fg-muted whitespace-nowrap @max-[1080px]/dock:hidden" />
        {/* Right-aligned by the first tool's margin, so tools wider than the room scroll rather than being cut at the left. */}
        <div ref={setTools} className="flex-1 min-w-0 flex items-center gap-3 overflow-x-auto overflow-y-hidden [&>:first-child]:ml-auto" />
        <Button
          size="sm"
          onMouseDown={keepFocus}
          onClick={() => useShell.getState().setDock(!open)}
          title={open ? "Hide the dock (E)" : "Show the dock (E)"}
          aria-label={open ? "Hide the dock" : "Show the dock"}
          aria-expanded={open}
          className="w-ctl-sm !px-0 shrink-0"
        >
          {open ? <ChevronDown size={16} /> : <ChevronUp size={16} />}
        </Button>
      </div>
      <Slots.Provider value={{ title, tools }}>
        <div
          id="dock-body"
          role="tabpanel"
          className={clsx("relative flex flex-col min-h-0", !open && "hidden")}
          style={{ height }}
          // An effect dragged from the browser onto Device goes at the end of the sound shown.
          onDragOver={(e) => {
            if (view !== "device" || !dragsEffect(e)) return;
            e.preventDefault();
            e.dataTransfer.dropEffect = "copy";
          }}
          onDrop={(e) => {
            const component = e.dataTransfer.getData(EFFECT_DRAG);
            if (view !== "device" || !component) return;
            e.preventDefault();
            addEffectTo(module, component);
          }}
        >
          <Body graph={graph} module={module} view={view} open={open} />
        </div>
      </Slots.Provider>
    </section>
  );
}

/** The view's body. Memoized: the playback status re-renders the canvas five times a second. */
const Body = memo(function Body({ graph, module, view, open }: { graph: GraphState; module: string; view: DockView; open: boolean }) {
  const clip = useShell((s) => s.clips[module]);
  const kit = kitOf(graph, module);
  // Once opened, the code editor stays mounted, hidden behind the other views: its unsaved text,
  // history and ⌘S hook outlive a look at the graph or a clip.
  const [coded, setCoded] = useState(view === "code");
  if (view === "code" && !coded) setCoded(true);
  const live = clip && findNoteClip(graph.timeline, clip) ? clip : null;
  return (
    <>
      {open && view === "device" && <NodeGraph graph={graph} module={module} />}
      {open && view === "notes" &&
        (live ? (
          <NoteView key={`${live.module}:${live.lane}:${live.id}`} graph={graph} clip={live} />
        ) : (
          <Empty testid="notes">Select a clip, or double-click in the timeline to make one.</Empty>
        ))}
      {open && view === "beat" && kit && <BeatView graph={graph} kit={kit} module={module} />}
      {open && view === "mix" && <Mix graph={graph} />}
      {/* No module before the code first compiles (WRITE-20): the editor opens on the song's files. */}
      {coded && (
        <div className={clsx("flex-1 min-h-0 flex-col", view === "code" && open ? "flex" : "hidden")}>
          <CodeView graph={graph} module={module} />
        </div>
      )}
    </>
  );
});

function Empty({ testid, children }: { testid?: string; children: React.ReactNode }) {
  return (
    <div data-testid={testid} data-empty className="flex-1 min-h-0 grid place-items-center px-6">
      <EmptyState>{children}</EmptyState>
    </div>
  );
}
