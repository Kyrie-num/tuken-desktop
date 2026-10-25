import { useEffect, useState } from "react";
import { Sheet } from "@/components/Sheet";
import { useLogin } from "@/app/context";

/**
 * A new Google account while sign-up is by invitation only: the code first,
 * then the same Google sign-in again.
 */
export function GoogleInviteSheet() {
  const login = useLogin();
  const [code, setCode] = useState("");

  useEffect(() => {
    if (login.googleInvite) setCode("");
  }, [login.googleInvite]);

  return (
    <Sheet
      open={login.googleInvite}
      onOpenChange={login.setGoogleInvite}
      title="填写邀请码"
      width={420}
      focusField
    >
      <form
        className="flex flex-col"
        onSubmit={(e) => {
          e.preventDefault();
          if (!code) return;
          login.setGoogleInvite(false);
          void login.startGoogle(code);
        }}
      >
        <p className="text-body text-ink/75">
          这个 Google 账号还没注册兔啃，现在注册需要邀请码。
        </p>
        <div className="field-group mt-4">
          <div className="field-row">
            <input
              aria-label="邀请码"
              spellCheck={false}
              maxLength={32}
              placeholder="邀请码"
              value={code}
              onChange={(e) => setCode(e.target.value.trim())}
            />
          </div>
        </div>
        <button
          type="submit"
          className="btn btn-primary btn-lg mt-5 w-full"
          disabled={!code}
        >
          继续用 Google 注册
        </button>
      </form>
    </Sheet>
  );
}
