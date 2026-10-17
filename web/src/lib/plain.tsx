// Problems in plain words (the studio plan's D2): for the common problems the server sends a
// sentence and a fix (`plain`, `fix`, made by m2_editor::explain) beside the problem's own message.
// The app leads with them; the problem's own words, its code and the place inside a package go
// under Details. A problem no rule knows keeps its own words.

import type { ReactNode } from "react";

declare module "./graph" {
  interface Problem {
    /** What went wrong, in plain words: "Cutoff wants a frequency (Hz), but gets a level (dB)." */
    plain?: string;
    /** What to do about it: "Write it in Hz, such as hz(800)." */
    fix?: string;
    /** Where inside a package it happened, when `file` is the call in your own code. */
    inner?: string;
  }
  interface Diagnostic {
    plain?: string;
    fix?: string;
  }
}

/** Where a problem is, in words: "tracks/bass.ts, line 5". */
export function placeOf(p: { file: string | null; line: number | null }) {
  if (!p.file) return null;
  return p.line != null ? `${p.file}, line ${p.line}` : p.file;
}

/** Words with the code in them (between backticks) set as code: "Import it: `import { pluck } …`". */
export function Words({ text }: { text: string }): ReactNode {
  return text.split("`").map((part, i) =>
    i % 2 ? (
      <code key={i} data-code className="font-mono text-xs">
        {part}
      </code>
    ) : (
      part
    ),
  );
}

/** The technical words behind plain ones, for Details: the problem's own message, its code, and where inside a package. */
export function technical(p: { message: string; code: string; inner?: string; src?: string | null }) {
  return [p.message, p.code, p.inner ?? p.src].filter(Boolean).join(" · ");
}
