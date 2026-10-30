import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "framer-motion";
import { ChevronLeft, ChevronRight, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Sheet } from "@/components/Sheet";
import { tuken } from "@/lib/api";
import { cn } from "@/lib/cn";
import { beijingDate, monthDay, usd } from "@/lib/format";
import {
  qk,
  refreshCredit,
  useCheckin,
  useCheckinHistory,
} from "@/lib/queries";
import { useAction, useSession, useUi } from "@/app/context";

const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

/** `YYYY-MM` moved by `n` months. */
function shift(month: string, n: number) {
  const [y, m] = month.split("-").map(Number);
  const d = new Date(Date.UTC(y, m - 1 + n, 1));
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
}

/** The month's days, Monday first, with blanks before the 1st. */
function grid(month: string) {
  const [y, m] = month.split("-").map(Number);
  const lead = (new Date(Date.UTC(y, m - 1, 1)).getUTCDay() + 6) % 7;
  const days = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return [
    ...Array.from({ length: lead }, () => null),
    ...Array.from(
      { length: days },
      (_, i) => `${month}-${String(i + 1).padStart(2, "0")}`,
    ),
  ];
}

/** Daily check-in: the button, and a calendar of what each day brought. */
export function CheckinSheet() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { checkinOpen, setCheckinOpen } = useUi();
  const today = beijingDate(0);
  const thisMonth = today.slice(0, 7);
  const [month, setMonth] = useState(thisMonth);
  const status = useCheckin(uid);
  const history = useCheckinHistory(uid, checkinOpen ? month : null);
  const action = useAction();

  useEffect(() => {
    if (checkinOpen) setMonth(thisMonth);
  }, [checkinOpen, thisMonth]);

  const c = status.data?.supported ? status.data : null;
  const fixed =
    c && c.reward_min !== null && c.reward_min === c.reward_max
      ? usd(c.reward_min)
      : null;
  // What each day brought, its streak reward included.
  const rewards = new Map(
    (history.data?.days ?? []).map((d) => [
      d.date,
      d.reward === null && !d.bonus ? null : (d.reward ?? 0) + (d.bonus ?? 0),
    ]),
  );
  // Every `streakDays` days in a row earn a reward on top: how far to the next.
  const streakDays = c?.streak_days ?? 0;
  const streakReward = c?.streak_reward ?? 0;
  const left =
    c && streakDays > 0 && streakReward > 0
      ? streakDays - (c.streak % streakDays)
      : null;
  const streakNote =
    left === null
      ? null
      : !c?.checked_in && left === 1
        ? `今天签到，额外送 ${usd(streakReward)}`
        : `连续签到每满 ${streakDays} 天额外送 ${usd(streakReward)}，再签 ${left} 天就到`;
  const lifetime = c?.credit_days ?? 0;

  const checkIn = () =>
    action.run(async () => {
      try {
        const r = await tuken<{
          reward: number | null;
          streak: number;
          streak_reward?: number | null;
          expires_at?: string | null;
        }>("checkin", { as_user: uid });
        const total = (r.reward ?? 0) + (r.streak_reward ?? 0);
        const notes = [
          r.streak_reward
            ? `含连续签到 ${r.streak} 天奖励 ${usd(r.streak_reward)}`
            : "",
          r.expires_at ? `${monthDay(r.expires_at)}到期` : "",
        ].filter(Boolean);
        toast.success(total ? `签到成功，额度 +${usd(total)}` : "签到成功", {
          description: notes.length ? notes.join("，") : undefined,
        });
        refreshCredit(client, uid);
      } finally {
        if (uid) {
          void client.invalidateQueries({ queryKey: qk.checkin(uid) });
          void client.invalidateQueries({ queryKey: ["u", uid, "checkins"] });
        }
      }
    });

  return (
    <Sheet
      open={checkinOpen}
      onOpenChange={setCheckinOpen}
      title="每日签到"
      description={c && c.streak > 0 ? `已连续签到 ${c.streak} 天` : undefined}
      width={460}
    >
      <div className="mb-3 flex items-center justify-between">
        <button
          type="button"
          className="icon-btn"
          aria-label="上个月"
          onClick={() => setMonth((m) => shift(m, -1))}
        >
          <ChevronLeft size={18} />
        </button>
        <p className="text-strong font-semibold tnum">
          {Number(month.slice(0, 4))} 年 {Number(month.slice(5))} 月
        </p>
        <button
          type="button"
          className="icon-btn"
          aria-label="下个月"
          disabled={month >= thisMonth}
          onClick={() => setMonth((m) => shift(m, 1))}
        >
          <ChevronRight size={18} />
        </button>
      </div>

      <div className="grid grid-cols-7 gap-1 text-center" role="grid">
        {WEEK.map((w) => (
          <span key={w} className="pb-1 text-caption text-ink/45">
            {w}
          </span>
        ))}
        {grid(month).map((date, i) => {
          if (!date) return <span key={`blank-${i}`} />;
          const reward = rewards.get(date) ?? null;
          const got = rewards.has(date) || (date === today && !!c?.checked_in);
          return (
            <div
              key={date}
              role="gridcell"
              aria-label={`${Number(date.slice(8))} 日${got ? "，已签到" : ""}${reward ? `，${usd(reward)}` : ""}`}
              className={cn(
                "flex h-[52px] flex-col items-center justify-center rounded-[12px] tnum",
                got ? "bg-cyan/[0.14]" : "",
                date === today && "ring-2 ring-orange/70",
                date > today && "opacity-35",
              )}
            >
              <span
                className={cn(
                  "text-small",
                  got ? "font-semibold text-ink" : "text-ink/65",
                )}
              >
                {Number(date.slice(8))}
              </span>
              <AnimatePresence initial={false}>
                {got && (
                  <motion.span
                    initial={{ opacity: 0, y: 4, scale: 0.8 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    className="text-[11px] font-semibold leading-tight text-cyan-ink"
                  >
                    {reward ? `+${usd(reward)}` : "已签"}
                  </motion.span>
                )}
              </AnimatePresence>
            </div>
          );
        })}
      </div>

      <button
        type="button"
        className="btn btn-primary btn-lg mt-5 w-full"
        disabled={!c?.enabled || !!c?.checked_in || action.busy}
        onClick={() => void checkIn()}
      >
        {action.busy && <Loader2 size={16} className="animate-spin" />}
        {c?.checked_in
          ? "今天已签到，明天再来"
          : fixed
            ? `签到领 ${fixed}`
            : "签到领额度"}
      </button>
      {(streakNote || lifetime > 0) && (
        <div className="mt-3 space-y-1 text-center text-small text-ink/60">
          {streakNote && <p>{streakNote}</p>}
          {lifetime > 0 && (
            <p>签到领的额度 {lifetime} 天内有效，用的时候会先扣它</p>
          )}
        </div>
      )}
    </Sheet>
  );
}
