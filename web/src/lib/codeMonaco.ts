// Monaco, set up for a song project's TypeScript: its workers, a theme in the app's tokens, and
// the compiler options songs are written for. Loaded on first use of the code view, so the editor
// (a few MB) stays out of the app's first load.

// Monaco's own entry points (its package's exports): the editor, all its features, and TypeScript
// alone of its languages (its colouring, then its checker and suggestions).
import "monaco-editor/features/register.all";
import * as monaco from "monaco-editor/editor";
import "monaco-editor/languages/definitions/typescript/register";
import * as ts from "monaco-editor/languages/features/typescript/register";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import TsWorker from "monaco-editor/languages/features/typescript/ts.worker?worker";
import type { TypeFile } from "./code";

export { monaco };

self.MonacoEnvironment = {
  getWorker: (_id: string, label: string) => (label === "typescript" || label === "javascript" ? new TsWorker() : new EditorWorker()),
};

export const THEME = "m2-light";

// The app's tokens (index.css, taste.md §2.15): the canvas, the grey ramp, the accent for the
// cursor and selection, warn-soft for find matches. Syntax takes the track palette's inks, so code
// is as muted as the rest; numbers stand out in amber: they are the knobs.
monaco.editor.defineTheme(THEME, {
  base: "vs",
  inherit: true,
  rules: [
    { token: "", foreground: "33373c" },
    { token: "keyword", foreground: "4e5482" },
    { token: "identifier", foreground: "33373c" },
    { token: "type.identifier", foreground: "146272" },
    { token: "number", foreground: "725124" },
    { token: "string", foreground: "356443" },
    { token: "string.escape", foreground: "14655d" },
    { token: "regexp", foreground: "744765" },
    { token: "comment", foreground: "7c8186" },
    { token: "comment.doc", foreground: "7c8186" },
    { token: "delimiter", foreground: "65696f" },
  ],
  colors: {
    "editor.background": "#f5f7f9",
    "editor.foreground": "#33373c",
    "editor.lineHighlightBackground": "#edeff2",
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": "#c8daf8",
    "editor.inactiveSelectionBackground": "#c8daf899",
    "editor.selectionHighlightBackground": "#eff4fc",
    "editor.wordHighlightBackground": "#eff4fc",
    "editor.wordHighlightStrongBackground": "#e0eafb",
    "editor.findMatchBackground": "#fbefd6",
    "editor.findMatchBorder": "#b07a20",
    "editor.findMatchHighlightBackground": "#fbefd6b3",
    "editorCursor.foreground": "#3862a7",
    "editorLineNumber.foreground": "#a5aaaf",
    "editorLineNumber.activeForeground": "#65696f",
    "editorIndentGuide.background1": "#e6e8eb",
    "editorIndentGuide.activeBackground1": "#cbced2",
    "editorWhitespace.foreground": "#cbced2",
    "editorBracketMatch.background": "#eff4fc",
    "editorBracketMatch.border": "#c8daf8",
    "editorBracketHighlight.foreground1": "#65696f",
    "editorBracketHighlight.foreground2": "#4e5482",
    "editorBracketHighlight.foreground3": "#146272",
    "editorGutter.background": "#f5f7f9",
    "editorError.foreground": "#af3d36",
    "editorWarning.foreground": "#b07a20",
    "editorOverviewRuler.border": "#00000000",
    "editorWidget.background": "#fcfdfe",
    "editorWidget.border": "#dcdfe3",
    "editorHoverWidget.background": "#fcfdfe",
    "editorHoverWidget.border": "#dcdfe3",
    "editorSuggestWidget.background": "#fcfdfe",
    "editorSuggestWidget.border": "#dcdfe3",
    "editorSuggestWidget.selectedBackground": "#e0eafb",
    "editorSuggestWidget.highlightForeground": "#294e8b",
    "editorSuggestWidget.focusHighlightForeground": "#294e8b",
    "list.hoverBackground": "#e6e8eb",
    "list.activeSelectionBackground": "#e0eafb",
    "list.activeSelectionForeground": "#33373c",
    "list.activeSelectionIconForeground": "#294e8b",
    "list.inactiveSelectionBackground": "#e6e8eb",
    "list.highlightForeground": "#294e8b",
    "list.focusHighlightForeground": "#294e8b",
    "editorSuggestWidget.foreground": "#33373c",
    "editorSuggestWidget.selectedForeground": "#33373c",
    "editorSuggestWidget.selectedIconForeground": "#294e8b",
    "symbolIcon.functionForeground": "#4e5482",
    "symbolIcon.methodForeground": "#4e5482",
    "symbolIcon.propertyForeground": "#4e5482",
    "symbolIcon.fieldForeground": "#4e5482",
    "symbolIcon.variableForeground": "#146272",
    "symbolIcon.constantForeground": "#146272",
    "symbolIcon.classForeground": "#725124",
    "symbolIcon.interfaceForeground": "#146272",
    "symbolIcon.typeParameterForeground": "#146272",
    "symbolIcon.moduleForeground": "#65696f",
    "symbolIcon.keywordForeground": "#65696f",
    "symbolIcon.textForeground": "#65696f",
    "peekView.border": "#c8daf8",
    "peekViewTitle.background": "#edeff2",
    "peekViewTitleLabel.foreground": "#33373c",
    "peekViewTitleDescription.foreground": "#65696f",
    "peekViewEditor.background": "#fcfdfe",
    "peekViewEditorGutter.background": "#fcfdfe",
    "peekViewEditor.matchHighlightBackground": "#fbefd6",
    "peekViewResult.background": "#edeff2",
    "peekViewResult.fileForeground": "#33373c",
    "peekViewResult.lineForeground": "#65696f",
    "peekViewResult.selectionBackground": "#e0eafb",
    "peekViewResult.selectionForeground": "#33373c",
    "peekViewResult.matchHighlightBackground": "#fbefd6",
    "focusBorder": "#3862a7",
    "scrollbarSlider.background": "#cbced299",
    "scrollbarSlider.hoverBackground": "#a5aaafcc",
    "scrollbarSlider.activeBackground": "#7c8186cc",
    "scrollbar.shadow": "#00000000",
  },
});

