import { useEffect, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { cn } from "@/lib/cn";

/** Sheets slide in first; a native view can't follow that animation. */
const SETTLE_MS = 340;

/**
 * Keeps a box free for the in-app web page (the site's sign-in). The page
 * is a native view laid over this box, so the box only
 * reports where it is; what loads is decided natively. `children` show until
 * the page covers them, and in previews and tests, where there is none.
 */
export function WebPanel({
  className,
  children,
}: {
  className?: string;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let ready = false;
    let frame = 0;
    const report = () => {
      if (!ready) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const r = el.getBoundingClientRect();
        if (r.width < 16 || r.height < 16) return;
        const rect = { x: r.left, y: r.top, width: r.width, height: r.height };
        void invoke("panel_show", { rect }).catch(() => {});
      });
    };
    const settle = window.setTimeout(() => {
      ready = true;
      report();
    }, SETTLE_MS);
    const resize = new ResizeObserver(report);
    resize.observe(el);
    window.addEventListener("resize", report);
    document.addEventListener("scroll", report, true);
    return () => {
      window.clearTimeout(settle);
      cancelAnimationFrame(frame);
      resize.disconnect();
      window.removeEventListener("resize", report);
      document.removeEventListener("scroll", report, true);
      void invoke("panel_hide").catch(() => {});
    };
  }, []);
  return (
    <div
      ref={ref}
      className={cn(
        "relative flex items-center justify-center overflow-hidden rounded-[16px] bg-white",
        className,
      )}
    >
      {children}
    </div>
  );
}
