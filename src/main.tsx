import React from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Toaster } from "sonner";
import { App } from "@/app/App";
import { LoginProvider, UiProvider } from "@/app/context";
import { autostart, windowMaterial } from "@/lib/api";
import { persistCache, restoreCache } from "@/lib/persist";
import { loadPlatform } from "@/lib/platform";
import { queryClient } from "@/lib/queries";
import { applyTheme, storedTheme } from "@/lib/theme";
import "@/styles/index.css";

export function Root() {
  return (
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <UiProvider>
          <LoginProvider>
            <App />
            <Toaster
              position="bottom-center"
              offset={24}
              gap={10}
              toastOptions={{
                unstyled: true,
                classNames: {
                  toast:
                    "glass flex w-[min(400px,calc(100vw-48px))] items-center gap-3 rounded-[18px] px-4 py-3 text-body text-ink",
                  title: "font-medium",
                  description: "text-small text-ink/60",
                  actionButton: "btn btn-glass btn-sm ml-auto shrink-0",
                  success: "[&_[data-icon]]:text-cyan-ink",
                  error: "[&_[data-icon]]:text-danger",
                },
              }}
            />
          </LoginProvider>
        </UiProvider>
      </QueryClientProvider>
    </React.StrictMode>
  );
}

/**
 * Paints the cached pages first, then shows the window: no blank flash. A
 * start at sign-in leaves it in the tray until the user opens it.
 */
export async function boot() {
  const [material, platform, hidden] = await Promise.all([
    windowMaterial.get().catch(() => "none" as const),
    loadPlatform(),
    autostart.launched().catch(() => false),
  ]);
  document.documentElement.dataset.backdrop = material;
  document.documentElement.dataset.platform = platform;
  applyTheme(storedTheme());
  restoreCache(queryClient);
  persistCache(queryClient);
  createRoot(document.getElementById("root")!).render(<Root />);
  if (hidden) return;
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      try {
        void getCurrentWindow()
          .show()
          .catch(() => {});
      } catch {
        /* Not inside the app window (tests, a plain browser). */
      }
    }),
  );
}

if (!__PREVIEW__ || !location.pathname.endsWith("preview.html")) void boot();
