import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Mascot } from "@/components/Mascot";
import { Sheet } from "@/components/Sheet";
import type { CreditRow } from "@/lib/api";
import { usd } from "@/lib/format";
import { fetchers, qk, refreshCredit } from "@/lib/queries";
import { useSession } from "@/app/context";

/** The newest credit row this account has been told about, per computer. */
export const rewardsSeen = (uid: number) => `tuken-rewards-${uid}`;
const KINDS = ["signup", "invite"];
const DAY = 24 * 3600_000;

/**
 * Pops up credit the account was given: the sign-up bonus right after
 * registering, and invite rewards as friends sign up, also while 兔啃 is
 * open. Each once.
 */
export function RewardNotice() {
  const client = useQueryClient();
  const { uid } = useSession();
  const history = useQuery({
    queryKey: qk.history(uid ?? 0, 1),
    queryFn: () => fetchers.history(uid!, 1),
    enabled: !!uid,
    staleTime: 0,
    refetchInterval: 60_000,
    refetchIntervalInBackground: true,
  });
  const [shown, setShown] = useState<CreditRow[]>([]);

  useEffect(() => {
    if (!uid || !history.data) return;
    const rows = history.data.items;
    const raw = localStorage.getItem(rewardsSeen(uid));
    const seen = raw === null ? null : Number(raw);
    // First look on this computer: only what came in the last day is news.
    const fresh = rows.filter(
      (r) =>
        KINDS.includes(r.type) &&
        Number(r.value) > 0 &&
        (seen !== null
          ? r.id > seen
          : Date.now() - Date.parse(r.used_at ?? r.created_at) < DAY),
    );
    const newest = Math.max(seen ?? 0, ...rows.map((r) => r.id));
    localStorage.setItem(rewardsSeen(uid), String(newest));
    if (fresh.length) {
      setShown(fresh);
      refreshCredit(client, uid);
    }
  }, [uid, history.data, client]);

  const signup = shown.filter((r) => r.type === "signup");
  const invites = shown.filter((r) => r.type === "invite");
  const total = shown.reduce((s, r) => s + Number(r.value), 0);

  return (
    <Sheet
      open={shown.length > 0}
      onOpenChange={(open) => !open && setShown([])}
      title={signup.length ? "注册成功，送你额度" : "邀请奖励到账"}
      width={400}
    >
      <div className="flex flex-col items-center pb-1 text-center">
        <span className="flex h-[112px] w-[112px] items-center justify-center rounded-full bg-white/80">
          <Mascot mood="cheer" size={100} />
        </span>
        <p className="mt-4 font-display text-[40px] font-semibold leading-none text-cyan-ink tnum">
          +{usd(total)}
        </p>
        <div className="mt-3 space-y-1 text-body text-ink/70">
          {signup.length > 0 && <p>新用户注册奖励已到账，可以直接用。</p>}
          {invites.length > 0 && (
            <p>
              {invites.length === 1
                ? "有好友用你的邀请码注册了，"
                : `有 ${invites.length} 位好友用你的邀请码注册了，`}
              邀请奖励已到账。
            </p>
          )}
        </div>
        <button
          type="button"
          className="btn btn-primary mt-6 min-w-[140px]"
          onClick={() => setShown([])}
        >
          好的
        </button>
      </div>
    </Sheet>
  );
}
