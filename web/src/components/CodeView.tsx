import clsx from "clsx";
import { ArrowLeft, Check, CircleAlert, FileCode2, Folder, Lock, LoaderCircle, Save, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { codeApi, fileOrder, moduleFile } from "../lib/code";
import { Workspace, type Doc } from "../lib/codeWorkspace";
import { editorHooks, graphApi, moduleName, type GraphState, type Problem } from "../lib/graph";
import { chord, isMac } from "../lib/native";
import { trackColors } from "../lib/palette";
import { openTrackId, tracksIn } from "../lib/rekey";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import TrackIdDialog from "./TrackIdDialog";
import { technical, Words } from "../lib/plain";
import { Button, Details, MenuRow, Popover } from "./ui";

type Kit = typeof import("../lib/codeMonaco");

/** A type checker's complaint about a project file (the code may still run: types are stripped). */
interface TypeProblem { path: string; line: number; col: number; message: string }

const isLib = (path: string) => path.startsWith("node_modules/");
/**
 * The app's menu commands whose chords the editor binds, and its action for each (⌘D: the next match,
 * ⌘L the line, ⌘. a quick fix). ⌘⌫ deletes to the line's start on a Mac; Ctrl+Backspace deletes the
 * word before elsewhere, as Monaco binds them on each.
 */
const MONACO_CHORDS: Record<string, string> = {
  duplicate: "editor.action.addSelectionToNextFindMatch",
  delete_track: isMac ? "deleteAllLeft" : "deleteWordLeft",
  loop_selection: "expandLineSelection",
  panic: "editor.action.quickFix",
  // ⌘E and ⌘J split and join clips; in the editor, its find-with-selection and join lines.
  split: "actions.findWithSelection",
  join: "editor.action.joinLines",
  // ⌘↑ and ⌘↓ are Shorter and Taller Tracks; in the editor, to its top and foot on a Mac, and a
  // line's scroll elsewhere, as Monaco binds Ctrl+↑ and Ctrl+↓ there.
  shorter: isMac ? "cursorTop" : "scrollLineUp",
  taller: isMac ? "cursorBottom" : "scrollLineDown",
};

/** What the editor followed while the project had no module (its code never compiled). */
const NO_MODULES = "\0none";
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
  /** A file's menu (right-click in the file list): Change track ID… for each track written there. */
  const [fileMenu, setFileMenu] = useState<{ path: string; anchor: HTMLElement } | null>(null);

  // The editor is loaded on first use (it is a few MB), then the libraries and files.
  useEffect(() => {
    let live = true;
    import("../lib/codeMonaco").then((k) => live && setKit(k), (e) => live && setFailed(String(e)));
    return () => void (live = false);
  }, []);

  useEffect(() => {
    if (!kit) return;
    let live = true;
    kit.startProject(graph.session);
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
      // The system's own mono (taste.md §2.1): nothing is fetched.
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: 13,
      lineHeight: 20,
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
  // With no module at all (the code never compiled, WRITE-20), it opens where the first problem is.
  const followed = useRef<string | null>(null);
  const diagnostics = graph.diagnostics;
  useEffect(() => {
    if (!ws) return;
    const none = ws.modules.length === 0;
    // Once the code compiles, the file being fixed stays open.
    if (!none && followed.current === NO_MODULES) followed.current = module;
    if (followed.current === module || (!none && !ws.modules.some((m) => m.key === module))) return;
    const paths = [...ws.docs.keys()];
    const problem = none ? diagnostics?.map((p) => p.file).find((f): f is string => !!f && paths.includes(f)) : undefined;
    const path = problem ?? moduleFile(paths, ws.modules, module);
    if (path && open(path)) followed.current = none ? NO_MODULES : module;
  }, [ws, module, open, version, diagnostics]);

  // A place asked for (a Mix strip's Edit in code): opened at its line, once its file is listed.
  const codeAt = useShell((s) => s.codeAt);
  useEffect(() => {
    if (!codeAt || !ws?.docs.has(codeAt.file)) return;
    if (!open(codeAt.file, { lineNumber: codeAt.line, column: 1 })) return;
    useShell.getState().set({ codeAt: null });
    editor.current?.focus();
  }, [codeAt, ws, open, version]);

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
    // A menu chord the editor binds runs the editor's own action while it has the keyboard (taste.md §5.3).
    const chord = (id: string) => {
      const ed = editor.current, action = MONACO_CHORDS[id];
      if (!ed?.hasTextFocus() || !action) return false;
      ed.trigger("menu", action, null);
      return true;
    };
    editorHooks.save = save;
    editorHooks.saveAll = saveEvery;
    editorHooks.undo = undo;
    editorHooks.chord = chord;
    return () => {
      if (editorHooks.save === save) delete editorHooks.save;
      if (editorHooks.saveAll === saveEvery) delete editorHooks.saveAll;
      if (editorHooks.undo === undo) delete editorHooks.undo;
      if (editorHooks.chord === chord) delete editorHooks.chord;
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
  const name = (key: string) => moduleName(graph.view.modules.find((m) => m.key === key) ?? { key });
  const problems = graph.diagnostics ?? [];

  return (
    <div data-testid="code" className="flex-1 min-h-0 flex bg-canvas">
      <FileList
        paths={paths}
        active={active}
        dirty={(p) => !!ws && ws.dirty(ws.docs.get(p)!)}
        broken={(p) => problems.some((x) => x.file === p)}
        color={(p) => {
          const m = moduleOf(p);
          return m ? (trackColors(graph.timeline, m)?.bar ?? null) : null;
        }}
        onOpen={(p) => {
          open(p);
          editor.current?.focus();
        }}
        onMenu={(path, anchor) => tracksIn(graph, path).length > 0 && setFileMenu({ path, anchor })}
      />
      {fileMenu && (
        <Popover anchor={fileMenu.anchor} onClose={() => setFileMenu(null)} side="right" role="menu" aria-label={`${fileMenu.path} menu`} data-testid="file-menu" className="w-[240px]">
          {tracksIn(graph, fileMenu.path).map((t) => (
            <MenuRow
              key={t.module}
              data-testid="change-track-id"
              onClick={() => {
                setFileMenu(null);
                openTrackId(t.module);
              }}
            >
              {`Change ${t.name}'s track ID…`}
            </MenuRow>
          ))}
        </Popover>
      )}
      <TrackIdDialog />
      <div className="flex-1 min-w-0 flex flex-col">
        <div className="h-8 shrink-0 flex items-center gap-2 pl-3 pr-1.5 border-b border-line bg-chrome">
          {active && isLib(active) ? (
            <>
              {back && (
                <button
                  onClick={() => open(back)}
                  title={`Back to ${back}`}
                  aria-label="Back"
                  className="size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
                >
                  <ArrowLeft size={14} />
                </button>
              )}
              <FileCode2 size={14} className="text-fg-faint shrink-0" />
              <span data-code className="text-sm font-mono text-fg truncate">{libName(active)}</span>
              <span className="inline-flex items-center gap-1 h-ctl-xs px-1.5 rounded-sm bg-hover text-xs text-fg" title="A package's source: read it here, change it in your own files">
                <Lock size={12} className="text-fg-muted" />
                Read only
              </span>
            </>
          ) : (
            <>
              <FileCode2 size={14} className="text-fg-faint shrink-0" />
              <span data-code className="text-sm font-mono truncate" data-testid="code-path">
                {active?.includes("/") && <span className="text-fg-muted">{active.slice(0, active.lastIndexOf("/") + 1)}</span>}
                <span className="text-fg">{active ? active.slice(active.lastIndexOf("/") + 1) : ""}</span>
              </span>
              {doc && ws?.dirty(doc) && <span className="text-xs text-fg-muted shrink-0">Edited</span>}
            </>
          )}
          <div className="flex-1" />
          <Button
            size="sm"
            variant={dirtyCount ? "primary" : "ghost"}
            disabled={!dirtyCount || saving}
            onClick={() => void saveAll()}
            title={`Save ${dirtyCount > 1 ? "every edited file" : "the file"} (${chord("S")}). Code that doesn't compile is saved too; the last version that did keeps playing.`}
            aria-label="Save"
          >
            {saving ? <LoaderCircle size={14} className="animate-spin" /> : <Save size={14} />}
            {dirtyCount > 1 ? `Save ${dirtyCount} files` : "Save"}
            <span className="opacity-60 font-normal">{chord("S")}</span>
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
          {/* Literally code: the one place the whole text is mono. */}
          <div ref={host} data-code className="absolute inset-0" />
          {(!ws || failed) && (
            <div className="absolute inset-0 grid place-items-center bg-raised text-sm text-fg-muted">
              {failed ? `The editor couldn't load: ${failed}` : (
                <span className="inline-flex items-center gap-2">
                  <LoaderCircle size={16} className="animate-spin" />
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
          silent={graph.has_code === false}
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
  onMenu,
}: {
  paths: string[];
  active: string | null;
  dirty: (path: string) => boolean;
  broken: (path: string) => boolean;
  color: (path: string) => string | null;
  onOpen: (path: string) => void;
  /** A right-click on a file: its menu, when it has one. */
  onMenu: (path: string, anchor: HTMLElement) => void;
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
        onContextMenu={(e) => {
          e.preventDefault();
          onMenu(path, e.currentTarget);
        }}
        title={
          path === "song.ts"
            ? "song.ts: the song's main file. It brings the tracks together"
            : path === "music2.toml"
              ? "music2.toml: the song's name and the packages it uses"
              : tracks.includes(path)
                ? `${path}: one track's sound · right-click to change its track ID`
                : `${path}: code of your own, for the tracks to use`
        }
        className={clsx(
          "w-full h-ctl-sm flex items-center gap-2 pr-2 rounded-sm text-sm text-left cursor-pointer transition-colors duration-80 ease-linear",
          indent ? "pl-6" : "pl-2",
          active === path ? "bg-accent-soft text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
        )}
      >
        {c ? <span className="size-2 rounded-full shrink-0" style={{ background: c }} /> : <FileCode2 size={14} className="shrink-0 text-fg-faint" />}
        {/* Filenames are literal: mono. */}
        <span data-code className="truncate flex-1 font-mono">{name}</span>
        {broken(path) && <span className="size-1.5 rounded-full bg-danger shrink-0" title="Doesn't compile" />}
        {dirty(path) && <span className="size-2 rounded-full bg-fg-muted shrink-0" title="Unsaved edits" />}
      </button>
    );
  };
  return (
    <nav aria-label="Files" className="w-[200px] shrink-0 overflow-y-auto border-r border-line bg-chrome px-2 py-2">
      <div className="px-2 pb-1.5 text-sm font-semibold text-fg-muted">Files</div>
      {top.map((p) => row(p, p, false))}
      {tracks.length > 0 && (
        <>
          <div data-code className="h-ctl-sm flex items-center gap-2 pl-2 text-sm font-mono text-fg-faint">
            <Folder size={14} />
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
    <div role="alert" data-testid="code-changed" className="shrink-0 flex items-center gap-3 px-3 py-2 bg-warn-soft border-b border-line text-base text-warn-strong">
      <TriangleAlert size={16} className="shrink-0 text-warn" />
      <span className="flex-1 min-w-0">{text}</span>
      <Button size="sm" variant="secondary" onClick={onTheirs}>
        {doc.theirs === null ? "Close it" : "Use the new version"}
      </Button>
      {doc.theirs !== null && (
        <Button size="sm" variant="primary" onClick={onMine}>
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
  silent,
  canOpen,
  onReveal,
}: {
  problems: Problem[];
  /** What to call a module, by its key. */
  name: (key: string) => string;
  types: TypeProblem[];
  revision: number;
  /** No version of the code ever compiled (WRITE-20): nothing plays until one does. */
  silent?: boolean;
  canOpen: (path: string) => boolean;
  onReveal: (path: string, line: number | null, col: number | null) => void;
}) {
  const where = (file: string | null, line: number | null) => (file ? `${file}${line ? `:${line}` : ""}` : null);
  if (problems.length) {
    // The pill's tone (ST-1): a warning while the last version that compiled plays, danger only when nothing does.
    const Icon = silent ? CircleAlert : TriangleAlert;
    return (
      <div
        data-testid="problems"
        data-state="error"
        data-tone={silent ? "danger" : "warn"}
        className={clsx("shrink-0 max-h-28 overflow-y-auto border-t border-line py-1", silent ? "bg-danger-soft text-danger-strong" : "bg-warn-soft text-warn-strong")}
      >
        <div className="px-3 h-6 flex items-center gap-2 text-sm font-medium">
          <Icon size={14} className={silent ? "text-danger" : "text-warn"} />
          {silent ? "The code doesn't compile: nothing plays until it does." : "The code doesn't compile: playing the last version that did."}
        </div>
        {problems.map((p, i) => {
          const at = where(p.file, p.line);
          const local = !!p.file && canOpen(p.file);
          return (
            <div key={i} data-plain={p.plain ? "" : undefined}>
              <button
                disabled={!local}
                onClick={() => local && onReveal(p.file!, p.line, p.col)}
                className={clsx("w-full px-3 pl-8 min-h-6 py-0.5 flex items-baseline gap-2 text-left text-sm enabled:cursor-pointer", silent ? "enabled:hover:bg-danger/10" : "enabled:hover:bg-warn/10")}
              >
                {at && <span data-code className="font-mono shrink-0">{at}</span>}
                {p.module && !local && <span className="shrink-0">in {name(p.module)}</span>}
                {/* In plain words, with the fix, where a rule knows it (the studio plan's D2); its own words under Details. */}
                <span className="break-words min-w-0">
                  {p.plain ? (
                    <>
                      <Words text={p.plain} />
                      {p.fix && (
                        <>
                          {" "}
                          <Words text={p.fix} />
                        </>
                      )}
                    </>
                  ) : (
                    p.message
                  )}
                </span>
                {p.code !== "code.build" && !p.plain && (
                  <span data-code className="ml-auto pl-2 shrink-0 font-mono text-xs opacity-70" title="The problem's code">{p.code}</span>
                )}
              </button>
              {p.plain && (
                <div className="px-3 pl-8 pb-0.5">
                  <Details>
                    <div data-code className="mt-0.5 font-mono select-text break-words">
                      {technical(p)}
                    </div>
                  </Details>
                </div>
              )}
            </div>
          );
        })}
      </div>
    );
  }
  const first = types[0];
  return (
    // The one place the revision shows (taste.md CD-2). Fine is quiet: no colour.
    <div data-testid="problems" data-state={first ? "types" : "ok"} className="shrink-0 h-6 flex items-center gap-2 px-3 border-t border-line bg-chrome text-sm text-fg-muted">
      {first ? (
        <>
          <button onClick={() => onReveal(first.path, first.line, first.col)} className="min-w-0 flex items-center gap-2 cursor-pointer hover:text-fg" title={first.message}>
            <TriangleAlert size={14} className="text-warn shrink-0" />
            <span className="shrink-0 text-fg">
              {types.length} type {types.length === 1 ? "problem" : "problems"}
            </span>
            <span data-code className="font-mono shrink-0">{first.path}:{first.line}</span>
            <span className="truncate">{first.message.split("\n")[0]}</span>
          </button>
        </>
      ) : (
        <span className="flex items-center gap-2">
          <Check size={14} />
          Compiles
        </span>
      )}
      {/* The revision in one place, whatever the line says before it. */}
      <span className="ml-auto shrink-0 text-xs text-fg-faint" title="Every accepted change to the code is a new revision">
        playing rev {revision}
      </span>
    </div>
  );
}
