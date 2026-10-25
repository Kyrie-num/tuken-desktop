import { useRef } from "react";
import { Sheet } from "@/components/Sheet";
import { useLogin, type AuthMode } from "@/app/context";
import { AuthForm } from "./AuthForm";

const titles: Record<AuthMode, string> = {
  login: "添加账户",
  register: "添加账户",
  reset: "重设密码",
  totp: "两步验证",
};

/** Signing in another account, without leaving the current one's pages. */
export function AuthSheet() {
  const login = useLogin();
  // Keeps the last screen on show while the sheet animates closed.
  const shown = useRef<AuthMode>("login");
  if (login.auth) shown.current = login.auth;
  const mode = shown.current;
  return (
    <Sheet
      open={!!login.auth}
      onOpenChange={(open) => !open && login.setAuth(null)}
      title={titles[mode]}
      width={420}
      focusField
    >
      <div className="pt-1">
        <AuthForm
          id="sheet"
          mode={mode}
          onMode={login.setAuth}
          onDone={() => login.setAuth(null)}
        />
      </div>
    </Sheet>
  );
}
