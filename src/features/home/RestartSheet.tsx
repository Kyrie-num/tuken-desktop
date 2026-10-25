import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Sheet } from "@/components/Sheet";
import { desktop } from "@/lib/api";
import { qk } from "@/lib/queries";
import { message, useUi } from "@/app/context";

export function RestartSheet() {
  const client = useQueryClient();
  const { restartOpen, setRestartOpen } = useUi();
  const restart = async () => {
    setRestartOpen(false);
    try {
      await desktop.open(true);
      toast.success("正在重启 Claude Desktop，打开后就在用兔啃了");
      setTimeout(
        () => void client.invalidateQueries({ queryKey: qk.desktop }),
        3000,
      );
    } catch (e) {
      toast.error(message(e));
    }
  };
  return (
    <Sheet
      open={restartOpen}
      onOpenChange={setRestartOpen}
      title="重启 Claude Desktop？"
      width={440}
    >
      <p className="text-body text-ink/75">
        已经接好了。新设置要在 Claude Desktop
        重启后才生效；重启会关闭它所有的窗口，没发出去的内容会丢失。
      </p>
      <div className="mt-6 flex justify-end gap-2">
        <button
          type="button"
          className="btn btn-glass"
          onClick={() => setRestartOpen(false)}
        >
          稍后自己重启
        </button>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => void restart()}
        >
          立即重启
        </button>
      </div>
    </Sheet>
  );
}
