import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Segmented } from "@/components/Segmented";
import { WindowControls } from "@/components/WindowControls";
import { AccountMenu } from "@/features/account/AccountMenu";
import { NoticeBell } from "@/features/notice/Notices";
import { useSession, useUi, type Page } from "@/app/context";
import { isMac } from "@/lib/platform";
import icon from "@/assets/brand/tuken-icon-mini.svg";
import wordmark from "@/assets/brand/tuken-wordmark.svg";

const PAGES = [
  ["home", "首页"],
  ["bills", "账单"],
  ["models", "模型"],
] as const satisfies readonly (readonly [Page, string])[];

/** Whether the window fills the screen, where macOS hides the traffic lights. */
function useFullscreen(watch: boolean) {
  const [full, setFull] = useState(false);
  useEffect(() => {
    if (!watch) return;
    let w: ReturnType<typeof getCurrentWindow>;
    try {
      w = getCurrentWindow();
    } catch {
      return; // Not inside the app window (tests, a plain browser).
    }
    let gone = false;
    let stop: (() => void) | undefined;
    const sync = () =>
      void w
        .isFullscreen()
        .then((full) => !gone && setFull(full))
        .catch(() => {});
    sync();
    void w
      .onResized(sync)
      // Still listening after the toolbar went: stop at once.
      .then((u) => (gone ? u() : (stop = u)))
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  }, [watch]);
  return full;
}

/**
 * The floating title bar: brand, page capsule, account and window buttons.
 * On macOS the system draws the window buttons (the traffic lights) at its
 * left, so the brand moves over and 兔啃 draws none of its own.
 */
export function Toolbar() {
  const { uid } = useSession();
  const { page, go, setup } = useUi();
  const mac = isMac();
  const fullscreen = useFullscreen(mac);
  return (
    <header
      data-tauri-drag-region
      className="relative z-20 grid h-[64px] shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-3 px-4"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pl-1">
        {mac && !fullscreen && (
          <span
            data-tauri-drag-region
            data-testid="traffic-lights"
            aria-hidden="true"
            className="w-16 shrink-0 self-stretch"
          />
        )}
        <img
          src={icon}
          alt=""
          className="pointer-events-none h-7 w-7"
          draggable={false}
        />
        <img
          src={wordmark}
          alt="兔啃"
          className="pointer-events-none h-[22px] w-auto"
          draggable={false}
        />
      </div>
      <div data-tauri-drag-region className="flex justify-center">
        {uid && setup === "done" && (
          <div data-tour="pages" className="rounded-full">
            <Segmented<Page>
              id="nav"
              label="页面"
              value={page}
              options={PAGES}
              onChange={go}
            />
          </div>
        )}
      </div>
      <div
        data-tauri-drag-region
        className="flex items-center justify-end gap-2"
      >
        {uid && <NoticeBell />}
        {uid && <AccountMenu />}
        {!mac && <WindowControls />}
      </div>
    </header>
  );
}
