import { Loader2 } from "lucide-react";
import { Sheet } from "@/components/Sheet";
import { WebPanel } from "@/components/WebPanel";
import { useLogin } from "@/app/context";

/**
 * Email sign-in and sign-up, inside the app: the site's page in a glass
 * sheet. Google can't be embedded (Google refuses it), so it uses the browser.
 */
export function LoginSheet() {
  const login = useLogin();
  const open = !!login.code;
  return (
    <Sheet
      open={open}
      onOpenChange={(next) => !next && login.cancel()}
      title={login.method === "register" ? "注册兔啃" : "登录兔啃"}
      width={480}
    >
      {open && (
        <WebPanel className="h-[min(520px,calc(86vh-160px))] w-full">
          <Loader2 size={20} className="animate-spin text-ink/40" />
        </WebPanel>
      )}
      <p className="mt-3 text-center text-caption text-ink/55">
        最后一步点「确认授权当前账户」，验证码{" "}
        <span className="selectable font-mono font-semibold tracking-[0.12em] text-ink/80">
          {login.code}
        </span>
      </p>
    </Sheet>
  );
}
