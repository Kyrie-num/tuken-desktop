/** Dollars as the server bills them: cents, or finer for small amounts. */
export function usd(value: unknown, fine = false) {
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  const digits = fine && Math.abs(n) < 1 && n !== 0 ? 4 : 2;
  return `$${n.toFixed(digits)}`;
}

export function cny(value: unknown) {
  const n = Number(value);
  return Number.isFinite(n) ? `¥${n.toFixed(2)}` : "—";
}

export function tokens(value: unknown) {
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(2)}M`;
  if (n >= 1e4) return `${(n / 1e3).toFixed(1)}K`;
  return n.toLocaleString("zh-CN");
}

export function count(value: unknown) {
  const n = Number(value);
  return Number.isFinite(n) ? n.toLocaleString("zh-CN") : "—";
}

const beijing = (opts: Intl.DateTimeFormatOptions) =>
  new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour12: false,
    ...opts,
  });

/** "今天 14:05" / "10月3日 09:12" / "2025年12月1日" in Beijing time. */
export function when(value: unknown) {
  if (!value) return "—";
  const d = new Date(String(value));
  if (Number.isNaN(d.getTime())) return "—";
  const day = (x: Date) => beijing({ dateStyle: "short" }).format(x);
  const time = beijing({ hour: "2-digit", minute: "2-digit" }).format(d);
  const now = new Date();
  if (day(d) === day(now)) return `今天 ${time}`;
  if (day(d) === day(new Date(now.getTime() - 864e5))) return `昨天 ${time}`;
  if (
    beijing({ year: "numeric" }).format(d) ===
    beijing({ year: "numeric" }).format(now)
  )
    return `${beijing({ month: "numeric", day: "numeric" }).format(d)} ${time}`;
  return beijing({ year: "numeric", month: "numeric", day: "numeric" }).format(
    d,
  );
}

/** YYYY-MM-DD in Beijing time, `days` ago. */
export function beijingDate(days = 0) {
  return new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Shanghai" }).format(
    new Date(Date.now() - days * 864e5),
  );
}

const platforms: Record<string, string> = {
  anthropic: "Claude",
  openai: "GPT",
  gemini: "Gemini",
  antigravity: "Antigravity",
  grok: "Grok",
  kimi: "Kimi",
  zhipu: "智谱",
  deepseek: "DeepSeek",
  minimax: "MiniMax",
  opencode_go: "OpenCode",
  composite: "混合",
};

export function platformName(platform?: string | null) {
  if (!platform) return "通用";
  return platforms[platform.toLowerCase()] ?? platform;
}

/** What a platform's models are, for groups whose model list isn't known yet. */
export function platformHint(platform?: string | null) {
  switch (platform?.toLowerCase()) {
    case "anthropic":
      return "Claude 系列（Opus、Sonnet、Haiku），可接入 Claude Desktop 和 Claude Code";
    case "openai":
      return "GPT 系列，适合 Codex 等 OpenAI 兼容工具";
    case "gemini":
      return "Gemini 系列";
    default:
      return "";
  }
}

/** Dollar credit in a shop item name such as「10刀额度」. */
export function creditOf(name: string) {
  const m = /(\d+(?:\.\d+)?)\s*刀/.exec(name);
  return m ? Number(m[1]) : null;
}

export function initial(text?: string | null) {
  const t = (text ?? "").trim();
  return t ? Array.from(t)[0].toUpperCase() : "兔";
}

/** "2026年9月1日" in Beijing time. */
export function day(value: unknown) {
  if (!value) return "—";
  const d = new Date(String(value));
  return Number.isNaN(d.getTime())
    ? "—"
    : beijing({ year: "numeric", month: "long", day: "numeric" }).format(d);
}
