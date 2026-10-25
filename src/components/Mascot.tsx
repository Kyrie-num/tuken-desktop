import { cn } from "@/lib/cn";
import bunny from "@/assets/brand/tuken.svg";
import cheer from "@/assets/brand/tuken-cheer.svg";
import thanks from "@/assets/brand/tuken-thanks.svg";
import oops from "@/assets/brand/tuken-error.svg";
import flat from "@/assets/brand/tuken-flat.svg";

const art = { bunny, cheer, thanks, oops, flat } as const;

/**
 * 兔啃 at an emotional moment. The ink outline vanishes on dark glass, so it
 * always sits on a light disc.
 */
export function Mascot({
  mood = "bunny",
  size = 112,
  className,
}: {
  mood?: keyof typeof art;
  size?: number;
  className?: string;
}) {
  return (
    <img
      src={art[mood]}
      alt=""
      draggable={false}
      width={size}
      height={size}
      className={cn(
        "shrink-0 rounded-full bg-[#D5F1F0] p-1 shadow-[inset_0_1px_0_rgb(255_255_255/0.8),0_10px_30px_-14px_rgb(0_63_67/0.5)]",
        className,
      )}
    />
  );
}
