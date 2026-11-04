import { useMemo, useState, type KeyboardEvent } from "react";
import type { Range, UsageTrend } from "@/lib/api";
import { cn } from "@/lib/cn";
import { count, tokens, usd } from "@/lib/format";

type Point = Extract<UsageTrend, { supported: true }>["points"][number];
type Slot = {
  key: string;
  /** Under the column: "10/31", "09:00". */
  axis: string;
  /** In the tooltip: "10月31日 周五", "09:00–10:00". */
  title: string;
  point: Point | undefined;
};

/** The columns' height in px, without the date row under them. */
const PLOT = 132;
const WEEKDAYS = "日一二三四五六";

/** Every day of the range, or every hour of a one-day range, empty ones too. */
function slotsOf(range: Range, points: Point[]): Slot[] {
  const byDate = new Map(points.map((p) => [p.date, p]));
  const hh = (h: number) => `${String(h).padStart(2, "0")}:00`;
  if (range.start_date === range.end_date)
    return Array.from({ length: 24 }, (_, h) => {
      const key = `${range.start_date} ${hh(h)}`;
      return {
        key,
        axis: hh(h),
        title: `${hh(h)}–${hh(h + 1)}`,
        point: byDate.get(key),
      };
    });
  const slots: Slot[] = [];
  const last = Date.parse(`${range.end_date}T00:00:00Z`);
  for (
    let t = Date.parse(`${range.start_date}T00:00:00Z`);
    t <= last;
    t += 864e5
  ) {
    const d = new Date(t);
    const key = d.toISOString().slice(0, 10);
    const [m, day] = [d.getUTCMonth() + 1, d.getUTCDate()];
    slots.push({
      key,
      axis: `${m}/${day}`,
      title: `${m}月${day}日 周${WEEKDAYS[d.getUTCDay()]}`,
      point: byDate.get(key),
    });
  }
  return slots;
}

/** The smallest 1, 2, 2.5 or 5 × 10ⁿ at or above n: the top line's value. */
function niceCeil(n: number) {
  if (!(n > 0)) return 1;
  const p = 10 ** Math.floor(Math.log10(n));
  return [1, 2, 2.5, 5, 10].map((f) => f * p).find((v) => v >= n)!;
}

/** "0", "500K", "1.25M" for the axis. */
function tick(n: number) {
  const [d, unit] =
    n >= 1e9
      ? [1e9, "B"]
      : n >= 1e6
        ? [1e6, "M"]
        : n >= 1e3
          ? [1e3, "K"]
          : [1, ""];
  return `${Number((n / d).toFixed(2))}${unit}`;
}

/** Beside the column, at the top of the plot: right of it on the left half,
 *  left of it on the right half, so it never covers the column itself. */
function Tip({ slot, index, of }: { slot: Slot; index: number; of: number }) {
  const p = slot.point;
  const x = ((index + 0.5) / of) * 100;
  const right = x < 50;
  const cache = p ? p.cache_read_tokens + p.cache_creation_tokens : 0;
  return (
    <div
      role="status"
      className={cn(
        "glass pointer-events-none absolute top-0 z-10 w-max max-w-[280px] rounded-[14px] px-3 py-2",
        !right && "-translate-x-full",
      )}
      style={{ left: `calc(${x}% ${right ? "+" : "-"} 18px)` }}
    >
      {p ? (
        <>
          <p className="text-strong font-semibold tnum">
            {tokens(p.total_tokens)} Token
          </p>
          <p className="text-caption text-ink/55">{slot.title}</p>
          <p className="mt-1 text-caption text-ink/70 tnum">
            输入 {tokens(p.input_tokens)} · 输出 {tokens(p.output_tokens)}
            {cache ? ` · 缓存 ${tokens(cache)}` : ""}
          </p>
          <p className="text-caption text-ink/70 tnum">
            {usd(p.actual_cost, true)} · {count(p.requests)} 次请求
          </p>
        </>
      ) : (
        <>
          <p className="text-strong font-semibold">没有用量</p>
          <p className="text-caption text-ink/55">{slot.title}</p>
        </>
      )}
    </div>
  );
}

/**
 * Tokens per day, or per hour of today, as columns. One series, so the title
 * names it and there is no legend. Hovering a column, or the arrow keys once
 * the chart has focus, shows its numbers; a table only screen readers see
 * carries them all.
 */
