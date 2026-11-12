// The camera library's bounds (taste.md §3.10, the camera note's §3): `web/src/camera/` imports
// nothing of the app's, only its own files, React and zustand, and never reads `navigator` (the app
// tells it the platform); the app imports it only through its `index.ts`. Run by `npm run typecheck`,
// so the web build fails on a crossing. Usage: `node scripts/camera-bounds.mjs [<src folder>]`.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const src = path.resolve(process.argv[2] ?? path.join(path.dirname(fileURLToPath(import.meta.url)), "../src"));
const lib = path.join(src, "camera");
/** The packages the library may use. */
const PACKAGES = ["react", "zustand"];

/** Every file under `dir` the build reads. */
const files = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? files(p) : /\.(ts|tsx|js|jsx|mjs)$/.test(e.name) ? [p] : [];
  });
/** What a file imports: `import … from`, `export … from`, a bare `import "…"` and `import("…")`. */
const imports = (text) =>
  [...text.matchAll(/(?:\bimport|\bexport)\s[^;]*?\bfrom\s*["']([^"']+)["']|\bimport\s*\(\s*["']([^"']+)["']\s*\)|\bimport\s+["']([^"']+)["']/g)].map((m) => m[1] ?? m[2] ?? m[3]);
/** Where a relative import lands, as a path without its extension. */
const target = (from, spec) => path.resolve(path.dirname(from), spec).replace(/\.(ts|tsx|js|jsx)$/, "");
const rel = (p) => path.relative(path.dirname(src), p);

const wrong = [];
if (!fs.existsSync(path.join(lib, "index.ts"))) wrong.push(`${rel(lib)} has no index.ts`);
for (const file of files(src)) {
  const text = fs.readFileSync(file, "utf8");
  const inLib = file.startsWith(lib + path.sep);
  for (const spec of imports(text)) {
    const relative = spec.startsWith(".");
    if (inLib) {
      if (relative && !target(file, spec).startsWith(lib + path.sep)) wrong.push(`${rel(file)} imports ${spec}: outside the camera library`);
      if (!relative && !PACKAGES.some((p) => spec === p || spec.startsWith(`${p}/`))) wrong.push(`${rel(file)} imports ${spec}: the library uses only ${PACKAGES.join(" and ")}`);
    } else if (relative) {
      const t = target(file, spec);
      if ((t === lib || t.startsWith(lib + path.sep)) && t !== lib && t !== path.join(lib, "index")) wrong.push(`${rel(file)} imports ${spec}: the app imports the camera only through its index.ts`);
    }
  }
  if (inLib && /\bnavigator\b/.test(text.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, ""))) wrong.push(`${rel(file)} reads navigator: the app tells the library the platform (setPlatform)`);
}
if (wrong.length) {
  console.error(`camera-bounds: ${wrong.length} crossing${wrong.length === 1 ? "" : "s"} of the camera library's bounds:\n  ${wrong.join("\n  ")}`);
  process.exit(1);
}
console.log(`camera-bounds: ${files(lib).length} files in ${rel(lib)}, each importing only its own files, ${PACKAGES.join(" and ")}; the app through its index.ts`);
