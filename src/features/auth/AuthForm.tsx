import { useEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  ArrowLeft,
  Eye,
  EyeOff,
  Loader2,
  MailCheck,
} from "lucide-react";
import { toast } from "sonner";
import { GoogleLogo } from "@/components/GoogleLogo";
import { Segmented } from "@/components/Segmented";
import { tuken, type AppState } from "@/lib/api";
import { prefetchAccount, qk } from "@/lib/queries";
import { message, useLogin, type AuthMode } from "@/app/context";

type Reply =
  | { status: "approved"; user_id: number }
  | { status: "totp_required" }
  | { status: "sent"; cooldown: number }
  | { status: "refused"; reason: string; message: string };

const submitText: Record<AuthMode, string> = {
  login: "登录",
  register: "注册",
  reset: "重设密码并登录",
  totp: "验证并登录",
};

const ease = [0.22, 0.9, 0.24, 1] as const;

/** A field row that grows into the well when it is needed. */
function Row({ show, children }: { show: boolean; children: ReactNode }) {
  return (
    <AnimatePresence initial={false}>
      {show && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: "auto", opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.22, ease }}
          className="overflow-hidden"
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** One line of guidance next to the well: what happened, what to do. */
function Note({ text, tone }: { text: string; tone: "sent" | "error" }) {
  const Icon = tone === "error" ? AlertCircle : MailCheck;
  return (
    <AnimatePresence initial={false}>
      {text && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: "auto", opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.2, ease }}
          className="overflow-hidden"
        >
          <p
            role={tone === "error" ? "alert" : "status"}
            className={
              tone === "error"
                ? "flex gap-2 pt-3 text-small text-danger"
                : "flex gap-2 pb-3 text-small text-cyan-ink"
            }
          >
            <Icon size={15} className="mt-[3px] shrink-0" />
            {text}
          </p>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/**
 * Sign-in and sign-up in the app's own controls, on the desktop sign-in API.
 * An email without an account turns into sign-up with its code already sent.
 */
export function AuthForm({
  id,
  mode,
  onMode,
  onDone,
  onError,
}: {
  /** Separates the tab droplets of two forms. */
  id: string;
  mode: AuthMode;
  onMode: (mode: AuthMode) => void;
  /** Leaving the form: signed in, or off to Google in the browser. */
  onDone?: () => void;
  /** Whether an error is showing, for the mascot. */
  onError?: (shown: boolean) => void;
}) {
  const client = useQueryClient();
  const login = useLogin();
  const rules = login.config?.available ? login.config : undefined;
  const min = rules?.password_min_length ?? 6;
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [invite, setInvite] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [hint, setHint] = useState("");
  const [cooldown, setCooldown] = useState(0);
  const working = useRef(false);
  const codeRef = useRef<HTMLInputElement>(null);

  useEffect(() => onError?.(!!error), [error, onError]);

  // The admin may have just changed what sign-in offers (Google, sign-up).
  useEffect(() => {
    void client.invalidateQueries({ queryKey: qk.authConfig });
  }, [client]);

  useEffect(() => {
    if (cooldown <= 0) return;
    const t = window.setTimeout(() => setCooldown((c) => c - 1), 1000);
    return () => window.clearTimeout(t);
  }, [cooldown]);

  const go = (next: AuthMode, note = "") => {
    setError("");
    setHint(note);
    setCode("");
    onMode(next);
  };

  async function run(work: () => Promise<unknown>) {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(message(e));
    } finally {
      working.current = false;
      setBusy(false);
    }
  }

  const ask = (action: string, input: Record<string, unknown>) =>
    tuken<Reply>(action, input);

  async function signedIn(userId: number) {
    const state = await tuken<AppState>("state");
    client.setQueryData(qk.state, state);
    prefetchAccount(client, userId);
    const me = state.accounts.find((a) => a.user_id === userId);
    toast.success(`已登录 ${me?.email ?? "兔啃"}`);
    onDone?.();
  }

  /** Sends the email code; false (with the reason shown) if it didn't go. */
  async function sendCode() {
    const r = await ask("auth_send_code", {
      email,
      purpose: mode === "reset" ? "reset" : "register",
    });
    if (r.status === "sent") {
      setCooldown(r.cooldown);
      codeRef.current?.focus();
      return true;
    }
    if (r.status === "refused" && r.reason === "EMAIL_EXISTS") {
      go("login", "这个邮箱已经注册过了，输入密码登录吧");
    } else if (r.status === "refused") {
      setError(r.message);
    }
    return false;
  }

  const submit = () =>
    run(async () => {
      if (mode === "login") {
        const r = await ask("auth_login", { email, password });
        if (r.status === "approved") return signedIn(r.user_id);
        if (r.status === "totp_required") return go("totp");
        if (r.status !== "refused") return;
        if (
          r.reason === "EMAIL_NOT_REGISTERED" &&
          rules?.registration_enabled
        ) {
          // Straight on to sign-up: the code goes out now, the password stays.
          go("register");
          const r2 = await ask("auth_send_code", {
            email,
            purpose: "register",
          });
          if (r2.status === "sent") {
            setCooldown(r2.cooldown);
            setHint(
              `这个邮箱还没注册。验证码已发到 ${email.trim()}，填上就能注册`,
            );
            codeRef.current?.focus();
          } else if (r2.status === "refused") {
            setError(r2.message);
          }
          return;
        }
        return setError(r.message);
      }
      if (mode === "totp") {
        const r = await ask("auth_totp", { code });
        if (r.status === "approved") return signedIn(r.user_id);
        if (r.status === "refused") setError(r.message);
        return;
      }
      const r = await ask(mode === "reset" ? "auth_reset" : "auth_register", {
        email,
        code,
        password,
        invitation_code: rules?.invitation_code_enabled ? invite : "",
        affiliate_code: invite,
      });
      if (r.status === "approved") return signedIn(r.user_id);
      if (r.status !== "refused") return;
      if (r.reason === "EMAIL_EXISTS")
        return go("login", "这个邮箱已经注册过了，输入密码登录吧");
      setError(r.message);
    });

  const needsCode = mode === "register" || mode === "reset";
  const needsInvite =
    mode === "register" &&
    (!!rules?.invite_required || !!rules?.invitation_code_enabled);
  const takesInvite =
    mode === "register" && (needsInvite || !!rules?.invite_enabled);
  const ready =
    mode === "totp"
      ? code.length === 6
      : email.trim() !== "" &&
        password !== "" &&
        (!needsCode || code.trim() !== "") &&
        (!needsInvite || invite.trim() !== "");
  const tabs =
    (mode === "login" || mode === "register") &&
    rules?.registration_enabled !== false;

  return (
    <form
      noValidate
      className="flex flex-col"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) void submit();
      }}
    >
      {tabs && (
        <div className="mb-4">
          <Segmented
            stretch
            id={`auth-${id}`}
            label="登录或注册"
            value={mode}
            options={[
              ["login", "登录"],
              ["register", "注册"],
            ]}
            onChange={(next) => go(next)}
          />
        </div>
      )}

      <Note tone="sent" text={hint} />

      {mode === "totp" ? (
        <div className="field-group">
          <div className="field-row">
            <input
              aria-label="两步验证码"
              autoFocus
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={6}
              placeholder="验证器里的 6 位数字"
              className="text-center font-mono tracking-[0.3em] placeholder:font-sans placeholder:tracking-normal"
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
            />
          </div>
        </div>
      ) : (
        <div className="field-group">
          <div className="field-row">
            <input
              aria-label="邮箱"
              type="email"
              autoComplete="email"
              autoFocus
              spellCheck={false}
              placeholder="邮箱"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </div>
          <Row show={needsCode}>
            <div className="field-row">
              <input
                ref={codeRef}
                aria-label="邮箱验证码"
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={8}
                placeholder="邮箱验证码"
                value={code}
                onChange={(e) => setCode(e.target.value.trim())}
              />
              <button
                type="button"
                className="btn btn-ghost btn-sm shrink-0 tabular-nums text-orange"
                disabled={busy || cooldown > 0 || !email.trim()}
                onClick={() =>
                  void run(async () => {
                    if (await sendCode())
                      setHint(`验证码已发到 ${email.trim()}`);
                  })
                }
              >
                {cooldown > 0 ? `${cooldown} 秒后可重发` : "发送验证码"}
              </button>
            </div>
          </Row>
          <div className="field-row">
            <input
              aria-label={mode === "login" ? "密码" : "设置密码"}
              type={reveal ? "text" : "password"}
              autoComplete={
                mode === "login" ? "current-password" : "new-password"
              }
              maxLength={72}
              placeholder={
                mode === "login" ? "密码" : `设置密码，至少 ${min} 位`
              }
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <button
              type="button"
              className="icon-btn"
              aria-label={reveal ? "隐藏密码" : "显示密码"}
              aria-pressed={reveal}
              onClick={() => setReveal((v) => !v)}
            >
              {reveal ? <EyeOff size={16} /> : <Eye size={16} />}
            </button>
          </div>
          <Row show={takesInvite}>
            <div className="field-row">
              <input
                aria-label="邀请码"
                spellCheck={false}
                maxLength={32}
                placeholder={
                  needsInvite ? "邀请码（必填）" : "好友邀请码（选填）"
                }
                value={invite}
                onChange={(e) => setInvite(e.target.value.trim())}
              />
            </div>
          </Row>
        </div>
      )}

      <Note tone="error" text={error} />

      {mode === "register" && !!rules?.email_suffixes.length && (
        <p className="pt-2.5 text-caption text-ink/55">
          只能用 {rules.email_suffixes.join("、")} 结尾的邮箱注册
        </p>
      )}
      {mode === "login" && rules?.password_reset_enabled && (
        <button
          type="button"
          className="mt-2 self-end text-caption font-medium text-ink/55 hover:text-ink"
          onClick={() => go("reset")}
        >
          忘记密码？
        </button>
      )}

      <button
        type="submit"
        className="btn btn-primary btn-lg mt-4 w-full"
        disabled={busy || !ready}
      >
        {busy && <Loader2 size={16} className="animate-spin" />}
        {submitText[mode]}
      </button>

      {(mode === "reset" || mode === "totp") && (
        <button
          type="button"
          className="btn btn-ghost btn-sm mt-3 self-center"
          onClick={() => go("login")}
        >
          <ArrowLeft size={14} />
          返回登录
        </button>
      )}
      {/* Google signs in, or makes the account on first sign-in; from 注册 it
          brings the invitation code along. Only while the admin has it on. */}
      {(mode === "login" || mode === "register") && rules?.google && (
        <>
          <div
            aria-hidden="true"
            className="my-3 flex items-center gap-3 text-caption text-ink/45"
          >
            <span className="h-px flex-1 bg-ink/10" />或
            <span className="h-px flex-1 bg-ink/10" />
          </div>
          <button
            type="button"
            className="btn btn-glass btn-lg w-full"
            disabled={busy || !!login.google || (needsInvite && !invite)}
            onClick={() => {
              onDone?.();
              void login.startGoogle(
                takesInvite && invite ? invite : undefined,
              );
            }}
          >
            <GoogleLogo />
            {mode === "register" ? "使用 Google 注册" : "使用 Google 登录"}
          </button>
        </>
      )}
    </form>
  );
}
