import { useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  Compass,
  MessageCircle,
  UserPlus,
  LogOut,
  Moon,
  Plus,
  RefreshCw,
  UserRound,
} from "lucide-react";
import { toast } from "sonner";
import {
  MenuContent,
  MenuItem,
  MenuLabel,
  MenuRadio,
  MenuRadioGroup,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
} from "@/components/Menu";
import { tuken, type AppState } from "@/lib/api";
import { initial } from "@/lib/format";
import { prefetchAccount, qk, useInvite, useUpdate } from "@/lib/queries";
import { useTheme, type Theme } from "@/lib/theme";
import { message, useLogin, useSession, useUi } from "@/app/context";
import { installUpdate } from "@/features/update/Update";

export function Avatar({ text, size = 28 }: { text?: string; size?: number }) {
  return (
    <span
      aria-hidden="true"
      className="flex shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-[#FF9A45] to-orange font-semibold text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.5)]"
      style={{ width: size, height: size, fontSize: size * 0.45 }}
    >
      {initial(text)}
    </span>
  );
}

/** The account pill: switch, add and sign out accounts, profile and theme. */
export function AccountMenu() {
  const client = useQueryClient();
  const { account, accounts, uid } = useSession();
  const { go, startTour, setContactOpen, setInviteOpen } = useUi();
  const invite = useInvite(uid);
  const inviting = invite.data?.supported && invite.data.enabled;
  const login = useLogin();
  const update = useUpdate();
  const [theme, setTheme] = useTheme();

  async function switchTo(userId: number) {
    if (userId === uid) return;
    try {
      const state = await tuken<AppState>("switch", { user_id: userId });
      client.setQueryData(qk.state, state);
      prefetchAccount(client, userId);
      const a = state.accounts.find((x) => x.user_id === userId);
      toast.success(`已切换到 ${a?.email ?? "这个账户"}`);
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function checkUpdate() {
    if (update.data) return installUpdate(update.data.version);
    const r = await update.refetch();
    // A newer version gets its own offer from UpdateNotice.
    if (r.error) toast.error(message(r.error));
    else if (!r.data) toast.success(`已经是最新版本 ${__APP_VERSION__}`);
  }

  async function signOut() {
    if (!uid) return;
    try {
      const state = await tuken<AppState>("logout", { user_id: uid });
      client.setQueryData(qk.state, state);
      client.removeQueries({ queryKey: qk.user(uid) });
      toast.success("已退出这个账户");
    } catch (e) {
      toast.error(String(e));
    }
  }

  return (
    <MenuRoot>
      <MenuTrigger asChild>
        <button
          type="button"
          className="btn btn-glass btn-sm h-9 max-w-[240px] gap-2 pl-1 pr-3"
          aria-label="账户"
          data-tour="account"
        >
          <Avatar text={account?.username || account?.email} />
          <span className="truncate text-small font-semibold">
            {account?.username || account?.email || "账户"}
          </span>
          <ChevronDown size={14} className="shrink-0 opacity-60" />
        </button>
      </MenuTrigger>
      <MenuContent className="w-[280px]">
        <MenuLabel>账户</MenuLabel>
        {accounts.map((a) => (
          <MenuItem key={a.user_id} onSelect={() => void switchTo(a.user_id)}>
            <Avatar text={a.username || a.email} size={24} />
            <span className="min-w-0 flex-1 truncate">
              {a.email || a.username}
            </span>
            {a.user_id === uid && <Check size={15} className="text-cyan-ink" />}
          </MenuItem>
        ))}
        <MenuItem
          onSelect={login.add}
          disabled={login.busy || !!login.code || !!login.google}
        >
          <span className="flex h-6 w-6 items-center justify-center rounded-full bg-ink/[0.07]">
            <Plus size={14} />
          </span>
          添加账户
        </MenuItem>
        <MenuSeparator />
        <MenuItem onSelect={() => go("profile")}>
          <UserRound size={16} className="opacity-70" />
          个人资料
        </MenuItem>
        <MenuItem onSelect={startTour}>
          <Compass size={16} className="opacity-70" />
          新手指引
        </MenuItem>
        {inviting && (
          <MenuItem onSelect={() => setInviteOpen(true)}>
            <UserPlus size={16} className="opacity-70" />
            邀请好友
          </MenuItem>
        )}
        <MenuItem onSelect={() => setContactOpen(true)}>
          <MessageCircle size={16} className="opacity-70" />
          联系我们
        </MenuItem>
        <MenuItem onSelect={() => void checkUpdate()}>
          <RefreshCw size={16} className="opacity-70" />
          <span className="flex-1">
            {update.data ? `更新到 ${update.data.version}` : "检查更新"}
          </span>
          <span className="text-caption text-ink/45 tnum">
            {__APP_VERSION__}
          </span>
        </MenuItem>
        <MenuLabel>
          <span className="flex items-center gap-1.5">
            <Moon size={12} />
            外观
          </span>
        </MenuLabel>
        <MenuRadioGroup
          value={theme}
          onValueChange={(v) => setTheme(v as Theme)}
        >
          <MenuRadio value="light">浅色</MenuRadio>
          <MenuRadio value="dark">深色</MenuRadio>
          <MenuRadio value="system">跟随系统</MenuRadio>
        </MenuRadioGroup>
        <MenuSeparator />
        <MenuItem onSelect={() => void signOut()} danger>
          <LogOut size={16} />
          退出当前账户
        </MenuItem>
      </MenuContent>
    </MenuRoot>
  );
}
