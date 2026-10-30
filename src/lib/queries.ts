import {
  focusManager,
  keepPreviousData,
  QueryCache,
  QueryClient,
  useQuery,
  type QueryClient as Client,
} from "@tanstack/react-query";
import { toast } from "sonner";
import {
  autostart,
  desktop,
  EXPIRED,
  tuken,
  type AppState,
  type Announcements,
  type AuthConfig,
  type Checkin,
  type CheckinHistory,
  type Credit,
  type Invite,
  type SiteInfo,
  type Update,
  type Catalog,
  type CreditRow,
  type Dashboard,
  type Group,
  type Key,
  type Models,
  type Page,
  type Profile,
  type Range,
  type UsageRow,
  type UsageStats,
} from "./api";
import { beijingDate } from "./format";

// Coming back to the window counts as coming back, not only un-hiding it:
// switching from another app to 兔啃 refreshes what is out of date.
focusManager.setEventListener((handleFocus) => {
  const visibility = () => handleFocus(document.visibilityState === "visible");
  const focus = () => handleFocus(true);
  const blur = () => handleFocus(false);
  window.addEventListener("visibilitychange", visibility);
  window.addEventListener("focus", focus);
  window.addEventListener("blur", blur);
  return () => {
    window.removeEventListener("visibilitychange", visibility);
    window.removeEventListener("focus", focus);
    window.removeEventListener("blur", blur);
  };
});

