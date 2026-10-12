import { FolderOpen } from "lucide-react";
import { kbd, native, nativeAction } from "../lib/native";
import { Button } from "./ui";

/** What the app shows when no project is open: a way to open one. */
export default function StartScreen() {
  return (
    <div className="h-full flex flex-col" data-testid="start">
      {/* The macOS traffic lights sit in this strip, and the window drags by it. */}
      <div data-tauri-drag-region className="h-16 shrink-0" />
      <div className="flex-1 grid place-items-center pb-16">
        <div className="flex flex-col items-center text-center rise">
          <div className="size-16 rounded-[20px] pop-gradient grid place-items-center shadow-xl shadow-pop-500/20">
            <span className="text-white text-2xl font-bold">m2</span>
          </div>
          <h1 className="mt-5 text-3xl font-semibold tracking-tight text-ink-100">music2</h1>
          <p className="mt-2 text-[15px] text-ink-400">Open a song project folder to start.</p>
          {native ? (
            // The same native action as the menu item: the folder dialog, then the open.
            <div className="mt-8 flex items-center gap-3">
              <Button variant="pop" size="lg" onClick={() => void nativeAction("open_project")}>
                <FolderOpen className="size-[18px]" />
                Open Project Folder…
              </Button>
              <kbd className="h-7 px-2 grid place-items-center rounded-lg bg-ink-850 ring-1 ring-black/[0.08] font-sans text-xs text-ink-400">{kbd("O")}</kbd>
            </div>
          ) : (
            // Outside the app there is no folder dialog to show.
            <p className="mt-8 text-sm text-ink-300">Open a project with File → Open Project Folder… in the music2 app</p>
          )}
          <p className="mt-6 text-xs text-ink-500">New to music2? Open examples/first-song in the music2 folder.</p>
        </div>
      </div>
    </div>
  );
}
