// The top bar's Record button (lane B, Q17, Q18): right after Play / Stop. At rest a ghost with a
// red circle (the record sign everyone knows; taste.md §2.6 gains it, D18); counting in, the beats
// left (4, 3, 2, 1) on `danger-soft`; recording, a white circle on `danger`. No animation. A
// right-click (or Ctrl-click on a Mac) opens its options, kept on this machine.

import clsx from "clsx";
import { Check, Circle } from "lucide-react";
import { useEffect, useState } from "react";
import { beatsLeft, loadRecordOptions, setRecordOption, toggleRecord, useRecord, useRecordOptions } from "../lib/record";
import { openSettings } from "../lib/settings";
import { keepFocus, MenuRow, Popover } from "./ui";

const tick = (on: boolean) => (on ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />);

export default function RecordButton() {
  const take = useRecord((s) => s.take);
  const counted = useRecord((s) => s.counted);
  const opts = useRecordOptions();
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  // The beats left go down as the count clicks: drawn again 20 times a second while it counts.
  const [, redraw] = useState(0);
  const counting = take?.phase === "count";
  useEffect(() => {
    if (!counting) return;
    const t = setInterval(() => redraw((x) => x + 1), 50);
    return () => clearInterval(t);
  }, [counting]);
  const left = beatsLeft(take, counted);
  const recording = take?.phase === "take";
  const title = recording
    ? "Stop recording (R). The song plays on"
    : counting
      ? "Counting in. R, Esc or Space stops it"
      : "Record (R) · right-click for options";
  const close = () => setMenu(null);
  return (
    <>
      <button
        data-testid="record"
        data-phase={take?.phase ?? "off"}
        onMouseDown={keepFocus}
        onClick={toggleRecord}
        onContextMenu={(e) => {
          e.preventDefault();
          const b = e.currentTarget;
          void loadRecordOptions();
          setMenu((m) => (m ? null : b));
        }}
        title={title}
        aria-label={recording ? "Stop recording" : "Record"}
        aria-pressed={!!take}
        aria-haspopup="menu"
        className={clsx(
          "ml-1 size-ctl rounded-sm grid place-items-center cursor-pointer transition-colors duration-80 ease-linear",
          recording
            ? "bg-danger text-white hover:bg-danger-strong active:bg-danger-strong"
            : counting
              ? "bg-danger-soft text-danger-strong"
              : "text-danger hover:bg-hover active:bg-press",
        )}
      >
        {left !== null ? <span className="text-sm font-semibold tabular-nums leading-none">{left}</span> : <Circle size={14} fill="currentColor" />}
      </button>
      {menu && (
        <Popover anchor={menu} onClose={close} role="menu" aria-label="Record options" data-testid="record-menu" className="w-[264px]">
          <MenuRow
            role="menuitemcheckbox"
            data-menu-item="count_in"
            aria-checked={opts.count_in}
            icon={tick(opts.count_in)}
            onMouseDown={keepFocus}
            onClick={() => {
              close();
              void setRecordOption({ count_in: !opts.count_in });
            }}
          >
            Count in first (one bar of clicks)
          </MenuRow>
          <MenuRow
            role="menuitemcheckbox"
            data-menu-item="quantize"
            aria-checked={opts.quantize}
            icon={tick(opts.quantize)}
            onMouseDown={keepFocus}
            onClick={() => {
              close();
              void setRecordOption({ quantize: !opts.quantize });
            }}
          >
            Snap notes to the grid
          </MenuRow>
          <div role="separator" className="my-1 h-px bg-line" />
          <MenuRow
            data-menu-item="keyboards"
            icon={<span className="block size-3.5" />}
            onMouseDown={keepFocus}
            onClick={() => {
              close();
              openSettings();
            }}
          >
            MIDI keyboards…
          </MenuRow>
        </Popover>
      )}
    </>
  );
}
