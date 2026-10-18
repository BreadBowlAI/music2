import clsx from "clsx";
import { RefreshCw } from "lucide-react";
import { lookAgain, pluginsNote, usePluginList } from "../lib/plugins";

/**
 * The foot of a Plugins group (gaps #32): what it says in place of plugins (looking, none
 * installed, couldn't look), and Look again, which has the host process scan again.
 */
export default function PluginsFoot({ kind, className }: { kind: "effect" | "instrument"; className?: string }) {
  const listed = usePluginList();
  const note = pluginsNote(listed, kind);
  return (
    <div data-plugins-foot className={clsx("px-2 py-1 flex flex-col items-start gap-1 text-xs text-fg-muted", className)}>
      {note && <span data-plugins-note>{note}</span>}
      <button
        data-look-again
        disabled={listed.state === "looking"}
        title="Find plugins installed since this list was made"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => lookAgain()}
        className="h-ctl-sm flex items-center gap-1 font-medium text-accent hover:text-accent-strong disabled:text-fg-off cursor-pointer disabled:cursor-default"
      >
        <RefreshCw size={12} />
        Look again
      </button>
    </div>
  );
}
