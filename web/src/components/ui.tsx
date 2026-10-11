import clsx from "clsx";
import { useRef, type ButtonHTMLAttributes, type ReactNode } from "react";

export function Button({
  variant = "ghost",
  size = "md",
  active,
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "ghost" | "solid" | "pop" | "outline"; size?: "sm" | "md" | "lg"; active?: boolean }) {
  return (
    <button
      {...props}
      className={clsx(
        "inline-flex items-center justify-center gap-1.5 rounded-xl font-medium transition-all duration-150 disabled:opacity-40 disabled:pointer-events-none cursor-pointer whitespace-nowrap",
        size === "sm" && "h-7 px-2.5 text-xs",
        size === "md" && "h-9 px-3.5 text-sm",
        size === "lg" && "h-11 px-5 text-[15px]",
        variant === "ghost" && (active ? "bg-ink-600 text-ink-100" : "text-ink-300 hover:text-ink-100 hover:bg-ink-700"),
        variant === "solid" && "bg-ink-700 text-ink-100 hover:bg-ink-600",
        variant === "outline" && "border border-ink-600 text-ink-200 hover:border-ink-500 hover:bg-ink-800",
        variant === "pop" && "pop-gradient text-white shadow-lg shadow-pop-500/10 hover:brightness-110 active:scale-[0.98]",
        className,
      )}
    />
  );
}

export function Chip({ active, children, onClick }: { active?: boolean; children: ReactNode; onClick?: () => void }) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        "h-8 px-3 rounded-full text-[13px] font-medium transition-all cursor-pointer",
        active ? "bg-pop-500/15 text-pop-400 ring-1 ring-pop-500/50" : "bg-ink-800 text-ink-300 hover:text-ink-100 hover:bg-ink-700",
      )}
    >
      {children}
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { id: T; label: ReactNode }[]; onChange: (v: T) => void }) {
  return (
    <div className="flex p-1 rounded-xl bg-ink-900 ring-1 ring-black/[0.06]">
      {options.map((o) => (
        <button
          key={o.id}
          onClick={() => onChange(o.id)}
          className={clsx(
            "flex-1 h-8 px-3 rounded-lg text-[13px] font-medium inline-flex items-center justify-center gap-1.5 transition-all cursor-pointer",
            value === o.id ? "bg-ink-700 text-ink-100 shadow" : "text-ink-400 hover:text-ink-200",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Rotary knob, 0..1. Drag up/down. Calls onStart/onEnd so a drag is one undo step. */
export function Knob({
  value,
  label,
  onChange,
  onStart,
  onEnd,
  color = "var(--color-pop-500)",
  title,
  display,
}: { value: number; label: string; onChange: (v: number) => void; onStart?: () => void; onEnd?: () => void; color?: string; title?: string; display?: string }) {
  const drag = useRef<{ y: number; v: number } | null>(null);
  const a0 = -135, a1 = 135;
  const angle = a0 + (a1 - a0) * value;
  const arc = (from: number, to: number) => {
    const r = 15, cx = 20, cy = 20;
    const p = (a: number) => [cx + r * Math.sin((a * Math.PI) / 180), cy - r * Math.cos((a * Math.PI) / 180)];
    const [x0, y0] = p(from), [x1, y1] = p(to);
    return `M ${x0} ${y0} A ${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
  };
  return (
    <div className="flex flex-col items-center gap-1 w-[72px]" title={title ?? label}>
      <svg
        width="40"
        height="40"
        className="cursor-ns-resize touch-none"
        onPointerDown={(e) => {
          (e.target as Element).setPointerCapture(e.pointerId);
          drag.current = { y: e.clientY, v: value };
          onStart?.();
        }}
        onPointerMove={(e) => {
          if (!drag.current) return;
          const v = Math.min(1, Math.max(0, drag.current.v + (drag.current.y - e.clientY) / 160));
          if (Math.abs(v - value) > 0.002) onChange(v);
        }}
        onPointerUp={() => {
          drag.current = null;
          onEnd?.();
        }}
        onDoubleClick={() => onChange(0.5)}
      >
        <path d={arc(a0, a1)} stroke="var(--color-ink-600)" strokeWidth="3.5" fill="none" strokeLinecap="round" />
        {value > 0.005 && <path d={arc(a0, angle)} stroke={color} strokeWidth="3.5" fill="none" strokeLinecap="round" />}
        <circle cx="20" cy="20" r="9" fill="var(--color-ink-700)" />
        <line x1="20" y1="20" x2={20 + 7 * Math.sin((angle * Math.PI) / 180)} y2={20 - 7 * Math.cos((angle * Math.PI) / 180)} stroke="var(--color-ink-200)" strokeWidth="2" strokeLinecap="round" />
      </svg>
      <span className="text-[11px] text-ink-300 text-center leading-tight truncate w-full">{label}</span>
      {display && <span className="text-[10px] text-ink-500 tabular-nums -mt-0.5">{display}</span>}
    </div>
  );
}

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-xs font-medium text-ink-300">{label}</span>
      {children}
      {hint && <span className="block text-[11px] text-ink-400">{hint}</span>}
    </label>
  );
}

export const inputClass =
  "w-full h-10 px-3 rounded-xl bg-ink-900 ring-1 ring-black/[0.08] text-sm text-ink-100 placeholder:text-ink-400 outline-none focus:ring-pop-500/60 transition";

export function Modal({ children, onClose, wide }: { children: ReactNode; onClose?: () => void; wide?: boolean }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink-100/20 backdrop-blur-sm rise" onPointerDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className={clsx("relative rounded-3xl bg-ink-850 ring-1 ring-black/10 shadow-2xl shadow-black/15 p-7", wide ? "w-[640px]" : "w-[520px]")}>{children}</div>
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <span className={clsx("inline-block size-4 rounded-full border-2 border-current border-t-transparent animate-spin", className)} />;
}
