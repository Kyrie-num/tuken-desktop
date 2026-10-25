import { invoke } from "@tauri-apps/api/core";

/** The system 兔啃 runs on, as the native side reports it. */
export type Platform = "windows" | "macos" | "linux";

function known(p: unknown): Platform {
  return p === "macos" || p === "linux" ? p : "windows";
}

/** Asked once at start; Windows when there is no native side to ask. */
export async function loadPlatform(): Promise<Platform> {
  try {
    return known(await invoke<string>("platform"));
  } catch {
    return "windows";
  }
}

/** The platform `boot` put on `<html data-platform>`. */
export function platform(): Platform {
  return known(document.documentElement.dataset.platform);
}

/** macOS draws its own window buttons, the traffic lights. */
export const isMac = () => platform() === "macos";
