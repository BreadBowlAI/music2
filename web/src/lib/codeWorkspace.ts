// The code editor's open files: one Monaco model per project file, each with the text on
// disk it was last in step with (its base). Saves are based on it, so a file changed since
// (a knob, a cable, another editor) is never overwritten unasked. A clean file follows the
// disk in place; a file with unsaved edits keeps them and holds what the disk says instead.

import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { codeApi, onDisk, type ProjectFiles } from "./code";
import type { GraphState, Problem } from "./graph";
import { editing, recorded } from "./history";
import { useStore } from "./store";

export interface Doc {
  path: string;
  model: Monaco.editor.ITextModel;
  /** What the disk said when the buffer was last in step with it. */
  base: string;
  /** The model's alternative version at that point: back at it, the buffer is clean. */
  saved: number;
  /** What the disk says instead, while the buffer has unsaved edits (null: deleted). */
  theirs?: string | null;
  /** `theirs` came from a refused save. */
  conflict?: boolean;
  /** Text on its way to the disk. */
  saving?: string;
  /** Whether it had unsaved edits when listeners were last told. */
  edited: boolean;
}

export class Workspace {
  readonly docs = new Map<string, Doc>();
  modules: ProjectFiles["modules"] = [];
  /** Counts changes, for React to notice them. */
  version = 0;
  /** Counts finished saves: a listing asked for before one is older than what it wrote. */
  epoch = 0;
  private listeners = new Set<() => void>();
  private disposed = false;

  constructor(
    private monaco: typeof Monaco,
    private session: number,
    private uri: (path: string) => Monaco.Uri,
  ) {}

  subscribe(f: () => void) {
    this.listeners.add(f);
    return () => void this.listeners.delete(f);
  }

  private emit() {
    for (const d of this.docs.values()) d.edited = this.dirty(d);
    this.version++;
    this.listeners.forEach((f) => f());
  }

  dirty(d: Doc) {
    return d.model.getAlternativeVersionId() !== d.saved;
  }

  /**
   * Take in the files as the server has them now. With `epoch` (when the listing was asked
   * for), a listing older than a save since is ignored: the save's own refresh follows it.
   */
  update(project: ProjectFiles, epoch?: number) {
    if (this.disposed || (epoch !== undefined && epoch !== this.epoch)) return;
    this.modules = project.modules;
    const seen = new Set<string>();
    for (const f of project.files) {
      const disk = onDisk(f);
      seen.add(f.path);
      const d = this.docs.get(f.path);
      if (!d) {
        if (disk !== null) this.create(f.path, disk);
        continue;
      }
      if (d.saving !== undefined && disk === d.saving) {
        // Our own save, on its way back.
      } else if (disk === d.base) {
        d.theirs = undefined;
        d.conflict = false;
      } else if (disk !== null && d.model.getValue() === disk) {
        this.settle(d, disk);
      } else if (disk !== null && !this.dirty(d)) {
        this.replace(d, disk);
      } else if (d.theirs !== disk) {
        d.theirs = disk;
        d.conflict = false;
      }
    }
    for (const d of this.docs.values()) {
      if (seen.has(d.path)) continue;
      if (this.dirty(d)) d.theirs = null;
      else this.drop(d);
    }
    this.emit();
  }

  private create(path: string, text: string) {
    const uri = this.uri(path);
    // A model left by an earlier editor (React's dev double mount) is reused.
    const model = this.monaco.editor.getModel(uri) ?? this.monaco.editor.createModel(text, "typescript", uri);
    if (model.getValue() !== text) model.setValue(text);
    const d: Doc = { path, model, base: text, saved: model.getAlternativeVersionId(), edited: false };
    // Typing doesn't re-render anything, except where it makes the file edited or clean again.
    model.onDidChangeContent(() => this.dirty(d) !== d.edited && this.emit());
    this.docs.set(path, d);
  }

  private drop(d: Doc) {
    d.model.dispose();
    this.docs.delete(d.path);
  }

  /** The buffer says what the disk does: clean, nothing pending. */
  private settle(d: Doc, disk: string) {
    d.base = disk;
    d.saved = d.model.getAlternativeVersionId();
    d.theirs = undefined;
    d.conflict = false;
  }

