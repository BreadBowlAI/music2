import clsx from "clsx";
import { ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";
import { request } from "../lib/projects";

/** An installed pack as `GET /api/packs` lists it (PACK-11). */
interface Installed {
  name: string;
  versions: string[];
  license: string | null;
  description: string;
  items: { component: string; label: string; kind: "instrument" | "effect"; category: string }[];
}

const OPEN_KEY = "m2.browser.packs";

function readOpen(): boolean {
  try {
    return localStorage.getItem(OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * The browser's foot (PACK-11): the packs installed on this computer, folded until opened. Each row
 * is a pack's name and newest version; its tooltip says what is in it. Its items are in the lists
 * above, by their categories. Opening it asks for the list; nothing here writes.
 */
export default function PacksList() {
  const [open, setOpen] = useState(readOpen);
  const [packs, setPacks] = useState<Installed[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  // Asked for each time it opens: a pack installed since shows then.
  useEffect(() => {
    if (!open) return;
    let live = true;
    request<{ packs: Installed[] }>("/api/packs")
      .then((r) => live && (setPacks(r.packs), setFailed(null)))
      .catch((e) => live && setFailed(String(e?.message ?? e)));
    return () => {
      live = false;
    };
  }, [open]);
  const toggle = () => {
    const next = !open;
    setOpen(next);
    try {
      localStorage.setItem(OPEN_KEY, next ? "1" : "0");
    } catch {}
  };
  return (
    <div role="group" aria-label="Packs" data-browser-packs className="mt-2">
      <button
        aria-expanded={open}
        onMouseDown={(e) => e.preventDefault()}
        onClick={toggle}
        className="w-full h-ctl-sm px-1 flex items-center gap-1 rounded-sm text-sm font-semibold text-fg-muted hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
      >
        <ChevronRight size={12} className={clsx("shrink-0 transition-transform duration-140 ease-out", open && "rotate-90")} />
        Packs
      </button>
      {open && (
        <div className="pl-4 pr-2 pb-1 flex flex-col text-sm">
          {failed && <span className="text-xs text-fg-muted">Couldn't list your packs: {failed}</span>}
          {packs?.length === 0 && (
            <span data-packs-none className="py-1 text-xs text-fg-muted">
              No packs installed. Install one from its folder with m2 pack install.
            </span>
          )}
          {packs?.map((p) => (
            <div
              key={p.name}
              data-pack={p.name}
              title={[
                p.description,
                p.items.map((i) => `${i.label} (${i.kind === "instrument" ? i.category : "effect"})`).join(", "),
                p.license ? `Licence: ${p.license}` : "",
                p.versions.length > 1 ? `Installed: ${p.versions.join(", ")}` : "",
              ]
                .filter(Boolean)
                .join("\n")}
              className="h-ctl-sm flex items-center gap-2 rounded-sm cursor-default text-fg"
            >
              <span className="truncate">{p.name}</span>
              <span className="ml-auto shrink-0 text-xs text-fg-muted tabular-nums">{p.versions[p.versions.length - 1]}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
