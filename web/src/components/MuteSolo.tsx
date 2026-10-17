import clsx from "clsx";
import type { ReactNode } from "react";
import { mod } from "../lib/native";
import { drives, isMuted, isSoloed, listed, setMute, setSolo, soloOnly, toggleMute, toggleSolo, useSets } from "../lib/overlays";
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
/** A button's state: on, off, or "mixed" on a strip whose tracks differ (design-8b §2.3 step 4). */
type Pressed = boolean | "mixed";

/** Every M (or S) button: a header's names its track in `data-mute`, a strip's its tracks in `data-strip-mute`. */
const BUTTONS: Record<Kind, string> = { mute: "[data-mute],[data-strip-mute]", solo: "[data-solo],[data-strip-solo]" };
/** The tracks a button acts on, by key. */
const keysOf = (b: HTMLElement, kind: Kind) => (b.dataset[kind] ?? b.dataset[kind === "mute" ? "stripMute" : "stripSolo"] ?? "").split(" ").filter(Boolean);
const heardOf = (module: string) => useStore.getState().graph?.overlays?.heard[module] ?? "heard";
const pressedOf = (list: boolean[]): Pressed => (list.length && list.every(Boolean) ? true : list.some(Boolean) ? "mixed" : false);

/**
 * A track's M and S buttons in its header (taste.md §2.9, §3.4; design-8b §6.7), 20 px each, 2 px
 * apart, placed by the header. A click toggles; ⌥-click on S solos only this track; a drag across
 * other M (or S) buttons, a strip's too, gives each the first one's new state. Mute and solo while
 * listening are no edit: no undo step, nothing in the code. They work read-only and while the code
 * doesn't compile; only a lost connection turns them off. They never take focus, so Space still
 * plays after a click (M2).
 *
 * On a track muted in the song, M is lit and a click opens its menu, where Unmute in song is: that
 * is an edit. The M key and drags across M skip it. S still lights there, but mute wins.
 */
export default function MuteSolo({ module, name }: { module: string; name: string }) {
  return <Pair modules={[module]} name={name} onSongMenu={(anchor) => openTrackMenu(module, anchor)} />;
}

/**
 * A Mix strip's M and S (taste.md §3.5): the header's, acting on every track the strip plays
 * ("Kick + Snare"), with a dash when they differ. ⌥-click on S solos exactly those tracks. When
 * every one of them is muted in the song, a click on M opens the strip's menu (`onSongMenu`).
 */
export function StripMuteSolo({ modules, name, onSongMenu }: { modules: string[]; name: string; onSongMenu: (anchor: HTMLElement) => void }) {
  return <Pair modules={modules} name={name} strip onSongMenu={onSongMenu} />;
}

