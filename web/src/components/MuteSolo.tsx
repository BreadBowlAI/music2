import clsx from "clsx";
import type { ReactNode } from "react";
import { useHeard } from "../lib/graph8b";
import { mod } from "../lib/native";
import { drives, isMuted, isSoloed, setMute, setSolo, soloOnly, toggleMute, toggleSolo, useSets } from "../lib/overlays";
import { canvas } from "../lib/shell";
import { useStore } from "../lib/store";
import { muteInSong, openTrackMenu } from "./Tracks";
import { keepFocus } from "./ui";

// The M and S keys, and Track ▸ Mute, Solo (⌃M, ⌃S on a Mac) and Mute in Song, act on the
// selected track: App sends them here. Never the Song row (MON-1).
const track = (module: string) => useStore.getState().graph?.timeline.tracks.find((t) => t.module === module && t.track && t.module !== "song");
canvas.mute = (module) => {
  const t = track(module);
  if (t) toggleMute(module, t.label || module);
};
canvas.solo = (module) => {
  if (track(module)) toggleSolo(module);
};
canvas.muteInSong = (module) => {
  if (track(module)) void muteInSong(module, useStore.getState().graph?.overlays?.heard[module] !== "muted-in-song");
};

type Kind = "mute" | "solo";

/**
 * A track's M and S buttons (taste.md §2.9, §3.4; design-8b §6.7), 20 px each, 2 px apart, placed
 * by its header. A click toggles; ⌥-click on S solos only this track; a drag across other M (or S)
 * buttons gives each the first one's new state. Mute and solo while listening are no edit: no undo
 * step, nothing in the code. They work read-only and while the code doesn't compile; only a lost
 * connection turns them off. They never take focus, so Space still plays after a click (M2).
 *
 * On a track muted in the song, M is lit and a click opens its menu, where Unmute in song is: that
 * is an edit. The M key and drags across M skip it. S still lights there, but mute wins.
 */
export default function MuteSolo({ module, name }: { module: string; name: string }) {
  const heard = useHeard(module);
  const { mute, solo } = useSets();
  const connected = useStore((s) => s.connected);
  const still = useStore((s) => drives(s.graph?.overlays, module));
  const inSong = heard === "muted-in-song";
  const muted = mute.includes(module);
  const soloed = solo.includes(module);

  /** Press on M or S: set it, then follow the pointer across the others of its kind. */
  const press = (kind: Kind) => (e: React.PointerEvent<HTMLButtonElement>) => {
    // The header's own press (select, drag to reorder) is not this one.
    e.stopPropagation();
    if (e.button !== 0 || !connected) return;
    if (kind === "mute" && inSong) return openTrackMenu(module, e.currentTarget);
    if (kind === "solo" && e.altKey) return soloOnly(module);
    const on = kind === "mute" ? !muted : !soloed;
    (kind === "mute" ? setMute : setSolo)([module], on);
    const seen = new Set([module]);
    const move = (ev: PointerEvent) => {
      const b = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest<HTMLElement>(`[data-${kind}]`);
      const k = b?.dataset[kind];
      if (!b || !k || seen.has(k)) return;
      seen.add(k);
      // A track muted in the song keeps its M: that mute is an edit.
      if (kind === "mute" && b.dataset.inSong) return;
      if ((kind === "mute" ? isMuted(k) : isSoloed(k)) !== on) (kind === "mute" ? setMute : setSolo)([k], on);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };
  /** A click from the keyboard (Tab, then Return; Space stays the transport): the pointer's are handled on press. */
  const keyed = (kind: Kind) => (e: React.MouseEvent<HTMLButtonElement>) => {
    if (e.detail !== 0 || !connected) return;
    if (kind === "solo") return toggleSolo(module);
    if (inSong) return openTrackMenu(module, e.currentTarget);
    toggleMute(module, name);
  };

  const off = connected ? null : "Reconnecting…: mute and solo are back once it is";
  const muteTitle =
    off ?? (inSong ? "Muted in the song. Click to unmute it, in its menu" : muted ? `Muted${still ? ` · still drives ${still}` : ""}. Unmute (M)` : "Mute (M)");
  const soloTitle = off ?? (inSong ? "Muted in the song: unmute it to hear it" : `${soloed ? "Unsolo" : "Solo"} (S) · ${mod.alt}-click: solo only this track`);
  return (
    <div className="flex gap-0.5">
      <Toggle kind="mute" module={module} on={muted || inSong} inSong={inSong} disabled={!connected} title={muteTitle} label={`Mute ${name}`} onPointerDown={press("mute")} onClick={keyed("mute")}>
        M
      </Toggle>
      <Toggle kind="solo" module={module} on={soloed} disabled={!connected} title={soloTitle} label={`Solo ${name}`} onPointerDown={press("solo")} onClick={keyed("solo")}>
        S
      </Toggle>
    </div>
  );
}

/** One of M and S: `ctl-xs`, `text-xs` 600 (taste.md §2.9). On: M `gray-700`, S `accent`, white letter. */
function Toggle({
  kind,
  module,
  on,
  inSong,
  disabled,
  title,
  label,
  onPointerDown,
  onClick,
  children,
}: {
  kind: Kind;
  module: string;
  on: boolean;
  inSong?: boolean;
  disabled: boolean;
  title: string;
  label: string;
  onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => void;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
  children: ReactNode;
}) {
  const lit = kind === "mute" ? "bg-gray-700 border-gray-700 text-white" : "bg-accent border-accent text-white";
  return (
    <button
      {...{ [`data-${kind}`]: module }}
      data-in-song={inSong || undefined}
      aria-label={label}
      aria-pressed={on}
      aria-disabled={disabled || undefined}
      title={title}
      onMouseDown={keepFocus}
      onPointerDown={onPointerDown}
      onClick={onClick}
      className={clsx(
        "size-ctl-xs shrink-0 grid place-items-center rounded-sm border text-xs font-semibold select-none transition-colors duration-80 ease-linear",
        // A header is `hover` while the pointer is on it: M and S hover a step darker, as its ⋯ does.
        disabled
          ? clsx("cursor-default", on ? lit : "border-line-strong text-fg-off")
          : on
            ? clsx("cursor-pointer", lit, kind === "mute" ? "hover:bg-gray-600 hover:border-gray-600 active:bg-gray-800" : "hover:bg-accent-hover hover:border-accent-hover active:bg-accent-strong")
            : "cursor-pointer border-line-strong text-fg-muted hover:bg-press hover:text-fg active:bg-line-strong",
      )}
    >
      {children}
    </button>
  );
}
