import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useAction } from "@/app/context";
import { desktop, type DesktopStatus } from "@/lib/api";
import { qk } from "@/lib/queries";

/** Windows owns the restart and firmware settings; don't promise they're done. */
export function VirtualMachineNotice({ status }: { status: DesktopStatus }) {
  const client = useQueryClient();
  const action = useAction();
  const vm = status.vm;
  const busy = action.busy || status.installing;
  if (!status.installed || !vm || vm.state === "ready") return null;

  const prepare = () =>
    action.run(async () => {
      client.setQueryData<DesktopStatus>(qk.desktop, (old) =>
        old ? { ...old, installing: true, install_step: null } : old,
      );
      try {
        const result = await desktop.prepareVm();
        if (result.state === "ready")
          toast.success("Windows 虚拟机平台已准备好");
        else if (result.state === "error") toast.error(result.message);
        else toast.info(result.message);
      } finally {
        client.setQueryData(qk.desktop, await desktop.status());
      }
    });

  return (
    <div
      role="status"
      className="mt-4 rounded-[16px] bg-ink/[0.045] px-4 py-3 text-left"
    >
      <p className="text-small font-semibold">
        {vm.restart_required ? "还需要重启电脑" : "Claude 工作区的系统准备"}
      </p>
      <p className="mt-1 text-caption leading-relaxed text-ink/65">
        {vm.message}
      </p>
      {vm.can_prepare && (
        <button
          type="button"
          className="btn btn-glass btn-sm mt-3"
          disabled={busy}
          onClick={() => void prepare()}
        >
          {busy && <Loader2 size={14} className="animate-spin" />}
          {busy
            ? "正在准备"
            : vm.state === "disabled"
              ? "开启虚拟机平台"
              : "修复虚拟机平台"}
        </button>
      )}
    </div>
  );
}
