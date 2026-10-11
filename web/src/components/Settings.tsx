import { Settings as Gear } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { Button, Modal } from "./ui";

export default function Settings() {
  const server = useStore((s) => s.server!);
  const { set } = useStore.getState();
  const [cfg, setCfg] = useState<Awaited<ReturnType<typeof api.settings>> | null>(null);
  useEffect(() => void api.settings().then(setCfg), []);
  const back = () => set({ modal: null });
  const row = (label: string, value: React.ReactNode) => (
    <div className="flex gap-4 py-2.5 border-b border-black/[0.06] last:border-0">
      <div className="w-32 shrink-0 text-ink-400">{label}</div>
      <div className="flex-1 min-w-0 break-words text-ink-200 select-text">{value}</div>
    </div>
  );
  return (
    <Modal onClose={back} wide>
      <div className="flex items-center gap-3 mb-5">
        <div className="size-10 rounded-xl bg-ink-700 grid place-items-center"><Gear className="size-5 text-pop-400" /></div>
        <h2 className="text-xl font-semibold">Settings</h2>
      </div>
      <div className="rounded-2xl bg-ink-800 px-4 py-1 text-sm ring-1 ring-black/[0.06]">
        {row("Audio output", server.audio.ok ? `${server.audio.device} · ${server.audio.sample_rate} Hz` : server.audio.error)}
        {row("Songs folder", server.library)}
        {cfg && row("Plugins folder", <>{cfg.plugins_dir}<div className="text-xs text-ink-400 mt-1">Install with <code className="text-ink-200">music2-cli plugin add github:owner/repo</code></div></>)}
      </div>
      <div className="flex justify-end pt-5">
        <Button onClick={back}>Done</Button>
      </div>
    </Modal>
  );
}
