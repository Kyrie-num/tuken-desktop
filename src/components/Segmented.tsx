import { motion } from "framer-motion";
import { cn } from "@/lib/cn";

/**
 * A capsule of options; the chosen one sits under a glass droplet that slides
 * across, like iOS 26's tab bars.
 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  id,
  size = "md",
  stretch = false,
}: {
  value: T;
  options: readonly (readonly [T, string])[];
  onChange: (value: T) => void;
  label: string;
  /** Separates the droplets of two segmented controls on one screen. */
  id: string;
  size?: "sm" | "md";
  /** Fills the width, options sharing it equally. */
  stretch?: boolean;
}) {
  return (
    <div
      role="tablist"
      aria-label={label}
      className={cn(
        "glass items-center gap-0.5 rounded-full p-1",
        stretch ? "flex w-full" : "inline-flex",
      )}
    >
      {options.map(([key, text]) => {
        const active = key === value;
        return (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(key)}
            className={cn(
              "relative rounded-full font-semibold transition-colors",
              stretch && "flex-1",
              size === "sm" ? "h-7 px-3 text-small" : "h-8 px-4 text-body",
              active ? "text-ink" : "text-ink/60 hover:text-ink",
            )}
          >
            {active && (
              <motion.span
                layoutId={`droplet-${id}`}
                className="absolute inset-0 rounded-full bg-glass/90 shadow-[inset_0_1px_0_rgb(var(--rim)/0.9),inset_0_0_0_1px_rgb(var(--rim)/0.4),0_6px_16px_-8px_rgb(var(--shade)/0.5)] dark:bg-white/[0.13]"
                transition={{ type: "spring", stiffness: 420, damping: 34 }}
              />
            )}
            <span className="relative">{text}</span>
          </button>
        );
      })}
    </div>
  );
}
