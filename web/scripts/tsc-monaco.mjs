// Type-check a TypeScript project with the TypeScript the app's code editor runs: the one Monaco
// bundles (5.9.3 in monaco-editor 0.57), not the project's `tsc`. A song is typed by that one, so
// the SDK's and std's checks must pass under it too, each mistake marked where it is written
// (CARD-13). Usage: `node scripts/tsc-monaco.mjs <project folder>`; any error exits 1.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const lib = path.join(here, "../node_modules/monaco-editor/esm/vs/languages/features/typescript/lib");
const { typescript: ts } = await import(path.join(lib, "typescriptServices.js"));
const { libFileMap } = await import(path.join(lib, "lib.js"));

const project = path.resolve(process.argv[2] ?? ".");
const LIBS = "/__monaco_lib__";
const read = (f) => (f.startsWith(LIBS + "/") ? libFileMap[path.basename(f)] : fs.existsSync(f) && fs.statSync(f).isFile() ? fs.readFileSync(f, "utf8") : undefined);
const exists = (f) => read(f) !== undefined;
const configPath = path.join(project, "tsconfig.json");
const config = ts.parseConfigFileTextToJson(configPath, fs.readFileSync(configPath, "utf8"));
const parsed = ts.parseJsonConfigFileContent(
  config.config,
  { useCaseSensitiveFileNames: true, readDirectory: () => [], fileExists: exists, readFile: read },
  project,
  undefined,
  configPath,
);
const host = {
  getSourceFile: (f, target) => {
    const text = read(f);
    return text === undefined ? undefined : ts.createSourceFile(f, text, target, true);
  },
  getDefaultLibFileName: () => `${LIBS}/lib.d.ts`,
  getDefaultLibLocation: () => LIBS,
  writeFile: () => {},
  getCurrentDirectory: () => project,
  getCanonicalFileName: (f) => f,
  useCaseSensitiveFileNames: () => true,
  getNewLine: () => "\n",
  fileExists: exists,
  readFile: read,
  directoryExists: (d) => d.startsWith(LIBS) || (fs.existsSync(d) && fs.statSync(d).isDirectory()),
  realpath: (f) => (f.startsWith(LIBS) ? f : fs.realpathSync(f)),
};
const program = ts.createProgram({ rootNames: parsed.fileNames, options: parsed.options, host });
const diags = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)];
for (const d of diags) {
  const where = d.file ? (({ line, character }) => `${path.relative(process.cwd(), d.file.fileName)}(${line + 1},${character + 1}): `)(d.file.getLineAndCharacterOfPosition(d.start)) : "";
  console.log(`${where}error TS${d.code}: ${ts.flattenDiagnosticMessageText(d.messageText, "\n")}`);
}
console.log(`${path.relative(process.cwd(), project) || "."}: TypeScript ${ts.version} (the code editor's), ${diags.length} error${diags.length === 1 ? "" : "s"}`);
process.exit(diags.length ? 1 : 0);
