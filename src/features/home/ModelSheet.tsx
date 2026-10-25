import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Segmented } from "@/components/Segmented";
import { Sheet } from "@/components/Sheet";
import { desktop, EFFORTS, type Effort } from "@/lib/api";
import { cn } from "@/lib/cn";
import { byStrength, modelInfo } from "@/lib/models";
import { qk, useDesktop, useKeys, useModels } from "@/lib/queries";
import { message, useSession, useUi } from "@/app/context";

export const effortNames: Record<Effort, string> = {
  low: "低",
  medium: "中",
  high: "高",
  xhigh: "超高",
  max: "最高",
};

/** The effort 兔啃 starts with and recommends: the deepest thinking. */
export const recommendedEffort: Effort = "max";

/** Model ids Claude Desktop lists on a gateway; the native side checks too. */
export const claudeModel = (id: string) =>
  id.length <= 100 && /^(anthropic\/)?claude-[a-z0-9.-]+$/i.test(id);

/** Opus 5.5 when the group has it, else the newest Opus, else the newest
 * Sonnet, else the first one. */
export function recommended(ids: string[]) {
  const opus55 = ids.find((id) =>
    /^(anthropic\/)?claude-opus-5-5(?!\d)/i.test(id),
  );
  if (opus55) return opus55;
  const version = (id: string) => (id.match(/\d+/g) ?? []).map(Number);
  const newer = (a: string, b: string) => {
    const x = version(a);
    const y = version(b);
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      const d = (x[i] ?? -1) - (y[i] ?? -1);
      if (d) return d > 0;
    }
    return false;
  };
  for (const family of ["opus", "sonnet"]) {
    const found = ids.filter((id) => id.toLowerCase().includes(family));
    if (found.length)
      return found.reduce((best, id) => (newer(id, best) ? id : best));
  }
  return ids[0] ?? null;
}

/**
 * What Claude Desktop starts new chats with: the default model, from the
 * models the connected key's group offers, and how hard it thinks.
 */
