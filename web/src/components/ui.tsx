import clsx from "clsx";
import { ChevronRight } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type HTMLAttributes, type ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * The app's buttons (taste.md §2.9). `ghost` is a toolbar button, `active` its on state; `primary`
 * the one main action; `secondary` the others; `danger` a dialog's delete. Sizes are the control heights: `sm` 24px (dock
 * tools), `md` 28px (top bar, rows), `lg` 32px (dialogs).
 */
export function Button({
  variant = "ghost",
  size = "md",
  active,
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "ghost" | "primary" | "secondary" | "danger"; size?: "sm" | "md" | "lg"; active?: boolean }) {
  return (
    <button
      {...props}
      className={clsx(
        "inline-flex items-center justify-center gap-1.5 rounded-sm font-medium transition-colors duration-80 ease-linear disabled:pointer-events-none cursor-pointer whitespace-nowrap",
        size === "sm" && "h-ctl-sm px-2 text-sm",
        size === "md" && "h-ctl px-3 text-sm",
        size === "lg" && "h-8 px-3 text-sm",
        variant === "ghost" &&
          (active ? "bg-accent-soft text-accent-strong" : "text-fg-muted hover:text-fg hover:bg-hover active:bg-press disabled:text-fg-off"),
        variant === "primary" && "bg-accent text-white hover:bg-accent-hover active:bg-accent-strong disabled:bg-press disabled:text-fg-off",
        // What can't be taken back from here in one click: a delete that takes more with it.
        variant === "danger" && "bg-danger text-white hover:bg-danger-strong active:bg-danger-strong disabled:bg-press disabled:text-fg-off",
        variant === "secondary" &&
          "bg-raised border border-line-strong text-fg hover:border-fg-off active:bg-hover disabled:text-fg-off disabled:border-line",
        className,
      )}
    />
  );
}

/** Chrome buttons don't take focus from a mouse click, so Space stays the transport (taste.md §2.11); Tab still reaches them. */
export const keepFocus = (e: React.MouseEvent) => e.preventDefault();

/** The popovers open now, oldest first. */
const open: HTMLElement[] = [];

/**
 * The one popover and menu surface (taste.md §2.14): `raised`, `rounded-md`, `shadow-pop`, 4px in,
 * anchored to its trigger. Placed once, when it opens, below the trigger (above it at the window's
 * foot), and it stays there while its content changes. A click elsewhere or Escape closes it.
 */
export function Popover({
  anchor,
  onClose,
  align = "start",
  side = "below",
  className,
  children,
  ...rest
}: {
  anchor: HTMLElement;
  onClose: () => void;
  /** Its left edge on the trigger's, or (`end`) its right edge on the trigger's. */
  align?: "start" | "end";
  /** Below the trigger, or beside it on the right (a submenu, Colour ▸), on the left at the window's edge. */
  side?: "below" | "right";
  className?: string;
  children: ReactNode;
} & Omit<HTMLAttributes<HTMLDivElement>, "children">) {
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const a = anchor.getBoundingClientRect(), r = box.current!.getBoundingClientRect();
    if (side === "right") {
      const right = a.right + 4 + r.width <= window.innerWidth - 8;
      setPos({ left: right ? a.right + 4 : Math.max(8, a.left - 4 - r.width), top: Math.max(8, Math.min(a.top - 4, window.innerHeight - r.height - 8)) });
      return;
    }
    const left = Math.max(8, Math.min(align === "end" ? a.right - r.width : a.left, window.innerWidth - r.width - 8));
    const below = a.bottom + 4;
    setPos({ left, top: below + r.height > window.innerHeight - 8 ? Math.max(8, a.top - 4 - r.height) : below });
    // Only where it opens: it never moves after.
  }, []);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const me = box.current!;
    open.push(me);
    // A popover opened from this one (a menu in the status popover) is still inside it.
    const above = (t: Node) => open.slice(open.indexOf(me) + 1).some((p) => p.contains(t));
    const outside = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!me.contains(t) && !anchor.contains(t) && !above(t)) close.current();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || open.at(-1) !== me) return;
      e.preventDefault();
      e.stopPropagation();
      close.current();
    };
    const resize = () => close.current();
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", key, true);
    window.addEventListener("resize", resize);
    return () => {
      open.splice(open.indexOf(me), 1);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("resize", resize);
    };
  }, [anchor]);
  return createPortal(
    <div
      ref={box}
      {...rest}
      // Measured unseen before it is placed, yet focusable: a field in it takes the keys at once
      // (⌘T then Enter adds a track before a frame has passed).
      style={pos ?? { left: 0, top: 0, opacity: 0, pointerEvents: "none" }}
      className={clsx("fixed z-50 p-1 rounded-md bg-raised shadow-pop animate-rise cursor-default", className)}
      onContextMenu={(e) => e.preventDefault()}
    >
      {children}
    </div>,
    document.body,
  );
}

