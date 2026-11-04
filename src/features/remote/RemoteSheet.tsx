import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Check,
  Copy,
  FolderOpen,
  Loader2,
  MonitorUp,
  QrCode,
  Sparkles,
} from "lucide-react";
import { toast } from "sonner";
import { Sheet } from "@/components/Sheet";
import { tuken, type RemoteEvent, type RemoteStatus } from "@/lib/api";
import { cn } from "@/lib/cn";
import { qk, useRemote } from "@/lib/queries";
import { useAction, useSession, useUi } from "@/app/context";

/** The folder's own name, for short lines. */
export function folderName(path: string | null | undefined) {
  if (!path) return "";
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** "20261104" → "2026 1104", the way the phone asks for it. */
function spaced(code: string) {
  return `${code.slice(0, 4)} ${code.slice(4)}`;
}

/** One line of what happened, for the list in the sheet. */
function line(e: RemoteEvent): string | null {
  switch (e.kind) {
    case "user":
      return `手机：${e.text ?? ""}`;
    case "text":
      return `Claude：${e.text ?? ""}`;
    case "tool":
      return `· ${e.title ?? "使用工具"}`;
    case "ask":
      return `等手机确认：${e.title ?? ""}`;
    case "asked":
      return e.allowed ? "手机上允许了" : "手机上拒绝了";
    case "done":
      return e.ok === false
        ? `没有完成${e.text ? `：${e.text}` : ""}`
        : `完成${e.seconds ? ` · 用时 ${Math.round(e.seconds)} 秒` : ""}`;
    case "note":
      return e.text ?? null;
    default:
      return null;
  }
}

/** The QR code a phone scans: dark modules on white, whatever the theme. */
function Qr({ size, path }: { size: number; path: string }) {
  return (
    <svg
      role="img"
      aria-label="配对二维码"
      viewBox={`-3 -3 ${size + 6} ${size + 6}`}
      className="h-[196px] w-[196px] rounded-[14px] bg-white"
      shapeRendering="crispEdges"
    >
      <path d={path} fill="#003F43" />
    </svg>
  );
}

function Checkbox({
  checked,
  label,
  hint,
  disabled,
  onChange,
}: {
  checked: boolean;
  label: string;
  hint: string;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="flex w-full items-start gap-3 rounded-[14px] px-1 py-1.5 text-left disabled:opacity-50"
    >
      <span
        aria-hidden="true"
        className={cn(
          "mt-0.5 flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[6px] border-2 transition-colors",
          checked ? "border-orange bg-orange text-white" : "border-ink/25",
        )}
      >
        {checked && <Check size={12} strokeWidth={3} />}
      </span>
      <span className="min-w-0">
        <span className="block text-body font-medium">{label}</span>
        <span className="block text-caption text-ink/55">{hint}</span>
      </span>
    </button>
  );
}

/** 手机遥控: choose where Claude works, pair a phone, and see what it does. */
export function RemoteSheet() {
  const { remoteOpen, setRemoteOpen } = useUi();
  const { uid } = useSession();
  const client = useQueryClient();
  const remote = useRemote();
  const { busy, run } = useAction();
  const [pairing, setPairing] = useState<{
    link: string;
    qr: NonNullable<RemoteStatus["qr"]>;
    code: string | null;
  } | null>(null);
  const r = remote.data;

  // The phone came: the code has done its job.
  useEffect(() => {
    if (r?.phone_online) setPairing(null);
  }, [r?.phone_online]);
  useEffect(() => {
    if (!remoteOpen) setPairing(null);
  }, [remoteOpen]);

  const act = (action: string, input?: Record<string, unknown>) =>
    run(async () => {
      const next = await tuken<RemoteStatus>(action, {
        ...input,
        as_user: uid,
      });
      client.setQueryData(qk.remote, next);
      if (next.qr && next.link)
        setPairing({ link: next.link, qr: next.qr, code: next.code ?? null });
      // Unpaired: the code is no good any more.
      else if (!next.paired) setPairing(null);
    });

  const copy = () =>
    void navigator.clipboard
      .writeText(pairing?.link ?? "")
      .then(() => toast.success("链接已复制，在手机浏览器里打开"))
      .catch(() => toast.error("复制失败"));

  const unsynced = r?.unsynced ?? [];
  const models = r?.models ?? [];

  const recent = (r?.log ?? [])
    .map((e) => line(e))
    .filter((l): l is string => !!l)
    .slice(-30);

  return (
    <Sheet
      open={remoteOpen}
      onOpenChange={setRemoteOpen}
      title="手机遥控"
      description="用手机给这台电脑上的 Claude 发指令，它在你选的文件夹里干活。"
      width={480}
    >
      {!r ? (
        <div className="skeleton h-24" />
      ) : !r.supported ? (
        <p className="text-body text-ink/70">这个系统暂不支持手机遥控。</p>
      ) : (
        <div className="space-y-4">
          <div className="flex items-center gap-3 rounded-[18px] bg-ink/[0.045] px-4 py-3">
            <FolderOpen size={18} className="shrink-0 text-ink/60" />
            <div className="min-w-0 flex-1">
              <p className="text-caption text-ink/55">Claude 干活的文件夹</p>
              <p
                className="selectable truncate text-body font-medium"
                title={r.folder ?? ""}
              >
                {r.folder || "还没选"}
              </p>
            </div>
            <button
              type="button"
              className="btn btn-glass btn-sm"
              disabled={busy}
              onClick={() => void act("remote_folder")}
            >
              {r.folder ? "更换" : "选择"}
            </button>
          </div>

          {r.paired && models.length > 0 && (
            <div className="flex items-center gap-3 rounded-[18px] bg-ink/[0.045] px-4 py-3">
              <Sparkles size={18} className="shrink-0 text-ink/60" />
              <p className="min-w-0 flex-1 text-body font-medium">模型</p>
              <select
                aria-label="Claude 用的模型"
                className="field h-[30px] w-auto max-w-[220px] rounded-full px-3 text-small"
                value={r.model ?? models[0]}
                disabled={busy}
                onChange={(e) =>
                  void act("remote_set", { model: e.target.value })
                }
              >
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </div>
          )}

          {pairing ? (
            <div className="flex flex-col items-center gap-3 rounded-[18px] bg-ink/[0.045] px-4 py-5 text-center">
              <Qr size={pairing.qr.size} path={pairing.qr.path} />
              <p className="text-body font-semibold">用手机相机扫码打开</p>
              {pairing.code && (
                <div className="w-full rounded-[14px] bg-white/60 px-3 py-2.5">
                  <p className="text-caption text-ink/55">
                    或在兔啃手机 App 里输入配对码（10 分钟内有效）
                  </p>
                  <p className="selectable mt-1 text-[26px] font-semibold tracking-[0.18em] tnum">
                    {spaced(pairing.code)}
                  </p>
                </div>
              )}
              <p className="text-caption text-ink/55">
                配对后这台手机一直能用，不用再扫。别把这个码发给别人。
              </p>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={copy}
              >
                <Copy size={14} />
                复制链接
              </button>
            </div>
          ) : r.paired ? (
            <div className="rounded-[18px] bg-ink/[0.045] px-4 py-3">
              <div className="flex items-center gap-3">
                <span
                  aria-hidden="true"
                  className={cn(
                    "h-2.5 w-2.5 shrink-0 rounded-full",
                    r.on && r.phone_online ? "bg-cyan" : "bg-ink/25",
                  )}
                />
                <div className="min-w-0 flex-1">
                  <p className="text-body font-semibold">
                    {!r.on
                      ? "已关闭"
                      : r.busy
                        ? "Claude 正在干活"
                        : r.phone_online
                          ? "手机已连接"
                          : "已开启，等手机连上"}
                  </p>
                  <p className="text-caption text-ink/55">
                    {!r.on
                      ? "手机发来的指令不会执行"
                      : (r.error ?? "兔啃开着（缩到托盘也行）就能遥控")}
                  </p>
                </div>
                <button
                  type="button"
                  className="btn btn-glass btn-sm"
                  disabled={busy}
                  onClick={() => void act("remote_set", { on: !r.on })}
                >
                  {r.on ? "关闭" : "开启"}
                </button>
              </div>
            </div>
          ) : null}

          <Checkbox
            checked={r.accept_edits}
            label="改文件不用手机确认"
            hint="只有运行命令时才在手机上问你"
            disabled={busy || !r.paired}
            onChange={(on) => void act("remote_set", { accept_edits: on })}
          />

          {(r.paired || unsynced.length > 0) && (
            <Checkbox
              checked={r.sync !== false}
              label="同步到 Claude Desktop"
              hint="手机上的对话结束后，趁你没用电脑时放进 Claude Desktop 的对话列表（它会跳到这段对话）"
              disabled={busy}
              onChange={(on) => void act("remote_set", { sync: on })}
            />
          )}

          {unsynced.length > 0 && (
            <div className="flex items-center gap-3 rounded-[14px] bg-ink/[0.03] px-3 py-2">
              <MonitorUp size={16} className="shrink-0 text-ink/55" />
              <p className="min-w-0 flex-1 truncate text-small text-ink/70">
                {unsynced.length} 段对话等待同步：
                {unsynced[0].title || "手机上的对话"}
                {unsynced.length > 1 ? " 等" : ""}
              </p>
              <button
                type="button"
                className="btn btn-glass btn-sm"
                disabled={busy}
                onClick={() => void act("remote_sync")}
              >
                现在同步
              </button>
            </div>
          )}

          {r.paired && recent.length > 0 && (
            <div>
              <p className="mb-1.5 text-caption text-ink/55">最近的操作</p>
              <div className="selectable max-h-[180px] space-y-1 overflow-y-auto rounded-[14px] bg-ink/[0.03] px-3 py-2">
                {recent.map((text, i) => (
                  <p
                    key={i}
                    className="line-clamp-2 break-words text-small text-ink/75"
                  >
                    {text}
                  </p>
                ))}
              </div>
            </div>
          )}

          <div className="flex items-center justify-between gap-3 pt-1">
            {r.paired ? (
              <button
                type="button"
                className="btn btn-ghost btn-sm text-danger"
                disabled={busy}
                onClick={() => void act("remote_unpair")}
              >
                解除配对
              </button>
            ) : (
              <span className="text-caption text-ink/55">
                {r.folder ? "" : "先选好文件夹，再配对手机"}
              </span>
            )}
            <button
              type="button"
              className={cn("btn", r.paired ? "btn-glass" : "btn-primary")}
              disabled={busy || !r.folder}
              onClick={() => void act("remote_pair")}
            >
              {busy ? (
                <Loader2 size={16} className="animate-spin" />
              ) : (
                <QrCode size={16} />
              )}
              {r.paired ? "换一台手机" : "配对手机"}
            </button>
          </div>
        </div>
      )}
    </Sheet>
  );
}
