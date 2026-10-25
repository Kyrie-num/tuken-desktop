import { Segmented } from "@/components/Segmented";
import { WindowControls } from "@/components/WindowControls";
import { AccountMenu } from "@/features/account/AccountMenu";
import { NoticeBell } from "@/features/notice/Notices";
import { useSession, useUi, type Page } from "@/app/context";
import icon from "@/assets/brand/tuken-icon-mini.svg";
import wordmark from "@/assets/brand/tuken-wordmark.svg";

const PAGES = [
  ["home", "首页"],
  ["bills", "账单"],
  ["models", "模型"],
] as const satisfies readonly (readonly [Page, string])[];

/** The floating title bar: brand, page capsule, account and window buttons. */
export function Toolbar() {
  const { uid } = useSession();
  const { page, go, setup } = useUi();
  return (
    <header
      data-tauri-drag-region
      className="relative z-20 grid h-[64px] shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-3 px-4"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pl-1">
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
        <WindowControls />
      </div>
    </header>
  );
}
