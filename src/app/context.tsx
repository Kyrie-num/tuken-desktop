import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  desktop,
  EXPIRED,
  tuken,
  type AppState,
  type AuthConfig,
} from "@/lib/api";
import {
  prefetchAccount,
  qk,
  useAppState,
  useAuthConfig,
  useDesktop,
  useProfile,
} from "@/lib/queries";

export type Page = "home" | "bills" | "models" | "profile";

/** The signed-in account, from the native vault. */
export function useSession() {
  const { data, isPending } = useAppState();
  const uid = data?.active ?? null;
  return {
    state: data,
    uid,
    account: data?.accounts.find((a) => a.user_id === uid),
    accounts: data?.accounts ?? [],
    loading: isPending,
  };
}

type Ui = {
  page: Page;
  go: (page: Page) => void;
  rechargeOpen: boolean;
  setRechargeOpen: (open: boolean) => void;
  createKey: { open: boolean; groupId?: number };
  openCreateKey: (groupId?: number) => void;
  closeCreateKey: () => void;
  restartOpen: boolean;
  setRestartOpen: (open: boolean) => void;
  /** Claude Desktop's default model and effort. */
  modelsOpen: boolean;
  setModelsOpen: (open: boolean) => void;
  contactOpen: boolean;
  setContactOpen: (open: boolean) => void;
  noticesOpen: boolean;
  setNoticesOpen: (open: boolean) => void;
  checkinOpen: boolean;
  setCheckinOpen: (open: boolean) => void;
  inviteOpen: boolean;
  setInviteOpen: (open: boolean) => void;
  /**
   * The signed-in account's first-run setup on this computer: still to
   * decide, on show (until finished, even once Claude Desktop is
   * configured), or done.
   */
  setup: "unknown" | "show" | "done";
  beginSetup: () => void;
  finishSetup: () => void;
  /** The step of the first-run guide on show, if any. */
  tour: number | null;
  startTour: () => void;
  moveTour: (step: number) => void;
  endTour: () => void;
};

const UiContext = createContext<Ui | null>(null);

/** Set once an account has seen or skipped the guide on this computer. */
export const tourSeen = (uid: number) => `tuken-tour-${uid}`;
/** Set once an account's setup on this computer is finished (or not needed). */
export const setupDone = (uid: number) => `tuken-setup-${uid}`;
/** Accounts made within this many days are new: they get the setup and the guide. */
export const NEW_ACCOUNT_DAYS = 7;

export const isNewAccount = (createdAt?: string | null) =>
  Date.now() - (createdAt ? Date.parse(createdAt) : NaN) <
  NEW_ACCOUNT_DAYS * 864e5;

/**
 * Whether `key` is marked done. 0.2.0 kept one mark per computer (`legacy`):
 * it counts for the first account asked about, and goes.
 */
function marked(key: string, legacy: string) {
  const old = localStorage.getItem(legacy);
  if (old !== null) {
    if (old === "done") localStorage.setItem(key, "done");
    localStorage.removeItem(legacy);
  }
  return localStorage.getItem(key) === "done";
}
export const tourDone = (uid: number) => marked(tourSeen(uid), "tuken-tour");
const setupMarked = (uid: number) => marked(setupDone(uid), "tuken-setup");

export function UiProvider({ children }: { children: ReactNode }) {
  const [page, go] = useState<Page>("home");
  const [rechargeOpen, setRechargeOpen] = useState(false);
  const [createKey, setCreateKey] = useState<{
    open: boolean;
    groupId?: number;
  }>({
    open: false,
  });
  const [restartOpen, setRestartOpen] = useState(false);
  const [modelsOpen, setModelsOpen] = useState(false);
  const [contactOpen, setContactOpen] = useState(false);
  const [noticesOpen, setNoticesOpen] = useState(false);
  const [checkinOpen, setCheckinOpen] = useState(false);
  const [inviteOpen, setInviteOpen] = useState(false);
  const [tour, setTour] = useState<number | null>(null);
  const { uid } = useSession();
  // Each account's setup as decided in this run; before that, its mark.
  const [setups, setSetups] = useState<Record<number, Ui["setup"]>>({});
  const setup: Ui["setup"] =
    uid === null
      ? "unknown"
      : (setups[uid] ?? (setupMarked(uid) ? "done" : "unknown"));
  const beginSetup = useCallback(() => {
    if (uid !== null) setSetups((s) => ({ ...s, [uid]: "show" }));
  }, [uid]);
  const finishSetup = useCallback(() => {
    if (uid === null) return;
    localStorage.setItem(setupDone(uid), "done");
    setSetups((s) => ({ ...s, [uid]: "done" }));
    go("home");
  }, [uid]);
  // Another account starts on its home page.
  useEffect(() => go("home"), [uid]);
  const startTour = useCallback(() => {
    // The guide points at the home page.
    go("home");
    setTour(0);
  }, []);
  const endTour = useCallback(() => {
    setTour(null);
    if (uid !== null) localStorage.setItem(tourSeen(uid), "done");
  }, [uid]);
  const value = useMemo<Ui>(
    () => ({
      page,
      go,
      rechargeOpen,
      setRechargeOpen,
      createKey,
      openCreateKey: (groupId) => setCreateKey({ open: true, groupId }),
      closeCreateKey: () => setCreateKey({ open: false }),
      restartOpen,
      setRestartOpen,
      modelsOpen,
      setModelsOpen,
      contactOpen,
      setContactOpen,
      noticesOpen,
      setNoticesOpen,
      checkinOpen,
      setCheckinOpen,
      inviteOpen,
      setInviteOpen,
      setup,
      beginSetup,
      finishSetup,
      tour,
      startTour,
      moveTour: setTour,
      endTour,
    }),
    [
      page,
      rechargeOpen,
      createKey,
      restartOpen,
      modelsOpen,
      contactOpen,
      noticesOpen,
      checkinOpen,
      inviteOpen,
      setup,
      beginSetup,
      finishSetup,
      tour,
      startTour,
      endTour,
    ],
  );
  return <UiContext.Provider value={value}>{children}</UiContext.Provider>;
}

