import { describe, expect, it } from "vitest";
import { daysUntil, lapseLines, splitBalance } from "@/lib/credit";

/** 2026-10-31 12:00 in Beijing. */
const NOW = Date.parse("2026-10-31T04:00:00Z");
const grant = (amount: number, expires_at: string) => ({
  amount,
  expires_at,
  source: "checkin",
});
const credit = (limited_total: number, limited: ReturnType<typeof grant>[]) =>
  ({
    supported: true,
    limited_total,
    limited,
    first_recharge: { enabled: false, eligible: false, percent: 0 },
  }) as const;

describe("time-limited credit on the balance card", () => {
  it("splits the balance into parts that add up to it, in cents", () => {
    const s = splitBalance(
      28.5,
      credit(0.25, [
        grant(0.125, "2026-11-05T04:00:00Z"),
        grant(0.125, "2026-11-03T04:00:00Z"),
      ]),
      NOW,
    );
    expect(s.permanent).toBe(28.25);
    expect(s.limited).toBe(0.25);
    expect(s.lapses.map((l) => [l.amount, l.days])).toEqual([
      [0.13, 3],
      [0.12, 5],
    ]);
  });

  it("keeps the soonest grants when the balance can't cover them all", () => {
    const s = splitBalance(
      3,
      credit(3, [
        grant(5, "2026-11-01T04:00:00Z"),
        grant(5, "2026-11-06T04:00:00Z"),
      ]),
      NOW,
    );
    expect(s).toMatchObject({ permanent: 0, limited: 3 });
    expect(s.lapses.map((l) => [l.amount, l.days])).toEqual([[3, 1]]);
  });

  it("counts days by the Beijing calendar", () => {
    const late = Date.parse("2026-10-31T15:59:00Z"); // 23:59
    expect(daysUntil("2026-10-31T15:59:30Z", late)).toBe(0);
    expect(daysUntil("2026-10-31T16:01:00Z", late)).toBe(1);
    expect(daysUntil("2026-11-07T04:19:34Z", NOW)).toBe(7);
  });

  it("says when each part lapses, in three lines at most", () => {
    const s = splitBalance(
      28.5,
      credit(0.5, [
        grant(0.1, "2026-10-31T10:30:00Z"),
        grant(0.1, "2026-10-31T16:05:00Z"),
        grant(0.1, "2026-11-03T04:00:00Z"),
        grant(0.1, "2026-11-05T04:00:00Z"),
        grant(0.1, "2026-11-07T04:00:00Z"),
      ]),
      NOW,
    );
    expect(lapseLines(s.lapses).map((l) => [l.amount, l.text])).toEqual([
      [0.1, "今天 18:30 过期"],
      [0.1, "明天 00:05 过期"],
      [0.3, "3–7 天后陆续过期"],
    ]);
    expect(
      lapseLines(
        splitBalance(
          1,
          credit(0.12, [grant(0.12, "2026-11-06T02:00:00Z")]),
          NOW,
        ).lapses,
      ).map((l) => l.text),
    ).toEqual(["6 天后过期"]);
  });
});
