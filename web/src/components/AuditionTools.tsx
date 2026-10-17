import { Volume2 } from "lucide-react";
import type { GraphState } from "../lib/graph";
import { useHeard, type Heard } from "../lib/graph8b";
import { setAudition, useAudition, useSilent } from "../lib/audition";
import { inSong, trackName } from "../lib/overlays";
import { chord } from "../lib/native";
import { useStore } from "../lib/store";
import { Button, keepFocus } from "./ui";

/** "Bass", "Bass and Keys", "Bass, Keys and Lead". */
const names = (list: string[]) => (list.length < 2 ? list.join("") : `${list.slice(0, -1).join(", ")} and ${list.at(-1)}`);

/**
 * Why audition can't be heard on this lane, in the header's words (design-8b §6.7, §8.4), or null:
 * a mute wins over a solo, and a lane nothing plays says so before either.
 */
export function auditionHint(heard: Heard, silent: boolean, soloed: string[]): string | null {
  if (silent) return "Nothing plays this lane";
  if (heard === "muted-in-song") return "Muted in the song · audition is silent";
  if (heard === "muted") return "Muted · audition is silent";
  if (heard === "silenced") return `Silenced by the ${names(soloed)} ${soloed.length > 1 ? "solos" : "solo"} · audition is silent`;
  return null;
}

/**
 * The Notes header's audition: the hint, when the notes you play can't be heard, then the toggle
 * (NT-2: `Volume2`, `accent-soft` while on). The toggle never takes focus, so Space stays the
 * transport.
 */
export default function AuditionTools({ graph, module, lane }: { graph: GraphState; module: string; lane: string }) {
  const on = useAudition((s) => s.on);
  const heard = useHeard(module);
  const silent = useSilent(module, lane);
  const solo = useStore((s) => s.graph?.overlays?.solo) ?? [];
  // Only the solos that count. A soloed track taken out of the song keeps its entry (MON-4), but
  // it silences nothing, so the mutes pill leaves it out too.
  const hint = on ? auditionHint(heard, silent, solo.filter((k) => inSong(graph.overlays, k)).map((k) => trackName(graph, k))) : null;
  return (
    <>
      {hint && (
        <span data-testid="audition-hint" className="min-w-0 truncate text-xs text-fg-muted" title={hint}>
          {hint}
        </span>
      )}
      <Button
        size="sm"
        data-testid="audition"
        active={on}
        aria-pressed={on}
        aria-label="Audition"
        onMouseDown={keepFocus}
        onClick={() => setAudition(!on)}
        title={`Hear notes as you play them (${on ? "on" : "off"}). ${chord(".")} stops all sound.`}
        className="w-ctl-sm !px-0 shrink-0"
      >
        <Volume2 size={16} />
      </Button>
    </>
  );
}
