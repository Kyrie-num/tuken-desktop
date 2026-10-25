import { Plus } from "lucide-react";
import { platformName } from "@/lib/format";
import { useGroups } from "@/lib/queries";
import { ModelList } from "@/features/keys/CreateKeySheet";
import { useSession, useUi } from "@/app/context";

/** Which models the account can use, group by group. */
export function Models() {
  const { uid } = useSession();
  const groups = useGroups(uid);
  const { openCreateKey } = useUi();
  return (
    <div className="mx-auto w-full max-w-[1000px] px-6 pb-8">
      <div className="mb-4">
        <h1 className="font-display text-title font-semibold">可用模型</h1>
        <p className="mt-1 text-small text-ink/60">
          每个分组对应一类模型。新建密钥时选好分组，密钥就能用这个分组里的模型。
        </p>
      </div>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {groups.data
          ? groups.data.map((g) => (
              <section key={g.id} className="glass flex flex-col p-5">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <h2 className="truncate text-section font-semibold">
                        {g.name}
                      </h2>
                      <span className="chip">{platformName(g.platform)}</span>
                    </div>
                    <p className="mt-0.5 text-caption text-ink/55">
                      {g.rate_multiplier !== undefined &&
                        `计费倍率 ${g.rate_multiplier}×`}
                      {g.description ? ` · ${g.description}` : ""}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-glass btn-sm shrink-0"
                    onClick={() => openCreateKey(g.id)}
                  >
                    <Plus size={14} />
                    建密钥
                  </button>
                </div>
                <div className="mt-4">
                  <ModelList group={g} limit={30} />
                </div>
              </section>
            ))
          : [0, 1].map((i) => (
              <div key={i} className="skeleton h-40 rounded-glass" />
            ))}
      </div>
      {groups.data && groups.data.length === 0 && (
        <p className="text-body text-ink/60">
          账户里还没有可用的分组，请联系客服。
        </p>
      )}
    </div>
  );
}
