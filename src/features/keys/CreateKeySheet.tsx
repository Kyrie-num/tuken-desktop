import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Copy, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Sheet } from "@/components/Sheet";
import { tuken, type Group, type Key } from "@/lib/api";
import { platformHint, platformName } from "@/lib/format";
import { qk, useGroups, useModels } from "@/lib/queries";
import { useAction, useSession, useUi } from "@/app/context";

/** The models a group serves, or what is known about its platform. */
export function ModelList({
  group,
  limit = 18,
}: {
  group: Group;
  limit?: number;
}) {
  const { uid } = useSession();
  const models = useModels(uid, group.id);
  if (models.isPending && !models.data) {
    return (
      <div className="flex flex-wrap gap-1.5">
        {[64, 88, 72].map((w) => (
          <span key={w} className="skeleton h-[22px]" style={{ width: w }} />
        ))}
      </div>
    );
  }
  const list = models.data?.models ?? [];
  if (!list.length) {
    return (
      <p className="text-caption text-ink/55">
        {models.data?.source === "balance"
          ? "充值后就能看到这个分组的模型。"
          : platformHint(group.platform) || "这个分组的模型列表暂时看不到。"}
      </p>
    );
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {list.slice(0, limit).map((m) => (
        <span key={m.id} className="chip selectable font-medium" title={m.id}>
          {m.name}
        </span>
      ))}
      {list.length > limit && (
        <span className="chip chip-off">另有 {list.length - limit} 个</span>
      )}
    </div>
  );
}

export function CreateKeySheet() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { createKey, closeCreateKey } = useUi();
  const groups = useGroups(uid);
  const [name, setName] = useState("");
  const [groupId, setGroupId] = useState<number | null>(null);
  const action = useAction();

  useEffect(() => {
    if (createKey.open) {
      setName("");
      setGroupId(createKey.groupId ?? null);
    }
  }, [createKey.open, createKey.groupId]);

  const list = groups.data ?? [];
  const chosen = list.find((g) => g.id === groupId) ?? list[0];

  const create = () =>
    action.run(async () => {
      if (!chosen) return;
      const k = await tuken<Key>("key_create", {
        name: name.trim() || `${platformName(chosen.platform)} 密钥`,
        group_id: chosen.id,
        as_user: uid,
      });
      void client.invalidateQueries({ queryKey: qk.keys(uid!) });
      closeCreateKey();
      toast.success(`已创建「${k.name}」`, {
        action: {
          label: (
            <span className="flex items-center gap-1">
              <Copy size={13} />
              复制密钥
            </span>
          ),
          onClick: () =>
            void tuken<{ key: string }>("key_copy", { id: k.id, as_user: uid })
              .then((r) => navigator.clipboard.writeText(r.key))
              .then(() => toast.success("密钥已复制，请妥善保管"))
              .catch((e) => toast.error(String(e))),
        },
      });
    });

  return (
    <Sheet
      open={createKey.open}
      onOpenChange={(open) => !open && closeCreateKey()}
      title="新建密钥"
      description="密钥属于一个分组，分组决定它能用哪些模型、按什么倍率计费。"
      width={600}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <label className="block text-small font-semibold" htmlFor="key-name">
          名称
        </label>
        <input
          id="key-name"
          className="field mt-1.5"
          placeholder={
            chosen ? `${platformName(chosen.platform)} 密钥` : "我的密钥"
          }
          value={name}
          maxLength={50}
          onChange={(e) => setName(e.target.value)}
        />

        <p className="mt-5 text-small font-semibold">分组</p>
        <div
          role="radiogroup"
          aria-label="分组"
          className="mt-1.5 grid grid-cols-2 gap-2.5"
        >
          {groups.data
            ? list.map((g) => (
                <button
                  key={g.id}
                  type="button"
                  role="radio"
                  aria-checked={chosen?.id === g.id}
                  onClick={() => setGroupId(g.id)}
                  className="tile px-4 py-3 text-left"
                >
                  <span className="flex items-center justify-between gap-2">
                    <span className="truncate text-strong font-semibold">
                      {g.name}
                    </span>
                    <span className="chip shrink-0">
                      {platformName(g.platform)}
                    </span>
                  </span>
                  <span className="mt-1 block truncate text-caption text-ink/55">
                    {g.rate_multiplier !== undefined &&
                      `倍率 ${g.rate_multiplier}×`}
                    {g.description ? ` · ${g.description}` : ""}
                  </span>
                </button>
              ))
            : [0, 1].map((i) => (
                <div key={i} className="skeleton h-[62px] rounded-tile" />
              ))}
        </div>
        {groups.data && list.length === 0 && (
          <p className="mt-2 text-caption text-ink/60">
            账户里没有可用的分组，请联系客服。
          </p>
        )}

        {chosen && (
          <div className="mt-5 rounded-[18px] bg-ink/[0.045] px-4 py-3.5">
            <p className="mb-2 text-small font-semibold">
              「{chosen.name}」可以用的模型
            </p>
            <ModelList group={chosen} />
          </div>
        )}

        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            className="btn btn-glass"
            onClick={closeCreateKey}
          >
            取消
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={action.busy || !chosen}
          >
            {action.busy && <Loader2 size={15} className="animate-spin" />}
            创建
          </button>
        </div>
      </form>
    </Sheet>
  );
}
