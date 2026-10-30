import {
  CalendarCheck,
  Clock,
  Gift,
  ReceiptText,
  UserPlus,
  Wallet,
} from "lucide-react";
import {
  useAppState,
  useCheckin,
  useCredit,
  useDashboard,
  useInvite,
  useProfile,
} from "@/lib/queries";
import { monthDay, usd } from "@/lib/format";
import { Mascot } from "@/components/Mascot";
import { useSession, useUi } from "@/app/context";

/**
 * The one bold element: a liquid lens over two colour drops, with the balance
 * in large figures and 兔啃 peeking in from the corner.
 */
export function BalanceLens() {
  const { uid, account } = useSession();
  const { go, setRechargeOpen, setCheckinOpen, setInviteOpen } = useUi();
  const profile = useProfile(uid);
  const dashboard = useDashboard(uid);
  const { data: state } = useAppState();
  const pending = state?.orders.filter((o) =>
    ["awaiting_payment", "awaiting_delivery", "redeeming"].includes(o.status),
  ).length;
  const balance = profile.data?.balance;
  const today = dashboard.data?.today_actual_cost;
  const checkin = useCheckin(uid);
  const c =
    checkin.data?.supported && checkin.data.enabled ? checkin.data : null;
  const reward =
    c && c.reward_min !== null && c.reward_min === c.reward_max
      ? `+${usd(c.reward_min)}`
      : "领额度";
  const invite = useInvite(uid);
  const inviting =
    invite.data?.supported && invite.data.enabled ? invite.data : null;
  // Check-in credit that lapses: how much, and when the soonest goes.
  const credit = useCredit(uid).data;
  const limited =
    credit?.supported && credit.limited_total > 0 && credit.limited.length
      ? {
          total: credit.limited_total,
          first: credit.limited[0].expires_at,
          later: credit.limited.some(
            (l) =>
              monthDay(l.expires_at) !== monthDay(credit.limited[0].expires_at),
          ),
        }
      : null;

  return (
    <div className="relative isolate h-full">
      {/* What the lens bends: drops of SUSTech orange and cyan. */}
      <div
        aria-hidden="true"
        className="lens-drops pointer-events-none absolute inset-0 -z-10"
      >
        <div className="absolute left-[8%] top-[18%] h-36 w-36 rounded-full bg-orange/55 blur-2xl" />
        <div className="absolute bottom-[6%] right-[22%] h-40 w-48 rounded-full bg-cyan/50 blur-2xl" />
      </div>
      <section
        data-tour="balance"
        className="glass glass-lens relative flex h-full min-h-[236px] flex-col justify-between overflow-hidden p-7"
      >
        <Mascot
          size={150}
          className="pointer-events-none absolute -bottom-10 right-5 opacity-95"
        />
        {inviting && (
          <button
            type="button"
            data-tour="invite"
            className="btn btn-glass btn-sm absolute right-6 top-6"
            onClick={() => setInviteOpen(true)}
          >
            <UserPlus size={14} />
            邀请好友{inviting.reward ? ` 得 ${usd(inviting.reward)}` : ""}
          </button>
        )}
        <div className="relative max-w-[70%]">
          <p className="truncate text-small text-ink/60">
            {profile.data?.email ?? account?.email}
          </p>
          <p className="mt-3 font-display text-hero font-semibold tracking-tight tnum">
            {balance === undefined ? (
              <span className="skeleton inline-block h-[46px] w-44 align-middle" />
            ) : (
              usd(balance)
            )}
          </p>
          <p className="mt-1.5 text-small text-ink/65">
            可用余额
            {today !== undefined && <> · 今天用了 {usd(today, true)}</>}
            {Number(profile.data?.frozen_balance) > 0 && (
              <> · 冻结 {usd(profile.data?.frozen_balance)}</>
            )}
          </p>
          {limited && (
            <p className="mt-1 flex items-center gap-1.5 text-small text-ink/65">
              <Clock size={13} className="shrink-0" aria-hidden="true" />
              其中 {usd(limited.total)} 是限时额度，
              {limited.later ? "最早 " : ""}
              {monthDay(limited.first)}到期
            </p>
          )}
        </div>
        <div className="relative mt-6 flex flex-wrap items-center gap-2">
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setRechargeOpen(true)}
          >
            <Wallet size={16} />
            充值
          </button>
          <button
            type="button"
            className="btn btn-glass"
            onClick={() => go("bills")}
          >
            <ReceiptText size={16} />
            账单
          </button>
          {c && (
            <button
              type="button"
              data-tour="checkin"
              className="btn btn-glass"
              onClick={() => setCheckinOpen(true)}
            >
              {c.checked_in ? <CalendarCheck size={16} /> : <Gift size={16} />}
              {c.checked_in ? "签到日历" : `签到 ${reward}`}
            </button>
          )}
          {!!pending && (
            <button
              type="button"
              className="chip chip-warn ml-1 h-7 px-3"
              onClick={() => setRechargeOpen(true)}
            >
              {pending} 笔充值处理中
            </button>
          )}
        </div>
      </section>
    </div>
  );
}
