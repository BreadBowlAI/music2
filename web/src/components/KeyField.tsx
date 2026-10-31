// The song's key in the top bar (the polish plan's D2, taste.md §3.3 at 472): "Key" in `fg-faint`
// while none is set, "A minor" once one is. A click opens a picker: the twelve roots, the scales
// of the one table (NOTE-1: Major, Minor, the modes, the pentatonics…), and No key. Saved in
// layout.json, one undo step each, no toast.

import clsx from "clsx";
import { useEffect, useState } from "react";
import { lockReason, type GraphState } from "../lib/graph";
import { loadTheory, ROOTS, rootName, setSongKey, useTheory, type SongKey } from "../lib/songKey";
import { useStore } from "../lib/store";
import { Button, keepFocus, Popover, Segment } from "./ui";

const HELP = "The song's key. In Notes, rows not in the key are shaded. Saved in layout.json.\nIts scale is the set of notes that sound right together: Notes can lock notes to it and stamp its chords.";

export default function KeyField({ graph }: { graph: GraphState }) {
  const connected = useStore((s) => s.connected);
  const locked = lockReason(graph, connected);
  const key = graph.timeline.key ?? null;
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  // The scales come from the server's table, read once.
  useEffect(() => void loadTheory(), []);
  return (
    <>
      <button
        data-testid="key-field"
        aria-label="Key"
        aria-expanded={!!anchor}
        aria-disabled={!!locked}
        onMouseDown={keepFocus}
        onClick={(e) => {
          if (locked) return;
          setAnchor(anchor ? null : e.currentTarget);
        }}
        title={`${HELP}\n${locked ?? "Click to change it"}`}
        className={clsx(
          "h-ctl px-1.5 rounded-sm text-base whitespace-nowrap transition-colors duration-80 ease-linear",
          key ? "text-fg" : "text-fg-faint",
          locked ? "cursor-default" : "cursor-pointer hover:bg-hover",
          anchor && "bg-hover",
        )}
      >
        {key ? key.name : "Key"}
      </button>
      {anchor && <KeyPicker session={graph.session} songKey={key && { root: key.root, scale: key.scale }} onClose={() => setAnchor(null)} anchor={anchor} />}
    </>
  );
}

/**
 * The picker (taste.md §2.14): the roots in a 6×2 grid, the table's scales in two columns (Major and
 * Minor first), and No key. A root writes the key and closes.
 */
function KeyPicker({ session, songKey, anchor, onClose }: { session: number; songKey: SongKey | null; anchor: HTMLElement; onClose: () => void }) {
  const toast = useStore((s) => s.toast);
  const theory = useTheory((s) => s.theory);
  const scales = theory?.scales ?? [];
  const [scale, setScale] = useState<string>(songKey?.scale ?? "major");
  const write = (key: SongKey | null, close: boolean) => {
    if (close) onClose();
    setSongKey(session, key).catch((e) => toast(e instanceof Error ? e.message : String(e)));
  };
  return (
    <Popover anchor={anchor} onClose={onClose} aria-label="Key" data-testid="key-picker" className="w-[264px]">
      <div className="px-2 pt-1 text-sm font-medium text-fg">Key</div>
      <div className="px-2 py-1.5 space-y-2.5">
        <div role="radiogroup" aria-label="Scale" title="A scale is the set of notes that sound right together" className="p-0.5 rounded-sm bg-hover grid grid-cols-2 gap-0.5">
          {scales.map((s) => (
            <Segment
              key={s.name}
              role="radio"
              aria-checked={scale === s.name}
              data-scale={s.name}
              on={scale === s.name}
              title={`${s.label}: ${s.words}`}
              className="justify-start"
              // With a key set, its scale changes at once; the picker stays open for a root.
              onClick={() => {
                setScale(s.name);
                if (songKey && songKey.scale !== s.name) write({ root: songKey.root, scale: s.name }, false);
              }}
            >
              {s.label}
            </Segment>
          ))}
        </div>
        <div role="radiogroup" aria-label="Root" className="grid grid-cols-6 gap-1">
          {ROOTS.map((r) => {
            const on = songKey?.root === r && songKey.scale === scale;
            return (
              <Button
                key={r}
                size="sm"
                role="radio"
                aria-checked={on}
                data-root={r}
                active={on}
                onMouseDown={keepFocus}
                onClick={() => write({ root: r, scale }, true)}
                className="!px-0"
              >
                {rootName(r)}
              </Button>
            );
          })}
        </div>
        <Button
          size="sm"
          data-testid="no-key"
          disabled={!songKey}
          onMouseDown={keepFocus}
          onClick={() => write(null, true)}
          title={songKey ? "Take the key out: Notes shades the black keys' rows again, and has no key to lock to or chords to stamp" : "No key is set"}
          className="w-full"
        >
          No key
        </Button>
      </div>
    </Popover>
  );
}
