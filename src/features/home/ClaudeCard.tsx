import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronRight,
  Download,
  Loader2,
  MoreHorizontal,
  Plug,
  Terminal,
} from "lucide-react";
import { toast } from "sonner";
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from "@/components/Menu";
import { desktop, type DesktopStatus } from "@/lib/api";
import { InstallProgress } from "@/components/InstallProgress";
import { cn } from "@/lib/cn";
import { installText, useInstallStep } from "@/lib/install";
import { isMac } from "@/lib/platform";
import { qk, useDesktop, useGroups, useKeys } from "@/lib/queries";
import { useAction, useConnect, useSession, useUi } from "@/app/context";
import { modelInfo } from "@/lib/models";
import { effortNames } from "./ModelSheet";

function Step({
  n,
  done,
  title,
  detail,
  progress,
  children,
}: {
  n: number;
  done: boolean;
  title: string;
  detail: string;
  progress?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <li className="flex flex-wrap items-center gap-x-3.5 gap-y-2 py-3">
      <span
        aria-hidden="true"
        className={cn(
          "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-caption font-bold",
          done ? "bg-cyan/20 text-cyan-ink" : "bg-ink/[0.07] text-ink/60",
        )}
      >
        {done ? <Check size={15} strokeWidth={3} /> : n}
      </span>
      <div className="min-w-[9rem] flex-1">
        <p className="text-strong font-semibold">{title}</p>
        <p className="text-caption text-ink/60">{detail}</p>
        {progress && <div className="mt-2">{progress}</div>}
      </div>
      {children && (
        <div className="ml-auto flex items-center gap-2">{children}</div>
      )}
    </li>
  );
}

