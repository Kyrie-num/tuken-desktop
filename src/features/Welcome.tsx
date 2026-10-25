import { useState } from "react";
import { motion } from "framer-motion";
import { Loader2 } from "lucide-react";
import { GoogleLogo } from "@/components/GoogleLogo";
import { Mascot } from "@/components/Mascot";
import { AuthForm } from "@/features/auth/AuthForm";
import { useLogin, useUi, type AuthMode } from "@/app/context";

const titles: Partial<Record<AuthMode, string>> = {
  reset: "重设密码",
  totp: "两步验证",
};

/**
 * Signed out: one glass card with 兔啃 peeking over its edge, and the
 * sign-in form right in it.
 */
export function Welcome() {
  const login = useLogin();
  const { setContactOpen } = useUi();
  const [mode, setMode] = useState<AuthMode>("login");
  const [oops, setOops] = useState(false);
  const google = !!login.google;
  return (
    <div className="flex flex-1 items-center justify-center px-6 py-4">
      <motion.div
        initial={{ opacity: 0, y: 14, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 260, damping: 26 }}
        className="relative w-full max-w-[392px] pt-[44px]"
      >
        <Mascot
          mood={oops && !google ? "oops" : "bunny"}
          size={88}
          className="absolute left-1/2 top-0 z-10 -translate-x-1/2"
        />
        <section className="glass px-8 pb-7 pt-[56px]">
          <h1 className="mb-5 text-center font-display text-title font-semibold">
            {(!google && titles[mode]) || "你好，我是兔啃"}
          </h1>
          {google ? (
            <GoogleWait />
          ) : login.config?.available ? (
            <AuthForm
              id="welcome"
              mode={mode}
              onMode={setMode}
              onError={setOops}
            />
          ) : login.config ? (
            <SiteSignIn />
          ) : (
            <div className="flex h-40 items-center justify-center">
              <Loader2 size={20} className="animate-spin text-ink/40" />
            </div>
          )}
        </section>
        <p className="mt-4 text-center text-small text-ink/55">
          遇到问题？
          <button
            type="button"
            className="font-semibold text-ink/75 hover:text-ink"
            onClick={() => setContactOpen(true)}
          >
            联系我们
          </button>
        </p>
      </motion.div>
    </div>
  );
}

/** While Google signs in, in 兔啃's small window or in the browser. */
function GoogleWait() {
  const login = useLogin();
  const inWindow = login.google === "window";
  return (
    <div className="flex flex-col items-center text-center">
      <span className="flex h-12 w-12 items-center justify-center rounded-full bg-glass/70 shadow-[inset_0_0_0_1px_rgb(var(--ink)/0.08)]">
        <GoogleLogo size={22} />
      </span>
      <p className="mt-4 flex items-center gap-2 text-strong font-semibold">
        <Loader2 size={15} className="animate-spin text-ink/50" />
        {inWindow ? "在弹出的窗口里登录 Google" : "在浏览器里登录 Google"}
      </p>
      <p className="mt-1 text-small text-ink/60">登录好会自动回到兔啃</p>
      <div className="mt-5 flex gap-2">
        {inWindow && (
          <button
            type="button"
            className="btn btn-glass btn-sm"
            onClick={login.googleInBrowser}
          >
            改用浏览器
          </button>
        )}
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={login.cancelGoogle}
        >
          取消
        </button>
      </div>
    </div>
  );
}

/** Until the server has the desktop sign-in API: the site's own pages. */
function SiteSignIn() {
  const login = useLogin();
  return (
    <div className="flex flex-col gap-2.5">
      <div className="grid grid-cols-2 gap-2.5">
        <button
          type="button"
          className="btn btn-primary btn-lg"
          disabled={login.busy}
          onClick={() => void login.start("window")}
        >
          登录
        </button>
        <button
          type="button"
          className="btn btn-glass btn-lg"
          disabled={login.busy}
          onClick={() => void login.start("register")}
        >
          注册
        </button>
      </div>
      <button
        type="button"
        className="btn btn-glass btn-lg w-full"
        disabled={login.busy}
        onClick={() => void login.startGoogle()}
      >
        <GoogleLogo />
        使用 Google 登录
      </button>
    </div>
  );
}

/** While another account signs in with Google, a pill at the bottom. */
export function LoginBanner() {
  const login = useLogin();
  if (!login.google) return null;
  const inWindow = login.google === "window";
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-6 z-30 flex justify-center">
      <motion.div
        initial={{ opacity: 0, y: 16 }}
        animate={{ opacity: 1, y: 0 }}
        className="glass pointer-events-auto flex items-center gap-3 rounded-full py-2 pl-5 pr-2 text-small"
      >
        <Loader2 size={14} className="animate-spin text-cyan-ink" />
        <span>
          {inWindow
            ? "在弹出的窗口里用 Google 登录另一个账户"
            : "在浏览器里用 Google 登录另一个账户"}
        </span>
        {inWindow && (
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={login.googleInBrowser}
          >
            改用浏览器
          </button>
        )}
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={login.cancelGoogle}
        >
          取消
        </button>
      </motion.div>
    </div>
  );
}
