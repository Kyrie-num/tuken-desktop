import { Copy } from "lucide-react";
import { toast } from "sonner";
import { Sheet } from "@/components/Sheet";
import { usd } from "@/lib/format";
import { useInvite } from "@/lib/queries";
import { useSession, useUi } from "@/app/context";

/** 邀请好友: my code to share, what it brings, and how many came. */
export function InviteSheet() {
  const { uid } = useSession();
  const { inviteOpen, setInviteOpen } = useUi();
  const invite = useInvite(uid);
  const i = invite.data?.supported ? invite.data : null;

  const copy = () =>
    void navigator.clipboard
      .writeText(i?.code ?? "")
      .then(() => toast.success("邀请码已复制"))
      .catch(() => toast.error("复制失败，请手动选中复制"));

  return (
    <Sheet
      open={inviteOpen}
      onOpenChange={setInviteOpen}
      title="邀请好友"
      width={440}
    >
      {i ? (
        <>
          <div className="flex items-center gap-3 rounded-[18px] bg-ink/[0.04] px-4 py-3.5">
            <div className="min-w-0 flex-1">
              <p className="text-caption text-ink/55">我的邀请码</p>
              <p className="selectable mt-0.5 font-mono text-[24px] font-semibold tracking-[0.12em]">
                {i.code}
              </p>
            </div>
            <button
              type="button"
              className="btn btn-glass btn-sm"
              onClick={copy}
            >
              <Copy size={14} />
              复制
            </button>
          </div>
          <p className="mt-4 text-body text-ink/75">
            好友在兔啃注册时填上你的邀请码
            {i.reward ? `，你得 ${usd(i.reward)}` : ""}
            {i.invitee_reward ? `，好友也得 ${usd(i.invitee_reward)}` : ""}。
          </p>
          <p className="mt-2 text-small text-ink/55">
            已邀请 {i.invited} 人，共得 {usd(i.earned)}
          </p>
        </>
      ) : (
        <div className="skeleton h-20" />
      )}
    </Sheet>
  );
}
