import { useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "framer-motion";
import {
  Check,
  Download,
  ExternalLink,
  Gift,
  Loader2,
  Wallet,
} from "lucide-react";
import { Mascot } from "@/components/Mascot";
import { Segmented } from "@/components/Segmented";
import { desktop, EFFORTS, tuken, type Effort, type Key } from "@/lib/api";
import { cn } from "@/lib/cn";
import { usd } from "@/lib/format";
import { installText, useInstallStep } from "@/lib/install";
import { isMac } from "@/lib/platform";
import { byStrength, modelInfo } from "@/lib/models";
import {
  fetchers,
  qk,
  useCheckin,
  useDesktop,
  useProfile,
} from "@/lib/queries";
import {
  claudeModel,
  effortNames,
  recommended,
  recommendedEffort,
} from "@/features/home/ModelSheet";
import { message, useSession, useUi } from "@/app/context";

type Step = "install" | "credit" | "config" | "ready";

const stepName: Record<Step, string> = {
  install: "安装 Claude Desktop",
  credit: "准备额度",
  config: "一键配置",
  ready: "完成",
};

const DOWNLOAD_PAGE = "https://claude.ai/download";
const WINDOWS_DOWNLOAD_PAGE =
  "https://download.tuken.site/claude-code-desktop/v3.6556.2/Claude-Code-Desktop-Windows-x64.exe";

/** Where the setup stands, step by step: done, on show, or still ahead. */
function Stepper({ steps, at }: { steps: Step[]; at: number }) {
  return (
    <ol
      aria-label="激活步骤"
      className="flex flex-wrap items-center justify-center gap-x-1.5 gap-y-2"
    >
      {steps.map((s, i) => (
        <li
          key={s}
          aria-current={i === at ? "step" : undefined}
          className="flex items-center gap-1.5"
        >
          <span
            className={cn(
              "flex h-6 w-6 items-center justify-center rounded-full text-caption font-bold tnum",
              i < at
                ? "bg-cyan/20 text-cyan-ink"
                : i === at
                  ? "text-ink ring-2 ring-inset ring-ink/70"
                  : "bg-ink/[0.07] text-ink/45",
            )}
          >
            {i < at ? <Check size={13} strokeWidth={3} /> : i + 1}
          </span>
          <span
            className={cn(
              "text-small",
              i === at ? "font-semibold text-ink" : "text-ink/55",
            )}
          >
            {stepName[s]}
          </span>
          {i < steps.length - 1 && (
            <span aria-hidden="true" className="mx-1.5 h-px w-6 bg-ink/15" />
          )}
        </li>
      ))}
    </ol>
  );
}

/** One page of the setup: 兔啃, what this step is, and what to press. */
function Page({
  mood = "bunny",
  title,
  children,
  actions,
}: {
  mood?: "bunny" | "cheer" | "thanks" | "oops" | "flat";
  title: string;
  children?: ReactNode;
  actions: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center text-center">
      <span className="flex h-[104px] w-[104px] items-center justify-center rounded-full bg-white/80">
        <Mascot mood={mood} size={92} />
      </span>
      <h1 className="mt-5 text-title font-semibold">{title}</h1>
      {children && (
        <div className="mt-2 w-full max-w-[440px] text-body text-ink/70">
          {children}
        </div>
      )}
      <div className="mt-7 flex flex-wrap items-center justify-center gap-2.5">
        {actions}
      </div>
    </div>
  );
}

/**
 * The first run on this computer, like activating a new phone: Claude
 * Desktop installed, a key written into it, the newest Opus at the highest
 * effort unless the user picks otherwise, and only then the home page.
 * Opening Claude Desktop afterwards just works.
 */
export function Setup() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { finishSetup, setCheckinOpen, setRechargeOpen, setContactOpen } =
    useUi();
  const { data: status } = useDesktop();
  const profile = useProfile(uid);
  const checkin = useCheckin(uid);

  const balance = profile.data ? Number(profile.data.balance) : undefined;
  // Asked once: an account that starts with credit never sees this step.
  const [creditStep, setCreditStep] = useState<boolean | null>(null);
  useEffect(() => {
    if (creditStep === null && balance !== undefined)
      setCreditStep(balance <= 0);
  }, [balance, creditStep]);

  const installed = !!status?.installed;
  const funded = balance !== undefined && balance > 0;
  const configured = !!status?.connected && !!status.models?.models.length;
  const step: Step = !installed
    ? "install"
    : creditStep && !funded
      ? "credit"
      : !configured
        ? "config"
        : "ready";
  const steps: Step[] = [
    "install",
    ...(creditStep ? (["credit"] as const) : []),
    "config",
    "ready",
  ];

  // Installing in the browser: notice when it is there.
  useEffect(() => {
    if (step !== "install") return;
    const t = window.setInterval(
      () => void client.invalidateQueries({ queryKey: qk.desktop }),
      3000,
    );
    return () => window.clearInterval(t);
  }, [step, client]);

  // Credit lands from the check-in or the recharge sheet: look again soon.
  useEffect(() => {
    if (step !== "credit" || !uid) return;
    const t = window.setInterval(
      () => void client.invalidateQueries({ queryKey: qk.profile(uid) }),
      4000,
    );
    return () => window.clearInterval(t);
  }, [step, client, uid]);

  const [install, setInstall] = useState<
    "idle" | "busy" | "browser" | "failed"
  >("idle");
  const [installError, setInstallError] = useState("");
  const progress = installText(
    useInstallStep(install === "busy" || !!status?.installing),
  );
  async function runInstall() {
    setInstall("busy");
    try {
      const r = await desktop.install();
      client.setQueryData(qk.desktop, await desktop.status());
      setInstall(r.method === "browser" ? "browser" : "idle");
    } catch (e) {
      setInstallError(message(e));
      setInstall("failed");
    }
  }
  const openDownloadPage = () =>
    void tuken("open_link", {
      url: isMac() ? DOWNLOAD_PAGE : WINDOWS_DOWNLOAD_PAGE,
    }).catch(() => {});

  // Before the one-click setup: a key in a Claude group (kept, or made once)
  // and the models it offers, so the user can choose.
  const [prep, setPrep] = useState<
    { keyId: number; ids: string[] } | { error: string } | null
  >(null);
  const preparing = useRef(false);
  useEffect(() => {
    if (step !== "config" || prep || preparing.current || !uid) return;
    preparing.current = true;
    void (async () => {
      try {
        const groups = (await fetchers.groups(uid)).filter(
          (g) => (g.platform ?? "").toLowerCase() === "anthropic",
        );
        if (!groups.length) throw "账户里没有可用于 Claude 的分组，请联系客服";
        const usable = (k: Key) =>
          k.status === "active" && groups.some((g) => g.id === k.group_id);
        const keys = await fetchers.keys(uid);
        const key =
          keys.find((k) => k.in_desktop && usable(k)) ??
          keys.find(usable) ??
          (await tuken<Key>("key_create", {
            name: "Claude Desktop",
            group_id: groups[0].id,
            as_user: uid,
          }));
        void client.invalidateQueries({ queryKey: qk.keys(uid) });
        const listed = await fetchers.models(uid, key.group_id!);
        const ids = byStrength(
          listed.models.map((m) => m.id).filter(claudeModel),
        );
        if (!ids.length)
          throw listed.source === "balance"
            ? "余额为 0，充值后才能选模型"
            : "这个分组里没有 Claude 模型，请联系客服";
        setPrep({ keyId: key.id, ids });
      } catch (e) {
        setPrep({ error: message(e) });
      } finally {
        preparing.current = false;
      }
    })();
  }, [step, prep, uid, client]);

  const ids = prep && "ids" in prep ? prep.ids : [];
  const [pick, setPick] = useState<string | null>(null);
  const [effort, setEffort] = useState<Effort>(recommendedEffort);
  const suggested = recommended(ids);
  const chosen = pick && ids.includes(pick) ? pick : suggested;

  const [configuring, setConfiguring] = useState(false);
  const [configError, setConfigError] = useState("");
  async function configure() {
    if (!uid || !prep || !("keyId" in prep) || !chosen) return;
    setConfiguring(true);
    setConfigError("");
    try {
      await tuken("connect", { app: "desktop", id: prep.keyId, as_user: uid });
      await desktop.setModels(
        [chosen, ...ids.filter((m) => m !== chosen)],
        effort,
      );
      void client.invalidateQueries({ queryKey: qk.keys(uid) });
      client.setQueryData(qk.desktop, await desktop.status());
    } catch (e) {
      setConfigError(message(e));
    } finally {
      setConfiguring(false);
    }
  }

  const [opening, setOpening] = useState(false);
  async function openClaude() {
    setOpening(true);
    try {
      // Running already: restarted so the new settings take effect.
      await desktop.open(!!status?.running && !!status.can_restart);
    } catch {
      /* It still opens from the home page. */
    }
    finishSetup();
  }

  const skip = (
    <button
      type="button"
      className="btn btn-ghost btn-sm text-ink/55"
      onClick={finishSetup}
    >
      先跳过，以后在首页设置
    </button>
  );
  const help = (
    <button
      type="button"
      className="btn btn-ghost btn-sm text-ink/55"
      onClick={() => setContactOpen(true)}
    >
      联系我们
    </button>
  );

  const c =
    checkin.data?.supported && checkin.data.enabled && !checkin.data.checked_in
      ? checkin.data
      : null;

  let page: ReactNode;
  if (!status || creditStep === null) {
    page = (
      <div className="flex h-[300px] items-center justify-center">
        <Loader2 size={22} className="animate-spin text-ink/40" />
      </div>
    );
  } else if (step === "install") {
    page =
      install === "busy" || status.installing ? (
        <Page
          title="正在安装 Claude Desktop"
          actions={
            <p
              role="status"
              className="flex items-center gap-2 text-small text-ink/60"
            >
              <Loader2 size={15} className="animate-spin" />
              {progress ?? "通常 1 到 3 分钟"}，装好会自动继续
            </p>
          }
        />
      ) : install === "browser" ? (
        <Page
          title="在浏览器里下载安装"
          actions={
            <>
              <button
                type="button"
                className="btn btn-glass"
                onClick={openDownloadPage}
              >
                <ExternalLink size={16} />
                重新打开下载页
              </button>
              {skip}
            </>
          }
        >
          {isMac()
            ? "已经打开 Claude 官网的下载页。下载后打开它，把 Claude 拖进「应用程序」，这里会自动继续。"
            : "已经打开下载页，装好后这里会自动继续。"}
        </Page>
      ) : install === "failed" ? (
        <Page
          mood="oops"
          title="安装没有完成"
          actions={
            <>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => void runInstall()}
              >
                再试一次
              </button>
              {isMac() && (
                <button
                  type="button"
                  className="btn btn-glass"
                  onClick={openDownloadPage}
                >
                  <ExternalLink size={16} />
                  去官网下载
                </button>
              )}
              {skip}
              {help}
            </>
          }
        >
          {installError || "可能是网络不稳定。"}{" "}
          {isMac()
            ? "也可以去官网下载安装，装好后这里会自动继续。"
            : "请重试，装好后这里会自动继续。"}
        </Page>
      ) : (
        <Page
          title="先装好 Claude Desktop"
          actions={
            <button
              type="button"
              className="btn btn-primary btn-lg min-w-[180px]"
              onClick={() => void runInstall()}
            >
              <Download size={17} />
              一键安装
            </button>
          }
        >
          {isMac()
            ? "从 Anthropic 官方下载安装，装好会自动进入下一步。"
            : "从兔啃下载站下载安装，装好会自动进入下一步。"}
        </Page>
      );
  } else if (step === "credit") {
    page = (
      <Page
        title="先领一点额度"
        actions={
          <>
            {c && (
              <button
                type="button"
                className="btn btn-primary btn-lg"
                onClick={() => setCheckinOpen(true)}
              >
                <Gift size={17} />
                签到领额度
              </button>
            )}
            <button
              type="button"
              className={cn("btn btn-lg", c ? "btn-glass" : "btn-primary")}
              onClick={() => setRechargeOpen(true)}
            >
              <Wallet size={17} />
              充值
            </button>
          </>
        }
      >
        现在余额是 {usd(balance ?? 0)}，{c ? "签到或充值" : "充值"}
        到账后会自动继续。
      </Page>
    );
  } else if (step === "config") {
    const prepError = prep && "error" in prep ? prep.error : "";
    const error = prepError || configError;
    page = (
      <Page
        mood={error ? "oops" : "bunny"}
        title="一键配置 Claude Desktop"
        actions={
          prepError ? (
            <>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => setPrep(null)}
              >
                再试一次
              </button>
              {skip}
              {help}
            </>
          ) : (
            <>
              <button
                type="button"
                className="btn btn-primary btn-lg min-w-[180px]"
                disabled={!chosen || configuring}
                onClick={() => void configure()}
              >
                {configuring && <Loader2 size={17} className="animate-spin" />}
                {configuring ? "正在配置" : "一键配置"}
              </button>
              {configError && skip}
            </>
          )
        }
      >
        <div className="mx-auto mt-3 grid w-full max-w-[420px] gap-3 text-left">
          <div className="flex items-center gap-4">
            <span className="w-16 shrink-0 text-small text-ink/60">
              默认模型
            </span>
            {ids.length ? (
              <select
                aria-label="默认模型"
                className="field h-10 min-w-0 flex-1 rounded-[12px] px-3 text-small"
                value={chosen ?? ""}
                onChange={(e) => setPick(e.target.value)}
              >
                {ids.map((id) => {
                  const m = modelInfo(id);
                  return (
                    <option key={id} value={id}>
                      {[m.name, m.note, id === suggested && "推荐"]
                        .filter(Boolean)
                        .join(" · ")}
                    </option>
                  );
                })}
              </select>
            ) : (
              <span className="skeleton h-10 flex-1" />
            )}
          </div>
          <div className="flex items-center gap-4">
            <span className="w-16 shrink-0 text-small text-ink/60">
              推理程度
            </span>
            <Segmented<Effort>
              id="setup-effort"
              label="推理程度"
              size="sm"
              value={effort}
              options={EFFORTS.map((e) => [e, effortNames[e]] as const)}
              badge={[recommendedEffort, "推荐"]}
              onChange={setEffort}
            />
          </div>
        </div>
        {error && (
          <p role="alert" className="mt-4 text-small text-danger">
            {error}
          </p>
        )}
      </Page>
    );
  } else {
    const model = status.models?.models[0];
    const level = status.models?.effort;
    page = (
      <Page
        mood="cheer"
        title="全部准备好了"
        actions={
          <>
            <button
              type="button"
              className="btn btn-primary btn-lg min-w-[200px]"
              disabled={opening}
              onClick={() => void openClaude()}
            >
              {opening && <Loader2 size={17} className="animate-spin" />}
              打开 Claude Desktop
            </button>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={finishSetup}
            >
              先进入兔啃
            </button>
          </>
        }
      >
        默认用 {model ? modelInfo(model).name : "Opus"}，推理程度
        {level ? effortNames[level] : "最高"}，打开就能直接用。
        {status.running &&
          (status.can_restart
            ? "它正开着，会自动重启一下。"
            : "它正开着，请手动重启一下，新设置才会生效。")}
      </Page>
    );
  }

  const at = steps.indexOf(step);
  return (
    <div className="mx-auto flex w-full max-w-[680px] flex-1 flex-col items-center justify-center gap-6 px-6 pb-12 pt-2">
      <Stepper steps={steps} at={status && creditStep !== null ? at : -1} />
      <section className="glass w-full px-8 py-10">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={`${step}-${install}`}
            initial={{ opacity: 0, x: 14 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -14 }}
            transition={{ duration: 0.2, ease: [0.22, 0.9, 0.24, 1] }}
          >
            {page}
          </motion.div>
        </AnimatePresence>
      </section>
    </div>
  );
}
