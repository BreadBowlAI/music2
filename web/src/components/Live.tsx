import clsx from "clsx";
import { Archive, CircleStop } from "lucide-react";
import { useState } from "react";
import { closeFeed, keepFeed, useFeed } from "../lib/feeds";
import { lockReason } from "../lib/graph";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { MenuRow, Popover } from "./ui";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/**
 * The Live mark on a track's header (FEED-1): an agent plays this track, its notes written ahead of
 * the playhead. It names the writer ("Live · Claude"), and its menu keeps what played or closes the
 * feed. Calm: a chip like Bus's, the accent's soft fill as a toggle that is on, nothing that moves.
 */
export default function Live({ module, name }: { module: string; name: string }) {
  const feed = useFeed(module);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  if (!feed) return null;
  const who = feed.writer ?? "an agent";
  return (
    <>
      <button
        data-testid="live-mark"
        data-module={module}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        title={`${feed.writer ? `${feed.writer} plays` : "An agent can play"} ${name} live: its notes come ahead of the playhead, with the track's clips`}
        onMouseDown={(e) => e.preventDefault()}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => setAnchor(anchor ? null : e.currentTarget)}
        className={clsx(
          "shrink-0 inline-flex items-center gap-1 px-1 rounded-xs text-xs cursor-pointer transition-colors duration-80 ease-linear",
          "bg-accent-soft text-accent-strong hover:bg-accent-range",
        )}
      >
        <span aria-hidden className="block size-1.5 rounded-full bg-accent" />
        <span className="truncate max-w-[88px]">{feed.writer ? `Live · ${feed.writer}` : "Live"}</span>
      </button>
      {anchor && <LiveMenu module={module} name={name} who={who} anchor={anchor} onClose={() => setAnchor(null)} />}
    </>
  );
}

/** The Live mark's menu: Keep what played, Close. */
function LiveMenu({ module, name, who, anchor, onClose }: { module: string; name: string; who: string; anchor: HTMLElement; onClose: () => void }) {
  const graph = useStore((s) => s.graph);
  const connected = useStore((s) => s.connected);
  if (!graph) return null;
  const locked = lockReason(graph, connected);
  const toast = useStore.getState().toast;
  const session = graph.session;
  const keep = async () => {
    onClose();
    try {
      const r = await keepFeed(session, module);
      const how = r.as === "version" ? `as the version “${r.version}” of the clip under it` : "as a new clip";
      toast(`Kept what ${who} played on ${name} ${how}`, undoAction(session, r.step));
    } catch (error) {
      toast(message(error));
    }
  };
  const close = async () => {
    onClose();
    try {
      await closeFeed(session, module);
    } catch (error) {
      toast(message(error));
    }
  };
  return (
    <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={`${name} live`} data-testid="live-menu" className="w-[240px]">
      <MenuRow
        icon={<Archive size={14} />}
        disabled={!!locked}
        title={locked ?? undefined}
        data-testid="live-keep"
        detail="Into the song, as one change you can undo"
        onClick={() => void keep()}
      >
        Keep what played
      </MenuRow>
      <MenuRow icon={<CircleStop size={14} />} data-testid="live-close" detail="Its notes stop at once" onClick={() => void close()}>
        Close
      </MenuRow>
    </Popover>
  );
}
