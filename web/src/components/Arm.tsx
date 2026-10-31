// A track's R (MON-9, MON-13; taste.md §3.4): arm it for recording. 20 px, as M and S are, just left
// of M; `danger` with a white letter while armed. A click arms or disarms: Record records into the
// armed tracks (one notes track at a time: arming another moves it), and an armed audio track hears
// its input while Settings' Hear yourself is on. A right-click on an audio track's R (or ⌥-click)
// opens its input's channels: each pair (1–2, 3–4, …) as a stereo take, or one alone as a mono one.
// No undo step: arming is no edit. It never takes focus, so Space still plays after a click.

import clsx from "clsx";
import { Check } from "lucide-react";
import { useState } from "react";
import { arm, armLane, channelChoices, channelsSaid, inputChannels } from "../lib/record";
import { useRecord } from "../lib/record";
import type { TimelineTrack } from "../lib/graph";
import { useStore } from "../lib/store";
import { keepFocus, MenuRow, Popover } from "./ui";

export default function Arm({ track, name }: { track: TimelineTrack; name: string }) {
  const module = track.module;
  const target = armLane(track);
  const armed = useStore((s) => s.graph?.arm?.armed[module] ?? null);
  const chosen = useStore((s) => s.graph?.arm?.channels[module] ?? null);
  const connected = useStore((s) => s.connected);
  const recording = useRecord((s) => !!s.take);
  const [menu, setMenu] = useState<{ anchor: HTMLElement; name: string; choices: string[] } | null>(null);
  if (!target) return <span className="block size-ctl-xs" aria-hidden />;
  const on = armed !== null;
  const off = !connected ? "Reconnecting…: arming is back once it is" : recording ? "A take is recording: arm tracks before you record" : null;
  const input = target.audio ? ` from input ${channelsSaid(chosen ?? "1-2")}` : "";
  const title =
    off ??
    (on
      ? `Armed: Record records ${name}${input}. Click to disarm${target.audio ? " · right-click: its input" : ""}`
      : `Arm ${name}: Record records here${target.audio ? " · right-click: its input" : ", and the keyboards play it"}`);
  const openMenu = async (anchor: HTMLElement) => {
    const i = await inputChannels();
    setMenu({ anchor, name: i.name, choices: channelChoices(i.channels) });
  };
  return (
    <>
      <button
        data-arm={module}
        aria-label={`Arm ${name}`}
        aria-pressed={on}
        aria-disabled={!!off || undefined}
        title={title}
        onMouseDown={keepFocus}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          if (off) return;
          if (e.altKey && target.audio) return void openMenu(e.currentTarget);
          void arm(module, on ? { armed: false } : { lane: target.lane, armed: true });
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          if (!off && target.audio) void openMenu(e.currentTarget);
        }}
        className={clsx(
          "size-ctl-xs shrink-0 grid place-items-center rounded-sm border text-xs font-semibold select-none transition-colors duration-80 ease-linear",
          off
            ? clsx("cursor-default", on ? "bg-danger border-danger text-white" : "border-line-strong text-fg-off")
            : on
              ? "cursor-pointer bg-danger border-danger text-white hover:bg-danger-strong hover:border-danger-strong"
              : "cursor-pointer border-line-strong text-fg-muted hover:text-fg hover:bg-press active:bg-line-strong",
        )}
      >
        R
      </button>
      {menu && (
        <Popover anchor={menu.anchor} onClose={() => setMenu(null)} role="menu" aria-label={`${name}'s input`} data-testid="arm-menu" className="w-[200px]">
          <div className="px-2 py-1 text-xs text-fg-faint">{menu.name}</div>
          {menu.choices.map((c) => (
            <MenuRow
              key={c}
              role="menuitemcheckbox"
              data-menu-item={`input-${c}`}
              aria-checked={(chosen ?? menu.choices[0]) === c}
              icon={(chosen ?? menu.choices[0]) === c ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />}
              onMouseDown={keepFocus}
              onClick={() => {
                setMenu(null);
                void arm(module, { channels: c });
              }}
            >
              {c.includes("-") ? `Input ${channelsSaid(c)} (stereo)` : `Input ${c} (mono)`}
            </MenuRow>
          ))}
        </Popover>
      )}
    </>
  );
}
