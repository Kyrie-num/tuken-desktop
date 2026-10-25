import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, Loader2, LogOut } from "lucide-react";
import { toast } from "sonner";
import { tuken, type AppState, type Profile as ProfileData } from "@/lib/api";
import { day, usd } from "@/lib/format";
import { qk, useProfile } from "@/lib/queries";
import { Avatar } from "./AccountMenu";
import { useAction, useSession, useUi } from "@/app/context";

export function Profile() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { go } = useUi();
  const profile = useProfile(uid);
  const [name, setName] = useState("");
  const [oldPassword, setOldPassword] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const saving = useAction();
  const changing = useAction();
  const p = profile.data;

  useEffect(() => setName(p?.username ?? ""), [p?.username]);

  const saveName = () =>
    saving.run(async () => {
      const next = await tuken<ProfileData>("profile_update", {
        username: name.trim(),
        as_user: uid,
      });
      client.setQueryData(qk.profile(uid!), next);
      void client.invalidateQueries({ queryKey: qk.state });
      toast.success("昵称已保存");
    });

  const changePassword = () => {
    if (password !== confirm) {
      toast.error("两次输入的新密码不一样");
      return;
    }
    void changing.run(async () => {
      const state = await tuken<AppState>("password_change", {
        old_password: oldPassword,
        new_password: password,
        as_user: uid,
      });
      client.setQueryData(qk.state, state);
      client.removeQueries({ queryKey: qk.user(uid!) });
      toast.success("密码已修改，请用新密码重新登录");
    });
  };

  const signOut = async () => {
    try {
      const state = await tuken<AppState>("logout", { user_id: uid });
      client.setQueryData(qk.state, state);
      client.removeQueries({ queryKey: qk.user(uid!) });
      toast.success("已退出这个账户");
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="mx-auto w-full max-w-[640px] px-6 pb-8">
      <button
        type="button"
        className="btn btn-ghost btn-sm -ml-2 mb-3"
        onClick={() => go("home")}
      >
        <ChevronLeft size={16} />
        返回首页
      </button>

      <section className="glass flex items-center gap-5 p-6">
        <Avatar text={p?.username || p?.email} size={64} />
        <div className="min-w-0 flex-1">
          <p className="truncate font-display text-title font-semibold">
            {p?.username || "未设置昵称"}
          </p>
          <p className="selectable truncate text-body text-ink/65">
            {p?.email ?? "…"}
          </p>
          <p className="mt-1 text-caption text-ink/50">
            账户 #{p?.id ?? "—"}
            {p?.created_at ? ` · ${day(p.created_at)}注册` : ""}
            {p?.total_recharged ? ` · 累计充值 ${usd(p.total_recharged)}` : ""}
          </p>
        </div>
      </section>

      <section className="glass mt-4 p-6">
        <h2 className="text-section font-semibold">昵称</h2>
        <form
          className="mt-3 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void saveName();
          }}
        >
          <input
            aria-label="昵称"
            className="field"
            value={name}
            maxLength={50}
            placeholder="给自己起个名字"
            onChange={(e) => setName(e.target.value)}
          />
          <button
            type="submit"
            className="btn btn-glass h-10 shrink-0"
            disabled={saving.busy || name.trim() === (p?.username ?? "")}
          >
            {saving.busy && <Loader2 size={14} className="animate-spin" />}
            保存
          </button>
        </form>
      </section>

      <section className="glass mt-4 p-6">
        <h2 className="text-section font-semibold">修改密码</h2>
        <p className="mt-1 text-caption text-ink/55">
          改完需要重新登录。用 Google
          注册、还没设过密码的账户，请在登录窗口里点「忘记密码」设置。
        </p>
        <form
          className="mt-4 space-y-2.5"
          onSubmit={(e) => {
            e.preventDefault();
            changePassword();
          }}
        >
          <input
            aria-label="原密码"
            type="password"
            autoComplete="current-password"
            className="field"
            placeholder="原密码"
            required
            value={oldPassword}
            onChange={(e) => setOldPassword(e.target.value)}
          />
          <input
            aria-label="新密码"
            type="password"
            autoComplete="new-password"
            className="field"
            placeholder="新密码，至少 8 位"
            required
            minLength={8}
            maxLength={72}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <input
            aria-label="确认新密码"
            type="password"
            autoComplete="new-password"
            className="field"
            placeholder="再输一次新密码"
            required
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
          />
          <div className="flex justify-end pt-1">
            <button
              type="submit"
              className="btn btn-glass"
              disabled={changing.busy}
            >
              {changing.busy && <Loader2 size={14} className="animate-spin" />}
              修改密码
            </button>
          </div>
        </form>
      </section>

      <button
        type="button"
        className="btn btn-ghost mt-4 text-danger"
        onClick={() => void signOut()}
      >
        <LogOut size={16} />
        退出这个账户
      </button>
    </div>
  );
}
