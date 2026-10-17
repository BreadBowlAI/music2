import clsx from "clsx";
import { Check, ChevronDown, CircleAlert, FolderOpen } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { tilde } from "../lib/graph";
import { useModalKeys } from "../lib/modal";
import { native, nativeAction } from "../lib/native";
import { BUFFERS, bufferWords, rateWords, settingsApi, useSettings, type Settings, type SettingsChange } from "../lib/settings";
import { Button, MenuRow, Popover } from "./ui";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** The Settings window (gaps #31): ⌘, (Ctrl+, off the Mac), with a song open or on the start screen. */
export default function SettingsDialog() {
  const open = useSettings((s) => s.open);
  return open ? <Dialog onClose={() => useSettings.setState({ open: false })} /> : null;
}

/**
 * This machine's settings, applied at once and kept by the server, with no undo: the sound output,
 * its buffer size, what plays now, and where new songs go. Done or Esc closes it; Tab goes round
 * (`useModalKeys`). A change of output or buffer makes a new player where the song was, stopped:
 * the Now row follows, and says when play stopped. While an export runs, the output can't change.
 */
function Dialog({ onClose }: { onClose: () => void }) {
  const [s, setS] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [folderError, setFolderError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The folder typed (in a browser), until it is applied.
  const [typed, setTyped] = useState<string | null>(null);
  // The menu open, and the button it opened from.
  const [menu, setMenu] = useState<{ id: string; anchor: HTMLElement } | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const folderField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    settingsApi
      .get()
      .then(setS)
      .catch((e) => setError(message(e)));
  }, []);

  // While a menu is open, its keys come first: ↑ ↓ move, Return and Space pick, Esc closes the
  // menu only. Registered once, before the dialog's own keys (`useModalKeys`), which never see them.
  const open = useRef(menu);
  open.current = menu;
  useEffect(() => {
    const rows = () => [...document.querySelectorAll<HTMLButtonElement>("[data-settings-menu] [role=menuitemradio]")];
    const key = (e: KeyboardEvent) => {
      const m = open.current;
      if (!m) return;
      const all = rows();
      const at = all.indexOf(document.activeElement as HTMLButtonElement);
      if (e.key === "ArrowDown" || e.key === "ArrowUp") all[at < 0 ? 0 : (at + (e.key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
      else if ((e.key === "Enter" || e.key === " ") && at >= 0) all[at].click();
      else if (e.key === "Escape" || e.key === "Tab") {
        m.anchor.focus();
        setMenu(null);
      } else return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, []);
  // A menu that opens has the keyboard: on the row chosen.
  useEffect(() => {
    if (!menu) return;
    const rows = [...document.querySelectorAll<HTMLButtonElement>("[data-settings-menu] [role=menuitemradio]")];
    (rows.find((r) => r.getAttribute("aria-checked") === "true") ?? rows[0])?.focus();
  }, [menu]);

  const change = async (c: SettingsChange) => {
    setBusy(true);
    try {
      const next = await settingsApi.set(c);
      // Once stopped, it says so until the window closes.
      setS((was) => ({ ...next, stopped: next.stopped || !!was?.stopped }));
      setError(null);
      return true;
    } catch (e) {
      if (c.songs !== undefined) setFolderError(message(e));
      else setError(message(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  // The typed folder's change in flight: the field's blur starts it, and Done waits for it.
  const applying = useRef<Promise<boolean> | null>(null);
  /** Apply the folder typed, if any: whether it was taken (true when none is typed). */
  const applyTyped = (): Promise<boolean> => {
    if (typed === null) return Promise.resolve(true);
    applying.current ??= change({ songs: typed.trim() }).then((ok) => {
      applying.current = null;
      if (ok) {
        setTyped(null);
        setFolderError(null);
      }
      return ok;
    });
    return applying.current;
  };
  /** Done: a folder typed is applied first. Refused, the window stays open and says why. */
  const done = async () => {
    if (await applyTyped()) onClose();
  };

  const choose = async () => {
    if (!s) return;
    try {
      const picked = await nativeAction("song_folder", JSON.stringify({ start: s.songs, title: "Choose Where New Songs Go" }));
      if (typeof picked === "string" && (await change({ songs: picked }))) setFolderError(null);
    } catch (e) {
      setFolderError(message(e));
    }
  };

  useModalKeys(box, {
    onEscape: onClose,
    // Return in the folder field applies what was typed; anywhere else it is Done.
    onDefault: () => void (document.activeElement === folderField.current && typed !== null ? applyTyped() : done()),
  });

  const output = s?.output ?? { device: null, name: null, frames: null };
  const outputs = s?.outputs ?? [];
  const def = outputs.find((o) => o.default) ?? null;
  // The device chosen as listed now, by its id (a name can be many devices'); one not listed is gone.
  const chosen = output.device === null ? def : (outputs.find((o) => o.id === output.device) ?? null);
  const gone = output.device !== null && !chosen;
  const deviceLabel = (id: string | null) =>
    id === null ? `System default${def ? ` (${def.name})` : ""}` : (outputs.find((o) => o.id === id)?.name ?? `${output.name ?? id} (not connected)`);
  // The buffers the device takes, at its rate; a gone one plays on the default.
  const device = chosen ?? def;
  const rate = device?.rate || s?.now?.rate || 48_000;
  const range = device?.frames ?? null;
  const buffers = BUFFERS.filter((n) => !range || (n >= range[0] && n <= range[1]));
  const bufferLabel = (frames: number | null) => (frames === null ? "System default" : bufferWords(frames, rate));
  const exporting = !!s?.exporting;

  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && void done()}>
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        data-testid="settings-dialog"
        className="w-[560px] max-w-full rounded-md bg-raised shadow-pop overflow-hidden animate-dialog cursor-default"
      >
        <div className="px-6 pt-6">
          <h2 id="settings-title" className="text-lg font-semibold text-fg-strong">
            Settings
          </h2>
          <p className="mt-1 text-base text-fg-muted">For this computer, in every song. Changes apply at once.</p>
        </div>

        <div className="px-6 pt-4 space-y-4">
          <Row label="Sound output" id="settings-output">
            <Choose
              id="settings-output"
              menu={menu}
              setMenu={setMenu}
              testid="settings-output"
              label={deviceLabel(output.device)}
              disabled={!s || exporting || busy}
              rows={[
                { key: "default", label: deviceLabel(null), on: output.device === null, pick: () => void change({ output: { ...output, device: null, name: null } }) },
                ...(gone ? [{ key: output.device!, label: deviceLabel(output.device), on: true, pick: () => {} }] : []),
                ...outputs.map((o) => ({ key: o.id, label: o.name, on: output.device === o.id, pick: () => void change({ output: { ...output, device: o.id, name: o.name } }) })),
              ]}
            />
          </Row>

          <Row label="Buffer size" id="settings-buffer">
            <Choose
              id="settings-buffer"
              menu={menu}
              setMenu={setMenu}
              testid="settings-buffer"
              label={bufferLabel(output.frames)}
              disabled={!s || exporting || busy}
              rows={[
                { key: "default", label: bufferLabel(null), on: output.frames === null, pick: () => void change({ output: { ...output, frames: null } }) },
                ...buffers.map((n) => ({ key: String(n), label: bufferLabel(n), on: output.frames === n, pick: () => void change({ output: { ...output, frames: n } }) })),
              ]}
            />
            <p className="mt-1.5 text-sm text-fg-muted">Smaller is quicker to respond. Larger is safer if the sound drops out.</p>
            {exporting && <p className="mt-1 text-sm text-fg-muted">Wait for the export to finish.</p>}
          </Row>

          <Row label="Now">
            <div className="min-h-8 flex flex-col justify-center text-base text-fg" data-testid="settings-now">
              <span>{nowWords(s)}</span>
              {s?.stopped && <span className="text-sm text-fg-muted">Playback stopped. Press Play to go on.</span>}
            </div>
          </Row>

          <Row label="New songs go in" id="settings-songs">
            {native ? (
              <div className="min-h-8 flex items-center gap-2">
                <span className="flex-1 min-w-0 flex items-center gap-2 text-base text-fg" title={s?.songs ?? undefined}>
                  <FolderOpen size={14} className="shrink-0 text-fg-muted" />
                  <span className="truncate" data-testid="settings-songs">
                    {s?.songs ? tilde(s.songs, s.home) : "…"}
                  </span>
                </span>
                <Button size="lg" variant="secondary" onClick={() => void choose()} disabled={!s || busy}>
                  Change…
                </Button>
              </div>
            ) : (
              <input
                id="settings-songs"
                ref={folderField}
                value={typed ?? s?.songs ?? ""}
                spellCheck={false}
                aria-invalid={!!folderError}
                data-testid="settings-songs"
                onChange={(e) => {
                  setTyped(e.target.value);
                  setFolderError(null);
                }}
                onBlur={() => void applyTyped()}
                className={clsx(
                  "w-full h-8 px-3 rounded-sm bg-raised border text-base text-fg transition-colors duration-80 ease-linear",
                  folderError ? "border-danger" : "border-line-strong hover:border-fg-off focus:border-accent",
                )}
              />
            )}
            {folderError && (
              <p role="alert" data-testid="settings-songs-error" className="mt-1 text-sm text-danger-strong">
                {folderError}
              </p>
            )}
            {s?.songs_chosen && (
              <Button
                size="sm"
                variant="ghost"
                data-testid="settings-songs-default"
                className="mt-1 -ml-2"
                disabled={busy}
                onClick={() => {
                  setTyped(null);
                  setFolderError(null);
                  void change({ songs: null });
                }}
                title={s.songs_default ? tilde(s.songs_default, s.home) : undefined}
              >
                Use the default
              </Button>
            )}
          </Row>

          {error && (
            <div role="alert" className="flex items-start gap-2 px-3 py-2 rounded-sm bg-danger-soft text-sm text-danger-strong">
              <CircleAlert size={14} className="shrink-0 text-danger mt-px" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          )}
        </div>

        <div className="mt-6 px-6 py-3 flex items-center justify-end gap-2 border-t border-line bg-chrome">
          <Button autoFocus size="lg" variant="primary" data-testid="settings-done" onClick={() => void done()} className="min-w-[104px]">
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}

/** What plays now, in words: "MacBook Pro Speakers · 48 kHz · 512 samples (10.7 ms)". */
function nowWords(s: Settings | null) {
  if (!s) return "…";
  const now = s.now;
  if (!now) return "Nothing plays yet: it starts when a song opens.";
  if (now.no_output) return "No sound output: the song plays silently.";
  const parts = [now.device, rateWords(now.rate), ...(now.frames ? [bufferWords(now.frames, now.rate)] : [])];
  return `${parts.join(" · ")}${now.fell_back ? " (the output chosen can't be used now)" : ""}`;
}

/** A row of the window: its label on the left, what it sets on the right. */
function Row({ label, id, children }: { label: string; id?: string; children: ReactNode }) {
  return (
    <div className="flex items-start gap-4">
      <label htmlFor={id} className="w-32 shrink-0 min-h-8 flex items-center text-sm font-medium text-fg-muted">
        {label}
      </label>
      <div className="flex-1 min-w-0">{children}</div>
    </div>
  );
}

interface Choice {
  key: string;
  label: string;
  on: boolean;
  pick: () => void;
}

type Menu = { id: string; anchor: HTMLElement } | null;

/** A choice of one, as a button that opens its menu (taste.md ST-4: no native select). */
function Choose({
  id,
  testid,
  label,
  rows,
  disabled,
  menu,
  setMenu,
}: {
  id: string;
  testid: string;
  label: string;
  rows: Choice[];
  disabled?: boolean;
  menu: Menu;
  setMenu: (m: Menu) => void;
}) {
  const anchor = menu?.id === id ? menu.anchor : null;
  return (
    <>
      <button
        id={id}
        type="button"
        data-testid={testid}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        disabled={disabled}
        onClick={(e) => setMenu(anchor ? null : { id, anchor: e.currentTarget })}
        className="w-full h-8 px-3 rounded-sm bg-raised border border-line-strong enabled:hover:border-fg-off text-base text-fg disabled:text-fg-off disabled:border-line flex items-center gap-2 cursor-pointer disabled:cursor-default transition-colors duration-80 ease-linear"
      >
        <span className="flex-1 min-w-0 truncate text-left">{label}</span>
        <ChevronDown size={14} className="shrink-0 text-fg-muted" />
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setMenu(null)} role="menu" data-settings-menu data-testid={`${testid}-menu`} className="z-[80]! max-h-[50vh] overflow-y-auto">
          <div style={{ minWidth: anchor.offsetWidth - 8 }} />
          {rows.map((r) => (
            <MenuRow
              key={r.key}
              role="menuitemradio"
              aria-checked={r.on}
              data-choice={r.key}
              icon={<Check size={14} className={r.on ? "" : "invisible"} />}
              onClick={() => {
                anchor.focus();
                setMenu(null);
                r.pick();
              }}
            >
              {r.label}
            </MenuRow>
          ))}
        </Popover>
      )}
    </>
  );
}