/** A menu row (taste.md §2.14): 28px, an optional 14px icon, the shortcut right-aligned in `text-xs` `fg-faint`. */
export function MenuRow({
  icon,
  shortcut,
  detail,
  disabled,
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon?: ReactNode; shortcut?: string; detail?: ReactNode }) {
  return (
    <button
      role="menuitem"
      disabled={disabled}
      {...props}
      className={clsx(
        "w-full min-h-ctl px-2 py-1.5 rounded-sm flex items-start gap-2 text-left text-sm cursor-pointer transition-colors duration-80 ease-linear outline-none",
        disabled ? "text-fg-off cursor-default" : "text-fg hover:bg-accent-soft focus-visible:bg-accent-soft",
        className,
      )}
    >
      {icon && <span className={clsx("shrink-0 mt-px", !disabled && "text-fg-muted")}>{icon}</span>}
      <span className="flex-1 min-w-0">
        <span className="block">{children}</span>
        {detail && <span className="block text-xs text-fg-muted">{detail}</span>}
      </span>
      {shortcut && <span className="shrink-0 text-xs text-fg-faint">{shortcut}</span>}
    </button>
  );
}

/** A segmented control's item (taste.md §2.9): the track is `hover`, the chosen item `raised` with `shadow-chip`. */
export function Segment({ on, className, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { on: boolean }) {
  return (
    <button
      onMouseDown={keepFocus}
      {...props}
      className={clsx(
        "h-5 px-2 rounded-sm text-sm font-medium inline-flex items-center gap-1.5 cursor-pointer transition-colors duration-140 ease-out disabled:text-fg-off disabled:cursor-default",
        on ? "bg-raised text-fg shadow-chip" : "text-fg-muted enabled:hover:text-fg",
        className,
      )}
    />
  );
}

/**
 * The one empty state (taste.md §3.5): a line or two, centred, `text-sm` `fg-muted`, on the canvas,
 * so grid lines never run through it, with what to do next under it. Its place is the caller's.
 */
export function EmptyState({ className, children, bare, ...rest }: HTMLAttributes<HTMLDivElement> & { /** On a plain surface (a list, a popover): no backing. */ bare?: boolean }) {
  return (
    <div {...rest} className={clsx("max-w-md px-3 py-2 rounded-sm text-center text-sm text-fg-muted space-y-3", !bare && "bg-canvas", className)}>
      {children}
    </div>
  );
}

/** A disclosure of what is literally code (a problem's code, its file), behind the app's own chevron. */
export function Details({ children }: { children: ReactNode }) {
  return (
    <details className="group text-xs text-fg-faint">
      <summary className="w-fit list-none [&::-webkit-details-marker]:hidden flex items-center gap-1 cursor-pointer hover:text-fg-muted">
        <ChevronRight size={12} className="shrink-0 transition-transform duration-140 ease-out group-open:rotate-90" />
        Details
      </summary>
      {children}
    </details>
  );
}
