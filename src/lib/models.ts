/**
 * Claude models as people choose them: a short name, what each is good for,
 * and an order by overall strength (strongest first).
 */
const KNOWN: { id: string; name: string; note: string }[] = [
  { id: "claude-opus-5-5", name: "Opus 5.5", note: "最强旗舰" },
  { id: "claude-fable-5-1", name: "Fable 5.1", note: "顶级模型，花费额度更多" },
  { id: "claude-sonnet-5-5", name: "Sonnet 5.5", note: "均衡好用，性价比高" },
  {
    id: "claude-fable-5",
    name: "Fable 5",
    note: "上一代顶级模型，花费额度更多",
  },
  { id: "claude-opus-5", name: "Opus 5", note: "上一代旗舰" },
  { id: "claude-sonnet-5", name: "Sonnet 5", note: "上一代均衡款" },
  { id: "claude-opus-4-7", name: "Opus 4.7", note: "旧版旗舰" },
  { id: "claude-opus-4-6", name: "Opus 4.6", note: "旧版旗舰" },
  { id: "claude-haiku-5-5", name: "Haiku 5.5", note: "速度最快，最省额度" },
  { id: "claude-sonnet-4-6", name: "Sonnet 4.6", note: "旧版均衡款" },
  { id: "claude-sonnet-4-5", name: "Sonnet 4.5", note: "旧版均衡款" },
  { id: "claude-haiku-4-5", name: "Haiku 4.5", note: "旧版，速度快" },
];

const FAMILY: Record<string, number> = {
  opus: 4,
  fable: 4,
  sonnet: 3,
  haiku: 2,
};

/** The id without `anthropic/` and a trailing release date. */
const base = (id: string) =>
  id
    .toLowerCase()
    .replace(/^anthropic\//, "")
    .replace(/-\d{8}$/, "");

/** `claude-opus-4-6` → family `opus`, version `[4, 6]`. */
function parts(id: string) {
  const [, family = "", ...rest] = base(id).split("-");
  return { family, version: rest.filter((p) => /^\d+$/.test(p)).map(Number) };
}

/** What the model list shows for an id. */
export function modelInfo(id: string) {
  const known = KNOWN.find((k) => k.id === base(id));
  if (known) return { name: known.name, note: known.note };
  const { family, version } = parts(id);
  if (!FAMILY[family]) return { name: id, note: "" };
  const name = `${family[0].toUpperCase()}${family.slice(1)} ${version.join(".")}`;
  return { name: name.trim(), note: "" };
}

/** Strongest first: the known order, then the rest by family and version. */
export function byStrength(ids: string[]) {
  const rank = (id: string) => {
    const i = KNOWN.findIndex((k) => k.id === base(id));
    return i < 0 ? KNOWN.length : i;
  };
  const newer = (a: number[], b: number[]) => {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const d = (b[i] ?? -1) - (a[i] ?? -1);
      if (d) return d;
    }
    return 0;
  };
  return [...ids].sort((a, b) => {
    const r = rank(a) - rank(b);
    if (r) return r;
    const pa = parts(a);
    const pb = parts(b);
    const f = (FAMILY[pb.family] ?? 0) - (FAMILY[pa.family] ?? 0);
    return f || newer(pa.version, pb.version) || a.localeCompare(b);
  });
}
