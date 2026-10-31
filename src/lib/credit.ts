import type { Credit } from "./api";

/** Time-limited credit that lapses on one Beijing day: 0 today, 1 tomorrow. */
export type Lapse = { amount: number; at: string; days: number };

/** The same rounding as `usd`, in whole cents. */
const cents = (n: number) => Math.round(Number(n.toFixed(2)) * 100);

const ymd = (t: number) =>
  new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Shanghai" }).format(t);

/** Beijing calendar days from now until `at`: 0 today, 1 tomorrow. */
export function daysUntil(at: string, now = Date.now()) {
  return Math.round(
    (Date.parse(ymd(Date.parse(at))) - Date.parse(ymd(now))) / 864e5,
  );
}

/**
 * The balance split into credit that stays and credit that lapses, by the
 * day it lapses, soonest first. Spending takes the soonest-lapsing credit
 * first, and when the balance can't cover every grant, the soonest ones are
 * what is left, so grants count soonest first up to `limited_total`. Amounts
 * are rounded as shown and add up to the balance shown.
 */
export function splitBalance(
  balance: number,
  credit: Extract<Credit, { supported: true }>,
  now = Date.now(),
) {
  const grants = [...credit.limited].sort(
    (a, b) => Date.parse(a.expires_at) - Date.parse(b.expires_at),
  );
  let left = Math.max(0, Math.min(credit.limited_total, balance));
  let walked = 0;
  let shown = 0;
  const days: { cents: number; at: string; days: number }[] = [];
  for (const g of grants) {
    const take = Math.min(g.amount, left);
    if (!(take > 0)) break;
    left -= take;
    walked += take;
    // By running total, so the lines add up to the total.
    const c = cents(walked) - shown;
    shown += c;
    const d = daysUntil(g.expires_at, now);
    const last = days[days.length - 1];
    if (last?.days === d) last.cents += c;
    else days.push({ cents: c, at: g.expires_at, days: d });
  }
  return {
    permanent: (cents(balance) - shown) / 100,
    limited: shown / 100,
    lapses: days
      .filter((l) => l.cents > 0)
      .map((l): Lapse => ({ amount: l.cents / 100, at: l.at, days: l.days })),
  };
}

const clock = new Intl.DateTimeFormat("zh-CN", {
  timeZone: "Asia/Shanghai",
  hourCycle: "h23",
  hour: "2-digit",
  minute: "2-digit",
});

/** "今天 12:19 过期" / "明天 12:19 过期" / "3 天后过期", Beijing time. */
function lapseText({ at, days }: Lapse) {
  if (days > 1) return `${days} 天后过期`;
  return `${days === 1 ? "明天" : "今天"} ${clock.format(Date.parse(at))} 过期`;
}

/**
 * At most three lines of what lapses when; later days share the last line,
 * "4–7 天后陆续过期".
 */
export function lapseLines(lapses: Lapse[]) {
  const lines = lapses.map((l) => ({
    key: l.days,
    amount: l.amount,
    text: lapseText(l),
  }));
  if (lines.length <= 3) return lines;
  const rest = lapses.slice(2);
  return [
    ...lines.slice(0, 2),
    {
      key: rest[0].days,
      amount: rest.reduce((sum, l) => sum + cents(l.amount), 0) / 100,
      text: `${rest[0].days}–${rest[rest.length - 1].days} 天后陆续过期`,
    },
  ];
}
