import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { motion } from "framer-motion";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Mascot } from "@/components/Mascot";
import { Sheet } from "@/components/Sheet";
import { tuken, type AppState, type Order } from "@/lib/api";
import { cn } from "@/lib/cn";
import { cny, creditOf } from "@/lib/format";
import { qk, refreshCredit, useCatalog, useQuote } from "@/lib/queries";
import { useAction, useSession, useUi } from "@/app/context";

/** How often an open payment asks whether it has been paid. */
const POLL_MS = 3000;
const OPEN = ["awaiting_payment", "awaiting_delivery", "redeeming"];

const statusText: Record<string, string> = {
  awaiting_payment: "等待付款",
  awaiting_delivery: "已付款，等待发货",
  redeeming: "正在到账",
  attention: "需要核对",
  creating: "下单结果待确认",
};

/**
 * Buy credit without leaving the app: pick an amount, scan the code in
 * Alipay's own cashier window with the phone (小荷包 included), and the
 * credit lands by itself.
 */
export function RechargeSheet() {
  const client = useQueryClient();
  const { uid, account, state } = useSession();
  const { rechargeOpen, setRechargeOpen } = useUi();
  const catalog = useCatalog(!!uid);
  const [goodsId, setGoodsId] = useState<string | null>(null);
  const [channelId, setChannelId] = useState<number | null>(null);
  const channels = catalog.data?.channels ?? [];
  const channel =
    channels.find((c) => c.id === channelId)?.id ?? channels[0]?.id ?? null;
  const quote = useQuote(goodsId, channel);
  const [paying, setPaying] = useState<Order | null>(null);
  const [done, setDone] = useState<Order | null>(null);
  const action = useAction();

  const refreshState = () =>
    tuken<AppState>("state").then((s) => client.setQueryData(qk.state, s));

  useEffect(() => {
    if (!rechargeOpen) {
      setPaying(null);
      setDone(null);
    }
  }, [rechargeOpen]);

  const payingId = paying?.id;
  useEffect(() => {
    if (!rechargeOpen || !payingId) return;
    let stopped = false;
    let running = false;
    const timer = window.setInterval(async () => {
      if (running) return;
      running = true;
      try {
        const o = await tuken<Order>("order_poll", { id: payingId });
        if (stopped) return;
        if (o.status === "completed") {
          setPaying(null);
          setDone(o);
          refreshCredit(client, uid);
        } else if (o.status === "closed" || o.status === "attention") {
          setPaying(null);
          toast.error(o.message, { duration: 8000 });
          void refreshState();
        } else {
          setPaying(o);
        }
      } catch {
        /* Retried on the next tick; the native worker keeps checking too. */
      } finally {
        running = false;
      }
    }, POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [rechargeOpen, payingId, client, uid]);

  // Alipay's cashier opens in its own narrow window for the order on screen.
  const openCashier = (id: string) =>
    void tuken("order_cashier", { id }).catch((e) => toast.error(String(e)));
  const cashierFor = paying?.status === "awaiting_payment" ? paying.id : null;
  useEffect(() => {
    if (cashierFor) openCashier(cashierFor);
  }, [cashierFor]);

  const pay = () =>
    action.run(async () => {
      if (!goodsId || channel === null || !quote.data) return;
      try {
        const o = await tuken<Order>("order_create", {
          goods_id: goodsId,
          channel_id: channel,
          confirmed_amount: quote.data.total_amount,
        });
        setPaying(o);
      } catch (e) {
        // A changed price shows the new one before anything is paid.
        void client.invalidateQueries({ queryKey: ["quote"] });
        throw e;
      } finally {
        void refreshState();
      }
    });

  const resume = (o: Order) =>
    action.run(async () => {
      setPaying(await tuken<Order>("order_pay", { id: o.id }));
    });

  const cancel = (o: Order) =>
    action.run(async () => {
      await tuken<Order>("order_close", { id: o.id });
      setPaying(null);
      await refreshState();
      toast.success("订单已取消");
    });

  const pending = (state?.orders ?? []).filter(
    (o) => OPEN.includes(o.status) && o.id !== paying?.id,
  );
  const credit = done ? creditOf(done.name) : null;

  return (
    <Sheet
      open={rechargeOpen}
      onOpenChange={setRechargeOpen}
      title="充值"
      description={`付款后额度自动充值到 ${account?.email ?? "当前账户"}`}
      width={580}
    >
      {done ? (
        <motion.div
          initial={{ opacity: 0, scale: 0.96 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ type: "spring", stiffness: 300, damping: 22 }}
          className="flex flex-col items-center py-6 text-center"
        >
          <Mascot mood="cheer" size={120} />
          <p className="mt-5 font-display text-title font-semibold">
            {credit !== null ? `+$${credit} 已到账` : "额度已到账"}
          </p>
          <p className="mt-1.5 text-body text-ink/65">
            余额已经更新，接着用吧。
          </p>
          <button
            type="button"
            className="btn btn-primary mt-6 min-w-[140px]"
            onClick={() => setRechargeOpen(false)}
          >
            好的
          </button>
        </motion.div>
      ) : paying ? (
        <div className="flex flex-col items-center py-4 text-center">
          <p className="font-display text-[34px] font-semibold leading-none tnum">
            {cny(paying.amount)}
          </p>
          <p className="mt-2 text-body text-ink/60">
            {creditOf(paying.name) !== null
              ? `$${creditOf(paying.name)} 额度`
              : paying.name}
          </p>
          <p className="mt-6 max-w-[380px] text-body text-ink/80">
            {paying.status === "awaiting_payment"
              ? "已打开支付宝付款窗口，用手机支付宝扫码，可以用小荷包付款。付完额度自动到账。"
              : "付款已收到，额度马上到账"}
          </p>
          <p
            role="status"
            className="mt-3 flex items-center gap-2 text-small text-ink/60"
          >
            {paying.status === "awaiting_payment" ? (
              <span className="relative flex h-2 w-2" aria-hidden="true">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-orange/60" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-orange" />
              </span>
            ) : (
              <Loader2 size={14} className="animate-spin text-cyan-ink" />
            )}
            {statusText[paying.status] ?? paying.message}
          </p>
          {paying.status === "awaiting_payment" && (
            <div className="mt-6 flex items-center gap-2">
              <button
                type="button"
                className="btn btn-glass"
                onClick={() => openCashier(paying.id)}
              >
                重新打开付款窗口
              </button>
              <button
                type="button"
                className="btn btn-ghost text-ink/55"
                disabled={action.busy}
                onClick={() => void cancel(paying)}
              >
                不买了
              </button>
            </div>
          )}
        </div>
      ) : (
        <>
          {catalog.data ? (
            catalog.data.goods.length ? (
              <div
                role="radiogroup"
                aria-label="充值金额"
                className="grid grid-cols-5 gap-2.5"
              >
                {catalog.data.goods.map((g) => {
                  const value = creditOf(g.name);
                  const chosen = goodsId === g.id;
                  return (
                    <button
                      key={g.id}
                      type="button"
                      role="radio"
                      aria-checked={chosen}
                      aria-label={g.name}
                      disabled={g.stock === 0}
                      onClick={() => setGoodsId(g.id)}
                      className="tile flex h-[74px] flex-col items-center justify-center"
                    >
                      <span className="font-display text-[20px] font-semibold tnum">
                        {value !== null ? `$${value}` : g.name}
                      </span>
                      <span className="mt-0.5 text-caption text-ink/55 tnum">
                        {g.stock === 0 ? "售罄" : cny(g.price)}
                      </span>
                    </button>
                  );
                })}
              </div>
            ) : (
              <p className="py-6 text-center text-body text-ink/60">
                暂时没有可买的额度，晚点再来看看。
              </p>
            )
          ) : catalog.isError ? (
            <p className="py-6 text-center text-body text-ink/60">
              金额列表没加载出来。
              <button
                type="button"
                className="ml-1 text-cyan-ink underline-offset-4 hover:underline"
                onClick={() => void catalog.refetch()}
              >
                重试
              </button>
            </p>
          ) : (
            <div className="grid grid-cols-5 gap-2.5">
              {Array.from({ length: 10 }, (_, i) => (
                <div key={i} className="skeleton h-[74px] rounded-tile" />
              ))}
            </div>
          )}

          {channels.length > 1 && (
            <div
              role="radiogroup"
              aria-label="付款方式"
              className="mt-4 flex gap-2"
            >
              {channels.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  role="radio"
                  aria-checked={channel === c.id}
                  onClick={() => setChannelId(c.id)}
                  className="tile h-9 px-4 text-small font-semibold"
                >
                  {c.name}
                </button>
              ))}
            </div>
          )}

          <div className="mt-5 flex items-center justify-between gap-4 rounded-[18px] bg-ink/[0.045] px-4 py-3">
            <p className="text-body text-ink/70" aria-live="polite">
              {!goodsId ? (
                "选一个金额"
              ) : quote.isError ? (
                <span className="text-danger">{String(quote.error)}</span>
              ) : !quote.data ? (
                "正在计算应付金额…"
              ) : (
                <>
                  应付{" "}
                  <strong className="font-display text-[22px] font-semibold text-ink tnum">
                    {cny(quote.data.total_amount)}
                  </strong>
                </>
              )}
            </p>
            <button
              type="button"
              className="btn btn-primary"
              disabled={action.busy || !quote.data || !goodsId}
              onClick={() => void pay()}
            >
              {action.busy && <Loader2 size={15} className="animate-spin" />}
              {channels.find((c) => c.id === channel)?.name ?? "支付宝"}付款
            </button>
          </div>

          {pending.length > 0 && (
            <div className="mt-5">
              <p className="mb-1.5 text-caption font-semibold text-ink/55">
                没处理完的订单
              </p>
              {pending.map((o) => (
                <div
                  key={o.id}
                  className="row -mx-2 flex items-center gap-3 px-2 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-small font-semibold">
                      {o.name} · {cny(o.amount)}
                    </p>
                    <p className="truncate text-caption text-ink/55">
                      {o.message}
                    </p>
                  </div>
                  <span
                    className={cn(
                      "chip",
                      o.status === "awaiting_payment" ? "chip-warn" : "chip-ok",
                    )}
                  >
                    {statusText[o.status] ?? o.status}
                  </span>
                  {o.status === "awaiting_payment" && (
                    <>
                      <button
                        type="button"
                        className="btn btn-glass btn-sm"
                        disabled={action.busy}
                        onClick={() => void resume(o)}
                      >
                        继续付款
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        disabled={action.busy}
                        onClick={() => void cancel(o)}
                      >
                        取消
                      </button>
                    </>
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </Sheet>
  );
}