/** Getting Claude Desktop (and Claude Code) to run on 兔啃. */
export function ClaudeCard() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { data: status } = useDesktop();
  const groups = useGroups(uid);
  const keys = useKeys(uid);
  const { connect, disconnect, busy } = useConnect();
  const { setModelsOpen } = useUi();
  const install = useAction();
  const [groupId, setGroupId] = useState<number | null>(null);

  const claude = (groups.data ?? []).filter(
    (g) => (g.platform ?? "").toLowerCase() === "anthropic",
  );
  const chosen = claude.find((g) => g.id === groupId)?.id ?? claude[0]?.id;
  const desktopKey = keys.data?.find((k) => k.in_desktop);
  const installing = !!status?.installing || install.busy;
  const installStep = useInstallStep(installing, status?.install_step);
  const progress = installText(installStep);
  const mac = isMac();
  const ready = !!status?.installed && status.code_ready !== false;

  const runInstall = () =>
    install.run(async () => {
      client.setQueryData<DesktopStatus>(qk.desktop, (old) =>
        old ? { ...old, installing: true, install_step: null } : old,
      );
      let r;
      try {
        r = await desktop.install();
      } finally {
        client.setQueryData(qk.desktop, await desktop.status());
      }
      if (r.method === "browser") toast.info("没能自动安装，已打开官网下载页");
      else if (r.method === "dmg" || r.method === "msix")
        toast.success("Claude Desktop 安装好了");
      else if (r.method === "runtime")
        toast.success("Claude Code 运行组件已准备好，可以回去重试了");
    });

  const open = () =>
    void install.run(async () => {
      client.setQueryData<DesktopStatus>(qk.desktop, (old) =>
        old ? { ...old, installing: true, install_step: null } : old,
      );
      try {
        await desktop.open(false);
        setTimeout(
          () => void client.invalidateQueries({ queryKey: qk.desktop }),
          2500,
        );
      } finally {
        client.setQueryData(qk.desktop, await desktop.status());
      }
    });

  return (
    <section data-tour="claude" className="glass flex flex-col p-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-section font-semibold">Claude Desktop</h2>
          <p className="mt-0.5 text-caption text-ink/60">
            {status?.connected ? "正在使用兔啃" : "两步接好，之后打开就能用"}
          </p>
        </div>
        {status?.connected && (
          <MenuRoot>
            <MenuTrigger asChild>
              <button
                type="button"
                className="icon-btn -mr-2"
                aria-label="Claude Desktop 的更多操作"
              >
                <MoreHorizontal size={18} />
              </button>
            </MenuTrigger>
            <MenuContent>
              {!mac && (
                <MenuItem
                  disabled={installing}
                  onSelect={() => void runInstall()}
                >
                  修复 Claude Code 运行组件
                </MenuItem>
              )}
              <MenuItem danger onSelect={() => void disconnect("desktop")}>
                断开兔啃，恢复官方登录
              </MenuItem>
            </MenuContent>
          </MenuRoot>
        )}
      </div>

      {!status ? (
        <div className="mt-5 space-y-3">
          <div className="skeleton h-10" />
          <div className="skeleton h-10" />
        </div>
      ) : !status.supported ? (
        <p className="mt-5 text-body text-ink/70">
          目前只支持 Windows 和 macOS。
        </p>
      ) : status.connected && ready ? (
        <div className="mt-5 flex flex-1 flex-col justify-between gap-5">
          <div className="flex items-center gap-3">
            <span className="flex h-11 w-11 items-center justify-center rounded-full bg-cyan/20 text-cyan-ink">
              <Check size={22} strokeWidth={3} />
            </span>
            <div className="min-w-0">
              <p className="text-strong font-semibold">已接入兔啃</p>
              <p className="truncate text-caption text-ink/60">
                {desktopKey
                  ? `使用密钥「${desktopKey.name}」`
                  : "使用的是其他账户的密钥"}
              </p>
            </div>
          </div>
          <button
            type="button"
            aria-label="选择模型"
            className="flex items-center gap-3 rounded-[16px] bg-ink/[0.045] px-3.5 py-2.5 text-left transition-colors hover:bg-ink/[0.07]"
            onClick={() => setModelsOpen(true)}
          >
            <span className="min-w-0 flex-1">
              <span className="block text-caption text-ink/55">默认模型</span>
              <span className="block truncate text-small font-semibold">
                {status.models?.models[0]
                  ? modelInfo(status.models.models[0]).name
                  : "还没选，点这里选"}
              </span>
            </span>
            <span className="shrink-0 text-right">
              <span className="block text-caption text-ink/55">推理程度</span>
              <span className="block text-small font-semibold">
                {status.models?.effort
                  ? effortNames[status.models.effort]
                  : "默认"}
              </span>
            </span>
            <ChevronRight size={16} className="shrink-0 text-ink/40" />
          </button>
          {installing && (
            <div className="space-y-2">
              <p className="text-caption text-ink/60">
                {progress ?? "正在准备安装"}
              </p>
              <InstallProgress step={installStep} />
            </div>
          )}
          <button
            type="button"
            className="btn btn-glass self-start"
            disabled={installing}
            onClick={open}
          >
            打开 Claude Desktop
          </button>
        </div>
      ) : (
        <ol className="mt-2 divide-y divide-ink/[0.08]">
          <Step
            n={1}
            done={ready && !installing}
            title="安装 Claude Desktop"
            progress={
              installing ? <InstallProgress step={installStep} /> : null
            }
            detail={
              installing
                ? (progress ??
                  (mac
                    ? "正在从 Anthropic 官方下载安装，通常 1 到 3 分钟"
                    : "正在从兔啃下载站下载安装，通常 1 到 3 分钟"))
                : status.installed && !ready
                  ? "还缺 Claude Code 运行组件，补齐后才能开始对话"
                  : status.installed
                    ? `已安装${status.version ? `，版本 ${status.version}` : ""}`
                    : mac
                      ? "从 Anthropic 官方下载，安装前检查签名"
                      : "在兔啃内安装桌面程序和 Code 组件，首次打开默认进入 Code"
            }
          >
            {(!ready || installing) && (
              <button
                type="button"
                className="btn btn-glass btn-sm"
                disabled={installing}
                onClick={() => void runInstall()}
              >
                {installing ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <Download size={14} />
                )}
                {installing
                  ? "安装中"
                  : status.installed
                    ? "修复运行组件"
                    : "一键安装"}
              </button>
            )}
          </Step>
          <Step
            n={2}
            done={status.connected}
            title="接入兔啃"
            detail={
              groups.isPending && !groups.data
                ? "正在读取分组…"
                : claude.length
                  ? "自动建一个密钥，写进 Claude Desktop 的设置"
                  : "账户里没有可用于 Claude 的分组，请联系客服"
            }
          >
            <div className="flex items-center gap-2">
              {claude.length > 1 && (
                <select
                  aria-label="分组"
                  className="field h-[30px] w-auto rounded-full px-3 text-small"
                  value={chosen}
                  onChange={(e) => setGroupId(Number(e.target.value))}
                >
                  {claude.map((g) => (
                    <option key={g.id} value={g.id}>
                      {g.name}
                    </option>
                  ))}
                </select>
              )}
              <button
                type="button"
                className={cn(
                  "btn btn-sm",
                  status.installed ? "btn-primary" : "btn-glass",
                )}
                disabled={
                  !chosen ||
                  busy === "desktop" ||
                  installing ||
                  (status.installed && !ready)
                }
                onClick={() => void connect("desktop", { groupId: chosen })}
              >
                {busy === "desktop" ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <Plug size={14} />
                )}
                一键接入
              </button>
            </div>
          </Step>
        </ol>
      )}

      {status?.supported && (
        <div className="mt-4 flex items-center gap-3 rounded-[16px] bg-ink/[0.045] px-3.5 py-2.5">
          <Terminal size={16} className="shrink-0 text-ink/60" />
          <div className="min-w-0 flex-1">
            <p className="text-small font-semibold">Claude Code</p>
            <p className="truncate text-caption text-ink/55">
              {status.cli_connected
                ? "命令行和编辑器插件正在使用兔啃"
                : "命令行和编辑器插件"}
            </p>
          </div>
          {status.cli_connected ? (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              disabled={busy === "cli"}
              onClick={() => void disconnect("cli")}
            >
              断开
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-glass btn-sm"
              disabled={busy === "cli" || (!desktopKey && !chosen)}
              onClick={() =>
                void connect(
                  "cli",
                  desktopKey ? { keyId: desktopKey.id } : { groupId: chosen },
                )
              }
            >
              {busy === "cli" && <Loader2 size={14} className="animate-spin" />}
              接入
            </button>
          )}
        </div>
      )}
    </section>
  );
}
