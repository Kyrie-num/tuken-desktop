import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  MoreHorizontal,
  Plus,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/cn";
import { Mascot } from "@/components/Mascot";
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
} from "@/components/Menu";
import { Sheet } from "@/components/Sheet";
import { tuken, type Key } from "@/lib/api";
import { platformName, when } from "@/lib/format";
import { qk, useKeys } from "@/lib/queries";
import { useAction, useConnect, useSession, useUi } from "@/app/context";

function Platform({ platform }: { platform?: string }) {
  const name = platformName(platform);
  return (
    <span
      aria-hidden="true"
      className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[12px] bg-ink/[0.07] text-small font-bold text-ink/75"
    >
      {Array.from(name)[0]}
    </span>
  );
}

export function KeysPanel() {
  const client = useQueryClient();
  const { uid } = useSession();
  const keys = useKeys(uid);
  const { openCreateKey } = useUi();
  const { connect } = useConnect();
  const action = useAction();
  const [renaming, setRenaming] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [deleting, setDeleting] = useState<Key | null>(null);
  // Whole secrets on show: kept here only, never in the saved page cache.
  const [shown, setShown] = useState<Record<number, string>>({});

  const toggle = (k: Key) => {
    if (shown[k.id]) {
      setShown(({ [k.id]: _, ...rest }) => rest);
      return;
    }
    void action.run(async () => {
      const r = await tuken<{ key: string }>("key_copy", {
        id: k.id,
        as_user: uid,
      });
      setShown((s) => ({ ...s, [k.id]: r.key }));
    });
  };

  // Changes show at once; the list is re-read behind them.
  const patch = (id: number, change: Partial<Key> | null) =>
    client.setQueryData<Key[]>(qk.keys(uid!), (old) =>
      change === null
        ? old?.filter((k) => k.id !== id)
        : old?.map((k) => (k.id === id ? { ...k, ...change } : k)),
    );
  const reread = () => client.invalidateQueries({ queryKey: qk.keys(uid!) });

  const copy = (k: Key) =>
    action.run(async () => {
      const r = await tuken<{ key: string }>("key_copy", {
        id: k.id,
        as_user: uid,
      });
      await navigator.clipboard.writeText(r.key);
      toast.success("密钥已复制，请妥善保管");
    });

  const update = (k: Key, body: { name?: string; status?: string }) =>
    action.run(async () => {
      patch(k.id, body);
      try {
        await tuken("key_update", { id: k.id, ...body, as_user: uid });
      } finally {
        void reread();
      }
    });

  const remove = (k: Key) =>
    action.run(async () => {
      patch(k.id, null);
      try {
        await tuken("key_delete", { id: k.id, as_user: uid });
        toast.success(`已删除「${k.name}」`);
      } finally {
        void reread();
      }
    });

  return (
    <section
      data-tour="keys"
      className="glass flex flex-col p-6 lg:h-full lg:min-h-0"
    >
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-section font-semibold">我的密钥</h2>
          <p className="mt-0.5 text-caption text-ink/60">
            用在 Claude Desktop、Claude Code 或其他支持 API 的工具里
          </p>
        </div>
        <button
          type="button"
          className="btn btn-glass btn-sm"
          onClick={() => openCreateKey()}
        >
          <Plus size={15} />
          新建密钥
        </button>
      </div>

      {/* Many keys scroll here, not the whole page. */}
      <div className="-mx-2 mt-3 max-h-[50vh] space-y-0.5 overflow-y-auto px-2 lg:max-h-none lg:min-h-0 lg:flex-1">
        {!keys.data ? (
          [0, 1].map((i) => <div key={i} className="skeleton my-2 h-12" />)
        ) : keys.data.length === 0 ? (
          <div className="flex items-center gap-4 py-5">
            <Mascot mood="flat" size={64} />
            <div>
              <p className="text-strong font-semibold">还没有密钥</p>
              <p className="text-caption text-ink/60">
                接入 Claude Desktop 时会自动建一个，也可以自己新建。
              </p>
            </div>
          </div>
        ) : (
          keys.data.map((k) => {
            const active = k.status === "active";
            return (
              <div
                key={k.id}
                className="row -mx-2 flex items-center gap-3 px-2 py-2.5"
              >
                <Platform platform={k.group?.platform} />
                <div className="min-w-0 flex-1">
                  {renaming === k.id ? (
                    <form
                      className="flex gap-2"
                      onSubmit={(e) => {
                        e.preventDefault();
                        const next = name.trim();
                        setRenaming(null);
                        if (next && next !== k.name)
                          void update(k, { name: next });
                      }}
                    >
                      <input
                        aria-label="新的密钥名称"
                        className="field h-8"
                        value={name}
                        maxLength={50}
                        autoFocus
                        onChange={(e) => setName(e.target.value)}
                        onBlur={() => setRenaming(null)}
                        onKeyDown={(e) =>
                          e.key === "Escape" && setRenaming(null)
                        }
                      />
                    </form>
                  ) : (
                    <div className="flex min-w-0 items-center gap-2">
                      <p className="truncate text-strong font-semibold">
                        {k.name}
                      </p>
                      {k.in_desktop && (
                        <span className="chip chip-ok">
                          Claude Desktop 在用
                        </span>
                      )}
                      {k.in_cli && (
                        <span className="chip chip-ok">Claude Code 在用</span>
                      )}
                      {!active && <span className="chip chip-off">已停用</span>}
                    </div>
                  )}
                  {(k.masked || shown[k.id]) && (
                    <div className="mt-0.5 flex min-w-0 items-center gap-1">
                      <code
                        className={cn(
                          "min-w-0 truncate font-mono text-[12px] text-ink/70",
                          shown[k.id] && "selectable",
                        )}
                      >
                        {shown[k.id] ?? k.masked}
                      </code>
                      <button
                        type="button"
                        className="icon-btn h-6 w-6 shrink-0"
                        aria-label={
                          shown[k.id]
                            ? `隐藏「${k.name}」的密钥`
                            : `显示「${k.name}」的密钥`
                        }
                        aria-pressed={!!shown[k.id]}
                        disabled={action.busy && !shown[k.id]}
                        onClick={() => toggle(k)}
                      >
                        {shown[k.id] ? <EyeOff size={14} /> : <Eye size={14} />}
                      </button>
                    </div>
                  )}
                  <p className="truncate text-caption text-ink/55">
                    {platformName(k.group?.platform)}
                    {k.group?.name ? ` · ${k.group.name}` : ""}
                    {k.last_used_at
                      ? ` · 最近使用 ${when(k.last_used_at)}`
                      : " · 还没用过"}
                  </p>
                </div>
                <button
                  type="button"
                  className="icon-btn"
                  aria-label={`复制「${k.name}」`}
                  title="复制密钥"
                  disabled={action.busy}
                  onClick={() => void copy(k)}
                >
                  <Copy size={15} />
                </button>
                <MenuRoot>
                  <MenuTrigger asChild>
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label={`「${k.name}」的更多操作`}
                    >
                      <MoreHorizontal size={17} />
                    </button>
                  </MenuTrigger>
                  <MenuContent>
                    <MenuItem
                      disabled={
                        !active ||
                        k.in_desktop ||
                        k.group?.platform !== "anthropic"
                      }
                      onSelect={() => void connect("desktop", { keyId: k.id })}
                    >
                      <KeyRound size={15} className="opacity-70" />
                      用于 Claude Desktop
                    </MenuItem>
                    <MenuItem
                      disabled={
                        !active || k.in_cli || k.group?.platform !== "anthropic"
                      }
                      onSelect={() => void connect("cli", { keyId: k.id })}
                    >
                      <KeyRound size={15} className="opacity-70" />
                      用于 Claude Code
                    </MenuItem>
                    <MenuSeparator />
                    <MenuItem
                      onSelect={() => {
                        setName(k.name);
                        setRenaming(k.id);
                      }}
                    >
                      重命名
                    </MenuItem>
                    <MenuItem
                      onSelect={() =>
                        void update(k, {
                          status: active ? "inactive" : "active",
                        })
                      }
                    >
                      {active ? "停用" : "启用"}
                    </MenuItem>
                    <MenuItem danger onSelect={() => setDeleting(k)}>
                      删除
                    </MenuItem>
                  </MenuContent>
                </MenuRoot>
              </div>
            );
          })
        )}
      </div>

      <Sheet
        open={!!deleting}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`删除密钥「${deleting?.name ?? ""}」？`}
        width={420}
      >
        <p className="text-body text-ink/75">
          {deleting?.in_desktop || deleting?.in_cli
            ? "Claude Desktop 或 Claude Code 正在用这个密钥，删除后会马上不能用，需要重新接入。"
            : "用这个密钥的工具会马上不能访问兔啃。"}
        </p>
        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            className="btn btn-glass"
            onClick={() => setDeleting(null)}
          >
            取消
          </button>
          <button
            type="button"
            className="btn btn-danger"
            onClick={() => {
              const k = deleting;
              setDeleting(null);
              if (k) void remove(k);
            }}
          >
            删除
          </button>
        </div>
      </Sheet>
    </section>
  );
}