function Pair({ modules, name, strip = false, onSongMenu }: { modules: string[]; name: string; strip?: boolean; onSongMenu: (anchor: HTMLElement) => void }) {
  const { mute, solo } = useSets();
  const connected = useStore((s) => s.connected);
  // As strings, so the store hands back the same value while nothing changes.
  const heard = useStore((s) => modules.map((m) => s.graph?.overlays?.heard[m] ?? "heard").join(" ")).split(" ");
  const still = useStore((s) => listed(modules.map((m) => drives(s.graph?.overlays, m)).filter((d): d is string => !!d)));
  const songMuted = modules.map((_, i) => heard[i] === "muted-in-song");
  /** Every track is muted in the song: M opens the menu where Unmute in song is. */
  const inSong = songMuted.every(Boolean);
  /** The tracks M acts on: a mute in the song is an edit, in the menu. */
  const free = modules.filter((_, i) => !songMuted[i]);
  const muted = pressedOf(modules.map((m, i) => songMuted[i] || mute.includes(m)));
  const soloed = pressedOf(modules.map((m) => solo.includes(m)));

  /** The pointer passed over another button of `kind`: its tracks that aren't `on` yet follow; a track muted in the song keeps its M. */
  const follow = (kind: Kind, keys: string[], on: boolean) => {
    const todo = keys.filter((k) => (kind === "mute" ? heardOf(k) !== "muted-in-song" && isMuted(k) !== on : isSoloed(k) !== on));
    if (todo.length) (kind === "mute" ? setMute : setSolo)(todo, on);
  };
  /** A click: on when any of them is off (so a dash turns on). Returns the new state. */
  const toggle = (kind: Kind) => {
    const keys = kind === "mute" ? free : modules;
    const on = !keys.every((k) => (kind === "mute" ? isMuted(k) : isSoloed(k)));
    (kind === "mute" ? setMute : setSolo)(keys, on);
    return on;
  };

  /** Press on M or S: set it, then follow the pointer across the others of its kind. */
  const press = (kind: Kind) => (e: React.PointerEvent<HTMLButtonElement>) => {
    // The header's own press (select, drag to reorder) is not this one.
    e.stopPropagation();
    if (e.button !== 0 || !connected) return;
    if (kind === "mute" && inSong) return onSongMenu(e.currentTarget);
    if (kind === "solo" && e.altKey) return soloOnly(modules);
    const on = toggle(kind);
    const seen = new Set<HTMLElement>([e.currentTarget]);
    const move = (ev: PointerEvent) => {
      const b = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest<HTMLElement>(BUTTONS[kind]);
      if (!b || seen.has(b)) return;
      seen.add(b);
      follow(kind, keysOf(b, kind), on);
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
    if (kind === "mute" && inSong) return onSongMenu(e.currentTarget);
    if (!strip && kind === "mute") return toggleMute(modules[0], name);
    toggle(kind);
  };

  const off = connected ? null : "Reconnecting…: mute and solo are back once it is";
  // The M and S keys act on the selected track: a strip of several tracks names no key.
  const one = modules.length === 1;
  const [mKey, sKey] = one ? [" (M)", " (S)"] : ["", ""];
  const them = one ? "this track" : "these tracks";
  const muteTitle =
    off ??
    (inSong
      ? `Muted in the song. Click to unmute ${one ? "it, in its menu" : "them, in the menu"}`
      : muted === "mixed"
        ? "Some of these tracks are muted. Click to mute them all"
        : muted
          ? `Muted${still ? ` · still drives ${still}` : ""}. Unmute${mKey}`
          : `Mute${mKey}`);
  const soloTitle =
    off ??
    (inSong
      ? `Muted in the song: unmute ${one ? "it" : "them"} to hear ${one ? "it" : "them"}`
      : soloed === "mixed"
        ? `Some of these tracks are soloed. Click to solo them all · ${mod.alt}-click: solo only ${them}`
        : `${soloed ? "Unsolo" : "Solo"}${sKey} · ${mod.alt}-click: solo only ${them}`);
  const keys = modules.join(" ");
  return (
    <div className="flex gap-0.5">
      <Toggle
        kind="mute"
        data={{ [strip ? "data-strip-mute" : "data-mute"]: keys }}
        on={muted}
        inSong={inSong}
        disabled={!connected}
        title={muteTitle}
        label={`Mute ${name}`}
        onPointerDown={press("mute")}
        onClick={keyed("mute")}
      >
        M
      </Toggle>
      <Toggle
        kind="solo"
        data={{ [strip ? "data-strip-solo" : "data-solo"]: keys }}
        on={soloed}
        disabled={!connected}
        title={soloTitle}
        label={`Solo ${name}`}
        onPointerDown={press("solo")}
        onClick={keyed("solo")}
      >
        S
      </Toggle>
    </div>
  );
}

/**
 * One of M and S: `ctl-xs`, `text-xs` 600 (taste.md §2.9). On: M `gray-700`, S `accent`, white
 * letter. Mixed (a strip's tracks differ): a dash in the on colour, on the rest look.
 */
function Toggle({
  kind,
  data,
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
  /** Its data attribute: the tracks it acts on. */
  data: Record<string, string>;
  on: Pressed;
  inSong?: boolean;
  disabled: boolean;
  title: string;
  label: string;
  onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => void;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
  children: ReactNode;
}) {
  const lit = kind === "mute" ? "bg-gray-700 border-gray-700 text-white" : "bg-accent border-accent text-white";
  const dash = kind === "mute" ? "text-gray-700" : "text-accent";
  return (
    <button
      {...data}
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
          ? clsx("cursor-default", on === true ? lit : "border-line-strong text-fg-off")
          : on === true
            ? clsx("cursor-pointer", lit, kind === "mute" ? "hover:bg-gray-600 hover:border-gray-600 active:bg-gray-800" : "hover:bg-accent-hover hover:border-accent-hover active:bg-accent-strong")
            : clsx("cursor-pointer border-line-strong hover:bg-press active:bg-line-strong", on === "mixed" ? dash : "text-fg-muted hover:text-fg"),
      )}
    >
      {on === "mixed" ? <span aria-hidden>–</span> : children}
    </button>
  );
}
