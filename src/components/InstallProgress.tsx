import type { InstallStep } from "@/lib/api";
import { installText } from "@/lib/install";

/** Separate, real download and installation percentages; never a fake timer. */
export function InstallProgress({ step }: { step: InstallStep | null }) {
  const percent =
    step?.step === "download" || step?.step === "install"
      ? Math.max(0, Math.min(100, step.percent))
      : undefined;
  const label = installText(step) ?? "正在准备安装";
  return (
    <div
      role="progressbar"
      aria-label="Claude Desktop 安装进度"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={label}
      className="h-2 w-full overflow-hidden rounded-full bg-ink/10"
    >
      <div
        className={`h-full rounded-full bg-cyan transition-[width] duration-200 ${percent === undefined ? "animate-pulse" : ""}`}
        style={{ width: percent === undefined ? "35%" : `${percent}%` }}
      />
    </div>
  );
}
