// Monaco, set up for a song project's TypeScript: its workers, a theme in the app's light
// palette, and the compiler options songs are written for. Loaded on first use of the code
// view, so the editor (a few MB) stays out of the app's first load.

import "monaco-editor/esm/vs/editor/edcore.main";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import "monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution";
import "monaco-editor/esm/vs/language/typescript/monaco.contribution";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import TsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";
import type { TypeFile } from "./code";

export { monaco };

self.MonacoEnvironment = {
  getWorker: (_id: string, label: string) => (label === "typescript" || label === "javascript" ? new TsWorker() : new EditorWorker()),
};

export const THEME = "m2-light";

// The app's ink and pop colors (index.css). Numbers stand out in gold: they are the knobs.
monaco.editor.defineTheme(THEME, {
  base: "vs",
  inherit: true,
  rules: [
    { token: "", foreground: "2b2d32" },
    { token: "keyword", foreground: "7c5563" },
    { token: "identifier", foreground: "2b2d32" },
    { token: "type.identifier", foreground: "5b6b8c" },
    { token: "number", foreground: "a1701f" },
    { token: "string", foreground: "4f7a5c" },
    { token: "string.escape", foreground: "4f7a5c", fontStyle: "bold" },
    { token: "regexp", foreground: "8a5a7a" },
    { token: "comment", foreground: "9b9ea5", fontStyle: "italic" },
    { token: "comment.doc", foreground: "8b8e96", fontStyle: "italic" },
    { token: "delimiter", foreground: "7b7f87" },
  ],
  colors: {
    "editor.background": "#f8f8f9",
    "editor.foreground": "#2b2d32",
    "editor.lineHighlightBackground": "#efeff1",
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": "#8f657430",
    "editor.inactiveSelectionBackground": "#8f65741c",
    "editor.selectionHighlightBackground": "#8f657418",
    "editor.wordHighlightBackground": "#7f7a9218",
    "editor.wordHighlightStrongBackground": "#7f7a9226",
    "editor.findMatchBackground": "#c9a54e66",
    "editor.findMatchHighlightBackground": "#c9a54e33",
    "editorCursor.foreground": "#8f6574",
    "editorLineNumber.foreground": "#b4b7bd",
    "editorLineNumber.activeForeground": "#5e6169",
    "editorIndentGuide.background1": "#e4e5e8",
    "editorIndentGuide.activeBackground1": "#d0d3d7",
    "editorWhitespace.foreground": "#d0d3d7",
    "editorBracketMatch.background": "#8f657418",
    "editorBracketMatch.border": "#8f657455",
    "editorBracketHighlight.foreground1": "#7b7f87",
    "editorBracketHighlight.foreground2": "#8f6574",
    "editorBracketHighlight.foreground3": "#5b6b8c",
    "editorGutter.background": "#f8f8f9",
    "editorError.foreground": "#c0392b",
    "editorWarning.foreground": "#c9a54e",
    "editorOverviewRuler.border": "#00000000",
    "editorWidget.background": "#fbfbfc",
    "editorWidget.border": "#d0d3d7",
    "editorHoverWidget.background": "#fbfbfc",
    "editorHoverWidget.border": "#d0d3d7",
    "editorSuggestWidget.background": "#fbfbfc",
    "editorSuggestWidget.border": "#d0d3d7",
    "editorSuggestWidget.selectedBackground": "#8f657424",
    "editorSuggestWidget.highlightForeground": "#7c5563",
    "editorSuggestWidget.focusHighlightForeground": "#7c5563",
    "list.hoverBackground": "#e8e9eb",
    "list.activeSelectionBackground": "#8f657424",
    "list.activeSelectionForeground": "#2b2d32",
    "list.activeSelectionIconForeground": "#7c5563",
    "list.inactiveSelectionBackground": "#e8e9eb",
    "list.highlightForeground": "#7c5563",
    "list.focusHighlightForeground": "#7c5563",
    "editorSuggestWidget.foreground": "#2b2d32",
    "editorSuggestWidget.selectedForeground": "#2b2d32",
    "editorSuggestWidget.selectedIconForeground": "#7c5563",
    "symbolIcon.functionForeground": "#8f6574",
    "symbolIcon.methodForeground": "#8f6574",
    "symbolIcon.propertyForeground": "#8f6574",
    "symbolIcon.fieldForeground": "#8f6574",
    "symbolIcon.variableForeground": "#5b6b8c",
    "symbolIcon.constantForeground": "#5b6b8c",
    "symbolIcon.classForeground": "#a1701f",
    "symbolIcon.interfaceForeground": "#5b6b8c",
    "symbolIcon.typeParameterForeground": "#5b6b8c",
    "symbolIcon.moduleForeground": "#7b7f87",
    "symbolIcon.keywordForeground": "#7b7f87",
    "symbolIcon.textForeground": "#7b7f87",
    "peekView.border": "#8f657466",
    "peekViewTitle.background": "#efeff1",
    "peekViewTitleLabel.foreground": "#2b2d32",
    "peekViewTitleDescription.foreground": "#7b7f87",
    "peekViewEditor.background": "#f4f4f5",
    "peekViewEditorGutter.background": "#f4f4f5",
    "peekViewEditor.matchHighlightBackground": "#c9a54e44",
    "peekViewResult.background": "#efeff1",
    "peekViewResult.fileForeground": "#2b2d32",
    "peekViewResult.lineForeground": "#5e6169",
    "peekViewResult.selectionBackground": "#8f657424",
    "peekViewResult.selectionForeground": "#2b2d32",
    "peekViewResult.matchHighlightBackground": "#c9a54e44",
    "focusBorder": "#8f657466",
    "scrollbarSlider.background": "#d0d3d799",
    "scrollbarSlider.hoverBackground": "#b4b7bdcc",
    "scrollbarSlider.activeBackground": "#9b9ea5cc",
    "scrollbar.shadow": "#00000000",
  },
});

const ts = monaco.languages.typescript;

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

// Measure the editor's font again once the web font has loaded.
void document.fonts?.ready.then(() => monaco.editor.remeasureFonts());