/**
 * Whether the first-run setup shows: for a new account (made in the last
 * NEW_ACCOUNT_DAYS days), once on each computer it signs in on, where Claude
 * Desktop can be set up. Older accounts go straight to the home page.
 * Decided once per account and computer, and kept while it runs.
 */
export function useSetup() {
  const { uid } = useSession();
  const { setup, beginSetup, finishSetup } = useUi();
  const { data: profile } = useProfile(uid);
  const { data: status } = useDesktop();
  useEffect(() => {
    if (setup !== "unknown" || !uid || !profile) return;
    if (!isNewAccount(profile.created_at)) return finishSetup();
    if (!status) return;
    if (status.supported) beginSetup();
    else finishSetup();
  }, [setup, uid, profile, status, beginSetup, finishSetup]);
  return setup;
}

export function useUi() {
  const ui = useContext(UiContext);
  if (!ui) throw new Error("useUi outside UiProvider");
  return ui;
}

/** The site's sign-in or sign-up page, while the server has no native API. */
export type LoginMethod = "window" | "register";

/** Where a Google sign-in is happening: 兔啃's small window, or the browser. */
export type GoogleStep = "window" | "browser";

/** The screens of the native sign-in form. */
export type AuthMode = "login" | "register" | "reset" | "totp";

type Login = {
  /** The code to compare on the approval page, while waiting for it. */
  code: string;
  method: LoginMethod;
  busy: boolean;
  start: (method?: LoginMethod) => Promise<void>;
  cancel: () => void;
  /** The server's sign-up rules; native sign-in once `available`. */
  config: AuthConfig | undefined;
  /** The native form in a sheet, while adding another account. */
  auth: AuthMode | null;
  setAuth: (mode: AuthMode | null) => void;
  /** Signs in another account: the native form when possible, else the site. */
  add: () => void;
  /** A Google sign-in in progress, and where. */
  google: GoogleStep | null;
  /** `invite`: the code a new account signs up with, when sign-up needs one. */
  startGoogle: (invite?: string) => Promise<void>;
  /** A new Google account was turned away for want of an invitation code. */
  googleInvite: boolean;
  setGoogleInvite: (open: boolean) => void;
  cancelGoogle: () => void;
  /** Moves the Google sign-in from the small window to the browser. */
  googleInBrowser: () => void;
};

const LoginContext = createContext<Login | null>(null);

