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
    case "permission":
      return "请在 Windows 授权提示中选择「是」";
    case "install":
      return `正在安装 ${step.percent}%`;
    case "copy":
      return "正在装进「应用程序」";
  }
}

/** Listen before the click so fast native events are not lost. */
export function useInstallStep(active: boolean, snapshot?: InstallStep | null) {
  const [step, setStep] = useState<InstallStep | null>(null);
  useEffect(() => {
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
  }, []);
  useEffect(() => {
    if (!active) setStep(null);
  }, [active]);
  return active ? (step ?? snapshot ?? null) : null;
}