export function ModelSheet() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { modelsOpen, setModelsOpen, setRestartOpen, setRechargeOpen } =
    useUi();
  const { data: status } = useDesktop();
  const keys = useKeys(uid);
  const key = keys.data?.find((k) => k.in_desktop);
  const fetched = useModels(uid, modelsOpen ? (key?.group_id ?? null) : null);
  // Strongest first, the order Claude Desktop's menu keeps too.
  const ids = useMemo(
    () =>
      byStrength(
        (fetched.data?.models ?? []).map((m) => m.id).filter(claudeModel),
      ),
    [fetched.data],
  );
  const [model, setModel] = useState<string | null>(null);
  const [effort, setEffort] = useState<Effort>(recommendedEffort);
  const [saving, setSaving] = useState(false);

  // Each time it opens, it starts from what Claude Desktop has now.
  const saved = status?.models;
  useEffect(() => {
    if (!modelsOpen) return;
    setModel(saved?.models[0] ?? null);
    setEffort(saved?.effort ?? recommendedEffort);
    // Only on opening: a refresh while choosing keeps the choice.
  }, [modelsOpen]);

  const suggestion = recommended(ids);
  const chosen = model && ids.includes(model) ? model : suggestion;
  const running = !!status?.running;

  async function save() {
    if (!chosen) return;
    setSaving(true);
    try {
      await desktop.setModels(
        [chosen, ...ids.filter((m) => m !== chosen)],
        effort,
      );
      const next = await desktop.status();
      client.setQueryData(qk.desktop, next);
      setModelsOpen(false);
      if (!next.running) {
        await desktop.open(false);
        toast.success("正在打开 Claude Desktop");
        setTimeout(
          () => void client.invalidateQueries({ queryKey: qk.desktop }),
          2500,
        );
      } else if (next.can_restart) {
        setRestartOpen(true);
      } else {
        toast.success("已保存，手动重启 Claude Desktop 后生效");
      }
    } catch (e) {
      toast.error(message(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Sheet
      open={modelsOpen}
      onOpenChange={setModelsOpen}
      title="选择模型"
      description="Claude Desktop 的新对话默认用这个模型和推理程度"
      width={500}
    >
      <div className="mb-2 flex items-center justify-between">
        <p className="text-small font-semibold">默认模型</p>
        <button
          type="button"
          className="btn btn-ghost btn-sm -mr-2"
          disabled={!key || fetched.isFetching}
          onClick={() => void fetched.refetch()}
        >
          <RefreshCw
            size={14}
            className={cn(fetched.isFetching && "animate-spin")}
          />
          获取模型
        </button>
      </div>

      {!key && !keys.isFetching ? (
        <p className="rounded-[16px] bg-ink/[0.045] px-4 py-3 text-small text-ink/70">
          Claude Desktop
          用的不是这个账户的密钥。先在首页用这个账户「一键接入」，再来选模型。
        </p>
      ) : !key || fetched.isPending ? (
        <div className="field-group">
          {[0, 1, 2].map((i) => (
            <div key={i} className="px-4 py-3">
              <div className="skeleton h-9" />
            </div>
          ))}
        </div>
      ) : fetched.data?.source === "balance" ? (
        <div className="flex items-center gap-3 rounded-[16px] bg-orange/[0.08] px-4 py-3">
          <p className="min-w-0 flex-1 text-small text-ink/75">
            余额为 0，充值后就能选模型，到账后会自动显示。
          </p>
          <button
            type="button"
            className="btn btn-primary btn-sm shrink-0"
            onClick={() => {
              setModelsOpen(false);
              setRechargeOpen(true);
            }}
          >
            去充值
          </button>
        </div>
      ) : fetched.isError ? (
        <p className="rounded-[16px] bg-ink/[0.045] px-4 py-3 text-small text-ink/70">
          模型列表没取到：{message(fetched.error)}
        </p>
      ) : !ids.length ? (
        <p className="rounded-[16px] bg-ink/[0.045] px-4 py-3 text-small text-ink/70">
          这个分组没有能在 Claude Desktop 里用的模型，换一个分组接入试试。
        </p>
      ) : (
        <div
          role="radiogroup"
          aria-label="默认模型"
          className="field-group max-h-[264px] overflow-y-auto"
        >
          {ids.map((id) => {
            const { name, note } = modelInfo(id);
            const on = id === chosen;
            return (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={on}
                title={id}
                onClick={() => setModel(id)}
                className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-glass/40"
              >
                <span
                  aria-hidden="true"
                  className={cn(
                    "flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                    on ? "border-orange" : "border-ink/25",
                  )}
                >
                  {on && <span className="h-2 w-2 rounded-full bg-orange" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-strong font-semibold">
                    {name}
                  </span>
                  {note && (
                    <span className="block truncate text-caption text-ink/55">
                      {note}
                    </span>
                  )}
                </span>
                {id === suggestion && (
                  <span className="chip chip-ok">推荐</span>
                )}
              </button>
            );
          })}
        </div>
      )}

      <p className="mb-2 mt-5 text-small font-semibold">推理程度</p>
      <Segmented<Effort>
        stretch
        id="effort"
        label="推理程度"
        value={effort}
        options={EFFORTS.map((e) => [e, effortNames[e]] as const)}
        badge={[recommendedEffort, "推荐"]}
        onChange={setEffort}
      />
      <p className="mt-2 text-caption text-ink/55">
        越高想得越深，回答更慢，也更费额度。
      </p>

      <div className="mt-6 flex justify-end">
        <button
          type="button"
          className="btn btn-primary"
          disabled={!chosen || saving}
          onClick={() => void save()}
        >
          {saving && <Loader2 size={15} className="animate-spin" />}
          {running ? "保存" : "打开 Claude Desktop"}
        </button>
      </div>
    </Sheet>
  );
}
