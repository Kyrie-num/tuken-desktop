import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Mascot } from "@/components/Mascot";
import { Segmented } from "@/components/Segmented";
import type { CreditRow, Range } from "@/lib/api";
import { cn } from "@/lib/cn";
import { beijingDate, count, monthDay, tokens, usd, when } from "@/lib/format";
import {
  fetchers,
  qk,
  useHistory,
  useUsage,
  useUsageStats,
} from "@/lib/queries";
import { useSession } from "@/app/context";

type Preset = "today" | "week" | "month" | "30d";
const PRESETS = [
  ["today", "今天"],
  ["week", "近 7 天"],
  ["month", "本月"],
  ["30d", "近 30 天"],
] as const;

function rangeOf(p: Preset): Range {
  const today = beijingDate(0);
  switch (p) {
    case "today":
      return { start_date: today, end_date: today };
    case "week":
      return { start_date: beijingDate(6), end_date: today };
    case "month":
      return { start_date: `${today.slice(0, 8)}01`, end_date: today };
    case "30d":
      return { start_date: beijingDate(29), end_date: today };
  }
}

const creditType: Record<string, string> = {
  balance: "余额充值",
  admin_balance: "余额调整",
  concurrency: "并发额度",
  admin_concurrency: "并发调整",
  subscription: "订阅",
  invitation: "邀请码注册",
  signup: "新用户奖励",
  invite: "邀请奖励",
  checkin: "签到奖励",
  checkin_streak: "连续签到奖励",
  checkin_expired: "签到额度到期",
  first_recharge: "首充奖励",
};

/** Credits paid in dollars; the others count slots or days. */
const inDollars = (type: string) =>
  type.includes("balance") ||
  type === "first_recharge" ||
  ["signup", "invite"].includes(type) ||
  type.startsWith("checkin");

/** Check-in credit lapses: when, or that it has. */
function lapse(r: CreditRow) {
  if (!r.expires_at || !["checkin", "checkin_streak"].includes(r.type))
    return "";
  return Date.parse(r.expires_at) <= Date.now()
    ? " · 已到期"
    : ` · ${monthDay(r.expires_at)}到期`;
}

/** "+$50.00", "−$5.00" for a deduction, nothing for a code worth nothing. */
function amount(r: CreditRow) {
  const v = Number(r.value);
  if (!v) return "";
  const text = inDollars(r.type) ? usd(Math.abs(v)) : count(Math.abs(v));
  return `${v < 0 ? "−" : "+"}${text}`;
}

function Pager({
  page,
  pages,
  total,
  busy,
  onPage,
}: {
  page: number;
  pages: number;
  total: number;
  busy: boolean;
  onPage: (page: number) => void;
}) {
  if (pages <= 1) return null;
  return (
    <div className="mt-3 flex items-center justify-between text-caption text-ink/55">
      <span>
        共 {count(total)} 条 · 第 {page} / {pages} 页
      </span>
      <div className="flex gap-1">
        <button
          type="button"
          className="icon-btn"
          aria-label="上一页"
          disabled={busy || page <= 1}
          onClick={() => onPage(page - 1)}
        >
          <ChevronLeft size={16} />
        </button>
        <button
          type="button"
          className="icon-btn"
          aria-label="下一页"
          disabled={busy || page >= pages}
          onClick={() => onPage(page + 1)}
        >
          <ChevronRight size={16} />
        </button>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string | undefined }) {
  return (
    <div className="glass px-5 py-4">
      <p className="text-caption text-ink/55">{label}</p>
      <p className="mt-1 font-display text-[24px] font-semibold tnum">
        {value ?? (
          <span className="skeleton inline-block h-6 w-20 align-middle" />
        )}
      </p>
    </div>
  );
}

