import { invoke } from "@tauri-apps/api/core";

export type AccountSummary = {
  user_id: number;
  email: string;
  username: string;
};
export type Order = {
  id: string;
  name: string;
  amount: number;
  trade_no?: string | null;
  status: string;
  created_at: number;
  message: string;
  /** While paying: the payment link's QR modules, for an SVG path. */
  qr?: { size: number; path: string };
};
export type AppState = {
  active: number | null;
  accounts: AccountSummary[];
  orders: Order[];
};
/** The server's sign-up rules; `available` once it has the native sign-in API. */
export type AuthConfig =
  | { available: false }
  | {
      available: true;
      registration_enabled: boolean;
      invitation_code_enabled: boolean;
      password_reset_enabled: boolean;
      password_min_length: number;
      email_suffixes: string[];
      /** Sign-up takes a friend's invitation code. */
      invite_enabled?: boolean;
      /** Sign-up only with an invitation code. */
      invite_required?: boolean;
      /** Google sign-in is on in the admin. */
      google?: boolean;
    };
export type Profile = {
  id: number;
  email: string;
  username?: string;
  avatar_url?: string | null;
  balance: number;
  frozen_balance?: number;
  total_recharged?: number;
  concurrency?: number;
  created_at?: string;
};
export type Dashboard = {
  today_actual_cost?: number;
  today_requests?: number;
  today_tokens?: number;
  total_actual_cost?: number;
  total_requests?: number;
  total_tokens?: number;
};
export type Group = {
  id: number;
  name: string;
  description?: string;
  platform?: string;
  rate_multiplier?: number;
  subscription_type?: string;
  is_exclusive?: boolean;
};
export type Key = {
  /** The start and end of the secret; the whole one only on request. */
  masked?: string | null;
  id: number;
  name: string;
  status: string;
  group_id?: number | null;
  group?: { id: number; name: string; platform?: string } | null;
  quota?: number;
  quota_used?: number;
  last_used_at?: string | null;
  created_at?: string;
  expires_at?: string | null;
  in_desktop: boolean;
  in_cli: boolean;
};
export type Model = {
  id: string;
  name: string;
  input_price?: number;
  output_price?: number;
};
/** `balance`: the gateway lists nothing for an account without credit. */
export type Models = {
  source: "plaza" | "key" | "none" | "balance";
  models: Model[];
};
export type Page<T> = {
  items: T[];
  total: number;
  page: number;
  pages: number;
};
export type UsageRow = {
  id: number;
  model: string;
  api_key_id: number;
  key_name?: string | null;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  actual_cost: number;
  duration_ms?: number | null;
  first_token_ms?: number | null;
  stream?: boolean;
  created_at: string;
};
export type UsageStats = {
  total_requests?: number;
  total_tokens?: number;
  total_actual_cost?: number;
  average_duration_ms?: number;
};
/** A newer 兔啃 on GitHub, or null. */
export type Update = { version: string; notes?: string | null } | null;

export type CreditRow = {
  id: number;
  type: string;
  value: number;
  status: string;
  used_at?: string | null;
  created_at: string;
  notes?: string | null;
};
export type Catalog = {
  goods: { id: string; name: string; price: number; stock: number }[];
  channels: { id: number; name: string; rate?: number }[];
};
export type DesktopStatus = {
  supported: boolean;
  installed: boolean;
  version: string | null;
  running: boolean;
  installing: boolean;
  can_restart: boolean;
  /** Claude Desktop's applied gateway profile points at 兔啃. */
  connected: boolean;
  /** Claude Code's settings point at 兔啃. */
  cli_connected: boolean;
  /** While connected: the models offered (the default first) and the effort. */
  models: { models: string[]; effort: Effort | null } | null;
};
/** The site's contact details: free text, may hold links. */
export type SiteInfo = { contact: string };
export type Announcement = {
  id: number;
  title: string;
  /** Markdown. */
  content: string;
  /** Shown by itself once, rather than only under the bell. */
  popup: boolean;
  read: boolean;
  created_at: string;
};
/** `supported: false` until the server has the feature. */
export type Announcements = { supported: boolean; items: Announcement[] };
export type Checkin =
  | { supported: false }
  | {
      supported: true;
      enabled: boolean;
      checked_in: boolean;
      reward_min: number | null;
      reward_max: number | null;
      streak: number;
    };
/** Inviting friends: my code, what an invite brings, and how it went. */
export type Invite =
  | { supported: false }
  | {
      supported: true;
      enabled: boolean;
      code: string;
      reward: number | null;
      invitee_reward: number | null;
      invited: number;
      earned: number;
    };
/** One month of check-ins: what each day brought. */
export type CheckinHistory = {
  supported: boolean;
  days: { date: string; reward: number | null }[];
};
/** How hard Claude thinks, lowest first, as Claude Desktop names them. */
export const EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;
export type Effort = (typeof EFFORTS)[number];
export type Range = { start_date: string; end_date: string };

/** One call into the native app; errors arrive as plain Chinese messages. */
export function tuken<T>(action: string, input?: Record<string, unknown>) {
  return invoke<T>("tuken", { action, input });
}

export const desktop = {
  status: () => invoke<DesktopStatus>("desktop_status"),
  install: () =>
    invoke<{ method: "winget" | "browser" | "none" }>("desktop_install"),
  open: (restart = false) => invoke<void>("desktop_open", { restart }),
  /** The default model goes first in `models`. */
  setModels: (models: string[], effort: Effort) =>
    invoke<void>("desktop_models", { models, effort }),
};

export const windowMaterial = {
  get: () => invoke<"mica" | "none">("backdrop"),
  matchTheme: (dark: boolean) =>
    invoke<"mica" | "none">("set_backdrop_theme", { dark }),
};

/** Error text the native side uses for a token the server rejected. */
export const EXPIRED = "登录授权已过期";
