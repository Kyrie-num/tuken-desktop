import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { InstallStep } from "./api";

/** What an install of Claude Desktop is doing, in a few words. */
export function installText(step: InstallStep | null): string | null {
  if (!step) return null;
  switch (step.step) {
    case "download":
      return step.percent > 0 ? `正在下载 ${step.percent}%` : "正在下载";
    case "verify":
      return "正在校验安装包";
    case "copy":
      return "正在装进「应用程序」";
  }
}

/** The step the install in progress has reached (macOS reports them). */
export function useInstallStep(active: boolean) {
  const [step, setStep] = useState<InstallStep | null>(null);
  useEffect(() => {
    if (!active) {
      setStep(null);
      return;
    }
    let gone = false;
    let stop: (() => void) | undefined;
    void listen<InstallStep>("tuken://desktop-install", (e) =>
      setStep(e.payload),
    )
      // Still listening after the page moved on: stop at once.
      .then((u) => (gone ? u() : (stop = u)))
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  }, [active]);
  return active ? step : null;
}
