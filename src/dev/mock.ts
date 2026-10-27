/**
 * A stand-in for the native side with believable data, for the design preview
 * (preview.html) and the UI tests. Every account, key and order is fake.
 */
/* eslint-disable @typescript-eslint/no-explicit-any */
export type Scenario = "out" | "in" | "ok";

/** `YYYY-MM-DD` in Beijing, `n` days ago. */
const beijingDay = (n: number) =>
  new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Shanghai" }).format(
    new Date(Date.now() - n * 864e5),
  );

const hoursAgo = (h: number) =>
  new Date(Date.now() - h * 3600_000).toISOString();
const LONG_AGO = "2026-09-01T08:00:00Z";

/** The mock's sign-in: this password for every account, these codes. */
export const MOCK_PASSWORD = "tuken-demo";
export const MOCK_EMAIL_CODE = "246810";
export const MOCK_TOTP = "135790";

export function createMock(
  scenario: Scenario,
  {
    delay = 260,
    approveAfter = 2,
    paidAfter = 2,
    nativeAuth = true,
    /** How long the Google window takes to sign in, in ms. */
    googleAfter = 1500,
    /** The first account starts without credit. */
    noCredit = false,
    /** An unread announcement that should open by itself. */
    urgentNotice = false,
    /** A newer 兔啃 on GitHub. */
    update = "",
    /** Google sign-in, as switched in the admin. */
    google = true,
    /** A computer without Claude Desktop yet. */
    noDesktop = false,
    /** Sign-up only with an invitation code, Google included. */
    inviteOnly = false,
    /** Daily check-in: on, off in the admin, or not on the server yet. */
    checkin = "on" as "on" | "off" | "none",
    /** The system the app runs on. */
    platform = "windows" as "windows" | "macos",
    /** The first account was made long ago, not just now. */
    oldAccount = false,
    /** Installing waits for `mock:release_install`, so a test can send progress. */
    holdInstall = false,
    /** How installing Claude Desktop ends: as usual, in the browser, or with an error. */
    installEnds = "done" as "done" | "browser" | { error: string },
    /** macOS: the keychain keeps the accounts shut until 重试. */
    vaultLocked = "",
    /** The accounts couldn't be read and were set aside: told once. */
    vaultNotice = "",
  } = {},
) {
  let locked = vaultLocked;
  let notice = vaultNotice;
  let releaseInstall: (() => void) | undefined;
  /** Who listens to which event: [event, callback id]. */
  const listeners: [string, number][] = [];
  const wait = (ms = delay) =>
    ms ? new Promise((r) => setTimeout(r, ms)) : Promise.resolve();
  const accounts = [
    {
      user_id: 1,
      email: "xiaoming@mail.sustech.edu.cn",
      username: "小明",
      balance: noCredit ? 0 : 28.5,
      // Signed up just now (the bonus below), unless it is an old account.
      created_at: oldAccount ? LONG_AGO : hoursAgo(0.2),
    },
    {
      user_id: 2,
      email: "lab-share@example.test",
      username: "",
      balance: 3.2,
      created_at: LONG_AGO,
    },
  ];
  /** Accounts the server knows; the second one has two-step sign-in on. */
  const known = accounts.slice();
  const db = {
    accounts: scenario === "out" ? [] : accounts.slice(),
    active: scenario === "out" ? null : (1 as number | null),
    orders: [] as any[],
    /** Credit history, newest first: a sign-up bonus, an invite reward… */
    credits: [
      {
        id: 6,
        type: "signup",
        value: 50,
        status: "used",
        used_at: hoursAgo(0.2),
        created_at: hoursAgo(0.2),
      },
      ...[
        {
          id: 5,
          type: "invite",
          value: 2,
          status: "used",
          used_at: hoursAgo(3),
          created_at: hoursAgo(3),
        },
        {
          id: 4,
          type: "checkin",
          value: 0.12,
          status: "used",
          used_at: hoursAgo(20),
          created_at: hoursAgo(20),
        },
        {
          id: 3,
          type: "balance",
          value: 10,
          status: "used",
          used_at: hoursAgo(26),
          created_at: hoursAgo(26),
        },
        {
          id: 2,
          type: "balance",
          value: 20,
          status: "used",
          used_at: hoursAgo(240),
          created_at: hoursAgo(240),
        },
        {
          id: 1,
          type: "balance",
          value: 20,
          status: "used",
          used_at: hoursAgo(500),
          created_at: hoursAgo(500),
        },
      ],
    ],
    loginPolls: -1,
    payPolls: 0,
    codeFor: "",
    totpFor: 0,
    desktop: {
      supported: true,
      installed: scenario !== "out" && !noDesktop,
      version: "1.3.12",
      running: true,
      installing: false,
      can_restart: true,
      connected: scenario === "ok",
      cli_connected: false,
      models:
        scenario === "ok"
          ? {
              models: [
                "claude-sonnet-4-5",
                "claude-opus-4-1",
                "claude-haiku-4-5",
              ],
              effort: "high",
            }
          : (null as null | { models: string[]; effort: string | null }),
    },
    google: null as null | ((v: unknown) => void),
    checkedIn: false,
    notices: [
      {
        id: 2,
        title: "Claude Opus 5.5 上线了",
        content:
          "在首页「选择模型」里就能选。\n\n- **推理程度**选「高」效果最好\n- 详情看 https://www.tuken.site/docs",
        popup: urgentNotice,
        read: false,
        created_at: hoursAgo(5),
      },
      {
        id: 1,
        title: "周六凌晨维护",
        content: "10 月 26 日 02:00 到 03:00 维护，期间可能用不了。",
        popup: false,
        read: true,
        created_at: hoursAgo(50),
      },
    ],
    keys: [
      {
        id: 7,
        name: "Claude Desktop",
        masked: "sk-prev••••••••••••••-key",
        status: "active",
        group_id: 14,
        group: { id: 14, name: "Claude 通道", platform: "anthropic" },
        last_used_at: hoursAgo(0.3),
        in_desktop: scenario === "ok",
        in_cli: false,
      },
      {
        id: 8,
        name: "Codex 实验",
        masked: "sk-test••••••••••••••-key",
        status: "inactive",
        group_id: 21,
        group: { id: 21, name: "GPT 通道", platform: "openai" },
        last_used_at: null,
        in_desktop: false,
        in_cli: false,
      },
    ] as any[],
  };
  const groups = [
    {
      id: 14,
      name: "Claude 通道",
      platform: "anthropic",
      rate_multiplier: 1,
      description: "官方 Claude 模型，稳定",
    },
    {
      id: 30,
      name: "Claude 省钱通道",
      platform: "anthropic",
      rate_multiplier: 0.6,
      description: "高峰期可能排队",
    },
    {
      id: 21,
      name: "GPT 通道",
      platform: "openai",
      rate_multiplier: 0.8,
      description: "GPT-5 系列",
    },
  ];
  const models: Record<number, string[]> = {
    14: ["claude-opus-5-5", "claude-sonnet-4-5", "claude-haiku-4-5"],
    30: ["claude-sonnet-4-5", "claude-haiku-4-5"],
    21: ["gpt-5", "gpt-5-mini", "gpt-5-codex"],
  };
  const usage = Array.from({ length: 27 }, (_, i) => {
    const model = ["claude-sonnet-4-5", "claude-opus-4-1", "claude-haiku-4-5"][
      i % 3
    ];
    const input = 1800 + ((i * 7919) % 42000);
    const output = 300 + ((i * 104729) % 6000);
    return {
      id: 1000 - i,
      model,
      api_key_id: 7,
      key_name: "Claude Desktop",
      input_tokens: input,
      output_tokens: output,
      cache_creation_tokens: i % 4 ? 0 : 5200,
      cache_read_tokens: i % 2 ? 18000 + i * 300 : 0,
      actual_cost: Number(((input * 3 + output * 15) / 1e6).toFixed(6)),
      created_at: hoursAgo(i * 4.5 + 0.2),
    };
  });
  const me = () => db.accounts.find((a) => a.user_id === db.active)!;
  const view = () => ({
    active: db.active,
    accounts: db.accounts.map(({ user_id, email, username }) => ({
      user_id,
      email,
      username,
    })),
    orders: db.orders
      .filter((o) => o.owner === db.active)
      .map(({ owner: _, ...o }) => o),
  });
  const page = <T>(items: T[], n: number) => ({
    items: items.slice((n - 1) * 20, n * 20),
    total: items.length,
    page: n,
    pages: Math.max(1, Math.ceil(items.length / 20)),
  });
  const goods = [1, 5, 10, 20, 30, 50, 100, 200, 300, 500].map((n) => ({
    id: `g${n}`,
    name: `${n}刀额度`,
    price: n,
    stock: n === 300 ? 0 : 99,
  }));
  const price = (id: unknown) =>
    Math.round((goods.find((g) => g.id === id)?.price ?? 0) * 101.8) / 100;

  const refused = (reason: string, message: string) => ({
    status: "refused",
    reason,
    message,
  });
  const issue = (a: (typeof accounts)[number]) => {
    if (!db.accounts.includes(a)) db.accounts.push(a);
    db.active = a.user_id;
    return { status: "approved", user_id: a.user_id };
  };
  const emailOf = (input: any) => {
    const e = String(input.email ?? "")
      .trim()
      .toLowerCase();
    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(e)) throw "请输入正确的邮箱";
    return e;
  };

  async function auth(action: string, input: any): Promise<unknown> {
    if (!nativeAuth) throw "不支持的操作";
    switch (action) {
      case "auth_config":
        return {
          available: true,
          registration_enabled: true,
          invitation_code_enabled: false,
          password_reset_enabled: true,
          password_min_length: 6,
          email_suffixes: [],
          invite_enabled: true,
          invite_required: inviteOnly,
          google,
        };
      case "auth_login": {
        const a = known.find((x) => x.email === emailOf(input));
        if (!a) return refused("EMAIL_NOT_REGISTERED", "这个邮箱还没注册");
        if (input.password !== MOCK_PASSWORD)
          return refused("INVALID_CREDENTIALS", "邮箱或密码不对");
        if (a.user_id === 2) {
          db.totpFor = a.user_id;
          return { status: "totp_required" };
        }
        return issue(a);
      }
      case "auth_totp": {
        const a = known.find((x) => x.user_id === db.totpFor);
        if (!a) throw "请重新登录";
        if (input.code !== MOCK_TOTP)
          return refused("TOTP_INVALID_CODE", "两步验证码不对");
        db.totpFor = 0;
        return issue(a);
      }
      case "auth_send_code": {
        const e = emailOf(input);
        const exists = known.some((x) => x.email === e);
        if (input.purpose !== "reset" && exists)
          return refused("EMAIL_EXISTS", "这个邮箱已经注册过了，直接登录吧");
        db.codeFor = e;
        return { status: "sent", cooldown: 60 };
      }
      case "auth_register":
      case "auth_reset": {
        const e = emailOf(input);
        if (action === "auth_register" && inviteOnly && !input.affiliate_code)
          return refused("INVITE_REQUIRED", "需要邀请码才能注册");
        if (String(input.password ?? "").length < 6) throw "密码至少 6 位";
        if (db.codeFor !== e || input.code !== MOCK_EMAIL_CODE)
          return refused(
            "INVALID_VERIFY_CODE",
            "验证码不对或已过期，请重新获取",
          );
        db.codeFor = "";
        let a = known.find((x) => x.email === e);
        if (!a) {
          a = {
            user_id: 100 + known.length,
            email: e,
            username: "",
            balance: 0,
            created_at: new Date().toISOString(),
          };
          known.push(a);
        }
        return issue(a);
      }
    }
    throw `模拟数据不支持：${action}`;
  }

  async function tuken(action: string, input: any): Promise<unknown> {
    if (action === "google_sign_in") {
      // A new Google account under invitation-only sign-up needs a code.
      if (inviteOnly && !input.invite) return { status: "invite_required" };
      // The small window: signs in the next account after a moment.
      return new Promise((resolve) => {
        db.google = resolve;
        setTimeout(() => {
          if (db.google !== resolve) return;
          db.google = null;
          const next = known.find((a) => !db.accounts.includes(a)) ?? known[0];
          resolve(issue(next));
        }, googleAfter);
      });
    }
    if (action === "google_cancel") {
      db.google?.({ status: "cancelled" });
      db.google = null;
      return {};
    }
    if (action === "google_browser") return {};
    if (action === "site")
      return {
        contact:
          "QQ 群：123456789\n微信客服：tuken-help\n邮箱：help@tuken.site",
      };
    if (action === "open_link") return {};
    if (action.startsWith("auth_")) {
      if (action === "auth_config" && !nativeAuth) return { available: false };
      return auth(action, input);
    }
    const signedIn = () => {
      if (!db.active) throw "请先登录兔啃";
      if (input.as_user && input.as_user !== db.active)
        throw "账户刚刚切换，请稍候";
    };
    switch (action) {
      case "state":
      case "vault_retry": {
        if (action === "vault_retry") locked = "";
        if (locked)
          return { active: null, accounts: [], orders: [], locked: locked };
        const v: Record<string, unknown> = view();
        if (notice) v.notice = notice;
        notice = "";
        return v;
      }
      case "update_check":
        return update ? { version: update, notes: "修复了一些问题。" } : null;
      case "update_install":
        return null;
      case "login_start":
        db.loginPolls = 0;
        return { user_code: "7F3A9C21DE", expires_in: 600 };
      case "login_poll": {
        if (db.loginPolls < 0) throw "请重新登录";
        db.loginPolls += 1;
        if (db.loginPolls < approveAfter) return { status: "pending" };
        db.loginPolls = -1;
        const next =
          known.find(
            (a) => !db.accounts.some((x) => x.user_id === a.user_id),
          ) ?? known[0];
        if (!db.accounts.includes(next)) db.accounts.push(next);
        db.active = next.user_id;
        return { status: "approved", user_id: next.user_id };
      }
      case "login_cancel":
        db.loginPolls = -1;
        return view();
      case "switch":
        db.active = input.user_id;
        return view();
      case "logout":
        db.accounts = db.accounts.filter(
          (a) => a.user_id !== (input.user_id ?? db.active),
        );
        db.active = db.accounts[0]?.user_id ?? null;
        return view();
      case "catalog":
        return { goods, channels: [{ id: 1, name: "支付宝", rate: 1.8 }] };
      case "quote":
        return { total_amount: price(input.goods_id) };
      case "order_create": {
        signedIn();
        const g = goods.find((x) => x.id === input.goods_id)!;
        const order = {
          id: `o${Date.now()}`,
          owner: db.active,
          name: g.name,
          amount: price(g.id),
          trade_no: "T2026102500001",
          status: "awaiting_payment",
          created_at: Date.now() / 1000,
          message: "等待付款",
        };
        db.orders.push(order);
        db.payPolls = 0;
        const { owner: _, ...pub } = order;
        return pub;
      }
      case "order_pay": {
        const o = db.orders.find((x) => x.id === input.id);
        const { owner: _, ...pub } = o;
        return pub;
      }
      case "order_cashier":
        return {};
      case "order_poll": {
        const o = db.orders.find((x) => x.id === input.id);
        db.payPolls += 1;
        // Paid, then credited on the next check, as the shop delivers the code.
        if (o?.status === "redeeming") {
          o.status = "completed";
          o.message = "额度已到账";
          me().balance += Number(/(\d+)/.exec(o.name)?.[1] ?? 0);
        } else if (
          o?.status === "awaiting_payment" &&
          db.payPolls >= paidAfter
        ) {
          o.status = "redeeming";
          o.message = "正在到账";
        }
        const { owner: _, ...pub } = o;
        return pub;
      }
      case "order_close": {
        const o = db.orders.find((x) => x.id === input.id);
        o.status = "closed";
        return o;
      }
      case "sync":
        return view();
    }
    signedIn();
    const a = me();
    switch (action) {
      case "profile":
        return {
          id: a.user_id,
          email: a.email,
          username: a.username,
          balance: a.balance,
          frozen_balance: 0,
          total_recharged: 50,
          concurrency: 4,
          created_at: a.created_at,
        };
      case "dashboard":
        return {
          today_actual_cost: 0.4213,
          today_requests: 37,
          total_actual_cost: 21.5,
        };
      case "keys":
        return db.keys;
      case "invite":
        return {
          supported: true,
          enabled: true,
          code: "XM2026",
          reward: 2,
          invitee_reward: 1,
          invited: 3,
          earned: 6,
        };
      case "announcements":
        return { supported: true, items: db.notices };
      case "announcement_read": {
        const n = db.notices.find((x) => x.id === input.id);
        if (n) n.read = true;
        return {};
      }
      case "checkin_status":
        if (checkin === "none") return { supported: false };
        return {
          supported: true,
          enabled: checkin === "on",
          checked_in: db.checkedIn,
          reward_min: 0.1,
          reward_max: 0.1,
          streak: db.checkedIn ? 3 : 2,
        };
      case "checkin_history": {
        const days = [3, 2, 1]
          .map((n) => ({ date: beijingDay(n), reward: 0.1 }))
          .concat(db.checkedIn ? [{ date: beijingDay(0), reward: 0.1 }] : [])
          .filter((d) => d.date.startsWith(input.month));
        return { supported: checkin !== "none", days };
      }
      case "checkin":
        if (checkin !== "on") throw "签到暂时关闭了";
        if (db.checkedIn) throw "今天已经签到过了";
        db.checkedIn = true;
        a.balance = Math.round((a.balance + 0.1) * 100) / 100;
        return { reward: 0.1, streak: 3 };
      case "groups":
        return groups;
      case "models":
        // Like the gateway: no model list for an account without credit.
        if (a.balance <= 0) return { source: "balance", models: [] };
        return {
          source: "key",
          models: (models[input.group_id] ?? []).map((id) => ({
            id,
            name: id,
          })),
        };
      case "usage":
        return page(usage, input.page ?? 1);
      case "usage_stats":
        return {
          total_requests: usage.length,
          total_tokens: usage.reduce(
            (s, u) => s + u.input_tokens + u.output_tokens,
            0,
          ),
          total_actual_cost: usage.reduce((s, u) => s + u.actual_cost, 0),
        };
      case "recharge_history":
        return page(db.credits, input.page ?? 1);
      case "key_create": {
        const g = groups.find((x) => x.id === input.group_id)!;
        const k = {
          id: 100 + db.keys.length,
          name: input.name,
          status: "active",
          group_id: g.id,
          group: { id: g.id, name: g.name, platform: g.platform },
          last_used_at: null,
          in_desktop: false,
          in_cli: false,
        };
        db.keys.push(k);
        return k;
      }
      case "key_update": {
        const k = db.keys.find((x) => x.id === input.id);
        Object.assign(
          k,
          input.name ? { name: input.name } : {},
          input.status ? { status: input.status } : {},
        );
        return k;
      }
      case "key_delete":
        db.keys = db.keys.filter((x) => x.id !== input.id);
        return {};
      case "key_copy":
        return { key: "sk-preview-not-a-real-key" };
      case "connect": {
        const id = input.id ?? db.keys[0].id;
        for (const k of db.keys)
          k[input.app === "cli" ? "in_cli" : "in_desktop"] = k.id === id;
        if (input.app === "cli") db.desktop.cli_connected = true;
        else {
          db.desktop.connected = true;
          // A new key starts from Claude Desktop's own model discovery.
          db.desktop.models = { models: [], effort: null };
        }
        return { key_id: id };
      }
      case "disconnect":
        for (const k of db.keys)
          k[input.app === "cli" ? "in_cli" : "in_desktop"] = false;
        if (input.app === "cli") db.desktop.cli_connected = false;
        else {
          db.desktop.connected = false;
          db.desktop.models = null;
        }
        return {};
      case "profile_update":
        a.username = input.username;
        return tuken("profile", {});
      case "password_change":
        return tuken("logout", { user_id: a.user_id });
    }
    throw `模拟数据不支持：${action}`;
  }

  return async function invoke(cmd: string, args: any = {}): Promise<unknown> {
    switch (cmd) {
      case "tuken": {
        await wait();
        // Like IPC, a copy: the UI never holds on to the mock's own state.
        const r = await tuken(args.action, args.input ?? {});
        return r === undefined ? r : JSON.parse(JSON.stringify(r));
      }
      case "desktop_status":
        await wait(Math.min(delay, 120));
        return JSON.parse(JSON.stringify(db.desktop));
      case "mock:google":
        google = args?.on === true;
        return null;
      case "mock:invite":
        db.credits.unshift({
          id: db.credits[0].id + 1,
          type: "invite",
          value: 2,
          status: "used",
          used_at: new Date().toISOString(),
          created_at: new Date().toISOString(),
        });
        return null;
      case "mock:publish":
        db.notices.unshift({
          id: db.notices.length + 10,
          title: "新公告",
          content: "刚刚发布。",
          popup: false,
          read: false,
          created_at: new Date().toISOString(),
        });
        return null;
      case "desktop_models":
        if (!db.desktop.connected)
          throw "Claude Desktop 还没接入兔啃，请先接入";
        db.desktop.models = { models: args.models, effort: args.effort };
        return null;
      case "desktop_install":
        await wait(delay * 6);
        if (holdInstall) await new Promise<void>((r) => (releaseInstall = r));
        if (typeof installEnds === "object") throw installEnds.error;
        if (installEnds === "browser") return { method: "browser" };
        db.desktop.installed = true;
        return { method: platform === "macos" ? "dmg" : "msix" };
      case "mock:release_install":
        releaseInstall?.();
        return null;
      case "mock:emit":
        for (const [event, id] of listeners)
          if (event === args.event)
            (window as any)[`_${id}`]?.({ event, id, payload: args.payload });
        return null;
      case "platform":
        return platform;
      case "desktop_open":
      case "quit":
      case "panel_show":
      case "panel_hide":
        return null;
      case "backdrop":
      case "set_backdrop_theme":
        return platform === "macos" ? "vibrancy" : "mica";
      case "plugin:window|is_maximized":
        return false;
      case "plugin:event|listen":
        listeners.push([args.event, args.handler]);
        return listeners.length;
      default:
        if (cmd.startsWith("plugin:")) return null;
        throw `模拟数据不支持：${cmd}`;
    }
  };
}

/** Installs `invoke` where @tauri-apps/api looks for the native bridge. */
export function installMock(
  invoke: (cmd: string, args?: any) => Promise<unknown>,
) {
  let next = 0;
  const w = window as any;
  w.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: (fn: unknown) => {
      const id = ++next;
      w[`_${id}`] = fn;
      return id;
    },
    unregisterCallback: () => {},
    convertFileSrc: (s: string) => s,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main", windowLabel: "main" },
    },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}