export function LoginProvider({ children }: { children: ReactNode }) {
  const client = useQueryClient();
  const { data: config } = useAuthConfig();
  const [code, setCode] = useState("");
  const [method, setMethod] = useState<LoginMethod>("window");
  const [busy, setBusy] = useState(false);
  const [auth, setAuth] = useState<AuthMode | null>(null);
  const [google, setGoogle] = useState<GoogleStep | null>(null);
  const [googleInvite, setGoogleInvite] = useState(false);

  const signedIn = useCallback(
    async (userId: number) => {
      const state = await tuken<AppState>("state");
      client.setQueryData(qk.state, state);
      prefetchAccount(client, userId);
      const me = state.accounts.find((a) => a.user_id === userId);
      toast.success(`已登录 ${me?.email ?? "兔啃"}`);
    },
    [client],
  );

  // Google in 兔啃's own small window; resolves once it is over.
  const startGoogle = useCallback(
    async (invite?: string) => {
      setGoogle("window");
      try {
        const r = await tuken<{ status: string; user_id?: number }>(
          "google_sign_in",
          { invite },
        );
        if (r.status === "approved" && r.user_id) await signedIn(r.user_id);
        if (r.status === "invite_required") setGoogleInvite(true);
      } catch (e) {
        toast.error(message(e));
      } finally {
        setGoogle(null);
      }
    },
    [signedIn],
  );

  const cancelGoogle = useCallback(() => {
    void tuken("google_cancel").catch(() => {});
  }, []);

  const googleInBrowser = useCallback(() => {
    void tuken("google_browser")
      .then(() => setGoogle("browser"))
      .catch((e) => toast.error(message(e)));
  }, []);

  // Google refused the small window and the browser took over by itself.
  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen("tuken://google", () => setGoogle("browser"))
      .then((u) => (stop = u))
      .catch(() => {});
    return () => stop?.();
  }, []);

  const start = useCallback(async (how: LoginMethod = "window") => {
    setBusy(true);
    try {
      const r = await tuken<{ user_code: string }>("login_start", {
        register: how === "register",
      });
      setMethod(how);
      setCode(r.user_code);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const cancel = useCallback(() => {
    setCode("");
    void tuken("login_cancel").catch(() => {});
  }, []);

  useEffect(() => {
    if (!code) return;
    let stopped = false;
    let running = false;
    const timer = window.setInterval(async () => {
      if (running) return;
      running = true;
      try {
        const r = await tuken<{ status: string; user_id?: number }>(
          "login_poll",
        );
        if (stopped || r.status !== "approved") return;
        setCode("");
        if (r.user_id) await signedIn(r.user_id);
      } catch (e) {
        if (!stopped) {
          setCode("");
          toast.error(String(e));
        }
      } finally {
        running = false;
      }
    }, 2500);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [code, signedIn]);

  const native = config?.available === true;
  const add = useCallback(() => {
    if (native) setAuth("login");
    else void start("window");
  }, [native, start]);

  const value = useMemo(
    () => ({
      code,
      method,
      busy,
      start,
      cancel,
      config,
      auth,
      setAuth,
      add,
      google,
      startGoogle,
      googleInvite,
      setGoogleInvite,
      cancelGoogle,
      googleInBrowser,
    }),
    [
      code,
      method,
      busy,
      start,
      cancel,
      config,
      auth,
      add,
      google,
      startGoogle,
      googleInvite,
      cancelGoogle,
      googleInBrowser,
    ],
  );
  return (
    <LoginContext.Provider value={value}>{children}</LoginContext.Provider>
  );
}

export function useLogin() {
  const login = useContext(LoginContext);
  if (!login) throw new Error("useLogin outside LoginProvider");
  return login;
}

/**
 * Pointing Claude Desktop or Claude Code at a key, with the follow-ups every
 * entry point shares: refreshed key badges, and for Claude Desktop, choosing
 * its default model before it opens.
 */
export function useConnect() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { setModelsOpen } = useUi();
  const { data: status } = useDesktop();
  const [busy, setBusy] = useState<"desktop" | "cli" | null>(null);

  const refresh = useCallback(async () => {
    const next = await desktop.status().catch(() => null);
    if (next) client.setQueryData(qk.desktop, next);
    if (uid) void client.invalidateQueries({ queryKey: qk.keys(uid) });
    return next;
  }, [client, uid]);

  const connect = useCallback(
    async (
      app: "desktop" | "cli",
      target: { keyId?: number; groupId?: number },
    ) => {
      setBusy(app);
      try {
        await tuken("connect", {
          app,
          id: target.keyId,
          group_id: target.groupId,
          as_user: uid,
        });
        await refresh();
        if (app === "cli") {
          toast.success("Claude Code 已接入兔啃，新开的终端会话生效");
        } else {
          setModelsOpen(true);
        }
        return true;
      } catch (e) {
        toast.error(String(e));
        return false;
      } finally {
        setBusy(null);
      }
    },
    [uid, refresh, setModelsOpen],
  );

  const disconnect = useCallback(
    async (app: "desktop" | "cli") => {
      setBusy(app);
      try {
        await tuken("disconnect", { app, as_user: uid });
        await refresh();
        toast.success(
          app === "cli"
            ? "Claude Code 已不再使用兔啃"
            : status?.running
              ? "已断开，重启 Claude Desktop 后恢复官方登录"
              : "已断开，Claude Desktop 会恢复官方登录",
        );
      } catch (e) {
        toast.error(String(e));
      } finally {
        setBusy(null);
      }
    },
    [uid, refresh, status?.running],
  );

  return { connect, disconnect, busy };
}

/** Errors of a call made as a signed-out account mean "sign in again". */
export function isExpired(e: unknown) {
  return String(e).startsWith(EXPIRED);
}

/** Native errors arrive as plain strings; anything else as an Error. */
export function message(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}

/** Runs an action with a toast for its error; resolves to whether it worked. */
export function useAction() {
  const ref = useRef(false);
  const [busy, setBusy] = useState(false);
  const run = useCallback(async (work: () => Promise<unknown>) => {
    if (ref.current) return false;
    ref.current = true;
    setBusy(true);
    try {
      await work();
      return true;
    } catch (e) {
      toast.error(message(e));
      return false;
    } finally {
      ref.current = false;
      setBusy(false);
    }
  }, []);
  return { busy, run };
}
