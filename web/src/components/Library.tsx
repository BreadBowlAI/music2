import { FolderOpen, Music } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { Button, Modal } from "./ui";

export default function Library() {
  const { set, toast } = useStore.getState();
  const [lib, setLib] = useState<Awaited<ReturnType<typeof api.library>> | null>(null);
  useEffect(() => {
    api.library().then(setLib).catch((e) => toast(e.message));
  }, [toast]);
  const back = () => set({ modal: null });
  const ago = (s: number) => {
    const d = Date.now() / 1000 - s;
    if (d < 3600) return `${Math.max(1, Math.round(d / 60))} min ago`;
    if (d < 86400) return `${Math.round(d / 3600)} h ago`;
    return new Date(s * 1000).toLocaleDateString();
  };
  return (
    <Modal onClose={back} wide>
      <h2 className="text-xl font-semibold">Your songs</h2>
      <p className="text-sm text-ink-400 mb-5">{lib?.dir}</p>
      <div className="max-h-[420px] overflow-y-auto space-y-1.5 -mx-2 px-2">
        {lib && lib.songs.length === 0 && <p className="text-sm text-ink-400 py-6 text-center">No saved songs yet. Press ⌘S to save one.</p>}
        {lib?.songs.map((s) => (
          <button
            key={s.path}
            onClick={async () => {
              await api.open(s.path).catch((e) => toast(e.message));
              set({ modal: null, selectedClip: null, selectedTrack: null });
            }}
            className="w-full flex items-center gap-3 p-3 rounded-2xl bg-ink-800 hover:bg-ink-700 ring-1 ring-black/[0.06] text-left cursor-pointer transition"
          >
            <div className="size-10 rounded-xl pop-gradient grid place-items-center"><Music className="size-5 text-white" /></div>
            <div className="flex-1">
              <div className="font-medium">{s.name}</div>
              <div className="text-xs text-ink-400">{ago(s.modified)}</div>
            </div>
          </button>
        ))}
      </div>
      <div className="flex justify-between mt-5">
        <Button onClick={() => lib && api.reveal(lib.dir)}>
          <FolderOpen className="size-4" /> Show in Finder
        </Button>
        <Button onClick={back}>Close</Button>
      </div>
    </Modal>
  );
}
