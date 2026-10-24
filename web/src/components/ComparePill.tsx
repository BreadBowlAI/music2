import { ArrowLeftRight } from "lucide-react";
import { compare, useCompare } from "../lib/compare";
import { keepFocus } from "./ui";

/**
 * The Compare pill (D2): "Before: Claude: brighter hats" while the song plays without the newest
 * change made elsewhere, "After: …" while it plays as it is. It shows what the server says. V (or a
 * click on it) flips it; Esc closes it and keeps what plays (on Before, that undoes the change);
 * anything else you do closes it at After. Compare never writes the song. While it shows, agents
 * wait.
 */
export default function ComparePill({ session }: { session: number }) {
  const pill = useCompare((s) => (s.pill?.session === session ? s.pill : null));
  if (!pill) return null;
  const side = pill.after ? "After" : "Before";
  return (
    <button
      data-testid="compare-pill"
      data-side={side.toLowerCase()}
      onMouseDown={keepFocus}
      onClick={() => void compare()}
      title={`${side}: ${pill.name}. V flips between before and after. Esc keeps what you hear: on Before, that undoes the change. Anything else goes back to the song as it is. Agents wait meanwhile.`}
      className="ml-2 h-ctl-sm px-2 max-w-[320px] shrink-0 rounded-sm inline-flex items-center gap-1.5 text-sm font-medium whitespace-nowrap cursor-pointer bg-accent-soft text-accent-strong hover:bg-[color-mix(in_oklab,var(--color-accent-soft)_88%,var(--color-accent-strong))] transition-colors duration-80 ease-linear"
    >
      <ArrowLeftRight size={14} className="shrink-0" aria-hidden />
      <span className="min-w-0 truncate">
        {side}: <span className="font-normal">{pill.name}</span>
      </span>
      <span aria-hidden className="ml-0.5 shrink-0 px-1 rounded-xs border border-current/30 text-xs font-medium leading-4">
        V
      </span>
    </button>
  );
}
