import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { tuken } from "@/lib/api";
import { useUpdate } from "@/lib/queries";
import { message } from "@/app/context";

/** Downloads the new version; its installer closes 兔啃 and opens it again. */
export function installUpdate(version: string) {
  const id = toast.loading(`正在下载兔啃 ${version}，装好后会自动重新打开`);
  void tuken("update_install").catch((e) => toast.error(message(e), { id }));
}

/** Offers a newer version when one turns up, once per version. */
export function UpdateNotice() {
  const version = useUpdate().data?.version;
  const offered = useRef("");
  useEffect(() => {
    if (!version || offered.current === version) return;
    offered.current = version;
    toast(`兔啃 ${version} 可以更新了`, {
      duration: 20_000,
      action: { label: "更新", onClick: () => installUpdate(version) },
    });
  }, [version]);
  return null;
}
