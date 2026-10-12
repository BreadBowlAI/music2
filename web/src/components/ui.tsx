import clsx from "clsx";
import type { ButtonHTMLAttributes } from "react";

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