// Songs run as ES modules in a sandbox with no DOM, built by the bundled SDK.
ts.typescriptDefaults.setCompilerOptions({
  target: ts.ScriptTarget.ES2020,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.NodeJs,
  lib: ["es2022"],
  strict: true,
  noEmit: true,
  allowNonTsExtensions: true,
  isolatedModules: true,
});
// Every project file is a model, so imports between them are checked without opening them.
ts.typescriptDefaults.setEagerModelSync(true);

/** Where a project file lives for the type checker. */
export const fileUri = (path: string) => monaco.Uri.parse(`file:///${path}`);

/** The project path of a model's URI, or null for a library file. */
export function projectPath(uri: monaco.Uri): string | null {
  const path = uri.path.replace(/^\//, "");
  return uri.scheme === "file" && !path.startsWith("node_modules/") ? path : null;
}

let libs: monaco.IDisposable[] = [];

/** The song the editor's models and markers are of. */
let project: number | null = null;

/**
 * A new song in the editor (taste.md CD-1, D1): no model, marker or type-checker state of the one
 * before is left. Models are global by path, and the checker keeps a file's text by its version,
 * so the next song's song.ts at the same version would be checked as the old one's.
 */
export function startProject(session: number) {
  if (project === session) return;
  project = session;
  for (const m of monaco.editor.getModels()) m.dispose();
  monaco.editor.removeAllMarkers("music2");
  monaco.editor.removeAllMarkers("typescript");
  // Setting the options again restarts the checker's worker, empty.
  ts.typescriptDefaults.setCompilerOptions(ts.typescriptDefaults.getCompilerOptions());
}

/** Resolve `@m2/core` and package imports to these sources, as if under `node_modules`. */
export function setTypes(files: TypeFile[]) {
  libs.forEach((d) => d.dispose());
  libs = files.map((f) => ts.typescriptDefaults.addExtraLib(f.text, `file:///node_modules/${f.path}`));
}

/** The source of a library file (an import's target), for read-only viewing. */
export function libText(uri: monaco.Uri): string | undefined {
  return ts.typescriptDefaults.getExtraLibs()[uri.toString()]?.content;
}

// Monaco cancels requests in flight when an editor moves to another model (as Go to Definition
// does); some of those cancellations surface as unhandled rejections. They are expected.
window.addEventListener("unhandledrejection", (e) => {
  if (e.reason instanceof Error && e.reason.name === "Canceled" && e.reason.message === "Canceled") e.preventDefault();
});
