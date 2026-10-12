import clsx from "clsx";
import { ArrowLeft, Check, CircleAlert, FileCode2, Folder, Lock, LoaderCircle, Save, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { codeApi, fileOrder, moduleFile } from "../lib/code";
import { Workspace, type Doc } from "../lib/codeWorkspace";
import { editorHooks, graphApi, moduleName, type GraphState, type Problem } from "../lib/graph";
import { kbd } from "../lib/native";
import { useStore } from "../lib/store";
import { trackColor } from "./SongTimeline";
import { Button } from "./ui";

type Kit = typeof import("../lib/codeMonaco");

/** A type checker's complaint about a project file (the code may still run: types are stripped). */
interface TypeProblem { path: string; line: number; col: number; message: string }

const isLib = (path: string) => path.startsWith("node_modules/");
/** `node_modules/std/dsp.ts` → `std/dsp.ts`: how its imports name it. */
const libName = (path: string) => path.replace(/^node_modules\//, "");

/**
 * The code editor for the project's TypeScript, opening on the source of the selected module
 * (`module`, a key).
 * Once opened it stays mounted while other views show, so its unsaved text and its ⌘S hook
 * (`editorHooks.save`) outlive a look at the graph.
 */
export default function CodeView({ graph, module }: { graph: GraphState; module: string }) {
  const toast = useStore((s) => s.toast);
  const [kit, setKit] = useState<Kit | null>(null);
  const [ws, setWs] = useState<Workspace | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const activeRef = useRef<string | null>(null);
  /** The project file shown before a library file was opened from it, to go back to. */
  const [back, setBack] = useState<string | null>(null);
  const views = useRef(new Map<string, Monaco.editor.ICodeEditorViewState | null>());
  const [typeProblems, setTypeProblems] = useState<TypeProblem[]>([]);

  // The editor is loaded on first use (it is a few MB), then the libraries and files.
  useEffect(() => {
    let live = true;
    import("../lib/codeMonaco").then((k) => live && setKit(k), (e) => live && setFailed(String(e)));
    return () => void (live = false);
  }, []);

  useEffect(() => {
    if (!kit) return;
    let live = true;
    const w = new Workspace(kit.monaco, graph.session, kit.fileUri);
    Promise.all([codeApi.types(), codeApi.files()]).then(
      ([types, files]) => {
        if (!live) return;
        kit.setTypes(types.files);
        w.update(files);
        setWs(w);
      },
      (e) => live && setFailed(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      live = false;
      w.dispose();
    };
  }, [kit, graph.session]);

  const version = useSyncExternalStore(
    useCallback((f: () => void) => ws?.subscribe(f) ?? (() => {}), [ws]),
    () => ws?.version ?? 0,
  );

  // The editor itself, once.
  useEffect(() => {
    if (!kit || !host.current) return;
    const ed = kit.monaco.editor.create(host.current, {
      model: null,
      theme: kit.THEME,
      automaticLayout: true,
      fontFamily: '"JetBrains Mono", ui-monospace, Menlo, monospace',
      fontSize: 13,
      lineHeight: 21,
      fontLigatures: false,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      padding: { top: 14, bottom: 14 },
      lineNumbersMinChars: 3,
      lineDecorationsWidth: 12,
      glyphMargin: false,
      folding: true,
      showFoldingControls: "mouseover",
      renderLineHighlight: "line",
      overviewRulerLanes: 0,
      hideCursorInOverviewRuler: true,
      overviewRulerBorder: false,
      scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
      smoothScrolling: true,
      cursorBlinking: "smooth",
      cursorSmoothCaretAnimation: "on",
      tabSize: 2,
      stickyScroll: { enabled: false },
      fixedOverflowWidgets: true,
      // A component is a const and the function it wraps: go to it rather than pick from two.
      gotoLocation: { multipleDefinitions: "goto", multipleTypeDefinitions: "goto", multipleDeclarations: "goto", multipleImplementations: "goto" },
      suggest: { showStatusBar: false, preview: true },
      quickSuggestions: { other: true, comments: false, strings: false },
    });
    editor.current = ed;
    return () => {
      ed.dispose();
      editor.current = null;
      // Library files opened to read: the next editor may have other packages.
      for (const m of kit.monaco.editor.getModels()) if (isLib(m.uri.path.replace(/^\//, ""))) m.dispose();
    };
  }, [kit]);

  /** Show a project file, or a library file read-only; keeps each file's cursor and scroll. */
  const open = useCallback(
    (path: string, at?: Monaco.IRange | Monaco.IPosition): boolean => {
      const ed = editor.current;
      if (!kit || !ws || !ed) return false;
      const from = activeRef.current;
      let model: Monaco.editor.ITextModel | null = null;
      if (isLib(path)) {
        const uri = kit.monaco.Uri.parse(`file:///${path}`);
        const text = kit.libText(uri);
        model = kit.monaco.editor.getModel(uri) ?? (text === undefined ? null : kit.monaco.editor.createModel(text, "typescript", uri));
      } else {
        model = ws.docs.get(path)?.model ?? null;
      }
      if (!model) return false;
      if (from !== path) {
        if (from) views.current.set(from, ed.saveViewState());
        ed.setModel(model);
        ed.updateOptions({ readOnly: isLib(path) });
        const view = views.current.get(path);
        if (view) ed.restoreViewState(view);
        activeRef.current = path;
        setActive(path);
        if (isLib(path) && from && !isLib(from)) setBack(from);
      }
      if (at && "startLineNumber" in at) {
        ed.setSelection(at);
        ed.revealRangeInCenterIfOutsideViewport(at);
      } else if (at) {
        ed.setPosition(at);
        ed.revealPositionInCenterIfOutsideViewport(at);
      }
      return true;
    },
    [kit, ws],
  );

  // Go to Definition (⌘-click, F12) opens the file it leads to, here.
  useEffect(() => {
    if (!kit || !ws) return;
    const d = kit.monaco.editor.registerEditorOpener({
      openCodeEditor: (_source, resource, at) => {
        const path = resource.path.replace(/^\//, "");
        return resource.scheme === "file" && (isLib(path) || ws.docs.has(path)) && open(path, at);
      },
    });
    return () => d.dispose();
  }, [kit, ws, open]);

  // Follow the selected module to its file, once the files listed know it: a module just made
  // or renamed (a save, an edit elsewhere) waits for the listing that has it.
  const followed = useRef<string | null>(null);
  useEffect(() => {
    if (!ws || followed.current === module || !ws.modules.some((m) => m.key === module)) return;
    const path = moduleFile([...ws.docs.keys()], ws.modules, module);
    if (path && open(path)) followed.current = module;
  }, [ws, module, open, version]);

  // The accepted revision or the draft changed (a knob, a cable, a save elsewhere): take in the files.
  const fetched = useRef(0);
  useEffect(() => {
    if (!ws) return;
    const n = ++fetched.current;
    const epoch = ws.epoch;
    codeApi.files().then(
      (files) => n === fetched.current && ws.update(files, epoch),
      () => {},
    );
  }, [ws, graph]);

  // A file that is gone (deleted, clean) closes; its module's file shows instead.
  useEffect(() => {
    if (ws && active && !isLib(active) && !ws.docs.has(active)) {
      activeRef.current = null;
      followed.current = null;
      setActive(null);
    }
  }, [ws, active, version]);

  // The draft's problems as markers in their files.
  useEffect(() => {
    ws?.mark(graph.diagnostics ?? []);
  }, [ws, graph.diagnostics, version]);

  // The type checker's problems in project files, for the strip.
  useEffect(() => {
    if (!kit) return;
    const read = () =>
      setTypeProblems(
        kit.monaco.editor
          .getModelMarkers({ owner: "typescript" })
          .filter((m) => m.severity >= kit.monaco.MarkerSeverity.Warning && kit.projectPath(m.resource) !== null)
          .map((m) => ({ path: kit.projectPath(m.resource)!, line: m.startLineNumber, col: m.startColumn, message: m.message })),
      );
    read();
    const d = kit.monaco.editor.onDidChangeMarkers(read);
    return () => d.dispose();
  }, [kit]);

  const onGraph = useCallback((g: GraphState) => useStore.getState().set({ graph: g }), []);
  const saveAll = useCallback(async () => {
    if (!ws) return;
    try {
      await ws.saveAll(onGraph);
    } catch (e) {
      toast(`Couldn't save: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [ws, onGraph, toast]);

  // ⌘S saves (from the keyboard, or the desktop app's File menu); the menu's Undo reaches the editor.
  const latestSave = useRef(saveAll);
  latestSave.current = saveAll;
  useEffect(() => {
    const save = () => {
      void latestSave.current();
      return true;
    };
    const saveEvery = () => void latestSave.current();
    const undo = (redo: boolean) => {
      const ed = editor.current;
      if (!ed?.hasTextFocus()) return false;
      ed.trigger("menu", redo ? "redo" : "undo", null);
      return true;
    };
    editorHooks.save = save;
    editorHooks.saveAll = saveEvery;
    editorHooks.undo = undo;
    return () => {
      if (editorHooks.save === save) delete editorHooks.save;
      if (editorHooks.saveAll === saveEvery) delete editorHooks.saveAll;
      if (editorHooks.undo === undo) delete editorHooks.undo;
    };
  }, []);

  // The app is told which files have unsaved edits, so the desktop app asks before it closes or
  // replaces the project (opening a folder, quitting) as the close button does.
  useEffect(() => {
    if (!ws) return;
    let told = "[]";
    const tell = () => {
      const files = JSON.stringify([...ws.docs.values()].filter((d) => ws.dirty(d)).map((d) => d.path).sort());
      if (files === told) return;
      told = files;
      void graphApi.unsaved(graph.session, JSON.parse(files)).catch(() => {});
    };
    tell();
    return ws.subscribe(tell);
  }, [ws, graph.session]);

  // Unsaved edits: closing the project asks first, and so does leaving the page.
  const latestWs = useRef(ws);
  latestWs.current = ws;
  useEffect(() => {
    const unsaved = () => {
      const w = latestWs.current;
      return w ? [...w.docs.values()].filter((d) => w.dirty(d)).length : 0;
    };
    const leave = (e: BeforeUnloadEvent) => {
      if (unsaved()) e.preventDefault();
    };
    editorHooks.unsaved = unsaved;
    window.addEventListener("beforeunload", leave);
    return () => {
      if (editorHooks.unsaved === unsaved) delete editorHooks.unsaved;
      window.removeEventListener("beforeunload", leave);
    };
  }, []);

  const reveal = (path: string, line: number | null, col: number | null) => {
    open(path, line ? { lineNumber: line, column: col ?? 1 } : undefined);
    editor.current?.focus();
  };

  const doc = active && !isLib(active) ? ws?.docs.get(active) : undefined;
  const paths = ws ? [...ws.docs.keys()].sort(fileOrder) : [];
  // Files changed on disk under unsaved edits wait for a choice in their banner before they save.
  const dirtyCount = ws ? [...ws.docs.values()].filter((d) => ws.dirty(d) && d.theirs === undefined).length : 0;
  const saving = ws ? [...ws.docs.values()].some((d) => d.saving !== undefined) : false;
  const moduleOf = (path: string) => ws?.modules.find((m) => m.file === path)?.key;
  const shownModule = active ? (isLib(active) ? undefined : moduleOf(active)) : module;
  const shownColor = shownModule ? trackColor(graph.timeline, shownModule) : null;
  const name = (key: string) => moduleName(graph.view.modules.find((m) => m.key === key) ?? { key });
  const problems = graph.diagnostics ?? [];

  return (
    <div data-testid="code" className="flex-1 min-h-0 flex bg-ink-950">
      <FileList
        paths={paths}
        active={active}
        dirty={(p) => !!ws && ws.dirty(ws.docs.get(p)!)}
        broken={(p) => problems.some((x) => x.file === p)}
        color={(p) => {
          const m = moduleOf(p);
          return m ? trackColor(graph.timeline, m) : null;
        }}
        onOpen={(p) => {
          open(p);
          editor.current?.focus();
        }}
      />
      <div className="flex-1 min-w-0 flex flex-col">
        <div className="h-10 shrink-0 flex items-center gap-2 pl-3 pr-2 border-b border-black/[0.06] bg-ink-850">
          {active && isLib(active) ? (
            <>
              {back && (
                <button
                  onClick={() => open(back)}
                  title={`Back to ${back}`}
                  aria-label="Back"
                  className="size-6 grid place-items-center rounded-md text-ink-400 hover:text-ink-100 hover:bg-ink-700 cursor-pointer"
                >
                  <ArrowLeft className="size-3.5" />
                </button>
              )}
              <FileCode2 className="size-3.5 text-ink-400 shrink-0" />
              <span className="text-[13px] font-mono text-ink-200 truncate">{libName(active)}</span>
              <span className="inline-flex items-center gap-1 h-5 px-1.5 rounded-md bg-ink-700 text-[11px] text-ink-300" title="A package's source: read it here, change it in your own files">
                <Lock className="size-3" />
                Read only
              </span>
            </>
          ) : (
            <>
              <FileCode2 className="size-3.5 text-ink-400 shrink-0" />
              <span className="text-[13px] font-mono truncate" data-testid="code-path">
                {active?.includes("/") && <span className="text-ink-400">{active.slice(0, active.lastIndexOf("/") + 1)}</span>}
                <span className="text-ink-100">{active ? active.slice(active.lastIndexOf("/") + 1) : ""}</span>
              </span>
              {shownModule && (
                <span className="inline-flex items-center gap-1.5 h-5 px-2 rounded-full bg-ink-800 ring-1 ring-black/[0.05] text-[11px] font-medium text-ink-300 shrink-0">
                  {shownColor && <span className="size-1.5 rounded-full" style={{ background: shownColor }} />}
                  {name(shownModule)}
                </span>
              )}
              {doc && ws?.dirty(doc) && <span className="text-[11px] text-ink-400 shrink-0">Edited</span>}
            </>
          )}
          <div className="flex-1" />
          <Button
            size="sm"
            variant={dirtyCount ? "pop" : "ghost"}
            disabled={!dirtyCount || saving}
            onClick={() => void saveAll()}
            title={`Save ${dirtyCount > 1 ? "every edited file" : "the file"} (${kbd("S")}). Code that doesn't compile is saved too; the last version that did keeps playing.`}
            aria-label="Save"
          >
            {saving ? <LoaderCircle className="size-3.5 animate-spin" /> : <Save className="size-3.5" />}
            {dirtyCount > 1 ? `Save ${dirtyCount} files` : "Save"}
            <span className="opacity-60 font-normal">{kbd("S")}</span>
          </Button>
        </div>
        {doc && doc.theirs !== undefined && ws && (
          <Changed
            doc={doc}
            onTheirs={() => ws.takeTheirs(doc)}
            onMine={() => void ws.keepMine(doc, onGraph).catch((e) => toast(`Couldn't save: ${e instanceof Error ? e.message : String(e)}`))}
          />
        )}
        <div className="relative flex-1 min-h-0">
          <div ref={host} className="absolute inset-0" />
          {(!ws || failed) && (
            <div className="absolute inset-0 grid place-items-center bg-ink-800 text-sm text-ink-400">
              {failed ? `The editor couldn't load: ${failed}` : (
                <span className="inline-flex items-center gap-2">
                  <LoaderCircle className="size-4 animate-spin" />
                  Opening {module === "song" ? "the song's" : `${name(module)}'s`} code…
                </span>
              )}
            </div>
          )}
        </div>
        <Problems
          problems={problems}
          name={name}
          types={typeProblems}
          revision={graph.view.revision}
          canOpen={(p) => !!ws?.docs.has(p)}
          onReveal={reveal}
        />
      </div>
    </div>
  );
}

/** The project's files: the song, its tracks, then the rest. */
function FileList({
  paths,
  active,
  dirty,
  broken,
  color,
  onOpen,
}: {
  paths: string[];
  active: string | null;
  dirty: (path: string) => boolean;
  broken: (path: string) => boolean;
  color: (path: string) => string | null;
  onOpen: (path: string) => void;
}) {
  const tracks = paths.filter((p) => p.startsWith("tracks/") && !p.slice(7).includes("/"));
  const top = paths.filter((p) => !tracks.includes(p) && !p.includes("/"));
  const other = paths.filter((p) => !tracks.includes(p) && p.includes("/"));
  const row = (path: string, name: string, indent: boolean) => {
    const c = color(path);
    return (
      <button
        key={path}
        data-file={path}
        aria-current={active === path ? "true" : undefined}
        data-dirty={dirty(path) || undefined}
        onClick={() => onOpen(path)}
        title={path}
        className={clsx(
          "w-full h-7 flex items-center gap-2 pr-2 rounded-md text-[12.5px] font-mono text-left cursor-pointer transition-colors",
          indent ? "pl-6" : "pl-2",
          active === path ? "bg-ink-700 text-ink-100" : "text-ink-300 hover:bg-ink-850 hover:text-ink-100",
        )}
      >
        {c ? <span className="size-2 rounded-full shrink-0" style={{ background: c }} /> : <FileCode2 className="size-3.5 shrink-0 text-ink-400" />}
        <span className="truncate flex-1">{name}</span>
        {broken(path) && <span className="size-1.5 rounded-full bg-red-500 shrink-0" title="Doesn't compile" />}
        {dirty(path) && <span className="size-2 rounded-full bg-pop-500 shrink-0" title="Unsaved edits" />}
      </button>
    );
  };
  return (
    <nav aria-label="Files" className="w-48 shrink-0 overflow-y-auto border-r border-black/[0.06] bg-ink-900 px-2 py-2.5">
      <div className="px-2 pb-1.5 text-[10.5px] font-semibold uppercase tracking-wider text-ink-400">Files</div>
      {top.map((p) => row(p, p, false))}
      {tracks.length > 0 && (
        <>
          <div className="h-7 flex items-center gap-2 pl-2 text-[12.5px] font-mono text-ink-400">
            <Folder className="size-3.5" />
            tracks
          </div>
          {tracks.map((p) => row(p, p.slice(7), true))}
        </>
      )}
      {other.map((p) => row(p, p, false))}
    </nav>
  );
}

/** The open file changed on disk while it has unsaved edits (or a save found it changed). */
function Changed({ doc, onTheirs, onMine }: { doc: Doc; onTheirs: () => void; onMine: () => void }) {
  const name = doc.path.slice(doc.path.lastIndexOf("/") + 1);
  const text =
    doc.theirs === null
      ? `${name} was deleted outside the editor.`
      : doc.conflict
        ? `${name} changed since you opened it (a knob, a cable or another editor), so your save didn't overwrite it.`
        : `${name} changed outside the editor (a knob, a cable or another editor) while you had unsaved edits.`;
  return (
    <div role="alert" data-testid="code-changed" className="shrink-0 flex items-center gap-3 px-3 py-2 bg-amber-500/10 border-b border-amber-600/20 text-[13px] text-amber-900">
      <TriangleAlert className="size-4 shrink-0" />
      <span className="flex-1 min-w-0">{text}</span>
      <Button size="sm" variant="outline" onClick={onTheirs}>
        {doc.theirs === null ? "Close it" : "Use the new version"}
      </Button>
      {doc.theirs !== null && (
        <Button size="sm" variant="solid" onClick={onMine}>
          {doc.conflict ? "Save mine over it" : "Keep my edits"}
        </Button>
      )}
    </div>
  );
}

/** Whether the code compiles, and if not, where it doesn't. Type problems come second: the code still runs. */
function Problems({
  problems,
  name,
  types,
  revision,
  canOpen,
  onReveal,
}: {
  problems: Problem[];
  /** What to call a module, by its key. */
  name: (key: string) => string;
  types: TypeProblem[];
  revision: number;
  canOpen: (path: string) => boolean;
  onReveal: (path: string, line: number | null, col: number | null) => void;
}) {
  const where = (file: string | null, line: number | null) => (file ? `${file}${line ? `:${line}` : ""}` : null);
  if (problems.length) {
    return (
      <div data-testid="problems" data-state="error" className="shrink-0 max-h-28 overflow-y-auto border-t border-red-600/15 bg-red-500/[0.06] py-1">
        <div className="px-3 h-6 flex items-center gap-2 text-[12px] font-medium text-red-800">
          <CircleAlert className="size-3.5" />
          The code doesn't compile: playing the last version that did.
        </div>
        {problems.map((p, i) => {
          const at = where(p.file, p.line);
          const local = !!p.file && canOpen(p.file);
          return (
            <button
              key={i}
              disabled={!local}
              onClick={() => local && onReveal(p.file!, p.line, p.col)}
              className="w-full px-3 pl-8 min-h-6 py-0.5 flex items-baseline gap-2.5 text-left text-[12px] enabled:hover:bg-red-500/[0.06] enabled:cursor-pointer"
            >
              {at && <span className="font-mono text-red-900/70 shrink-0">{at}</span>}
              {p.module && !local && <span className="text-red-900/70 shrink-0">in {name(p.module)}</span>}
              <span className="text-red-950 break-words min-w-0">{p.message}</span>
              {p.code !== "code.build" && (
                <span className="ml-auto pl-2 shrink-0 font-mono text-[10.5px] text-red-900/50" title="The problem's code">{p.code}</span>
              )}
            </button>
          );
        })}
      </div>
    );
  }
  const first = types[0];
  return (
    <div data-testid="problems" data-state={first ? "types" : "ok"} className="shrink-0 h-8 flex items-center gap-2 px-3 border-t border-black/[0.06] bg-ink-850 text-[12px] text-ink-400">
      {first ? (
        <button onClick={() => onReveal(first.path, first.line, first.col)} className="min-w-0 flex items-center gap-2 cursor-pointer hover:text-ink-100" title={first.message}>
          <TriangleAlert className="size-3.5 text-amber-600 shrink-0" />
          <span className="shrink-0 text-ink-300">
            {types.length} type {types.length === 1 ? "problem" : "problems"}
          </span>
          <span className="font-mono shrink-0">{first.path}:{first.line}</span>
          <span className="truncate">{first.message.split("\n")[0]}</span>
        </button>
      ) : (
        <span className="flex items-center gap-2">
          <Check className="size-3.5 text-emerald-600" />
          Compiles
        </span>
      )}
      <span className="ml-auto font-mono shrink-0" title="Every accepted change to the code is a new revision">
        playing rev {revision}
      </span>
    </div>
  );
}
