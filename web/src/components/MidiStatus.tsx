import { KeyboardMusic } from "lucide-react";
import type { GraphState } from "../lib/graph";
import { useMidi } from "../lib/midi";

/** "Keystation 49", "Keystation 49 and Launchkey", "A, B and C". */
const names = (list: string[]) => (list.length < 2 ? list.join("") : `${list.slice(0, -1).join(", ")} and ${list.at(-1)}`);

/**
 * The MIDI keyboard's mark in the top bar (8b.10), before the status pill: there only while a
 * keyboard is plugged in. Its tooltip names the ones that play and what they play: "MIDI keyboard:
 * Keystation 49 · plays Bass", or how to make it play; and the ones turned off in Settings (studio
 * lane B): "Pads is turned off in Settings".
 */
export default function MidiStatus({ graph }: { graph: GraphState }) {
  const { devices, target, off = [] } = useMidi();
  if (!devices.length) return null;
  // The lane named, if the song still has it.
  const track = target && graph.timeline.tracks.find((t) => t.module === target.module && t.lanes.some((l) => l.key === target.lane && l.kind === "notes"));
  const plays = track ? `plays ${track.label}` : "Select a track with notes to play it";
  const on = devices.filter((d) => !off.includes(d));
  const quiet = devices.filter((d) => off.includes(d));
  const title = [
    on.length ? `MIDI ${on.length > 1 ? "keyboards" : "keyboard"}: ${names(on)} · ${plays}` : null,
    quiet.length ? `${names(quiet)} ${quiet.length > 1 ? "are" : "is"} turned off in Settings` : null,
  ]
    .filter(Boolean)
    .join(". ");
  return (
    <span data-testid="midi-status" role="img" aria-label={title} title={title} className="mr-1 size-ctl shrink-0 grid place-items-center text-fg-muted">
      <KeyboardMusic size={16} />
    </span>
  );
}