  /** Make the buffer say `text` with the smallest edit, so the cursor and the scroll stay put. Undoable. */
  private replace(d: Doc, text: string) {
    const old = d.model.getValue();
    let a = 0;
    while (a < old.length && a < text.length && old[a] === text[a]) a++;
    let b = 0;
    while (b < old.length - a && b < text.length - a && old[old.length - 1 - b] === text[text.length - 1 - b]) b++;
    const range = this.monaco.Range.fromPositions(d.model.getPositionAt(a), d.model.getPositionAt(old.length - b));
    d.model.pushStackElement();
    d.model.pushEditOperations([], [{ range, text: text.slice(a, text.length - b) }], () => null);
    d.model.pushStackElement();
    this.settle(d, text);
  }

  /** Load what the disk says, dropping the buffer's edits (undoable). */
  takeTheirs(d: Doc) {
    if (d.theirs === null) this.drop(d);
    else if (d.theirs !== undefined) this.replace(d, d.theirs);
    this.emit();
  }

  /** Keep the buffer's edits over what the disk says now; a refused save is made again. */
  async keepMine(d: Doc, onGraph: (g: GraphState) => void) {
    if (d.theirs === undefined || d.theirs === null) return;
    const conflict = d.conflict;
    d.base = d.theirs;
    d.theirs = undefined;
    d.conflict = false;
    this.emit();
    if (conflict) await this.save(d, onGraph);
  }

  /** Save one file over its base. A file changed since is not written: its text becomes `theirs`. */
  async save(d: Doc, onGraph: (g: GraphState) => void) {
    if (d.saving !== undefined || d.theirs !== undefined) return;
    const text = d.model.getValue();
    const version = d.model.getAlternativeVersionId();
    d.saving = text;
    this.emit();
    const answered = editing();
    try {
      const was = useStore.getState().graph?.view.revision ?? Infinity;
      const r = await codeApi.save(this.session, d.path, d.base, text);
      if (r.ok) {
        d.base = text;
        d.saved = version;
        // A save that compiles is a new revision and an undo step: the top bar's Undo names it.
        if (r.graph.view.revision > was && !r.graph.view.draft_errors.length) recorded(this.session, { revision: r.graph.view.revision, step: r.step }, { name: "the code edit" });
        onGraph(r.graph);
      } else if (r.current === text) {
        // Someone wrote exactly this already.
        d.base = text;
        d.saved = version;
      } else {
        d.theirs = r.current;
        d.conflict = true;
      }
    } finally {
      answered();
      d.saving = undefined;
      this.epoch++;
      this.emit();
    }
  }

  /** Save every file with unsaved edits. */
  async saveAll(onGraph: (g: GraphState) => void) {
    for (const d of [...this.docs.values()]) if (this.dirty(d)) await this.save(d, onGraph);
  }

  /** Show the draft's problems on the files they are in. */
  mark(problems: Problem[]) {
    for (const d of this.docs.values()) {
      const lines = d.model.getLineCount();
      const markers = problems
        .filter((p) => p.file === d.path && p.line != null)
        .map((p) => {
          const line = Math.min(Math.max(p.line!, 1), lines);
          const col = Math.min(Math.max(p.col ?? 1, 1), d.model.getLineMaxColumn(line));
          const word = d.model.getWordAtPosition({ lineNumber: line, column: col });
          return {
            severity: this.monaco.MarkerSeverity.Error,
            // In plain words where a rule knows it (the studio plan's D2), the problem's own after.
            message: p.plain ? `${p.plain}${p.fix ? ` ${p.fix}` : ""}\n\n${p.message}` : p.message,
            source: "music2",
            startLineNumber: line,
            startColumn: word?.startColumn ?? col,
            endLineNumber: line,
            endColumn: word?.endColumn ?? col,
          };
        });
      this.monaco.editor.setModelMarkers(d.model, "music2", markers);
    }
  }

  dispose() {
    this.disposed = true;
    for (const d of this.docs.values()) d.model.dispose();
    this.docs.clear();
    this.listeners.clear();
  }
}