export const queryClient: QueryClient = new QueryClient({
  queryCache: new QueryCache({
    // The native side has already dropped an account whose token expired;
    // show the account list as it is now and say why.
    onError: (e) => {
      if (String(e).startsWith(EXPIRED)) {
        void queryClient.invalidateQueries({ queryKey: qk.state });
        toast.error("这个账户的登录已过期，请重新登录", { id: "expired" });
      }
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 7 * 24 * 3600_000,
      retry: (n, e) => n < 1 && !String(e).startsWith(EXPIRED),
      refetchOnWindowFocus: true,
    },
  },
});

/**
 * Per-account data lives under ["u", userId, …], so switching accounts shows
 * that account's cached pages at once instead of reloading everything.
 */
export const qk = {
  state: ["state"] as const,
  desktop: ["desktop"] as const,
  autostart: ["autostart"] as const,
  authConfig: ["auth_config"] as const,
  site: ["site"] as const,
  update: ["update"] as const,
  announcements: (uid: number) => ["u", uid, "announcements"] as const,
  checkin: (uid: number) => ["u", uid, "checkin"] as const,
  credit: (uid: number) => ["u", uid, "credit"] as const,
  invite: (uid: number) => ["u", uid, "invite"] as const,
  checkins: (uid: number, month: string) =>
    ["u", uid, "checkins", month] as const,
  catalog: ["catalog"] as const,
  quote: (goods: string, channel: number) => ["quote", goods, channel] as const,
  user: (uid: number) => ["u", uid] as const,
  profile: (uid: number) => ["u", uid, "profile"] as const,
  dashboard: (uid: number) => ["u", uid, "dashboard"] as const,
  keys: (uid: number) => ["u", uid, "keys"] as const,
  groups: (uid: number) => ["u", uid, "groups"] as const,
  models: (uid: number, gid: number) => ["u", uid, "models", gid] as const,
  usage: (uid: number, r: Range, page: number) =>
    ["u", uid, "usage", r.start_date, r.end_date, page] as const,
  usageStats: (uid: number, r: Range) =>
    ["u", uid, "usage_stats", r.start_date, r.end_date] as const,
  history: (uid: number, page: number) => ["u", uid, "history", page] as const,
};

/** The account's own call: refused natively if another account became current. */
const as = <T>(uid: number, action: string, input?: Record<string, unknown>) =>
  tuken<T>(action, { ...input, as_user: uid });

export const fetchers = {
  profile: (uid: number) => as<Profile>(uid, "profile"),
  dashboard: (uid: number) => as<Dashboard>(uid, "dashboard"),
  keys: (uid: number) => as<Key[]>(uid, "keys"),
  groups: (uid: number) => as<Group[]>(uid, "groups"),
  models: (uid: number, gid: number) =>
    as<Models>(uid, "models", { group_id: gid }),
  usage: (uid: number, r: Range, page: number) =>
    as<Page<UsageRow>>(uid, "usage", { ...r, page }),
  usageStats: (uid: number, r: Range) => as<UsageStats>(uid, "usage_stats", r),
  history: (uid: number, page: number) =>
    as<Page<CreditRow>>(uid, "recharge_history", { page }),
};

export const defaultRange = (): Range => ({
  start_date: beijingDate(6),
  end_date: beijingDate(0),
});

export function useAppState() {
  return useQuery({
    queryKey: qk.state,
    queryFn: () => tuken<AppState>("state"),
    staleTime: 5_000,
  });
}

/**
 * How accounts sign in: natively, once the server has the desktop sign-in
 * API, else on the site. Asked again now and then until it is there.
 */
export function useAuthConfig() {
  return useQuery({
    queryKey: qk.authConfig,
    queryFn: () => tuken<AuthConfig>("auth_config"),
    staleTime: 30_000,
    refetchInterval: (q) => (q.state.data?.available ? false : 60_000),
  });
}

/** A newer 兔啃: looked for at start and every few hours. */
export function useUpdate() {
  return useQuery({
    queryKey: qk.update,
    queryFn: () => tuken<Update>("update_check"),
    staleTime: 6 * 3600_000,
    refetchInterval: 6 * 3600_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

/** Contact details, also before signing in. */
export function useSite() {
  return useQuery({
    queryKey: qk.site,
    queryFn: () => tuken<SiteInfo>("site"),
    staleTime: 0,
  });
}

export function useAnnouncements(uid: number | null) {
  return useQuery({
    queryKey: qk.announcements(uid ?? 0),
    queryFn: () => tuken<Announcements>("announcements", { as_user: uid }),
    enabled: !!uid,
    // A new announcement shows within a minute, also while the window is
    // in the background, and at once on coming back to it.
    staleTime: 0,
    refetchInterval: 60_000,
    refetchIntervalInBackground: true,
  });
}

export function useCheckin(uid: number | null) {
  return useQuery({
    queryKey: qk.checkin(uid ?? 0),
    queryFn: () => tuken<Checkin>("checkin_status", { as_user: uid }),
    enabled: !!uid,
    staleTime: 60_000,
  });
}

/** The balance's time-limited part, and whether a recharge earns a bonus. */
export function useCredit(uid: number | null) {
  return useQuery({
    queryKey: qk.credit(uid ?? 0),
    queryFn: () => tuken<Credit>("credit", { as_user: uid }),
    enabled: !!uid,
    staleTime: 60_000,
  });
}

export function useInvite(uid: number | null) {
  return useQuery({
    queryKey: qk.invite(uid ?? 0),
    queryFn: () => tuken<Invite>("invite", { as_user: uid }),
    enabled: !!uid,
    staleTime: 60_000,
  });
}

/** `month` is `YYYY-MM`; null while the calendar is closed. */
export function useCheckinHistory(uid: number | null, month: string | null) {
  return useQuery({
    queryKey: qk.checkins(uid ?? 0, month ?? ""),
    queryFn: () =>
      tuken<CheckinHistory>("checkin_history", { month, as_user: uid }),
    enabled: !!uid && !!month,
    staleTime: 60_000,
  });
}

export function useDesktop() {
  return useQuery({
    queryKey: qk.desktop,
    queryFn: desktop.status,
    staleTime: 5_000,
    refetchInterval: (q) => (q.state.data?.installing ? 3_000 : 20_000),
    // An install finishing while the user is elsewhere still shows.
    refetchIntervalInBackground: true,
  });
}

/** Whether 兔啃 starts at sign-in; Task Manager can switch it off too. */
export function useAutostart() {
  return useQuery({
    queryKey: qk.autostart,
    queryFn: autostart.get,
    staleTime: 30_000,
  });
}

export function useProfile(uid: number | null) {
  return useQuery({
    queryKey: qk.profile(uid ?? 0),
    queryFn: () => fetchers.profile(uid!),
    enabled: !!uid,
    staleTime: 15_000,
    refetchInterval: 60_000,
  });
}

export function useDashboard(uid: number | null) {
  return useQuery({
    queryKey: qk.dashboard(uid ?? 0),
    queryFn: () => fetchers.dashboard(uid!),
    enabled: !!uid,
    refetchInterval: 60_000,
  });
}

export function useKeys(uid: number | null) {
  return useQuery({
    queryKey: qk.keys(uid ?? 0),
    queryFn: () => fetchers.keys(uid!),
    enabled: !!uid,
  });
}

export function useGroups(uid: number | null) {
  return useQuery({
    queryKey: qk.groups(uid ?? 0),
    queryFn: () => fetchers.groups(uid!),
    enabled: !!uid,
    staleTime: 5 * 60_000,
  });
}

export function useModels(uid: number | null, gid: number | null) {
  return useQuery({
    queryKey: qk.models(uid ?? 0, gid ?? 0),
    queryFn: () => fetchers.models(uid!, gid!),
    enabled: !!uid && !!gid,
    staleTime: 10 * 60_000,
  });
}

export function useUsage(uid: number | null, r: Range, page: number) {
  return useQuery({
    queryKey: qk.usage(uid ?? 0, r, page),
    queryFn: () => fetchers.usage(uid!, r, page),
    enabled: !!uid,
    placeholderData: keepPreviousData,
  });
}

export function useUsageStats(uid: number | null, r: Range) {
  return useQuery({
    queryKey: qk.usageStats(uid ?? 0, r),
    queryFn: () => fetchers.usageStats(uid!, r),
    enabled: !!uid,
    placeholderData: keepPreviousData,
  });
}

export function useHistory(uid: number | null, page: number) {
  return useQuery({
    queryKey: qk.history(uid ?? 0, page),
    queryFn: () => fetchers.history(uid!, page),
    enabled: !!uid,
    placeholderData: keepPreviousData,
  });
}

export function useCatalog(enabled = true) {
  return useQuery({
    queryKey: qk.catalog,
    queryFn: () => tuken<Catalog>("catalog"),
    staleTime: 5 * 60_000,
    enabled,
  });
}

export function useQuote(goods: string | null, channel: number | null) {
  return useQuery({
    queryKey: qk.quote(goods ?? "", channel ?? 0),
    queryFn: () =>
      tuken<{ total_amount: number }>("quote", {
        goods_id: goods,
        channel_id: channel,
      }),
    enabled: !!goods && channel !== null,
    staleTime: 2 * 60_000,
  });
}

/** Loads an account's pages in the background so opening them is instant. */
export function prefetchAccount(client: Client, uid: number) {
  const r = defaultRange();
  const jobs: [readonly unknown[], () => Promise<unknown>][] = [
    [qk.profile(uid), () => fetchers.profile(uid)],
    [qk.dashboard(uid), () => fetchers.dashboard(uid)],
    [qk.keys(uid), () => fetchers.keys(uid)],
    [qk.groups(uid), () => fetchers.groups(uid)],
    [qk.usage(uid, r, 1), () => fetchers.usage(uid, r, 1)],
    [qk.usageStats(uid, r), () => fetchers.usageStats(uid, r)],
    [qk.history(uid, 1), () => fetchers.history(uid, 1)],
    [qk.catalog, () => tuken<Catalog>("catalog")],
  ];
  for (const [queryKey, queryFn] of jobs) {
    void client.prefetchQuery({ queryKey, queryFn, staleTime: 30_000 });
  }
}

/** After money moved: balance, today's spend and the credit list. */
export function refreshCredit(client: Client, uid: number | null) {
  void client.invalidateQueries({ queryKey: qk.state });
  if (!uid) return;
  // Model lists too: the gateway lists models only once there is credit.
  for (const key of [
    qk.profile(uid),
    qk.dashboard(uid),
    qk.credit(uid),
    ["u", uid, "history"],
    ["u", uid, "models"],
  ])
    void client.invalidateQueries({ queryKey: key });
}