export function TokenChart({
  range,
  points,
  dim,
}: {
  range: Range;
  points: Point[];
  /** The previous range's chart while this one loads. */
  dim?: boolean;
}) {
  const slots = useMemo(() => slotsOf(range, points), [range, points]);
  const [active, setActive] = useState<number | null>(null);
  const hourly = range.start_date === range.end_date;
  const title = hourly ? "今天每小时用的 Token" : "每天用的 Token";
  const values = slots.map((s) => s.point?.total_tokens ?? 0);
  const most = Math.max(...values);
  const peak = values.indexOf(most);
  const top = niceCeil(most);
  const lines = [top, top / 2, 0];
  // Dates under every few columns, counted back from today; hours every three.
  const step = hourly ? 3 : slots.length <= 8 ? 1 : Math.ceil(slots.length / 7);
  const labelled = (i: number) =>
    (hourly ? i : slots.length - 1 - i) % step === 0;

  const move = (e: KeyboardEvent) => {
    const by: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      Home: -slots.length,
      End: slots.length,
    };
    if (!(e.key in by)) return;
    e.preventDefault();
    setActive((i) =>
      Math.min(
        slots.length - 1,
        Math.max(0, (i ?? slots.length - 1) + by[e.key]),
      ),
    );
  };

  return (
    <section
      className={cn(
        "glass mt-4 px-5 pb-3 pt-4 transition-opacity",
        dim && "opacity-70",
      )}
    >
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-strong font-semibold">{title}</h2>
        {most > 0 && (
          <p className="text-caption text-ink/55">
            最多 {slots[peak].title} ·{" "}
            <span className="tnum">{tokens(most)}</span>
          </p>
        )}
      </div>
      <div className="mt-4 flex">
        <div
          aria-hidden="true"
          className="relative w-12 shrink-0"
          style={{ height: PLOT }}
        >
          {lines.map((v) => (
            <span
              key={v}
              className="absolute right-2.5 translate-y-1/2 text-caption leading-none text-ink/45 tnum"
              style={{ bottom: `${(v / top) * 100}%` }}
            >
              {tick(v)}
            </span>
          ))}
        </div>
        <div className="relative min-w-0 flex-1">
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 top-0"
            style={{ height: PLOT }}
          >
            {lines.map((v) => (
              <div
                key={v}
                className="absolute inset-x-0 border-t border-ink/10"
                style={{ bottom: `${(v / top) * 100}%` }}
              />
            ))}
          </div>
          <div
            role="group"
            aria-label={`${title}，按左右方向键逐个查看`}
            tabIndex={0}
            className="relative flex rounded-md"
            style={{ height: PLOT }}
            onKeyDown={move}
            onFocus={() => setActive((i) => i ?? slots.length - 1)}
            onBlur={() => setActive(null)}
            onPointerLeave={() => setActive(null)}
          >
            {slots.map((s, i) => (
              <div
                key={s.key}
                data-slot={s.key}
                className="flex h-full min-w-0 flex-1 items-end justify-center px-px"
                onPointerEnter={() => setActive(i)}
              >
                <div
                  className={cn(
                    "w-full max-w-[24px] rounded-t-[4px] bg-chart transition-opacity duration-150",
                    active !== null && active !== i && "opacity-40",
                  )}
                  style={{
                    height: values[i]
                      ? `max(2px, ${(values[i] / top) * 100}%)`
                      : 0,
                  }}
                />
              </div>
            ))}
          </div>
          <div aria-hidden="true" className="flex h-6 items-end">
            {slots.map((s, i) => (
              <div key={s.key} className="flex min-w-0 flex-1 justify-center">
                {labelled(i) && (
                  <span className="whitespace-nowrap text-caption leading-none text-ink/45 tnum">
                    {s.axis}
                  </span>
                )}
              </div>
            ))}
          </div>
          {active !== null && (
            <Tip slot={slots[active]} index={active} of={slots.length} />
          )}
        </div>
      </div>
      <table className="sr-only">
        <caption>{title}</caption>
        <thead>
          <tr>
            <th>时间</th>
            <th>Token</th>
            <th>花费</th>
            <th>请求次数</th>
          </tr>
        </thead>
        <tbody>
          {slots.map((s) => (
            <tr key={s.key}>
              <td>{s.title}</td>
              <td>{tokens(s.point?.total_tokens ?? 0)}</td>
              <td>{usd(s.point?.actual_cost ?? 0, true)}</td>
              <td>{count(s.point?.requests ?? 0)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
