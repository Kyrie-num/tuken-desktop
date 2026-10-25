import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { Sheet } from "./Sheet";

/** The native window; null outside the app (tests, a plain browser). */
function current() {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

const CHOICE = "tuken-close-choice";

/** Minimize, maximize and close for the frameless window. Close hides to tray. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  const [asking, setAsking] = useState(false);
  const [remember, setRemember] = useState(true);
  useEffect(() => {
    const w = current();
    if (!w) return;
    let stop: (() => void) | undefined;
    const sync = () =>
      void w
        .isMaximized()
        .then(setMaximized)
        .catch(() => {});
    sync();
    void w
      .onResized(sync)
      .then((u) => (stop = u))
      .catch(() => {});
    return () => stop?.();
  }, []);

  const act = (quit: boolean) =>
    quit ? void invoke("quit").catch(() => {}) : void current()?.close();

  // Closing asks once, so nobody thinks 兔啃 quit while it sits in the tray.
  const close = () => {
    const choice = localStorage.getItem(CHOICE);
    if (choice) act(choice === "quit");
    else setAsking(true);
  };
  const choose = (quit: boolean) => {
    if (remember) localStorage.setItem(CHOICE, quit ? "quit" : "tray");
    setAsking(false);
    act(quit);
  };

  return (
    <div className="flex items-center gap-0.5">
      <button
        type="button"
        className="icon-btn"
        aria-label="最小化"
        onClick={() => void current()?.minimize()}
      >
        <Minus size={15} />
      </button>
      <button
        type="button"
        className="icon-btn"
        aria-label={maximized ? "还原" : "最大化"}
        onClick={() => void current()?.toggleMaximize()}
      >
        {maximized ? <Copy size={13} /> : <Square size={12} />}
      </button>
      <button
        type="button"
        className="icon-btn hover:!bg-danger hover:!text-white"
        aria-label="关闭"
        onClick={close}
      >
        <X size={16} />
      </button>
      <Sheet
        open={asking}
        onOpenChange={setAsking}
        title="关闭兔啃？"
        width={420}
      >
        <p className="text-body text-ink/75">
          留在托盘里的话，没付完的充值会继续处理，到账后也能马上提醒你。点任务栏右下角的兔啃图标可以再打开。
        </p>
        <label className="mt-4 flex items-center gap-2 text-small text-ink/70">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            className="h-4 w-4 accent-[rgb(var(--orange))]"
          />
          不再询问，以后都这样做
        </label>
        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            className="btn btn-glass"
            onClick={() => choose(true)}
          >
            退出兔啃
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => choose(false)}
          >
            留在托盘
          </button>
        </div>
      </Sheet>
    </div>
  );
}