function Usage() {
  const client = useQueryClient();
  const { uid } = useSession();
  const [preset, setPreset] = useState<Preset>("week");
  const [page, setPage] = useState(1);
  const range = useMemo(() => rangeOf(preset), [preset]);
  const usage = useUsage(uid, range, page);
  const stats = useUsageStats(uid, range);
  useEffect(() => setPage(1), [preset]);
  // The next page is usually what comes next.
  useEffect(() => {
    if (uid && usage.data && page < usage.data.pages)
      void client.prefetchQuery({
        queryKey: qk.usage(uid, range, page + 1),
        queryFn: () => fetchers.usage(uid, range, page + 1),
      });
  }, [client, uid, range, page, usage.data]);

  const rows = usage.data?.items;
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        {PRESETS.map(([key, label]) => (
          <button
            key={key}
            type="button"
            onClick={() => setPreset(key)}
            className={cn(
              "btn btn-sm",
              preset === key ? "btn-glass" : "btn-ghost",
            )}
            aria-pressed={preset === key}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="mt-4 grid grid-cols-3 gap-3">
        <Stat
          label="请求次数"
          value={stats.data && count(stats.data.total_requests)}
        />
        <Stat
          label="Token"
          value={stats.data && tokens(stats.data.total_tokens)}
        />
        <Stat
          label="花费"
          value={stats.data && usd(stats.data.total_actual_cost, true)}
        />
      </div>
      <section
        className={cn(
          "glass mt-4 p-4 transition-opacity",
          usage.isPlaceholderData && "opacity-70",
        )}
      >
        {!rows ? (
          [0, 1, 2, 3].map((i) => (
            <div key={i} className="skeleton my-2 h-11" />
          ))
        ) : rows.length === 0 ? (
          <div className="flex items-center gap-4 px-2 py-6">
            <Mascot mood="flat" size={64} />
            <div>
              <p className="text-strong font-semibold">这段时间还没有调用</p>
              <p className="text-caption text-ink/55">
                用 Claude Desktop 聊几句，这里就会有记录。
              </p>
            </div>
          </div>
        ) : (
          <div className="selectable">
            {rows.map((r) => (
              <div
                key={r.id}
                className="row grid grid-cols-[1fr_auto] items-center gap-x-4 px-3 py-2.5"
              >
                <div className="min-w-0">
                  <p className="truncate text-strong font-semibold">
                    {r.model}
                  </p>
                  <p className="truncate text-caption text-ink/55">
                    {when(r.created_at)}
                    {r.key_name ? ` · ${r.key_name}` : ""} · 输入{" "}
                    {tokens(r.input_tokens)} · 输出 {tokens(r.output_tokens)}
                    {r.cache_read_tokens
                      ? ` · 缓存命中 ${tokens(r.cache_read_tokens)}`
                      : ""}
                  </p>
                </div>
                <p className="font-display text-strong font-semibold tnum">
                  {usd(r.actual_cost, true)}
                </p>
              </div>
            ))}
          </div>
        )}
        {usage.data && (
          <Pager
            page={page}
            pages={usage.data.pages}
            total={usage.data.total}
            busy={usage.isFetching}
            onPage={setPage}
          />
        )}
      </section>
    </>
  );
}

function Credits() {
  const client = useQueryClient();
  const { uid } = useSession();
  const [page, setPage] = useState(1);
  const history = useHistory(uid, page);
  useEffect(() => {
    if (uid && history.data && page < history.data.pages)
      void client.prefetchQuery({
        queryKey: qk.history(uid, page + 1),
        queryFn: () => fetchers.history(uid, page + 1),
      });
  }, [client, uid, page, history.data]);

  const rows = history.data?.items;
  return (
    <section
      className={cn(
        "glass p-4 transition-opacity",
        history.isPlaceholderData && "opacity-70",
      )}
    >
      {!rows ? (
        [0, 1, 2].map((i) => <div key={i} className="skeleton my-2 h-11" />)
      ) : rows.length === 0 ? (
        <div className="flex items-center gap-4 px-2 py-6">
          <Mascot mood="thanks" size={64} />
          <div>
            <p className="text-strong font-semibold">还没有充值记录</p>
            <p className="text-caption text-ink/55">
              在首页点「充值」，付款后会自动到账。
            </p>
          </div>
        </div>
      ) : (
        <div className="selectable">
          {rows.map((r) => (
            <div
              key={r.id}
              className="row grid grid-cols-[1fr_auto] items-center gap-x-4 px-3 py-2.5"
            >
              <div className="min-w-0">
                <p className="text-strong font-semibold">
                  {creditType[r.type] ?? r.type}
                </p>
                <p className="truncate text-caption text-ink/55">
                  {when(r.used_at ?? r.created_at)}
                  {lapse(r)}
                  {r.notes ? ` · ${r.notes}` : ""}
                </p>
              </div>
              <p
                className={cn(
                  "font-display text-strong font-semibold tnum",
                  Number(r.value) < 0 ? "text-ink/70" : "text-cyan-ink",
                )}
              >
                {amount(r)}
              </p>
            </div>
          ))}
        </div>
      )}
      {history.data && (
        <Pager
          page={page}
          pages={history.data.pages}
          total={history.data.total}
          busy={history.isFetching}
          onPage={setPage}
        />
      )}
    </section>
  );
}

export function Bills() {
  const [tab, setTab] = useState<"usage" | "credits">("usage");
  return (
    <div className="mx-auto w-full max-w-[1000px] px-6 pb-8">
      <div className="mb-4 flex items-center justify-between gap-3">
        <h1 className="font-display text-title font-semibold">账单</h1>
        <Segmented
          id="bills"
          label="账单类型"
          size="sm"
          value={tab}
          options={[
            ["usage", "用量"],
            ["credits", "充值记录"],
          ]}
          onChange={setTab}
        />
      </div>
      {tab === "usage" ? <Usage /> : <Credits />}
    </div>
  );
}
