import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { toast, Toaster } from "sonner";
import { App } from "@/app/App";
import {
  LoginProvider,
  setupDone,
  TOUR,
  tourSeen,
  UiProvider,
} from "@/app/context";
import {
  createMock,
  installMock,
  MOCK_EMAIL_CODE,
  MOCK_PASSWORD,
  MOCK_TOTP,
  type Scenario,
} from "@/dev/mock";
import { noticeDay } from "@/features/notice/Notices";
import { rewardsSeen } from "@/features/rewards/RewardNotice";
import { beijingDate } from "@/lib/format";
import { installText } from "@/lib/install";
import { persistCache, restoreCache } from "@/lib/persist";
import { loadPlatform } from "@/lib/platform";
import { qk, queryClient } from "@/lib/queries";
import { applyTheme } from "@/lib/theme";

function show(scenario: Scenario, opts: Parameters<typeof createMock>[1] = {}) {
  const invoke = vi.fn(createMock(scenario, { delay: 0, ...opts }));
  installMock(invoke);
  render(
    <QueryClientProvider client={queryClient}>
      <UiProvider>
        <LoginProvider>
          <App />
          <Toaster />
        </LoginProvider>
      </UiProvider>
    </QueryClientProvider>,
  );
  return invoke;
}

type Invoke = ReturnType<typeof show>;
const calls = (invoke: Invoke, action: string) =>
  invoke.mock.calls
    .filter(([cmd, args]) => cmd === "tuken" && args?.action === action)
    .map(([, args]) => args.input);

beforeEach(() => {
  queryClient.clear();
  // Toasts outlive a test in sonner's own store; the next one starts clean.
  toast.dismiss();
  localStorage.clear();
  // The first-run guide and the setup have their own tests.
  for (const uid of [1, 2]) {
    localStorage.setItem(tourSeen(uid), TOUR);
    localStorage.setItem(setupDone(uid), "done");
  }
  // Rewards on record have been shown already; their popup has its tests.
  localStorage.setItem(rewardsSeen(1), "999");
  localStorage.setItem(rewardsSeen(2), "999");
  // Today's announcement has been looked at; the daily popup has its tests.
  localStorage.setItem(noticeDay(1), beijingDate(0));
  localStorage.setItem(noticeDay(2), beijingDate(0));
  // The window is in front, not in the tray.
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
});

describe("兔啃", () => {
  it("signs in natively from the welcome card and lands on the balance", async () => {
    const user = userEvent.setup();
    const invoke = show("out");
    await user.type(
      await screen.findByLabelText("邮箱"),
      "xiaoming@mail.sustech.edu.cn",
    );
    await user.type(screen.getByLabelText("密码"), "wrong-one");
    await user.click(screen.getByRole("button", { name: "登录" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "邮箱或密码不对",
    );
    await user.clear(screen.getByLabelText("密码"));
    await user.type(screen.getByLabelText("密码"), MOCK_PASSWORD);
    await user.click(screen.getByRole("button", { name: "登录" }));
    expect(await screen.findByText("$28.50")).toBeInTheDocument();
    // No browser and no web page on the way.
    expect(calls(invoke, "login_start")).toHaveLength(0);
  });

  it("turns an unregistered email at sign-in into sign-up, code already sent", async () => {
    const user = userEvent.setup();
    const invoke = show("out");
    await user.type(
      await screen.findByLabelText("邮箱"),
      "new@mail.sustech.edu.cn",
    );
    await user.type(screen.getByLabelText("密码"), "my-secret-1");
    await user.click(screen.getByRole("button", { name: "登录" }));
    expect(
      await screen.findByText(
        "这个邮箱还没注册。验证码已发到 new@mail.sustech.edu.cn，填上就能注册",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "注册" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(calls(invoke, "auth_send_code")).toEqual([
      { email: "new@mail.sustech.edu.cn", purpose: "register" },
    ]);
    // The password typed stays; only the code is left to fill in.
    expect(screen.getByLabelText("设置密码")).toHaveValue("my-secret-1");
    await user.type(screen.getByLabelText("邮箱验证码"), MOCK_EMAIL_CODE);
    await user.click(screen.getByRole("button", { name: "注册" }));
    // A new account goes through the setup first (under its bonus popup).
    expect(
      await screen.findByRole("list", { name: "激活步骤", hidden: true }),
    ).toBeInTheDocument();
    expect(calls(invoke, "auth_register")[0]).toMatchObject({
      email: "new@mail.sustech.edu.cn",
      code: MOCK_EMAIL_CODE,
      password: "my-secret-1",
    });
  });

  it("signs up in the app: the email code, then a password", async () => {
    const user = userEvent.setup();
    const invoke = show("out");
    await user.click(await screen.findByRole("tab", { name: "注册" }));
    await user.type(screen.getByLabelText("邮箱"), "fresh@mail.sustech.edu.cn");
    await user.click(screen.getByRole("button", { name: "发送验证码" }));
    expect(
      await screen.findByText("验证码已发到 fresh@mail.sustech.edu.cn"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /秒后可重发/ })).toBeDisabled();
    await user.type(screen.getByLabelText("邮箱验证码"), MOCK_EMAIL_CODE);
    await user.type(screen.getByLabelText("设置密码"), "abc12345");
    await user.click(screen.getByRole("button", { name: "注册" }));
    // A new account goes through the setup first (under its bonus popup).
    expect(
      await screen.findByRole("list", { name: "激活步骤", hidden: true }),
    ).toBeInTheDocument();
    expect(calls(invoke, "login_start")).toHaveLength(0);
  });

  it("adds another account from the menu, two-step code included", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /添加账户/ }));
    const sheet = await screen.findByRole("dialog", { name: "添加账户" });
    await user.type(
      within(sheet).getByLabelText("邮箱"),
      "lab-share@example.test",
    );
    await user.type(within(sheet).getByLabelText("密码"), MOCK_PASSWORD);
    await user.click(within(sheet).getByRole("button", { name: "登录" }));
    const totp = await screen.findByLabelText("两步验证码");
    expect(
      screen.getByRole("dialog", { name: "两步验证" }),
    ).toBeInTheDocument();
    await user.type(totp, "000000");
    await user.click(screen.getByRole("button", { name: "验证并登录" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "两步验证码不对",
    );
    await user.clear(totp);
    await user.type(totp, MOCK_TOTP);
    await user.click(screen.getByRole("button", { name: "验证并登录" }));
    expect(await screen.findByText("$3.20")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("signs in with Google in 兔啃's own small window", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { googleAfter: 300 });
    await user.click(
      await screen.findByRole("button", { name: "使用 Google 登录" }),
    );
    expect(
      await screen.findByText("在弹出的窗口里登录 Google"),
    ).toBeInTheDocument();
    expect(calls(invoke, "google_sign_in")).toHaveLength(1);
    expect(calls(invoke, "login_start")).toHaveLength(0);
    expect(
      await screen.findByText("$28.50", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
  });

  it("hides Google sign-in while the admin has it off", async () => {
    const user = userEvent.setup();
    show("out", { google: false });
    await screen.findByRole("button", { name: "登录" });
    expect(screen.queryByRole("button", { name: /Google/ })).toBeNull();
    await user.click(screen.getByRole("tab", { name: "注册" }));
    await screen.findByLabelText("邮箱验证码");
    expect(screen.queryByRole("button", { name: /Google/ })).toBeNull();
  });

  it("drops Google as soon as the admin turns it off", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await screen.findByText("$28.50");
    // Turned off in the admin a moment after the app last asked.
    await invoke("mock:google", { on: false });
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /添加账户/ }));
    await screen.findByRole("dialog", { name: "添加账户" });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Google/ })).toBeNull(),
    );
  });

  it("moves a Google sign-in to the browser, or cancels it", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { googleAfter: 60_000 });
    await user.click(
      await screen.findByRole("button", { name: "使用 Google 登录" }),
    );
    await user.click(await screen.findByRole("button", { name: "改用浏览器" }));
    expect(
      await screen.findByText("在浏览器里登录 Google"),
    ).toBeInTheDocument();
    expect(calls(invoke, "google_browser")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(
      await screen.findByRole("button", { name: "使用 Google 登录" }),
    ).toBeInTheDocument();
  });

  it("uses the site's pages while the server has no desktop sign-in", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { nativeAuth: false, approveAfter: 1 });
    await user.click(await screen.findByRole("button", { name: "注册" }));
    await waitFor(() =>
      expect(calls(invoke, "login_start")[0]).toEqual({ register: true }),
    );
    expect(
      await screen.findByRole("dialog", { name: "注册兔啃" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("$28.50", {}, { timeout: 5000 }),
    ).toBeInTheDocument();
  });

  it("sets up a new account before the home page", async () => {
    localStorage.removeItem(setupDone(1));
    const user = userEvent.setup();
    const invoke = show("in", { noDesktop: true });
    // No pages to wander off to until it is done.
    expect(
      await screen.findByRole("heading", { name: "先装好 Claude Desktop" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "账单" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键安装" }));
    expect(
      await screen.findByRole("heading", { name: "一键配置 Claude Desktop" }),
    ).toBeInTheDocument();
    // The newest Opus and the highest effort are chosen; both can change.
    const model = await screen.findByRole("combobox", { name: "默认模型" });
    expect(model).toHaveValue("claude-opus-5-5");
    const max = screen.getByRole("tab", { name: /^最高/ });
    expect(max).toHaveAttribute("aria-selected", "true");
    expect(within(max).getByText("推荐")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    expect(
      await screen.findByRole(
        "heading",
        { name: "全部准备好了" },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    // The newest Opus first, at the highest effort.
    const set = invoke.mock.calls.find(([cmd]) => cmd === "desktop_models");
    expect(set?.[1]).toMatchObject({ effort: "max" });
    expect(set?.[1].models[0]).toBe("claude-opus-5-5");
    // The account's own Claude key is used; no second one is made.
    expect(calls(invoke, "key_create")).toHaveLength(0);
    expect(calls(invoke, "connect")[0]).toMatchObject({
      app: "desktop",
      id: expect.any(Number),
    });
    await user.click(
      screen.getByRole("button", { name: "打开 Claude Desktop" }),
    );
    expect(await screen.findByText("$28.50")).toBeInTheDocument();
    expect(invoke.mock.calls.some(([cmd]) => cmd === "desktop_open")).toBe(
      true,
    );
    expect(localStorage.getItem(setupDone(1))).toBe("done");
  });

  it("lets the setup use another model and effort", async () => {
    localStorage.removeItem(setupDone(1));
    const user = userEvent.setup();
    const invoke = show("in");
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "默认模型" }),
      "claude-sonnet-4-5",
    );
    await user.click(screen.getByRole("tab", { name: "高" }));
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    expect(
      await screen.findByRole("heading", { name: "全部准备好了" }),
    ).toBeInTheDocument();
    const set = invoke.mock.calls.find(([cmd]) => cmd === "desktop_models");
    expect(set?.[1]).toMatchObject({ effort: "high" });
    expect(set?.[1].models[0]).toBe("claude-sonnet-4-5");
    expect(
      screen.getByText(/默认用 Sonnet 4\.5，推理程度高/),
    ).toBeInTheDocument();
  });

  it("asks for credit first when the account has none", async () => {
    localStorage.removeItem(setupDone(1));
    const user = userEvent.setup();
    show("in", { noCredit: true });
    expect(
      await screen.findByRole("heading", { name: "先领一点额度" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "签到领额度" }));
    const sheet = await screen.findByRole("dialog", { name: "每日签到" });
    await user.click(
      within(sheet).getByRole("button", { name: "签到领 $0.10" }),
    );
    await user.keyboard("{Escape}");
    expect(
      await screen.findByRole(
        "heading",
        { name: "一键配置 Claude Desktop" },
        { timeout: 6000 },
      ),
    ).toBeInTheDocument();
  }, 15_000);

  it("goes straight home for an account made before", async () => {
    localStorage.removeItem(setupDone(1));
    localStorage.removeItem(tourSeen(1));
    show("in", { oldAccount: true, noDesktop: true });
    expect(await screen.findByText("$28.50")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "激活步骤" })).toBeNull();
    await waitFor(() =>
      expect(localStorage.getItem(setupDone(1))).toBe("done"),
    );
    // No guide either, unless asked for from the account menu.
    await new Promise((r) => setTimeout(r, 1200));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("sets up a new account where Claude Desktop already runs on 兔啃", async () => {
    localStorage.removeItem(setupDone(1));
    show("ok");
    expect(
      await screen.findByRole("list", { name: "激活步骤" }),
    ).toBeInTheDocument();
  });

  it("counts 0.2.0's marks for the account signed in", async () => {
    localStorage.removeItem(setupDone(1));
    localStorage.removeItem(tourSeen(1));
    localStorage.setItem("tuken-setup", "done");
    localStorage.setItem("tuken-tour", "done");
    show("in");
    expect(await screen.findByText("$28.50")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "激活步骤" })).toBeNull();
    expect(localStorage.getItem(setupDone(1))).toBe("done");
    expect(localStorage.getItem("tuken-setup")).toBeNull();
    // That guide was the first edition: a new account sees this one once.
    await waitFor(() => expect(localStorage.getItem(tourSeen(1))).toBe("done"));
    expect(
      await screen.findByRole(
        "dialog",
        { name: "这是你的余额" },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
  });

  it("welcomes a new account with its sign-up bonus", async () => {
    localStorage.removeItem(rewardsSeen(1));
    const user = userEvent.setup();
    show("in");
    const sheet = await screen.findByRole("dialog", {
      name: "注册成功，送你额度",
    });
    // The bonus and the invite reward of the last day, together.
    expect(within(sheet).getByText("+$52.00")).toBeInTheDocument();
    expect(
      within(sheet).getByText("新用户注册奖励已到账，可以直接用。"),
    ).toBeInTheDocument();
    await user.click(within(sheet).getByRole("button", { name: "好的" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(localStorage.getItem(rewardsSeen(1))).toBe("6");
  });

  it("pops up an invite reward that comes in while 兔啃 is open", async () => {
    localStorage.setItem(rewardsSeen(1), "6");
    const user = userEvent.setup();
    const invoke = show("in");
    await screen.findByText("$28.50");
    expect(screen.queryByRole("dialog")).toBeNull();
    // A friend signs up with the code; the user comes back to the window.
    await invoke("mock:invite");
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    const sheet = await screen.findByRole("dialog", { name: "邀请奖励到账" });
    expect(within(sheet).getByText("+$2.00")).toBeInTheDocument();
    expect(
      within(sheet).getByText(/有好友用你的邀请码注册了/),
    ).toBeInTheDocument();
    await user.click(within(sheet).getByRole("button", { name: "好的" }));
  });

  it("guides a new user once, ending on today's check-in", async () => {
    localStorage.removeItem(tourSeen(1));
    const user = userEvent.setup();
    show("in");
    const titles = [
      "这是你的余额",
      "接入 Claude",
      "你的密钥",
      "账单和模型",
      "公告和通知",
      "账户和设置",
      "邀请好友得额度",
    ];
    for (const title of titles) {
      expect(
        await screen.findByRole("dialog", { name: title }, { timeout: 3000 }),
      ).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "下一步" }));
    }
    expect(
      await screen.findByRole("dialog", { name: "每天签到领额度" }),
    ).toHaveTextContent("第 8 步，共 8 步");
    // Not collected yet: the last button opens today's check-in.
    await user.click(screen.getByRole("button", { name: "去签到" }));
    const sheet = await screen.findByRole("dialog", { name: "每日签到" });
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "每天签到领额度" }),
      ).not.toBeInTheDocument(),
    );
    expect(localStorage.getItem(tourSeen(1))).toBe(TOUR);
    await user.click(
      within(sheet).getByRole("button", { name: "签到领 $0.10" }),
    );
    expect(
      await within(sheet).findByRole("button", {
        name: "今天已签到，明天再来",
      }),
    ).toBeDisabled();
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );

    // Again from the account menu; with today's check-in in, it just ends.
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /新手指引/ }));
    expect(
      await screen.findByRole("dialog", { name: "这是你的余额" }),
    ).toBeInTheDocument();
    for (let i = 1; i < 8; i++) await user.keyboard("{ArrowRight}");
    expect(
      await screen.findByRole("dialog", { name: "每天签到领额度" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "去签到" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "开始使用" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  }, 15_000);

  it("leaves a check-in switched off in the admin out of the guide", async () => {
    localStorage.removeItem(tourSeen(1));
    const user = userEvent.setup();
    show("in", { checkin: "off" });
    expect(
      await screen.findByRole(
        "dialog",
        { name: "这是你的余额" },
        { timeout: 3000 },
      ),
    ).toHaveTextContent("第 1 步，共 7 步");
    for (let i = 1; i < 7; i++) await user.keyboard("{ArrowRight}");
    expect(
      await screen.findByRole("dialog", { name: "邀请好友得额度" }),
    ).toHaveTextContent("第 7 步，共 7 步");
    await user.click(screen.getByRole("button", { name: "开始使用" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(localStorage.getItem(tourSeen(1))).toBe(TOUR);
  }, 15_000);

  it("shows the new guide once more to a new account that saw the first", async () => {
    localStorage.setItem(tourSeen(1), "done");
    const user = userEvent.setup();
    show("in");
    expect(
      await screen.findByRole(
        "dialog",
        { name: "这是你的余额" },
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(localStorage.getItem(tourSeen(1))).toBe(TOUR));
  });

  it("starts with the computer only once switched on in the account menu", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    const item = await screen.findByRole("menuitemcheckbox", {
      name: "开机自动启动",
    });
    await waitFor(() => expect(item).not.toHaveAttribute("aria-disabled"));
    expect(item).toHaveAttribute("aria-checked", "false");
    await user.click(item);
    await waitFor(() => expect(item).toHaveAttribute("aria-checked", "true"));
    // A switch: the menu stays open to show it, and flips back.
    await user.click(
      screen.getByRole("menuitemcheckbox", { name: "开机自动启动" }),
    );
    await waitFor(() => expect(item).toHaveAttribute("aria-checked", "false"));
    const switched = invoke.mock.calls
      .filter(([cmd]) => cmd === "autostart")
      .map(([, args]) => args?.on)
      .filter((on) => on !== undefined);
    expect(switched).toEqual([true, false]);
  });

  it("shows announcements under the bell and marks them read", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await user.click(
      await screen.findByRole("button", { name: "公告，1 条未读" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "公告" });
    expect(
      within(sheet).getByText("Claude Opus 5.5 上线了"),
    ).toBeInTheDocument();
    expect(within(sheet).getByText("推理程度")).toBeInTheDocument();
    await waitFor(() =>
      expect(calls(invoke, "announcement_read")).toEqual([
        { id: 2, as_user: 1 },
      ]),
    );
    await user.keyboard("{Escape}");
    expect(
      await screen.findByRole("button", { name: "公告" }),
    ).toBeInTheDocument();
  });

  it("lights the bell when an announcement comes out while it is open", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    // Read what is there now.
    await user.click(
      await screen.findByRole("button", { name: "公告，1 条未读" }),
    );
    await screen.findByRole("dialog", { name: "公告" });
    await user.keyboard("{Escape}");
    expect(
      await screen.findByRole("button", { name: "公告" }),
    ).toBeInTheDocument();
    // The admin publishes one; the user comes back to the window.
    await invoke("mock:publish");
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(
      await screen.findByRole("button", { name: "公告，1 条未读" }),
    ).toBeInTheDocument();
  });

  it("opens an important announcement by itself, once", async () => {
    show("in", { urgentNotice: true });
    expect(
      await screen.findByRole("dialog", { name: "公告" }, { timeout: 4000 }),
    ).toBeInTheDocument();
  });

  it("checks in from a calendar of what each day brought", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await user.click(
      await screen.findByRole("button", { name: "签到 +$0.10" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "每日签到" });
    // The streak only; no reward range in the sheet.
    expect(within(sheet).queryByText(/到 \$/)).toBeNull();
    // The last days' rewards, from this month's history.
    await waitFor(() =>
      expect(calls(invoke, "checkin_history").length).toBeGreaterThan(0),
    );
    await user.click(
      within(sheet).getByRole("button", { name: "签到领 $0.10" }),
    );
    expect(
      await screen.findByText("签到成功，额度 +$0.10"),
    ).toBeInTheDocument();
    expect(
      await within(sheet).findByRole("button", {
        name: "今天已签到，明天再来",
      }),
    ).toBeDisabled();
    const today = within(sheet).getByRole("gridcell", {
      name: new RegExp(
        `^${Number(new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Shanghai" }).format(new Date()).slice(8))} 日，已签到`,
      ),
    });
    expect(await within(today).findByText("+$0.10")).toBeInTheDocument();
    expect(await screen.findByText("$28.60")).toBeInTheDocument();
  });

  it("says how long check-in credit lasts and how far the streak reward is", async () => {
    const user = userEvent.setup();
    show("in");
    // The balance says which part of it lapses, and when.
    expect(
      await screen.findByText(/^其中 \$0\.12 是限时额度，\d+月\d+日到期$/),
    ).toBeInTheDocument();
    await user.click(
      await screen.findByRole("button", { name: "签到 +$0.10" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "每日签到" });
    expect(
      await within(sheet).findByText(
        "连续签到每满 7 天额外送 $1.00，再签 5 天就到",
      ),
    ).toBeInTheDocument();
    expect(
      within(sheet).getByText("签到领的额度 7 天内有效，用的时候会先扣它"),
    ).toBeInTheDocument();
    await user.click(
      within(sheet).getByRole("button", { name: "签到领 $0.10" }),
    );
    const toastTitle = await screen.findByText("签到成功，额度 +$0.10");
    const toastEl = toastTitle.closest("[data-sonner-toast]") as HTMLElement;
    expect(within(toastEl).getByText(/^\d+月\d+日到期$/)).toBeInTheDocument();
    expect(
      await within(sheet).findByText(
        "连续签到每满 7 天额外送 $1.00，再签 4 天就到",
      ),
    ).toBeInTheDocument();
  });

  it("shows no time-limited credit on a server without it", async () => {
    const user = userEvent.setup();
    show("in", { credit: "none", firstRecharge: true });
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "签到 +$0.10" }));
    const sheet = await screen.findByRole("dialog", { name: "每日签到" });
    await within(sheet).findByRole("button", { name: "签到领 $0.10" });
    expect(within(sheet).queryByText(/有效|额外送/)).toBeNull();
    expect(screen.queryByText(/限时额度/)).toBeNull();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "充值" }));
    await screen.findByRole("radio", { name: "10刀额度" });
    expect(screen.queryByText(/首次充值多送/)).toBeNull();
  });

  it("hides check-in when it is off or not on the server", async () => {
    show("in", { checkin: "off" });
    await screen.findByText("$28.50");
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /签到/ })).toBeNull(),
    );
  });

  it("shows the contact details, also before signing in", async () => {
    const user = userEvent.setup();
    show("out");
    await user.click(await screen.findByRole("button", { name: "联系我们" }));
    const sheet = await screen.findByRole("dialog", { name: "联系我们" });
    expect(
      await within(sheet).findByText(/QQ 群：123456789/),
    ).toBeInTheDocument();
    expect(
      within(sheet).queryByRole("button", { name: "使用文档" }),
    ).toBeNull();
  });

  it("shows a key masked, and whole behind the eye", async () => {
    const user = userEvent.setup();
    show("in");
    expect(
      await screen.findByText("sk-prev••••••••••••••-key"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "显示「Claude Desktop」的密钥" }),
    );
    expect(
      await screen.findByText("sk-preview-not-a-real-key"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "隐藏「Claude Desktop」的密钥" }),
    );
    expect(screen.queryByText("sk-preview-not-a-real-key")).toBeNull();
    expect(screen.getByText("sk-prev••••••••••••••-key")).toBeInTheDocument();
  });

  it("signs up with a friend's invitation code", async () => {
    const user = userEvent.setup();
    const invoke = show("out");
    await user.click(await screen.findByRole("tab", { name: "注册" }));
    await user.type(screen.getByLabelText("邮箱"), "pal@mail.sustech.edu.cn");
    await user.click(screen.getByRole("button", { name: "发送验证码" }));
    await user.type(
      await screen.findByLabelText("邮箱验证码"),
      MOCK_EMAIL_CODE,
    );
    await user.type(screen.getByLabelText("设置密码"), "abc12345");
    await user.type(screen.getByLabelText("邀请码"), "XM2026");
    await user.click(screen.getByRole("button", { name: "注册" }));
    // A new account goes through the setup first (under its bonus popup).
    expect(
      await screen.findByRole("list", { name: "激活步骤", hidden: true }),
    ).toBeInTheDocument();
    expect(calls(invoke, "auth_register")[0]).toMatchObject({
      affiliate_code: "XM2026",
    });
  });

  it("needs an invitation code to sign up while sign-up is by invitation", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { inviteOnly: true });
    await user.click(await screen.findByRole("tab", { name: "注册" }));
    await user.type(screen.getByLabelText("邮箱"), "new2@mail.sustech.edu.cn");
    await user.click(screen.getByRole("button", { name: "发送验证码" }));
    await user.type(
      await screen.findByLabelText("邮箱验证码"),
      MOCK_EMAIL_CODE,
    );
    await user.type(screen.getByLabelText("设置密码"), "abc12345");
    expect(screen.getByPlaceholderText("邀请码（必填）")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "注册" })).toBeDisabled();
    await user.type(screen.getByLabelText("邀请码"), "XM2026");
    await user.click(screen.getByRole("button", { name: "注册" }));
    // A new account goes through the setup first (under its bonus popup).
    expect(
      await screen.findByRole("list", { name: "激活步骤", hidden: true }),
    ).toBeInTheDocument();
    expect(calls(invoke, "auth_register")[0]).toMatchObject({
      affiliate_code: "XM2026",
    });
  });

  it("asks a new Google account for an invitation code, then signs it up", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { inviteOnly: true, googleAfter: 50 });
    await user.click(
      await screen.findByRole("button", { name: "使用 Google 登录" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "填写邀请码" });
    await user.type(within(sheet).getByLabelText("邀请码"), "XM2026");
    await user.click(
      within(sheet).getByRole("button", { name: "继续用 Google 注册" }),
    );
    expect(
      await screen.findByText("$28.50", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(calls(invoke, "google_sign_in").at(-1)).toEqual({
      invite: "XM2026",
    });
  });

  it("signs up with Google from 注册, bringing the invitation code", async () => {
    const user = userEvent.setup();
    const invoke = show("out", { googleAfter: 50 });
    await user.click(await screen.findByRole("tab", { name: "注册" }));
    await user.type(screen.getByLabelText("邀请码"), "XM2026");
    await user.click(screen.getByRole("button", { name: "使用 Google 注册" }));
    expect(
      await screen.findByText("$28.50", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(calls(invoke, "google_sign_in").at(-1)).toEqual({
      invite: "XM2026",
    });
  });

  it("invites friends from beside the balance", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(
      await screen.findByRole("button", { name: "邀请好友 得 $2.00" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "邀请好友" });
    expect(within(sheet).getByText("XM2026")).toBeInTheDocument();
    expect(within(sheet).getByText(/你得 \$2\.00/)).toBeInTheDocument();
    expect(within(sheet).getByText(/已邀请 3 人/)).toBeInTheDocument();
  });

  it("lists rewards by name and in dollars among the credits", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("tab", { name: "账单" }));
    await user.click(await screen.findByRole("tab", { name: "充值记录" }));
    expect(await screen.findByText("邀请奖励")).toBeInTheDocument();
    expect(screen.getByText("+$2.00")).toBeInTheDocument();
    expect(screen.getByText("签到奖励")).toBeInTheDocument();
    expect(screen.getByText("+$0.12")).toBeInTheDocument();
    // Check-in credit lapses: its row says when.
    expect(screen.getByText(/· \d+月\d+日到期/)).toBeInTheDocument();
    expect(screen.queryByText("invite")).not.toBeInTheDocument();
  });

  it("only reads when it opens: no key, order, install or Claude Desktop change", async () => {
    const invoke = show("in");
    await screen.findByText("$28.50");
    for (const action of [
      "connect",
      "key_create",
      "order_create",
      "disconnect",
    ]) {
      expect(calls(invoke, action)).toHaveLength(0);
    }
    expect(invoke.mock.calls.some(([cmd]) => cmd === "desktop_install")).toBe(
      false,
    );
    expect(invoke.mock.calls.some(([cmd]) => cmd === "desktop_open")).toBe(
      false,
    );
  });

  it("connects Claude Desktop, then sets its default model and effort", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await user.click(await screen.findByRole("button", { name: "一键接入" }));
    await waitFor(() =>
      expect(calls(invoke, "connect")[0]).toMatchObject({
        app: "desktop",
        group_id: 14,
      }),
    );
    const sheet = await screen.findByRole("dialog", { name: "选择模型" });
    // Opus 5.5 is suggested; the user picks another.
    expect(
      await within(sheet).findByRole("radio", { name: /Opus 5\.5\s*最强旗舰/ }),
    ).toHaveAttribute("aria-checked", "true");
    // Strongest first.
    expect(
      within(sheet)
        .getAllByRole("radio")
        .map((r) => r.getAttribute("title")),
    ).toEqual(["claude-opus-5-5", "claude-sonnet-4-5", "claude-haiku-4-5"]);
    // The highest effort is chosen and recommended to start with.
    const max = within(sheet).getByRole("tab", { name: /^最高/ });
    expect(max).toHaveAttribute("aria-selected", "true");
    expect(within(max).getByText("推荐")).toBeInTheDocument();
    await user.click(within(sheet).getByRole("radio", { name: /Sonnet 4\.5/ }));
    await user.click(within(sheet).getByRole("tab", { name: "超高" }));
    await user.click(within(sheet).getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(
        invoke.mock.calls.find(([cmd]) => cmd === "desktop_models")?.[1],
      ).toEqual({
        models: ["claude-sonnet-4-5", "claude-opus-5-5", "claude-haiku-4-5"],
        effort: "xhigh",
      }),
    );
    // Claude Desktop is running: it asks before restarting it.
    expect(
      await screen.findByText("重启 Claude Desktop？"),
    ).toBeInTheDocument();
    expect(invoke.mock.calls.some(([cmd]) => cmd === "desktop_open")).toBe(
      false,
    );
    await user.click(screen.getByRole("button", { name: "立即重启" }));
    await waitFor(() =>
      expect(
        invoke.mock.calls.some(
          ([cmd, args]) => cmd === "desktop_open" && args?.restart === true,
        ),
      ).toBe(true),
    );
    expect(await screen.findByText("已接入兔啃")).toBeInTheDocument();
    expect(await screen.findByText("Sonnet 4.5")).toBeInTheDocument();
    expect(screen.getByText("超高")).toBeInTheDocument();
  });

  it("asks for credit first when the gateway won't list models without it", async () => {
    const user = userEvent.setup();
    show("in", { noCredit: true });
    await user.click(await screen.findByRole("button", { name: "一键接入" }));
    const sheet = await screen.findByRole("dialog", { name: "选择模型" });
    expect(
      await within(sheet).findByText(/余额为 0，充值后就能选模型/),
    ).toBeInTheDocument();
    await user.click(within(sheet).getByRole("button", { name: "去充值" }));
    expect(
      await screen.findByRole("dialog", { name: "充值" }),
    ).toBeInTheDocument();
  });

  it("buys credit at the quoted price and shows it arriving", async () => {
    const user = userEvent.setup();
    const invoke = show("in", { paidAfter: 1 });
    await user.click(await screen.findByRole("button", { name: "充值" }));
    await user.click(await screen.findByRole("radio", { name: "10刀额度" }));
    expect(await screen.findByText("¥10.18")).toBeInTheDocument();
    expect(calls(invoke, "order_create")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "支付宝付款" }));
    await waitFor(() =>
      expect(calls(invoke, "order_create")[0]).toMatchObject({
        goods_id: "g10",
        channel_id: 1,
        confirmed_amount: 10.18,
      }),
    );
    // Alipay's own cashier opens in its own window, for 小荷包.
    expect(await screen.findByText(/已打开支付宝付款窗口/)).toBeInTheDocument();
    await waitFor(() => expect(calls(invoke, "order_cashier")).toHaveLength(1));
    await user.click(screen.getByRole("button", { name: "重新打开付款窗口" }));
    expect(calls(invoke, "order_cashier")).toHaveLength(2);
    expect(
      await screen.findByText(
        "付款已收到，额度马上到账",
        {},
        { timeout: 5000 },
      ),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("+$10 已到账", {}, { timeout: 5000 }),
    ).toBeInTheDocument();
  }, 15_000);

  it("offers the first-recharge bonus until the first recharge", async () => {
    localStorage.setItem(rewardsSeen(1), "6");
    const user = userEvent.setup();
    show("in", { paidAfter: 1, firstRecharge: true });
    await user.click(await screen.findByRole("button", { name: "充值" }));
    expect(await screen.findByText("首次充值多送 20%")).toBeInTheDocument();
    await user.click(await screen.findByRole("radio", { name: "10刀额度" }));
    expect(await screen.findByText("首充多送 $2.00")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "支付宝付款" }));
    expect(
      await screen.findByText("+$10 已到账", {}, { timeout: 8000 }),
    ).toBeInTheDocument();
    // The bonus has its own popup, once the recharge sheet is closed.
    expect(screen.queryByRole("dialog", { name: "首充奖励到账" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "好的" }));
    const reward = await screen.findByRole(
      "dialog",
      { name: "首充奖励到账" },
      { timeout: 5000 },
    );
    expect(within(reward).getByText("+$2.00")).toBeInTheDocument();
    await user.click(within(reward).getByRole("button", { name: "好的" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // Recharged now: no more offer.
    await user.click(screen.getByRole("button", { name: "充值" }));
    await screen.findByRole("radio", { name: "10刀额度" });
    await waitFor(() => expect(screen.queryByText(/首次充值多送/)).toBeNull());
  }, 30_000);

  it("opens the newest unread announcement on the first look each day", async () => {
    localStorage.removeItem(noticeDay(1));
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    // Started in the tray: it waits for the window.
    await new Promise((r) => setTimeout(r, 1600));
    expect(screen.queryByRole("dialog")).toBeNull();
    focus.mockReturnValue(true);
    window.dispatchEvent(new Event("focus"));
    const sheet = await screen.findByRole(
      "dialog",
      { name: "公告" },
      { timeout: 4000 },
    );
    expect(
      within(sheet).getByText("Claude Opus 5.5 上线了"),
    ).toBeInTheDocument();
    expect(localStorage.getItem(noticeDay(1))).toBe(beijingDate(0));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // Once a day: coming back to the window doesn't open it again.
    window.dispatchEvent(new Event("focus"));
    await new Promise((r) => setTimeout(r, 1600));
    expect(screen.queryByRole("dialog")).toBeNull();
  }, 15_000);

  it("switches accounts from the account pill", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(
      await screen.findByRole("menuitem", { name: /lab-share/ }),
    );
    expect(await screen.findByText("$3.20")).toBeInTheDocument();
  });

  it("creates a key in the chosen group and shows that group's models", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await user.click(await screen.findByRole("button", { name: "新建密钥" }));
    const sheet = await screen.findByRole("dialog");
    await user.click(within(sheet).getByRole("radio", { name: /GPT 通道/ }));
    expect(await within(sheet).findByText("gpt-5-codex")).toBeInTheDocument();
    await user.click(within(sheet).getByRole("button", { name: "创建" }));
    await waitFor(() =>
      expect(calls(invoke, "key_create")[0]).toMatchObject({
        group_id: 21,
        name: "GPT 密钥",
      }),
    );
  });

  it("offers a new version and installs it on the user's word", async () => {
    const user = userEvent.setup();
    const invoke = show("in", { update: "0.2.0" });
    expect(
      await screen.findByText(/兔啃 0\.2\.0 可以更新了/, {}, { timeout: 3000 }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "更新" }));
    await waitFor(() =>
      expect(calls(invoke, "update_install")).toHaveLength(1),
    );
  });

  it("checks for updates from the account menu", async () => {
    const user = userEvent.setup();
    const invoke = show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /检查更新/ }));
    expect(await screen.findByText("已经是最新版本 0.1.0")).toBeInTheDocument();
    expect(calls(invoke, "update_install")).toHaveLength(0);
  });

  it("keeps the profile one click away in the account menu", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /个人资料/ }));
    expect(await screen.findByLabelText("昵称")).toHaveValue("小明");
  });

  it("brings the last pages back after a restart, before any request", async () => {
    const stop = persistCache(queryClient);
    queryClient.setQueryData(qk.profile(1), {
      id: 1,
      email: "a@b",
      balance: 9.5,
    });
    await new Promise((r) => setTimeout(r, 900));
    stop();
    queryClient.clear();
    restoreCache(queryClient);
    expect(queryClient.getQueryData(qk.profile(1))).toMatchObject({
      balance: 9.5,
    });
  });

  it("draws its own window buttons on Windows", async () => {
    show("in", { noDesktop: true });
    expect(await screen.findByText("$28.50")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "最小化" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭" })).toBeInTheDocument();
    expect(screen.queryByTestId("traffic-lights")).toBeNull();
    expect(
      screen.getByText(
        "在兔啃内安装桌面程序和 Code 组件，首次打开默认进入 Code",
      ),
    ).toBeInTheDocument();
  });

  it("says once that saved sign-ins couldn't be read and were set aside", async () => {
    const notice = "这台电脑上保存的登录信息读不出来了，请重新登录";
    show("out", { vaultNotice: notice });
    expect(await screen.findByText(notice)).toBeInTheDocument();
    expect(await screen.findByLabelText("密码")).toBeInTheDocument();
    // Gone from the page's state, so it isn't said again.
    await waitFor(() =>
      expect(
        (queryClient.getQueryData(qk.state) as { notice?: string }).notice,
      ).toBeUndefined(),
    );
  });

  it("asks the native side which system it runs on", async () => {
    installMock(createMock("out", { platform: "macos" }));
    expect(await loadPlatform()).toBe("macos");
    installMock(async () => "plan9");
    expect(await loadPlatform()).toBe("windows");
    installMock(async () => {
      throw "no native side";
    });
    expect(await loadPlatform()).toBe("windows");
  });

  it("says in a few words where an install of Claude Desktop is", () => {
    expect(installText(null)).toBeNull();
    expect(installText({ step: "download", percent: 0 })).toBe("正在下载");
    expect(installText({ step: "download", percent: 42 })).toBe("正在下载 42%");
    expect(installText({ step: "verify" })).toBe("正在校验安装包");
    expect(installText({ step: "copy" })).toBe("正在装进「应用程序」");
  });

  it.each([false, true])(
    "keeps Windows download and installation progress in the app (setup=%s)",
    async (setup) => {
      if (setup) localStorage.removeItem(setupDone(1));
      const user = userEvent.setup();
      const invoke = show("in", { noDesktop: true, holdInstall: true });
      await user.click(await screen.findByRole("button", { name: "一键安装" }));
      await screen.findByRole("progressbar");
      const report = (payload: object) =>
        act(() =>
          invoke("mock:emit", { event: "tuken://desktop-install", payload }),
        );
      await report({ step: "download", percent: 42 });
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuenow",
        "42",
      );
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuetext",
        "正在下载 42%",
      );
      await report({ step: "permission" });
      expect(screen.getByRole("progressbar")).not.toHaveAttribute(
        "aria-valuenow",
      );
      await report({ step: "install", percent: 68 });
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuenow",
        "68",
      );
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuetext",
        "正在安装 68%",
      );
      expect(screen.queryByText("Claude Desktop 安装好了")).toBeNull();
      await report({ step: "code_download", percent: 37 });
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuenow",
        "37",
      );
      expect(screen.getByRole("progressbar")).toHaveAttribute(
        "aria-valuetext",
        "正在下载 Claude Code 运行组件 37%",
      );
      await report({ step: "code_install" });
      expect(screen.getByRole("progressbar")).not.toHaveAttribute(
        "aria-valuenow",
      );
      expect(calls(invoke, "open_link")).toHaveLength(0);
      await act(() => invoke("mock:release_install"));
      await waitFor(() => expect(screen.queryByRole("progressbar")).toBeNull());
      if (!setup)
        expect(
          await screen.findByText("已安装，版本 1.3.12"),
        ).toBeInTheDocument();
      expect(screen.queryByText("在浏览器里下载安装")).toBeNull();
    },
  );

  it("lets a Windows user retry after cancelling UAC without opening the browser", async () => {
    const user = userEvent.setup();
    const error =
      "你取消了 Windows 安装授权。点击一键安装重试，并在系统提示中选择「是」";
    const invoke = show("in", { noDesktop: true, installEnds: { error } });
    await user.click(await screen.findByRole("button", { name: "一键安装" }));
    expect(await screen.findByText(error)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "一键安装" })).toBeEnabled();
    expect(calls(invoke, "open_link")).toHaveLength(0);
    expect(screen.queryByText("Claude Desktop 安装好了")).toBeNull();
  });

  it("repairs a connected desktop whose first-use Code download is missing", async () => {
    const user = userEvent.setup();
    const invoke = show("ok", { missingCode: true, holdInstall: true });
    expect(
      await screen.findByText("还缺 Claude Code 运行组件，补齐后才能开始对话"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "打开 Claude Desktop" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "修复运行组件" }));
    await screen.findByRole("progressbar");
    await act(() =>
      invoke("mock:emit", {
        event: "tuken://desktop-install",
        payload: { step: "code_download", percent: 64 },
      }),
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "64",
    );
    await act(() => invoke("mock:release_install"));
    expect(
      await screen.findByRole("button", { name: "打开 Claude Desktop" }),
    ).toBeEnabled();
    expect(calls(invoke, "open_link")).toHaveLength(0);
  });

  it("does not skip setup just because Desktop exists without its Code engine", async () => {
    localStorage.removeItem(setupDone(1));
    const user = userEvent.setup();
    const invoke = show("in", { missingCode: true, holdInstall: true });
    await user.click(await screen.findByRole("button", { name: "一键安装" }));
    await screen.findByRole("progressbar");
    expect(
      invoke.mock.calls.filter(([cmd]) => cmd === "desktop_install"),
    ).toHaveLength(1);
    await act(() => invoke("mock:release_install"));
    await waitFor(() => expect(screen.queryByRole("progressbar")).toBeNull());
  });

  describe("on macOS", () => {
    beforeEach(() => {
      document.documentElement.dataset.platform = "macos";
    });
    afterEach(() => {
      delete document.documentElement.dataset.platform;
      document.documentElement.classList.remove("dark");
    });

    it("leaves the window buttons to macOS and makes room for them", async () => {
      show("in", { platform: "macos" });
      expect(await screen.findByText("$28.50")).toBeInTheDocument();
      expect(screen.getByTestId("traffic-lights")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "最小化" })).toBeNull();
      expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
    });

    it("installs Claude Desktop step by step as the native side reports", async () => {
      const user = userEvent.setup();
      const invoke = show("in", {
        platform: "macos",
        noDesktop: true,
        holdInstall: true,
      });
      expect(
        await screen.findByText("从 Anthropic 官方下载，安装前检查签名"),
      ).toBeInTheDocument();
      expect(screen.queryByText(/winget/)).toBeNull();
      await user.click(screen.getByRole("button", { name: "一键安装" }));
      const report = (payload: object) =>
        act(() =>
          invoke("mock:emit", { event: "tuken://desktop-install", payload }),
        );
      await report({ step: "download", percent: 42 });
      expect(await screen.findByText("正在下载 42%")).toBeInTheDocument();
      await report({ step: "verify" });
      expect(await screen.findByText("正在校验安装包")).toBeInTheDocument();
      await report({ step: "copy" });
      expect(
        await screen.findByText("正在装进「应用程序」"),
      ).toBeInTheDocument();
      // No install is reported done before the native side says so.
      expect(screen.queryByText("Claude Desktop 安装好了")).toBeNull();
      await act(() => invoke("mock:release_install"));
      expect(
        await screen.findByText("Claude Desktop 安装好了"),
      ).toBeInTheDocument();
      expect(
        await screen.findByText("已安装，版本 1.3.12"),
      ).toBeInTheDocument();
    });

    it("sends the user to the download page when the install can't finish", async () => {
      const user = userEvent.setup();
      show("in", {
        platform: "macos",
        noDesktop: true,
        installEnds: "browser",
      });
      await user.click(await screen.findByRole("button", { name: "一键安装" }));
      expect(
        await screen.findByText("没能自动安装，已打开官网下载页"),
      ).toBeInTheDocument();
      // Still to install.
      expect(
        screen.getByRole("button", { name: "一键安装" }),
      ).toBeInTheDocument();
    });

    it("says plainly what on this Mac stopped the install", async () => {
      const user = userEvent.setup();
      const full =
        "磁盘空间不够：装 Claude Desktop 需要大约 2 GB。请清理一些空间后重试";
      show("in", {
        platform: "macos",
        noDesktop: true,
        installEnds: { error: full },
      });
      await user.click(await screen.findByRole("button", { name: "一键安装" }));
      expect(await screen.findByText(full)).toBeInTheDocument();
      expect(screen.queryByText(/已打开官网下载页/)).toBeNull();
    });

    it("waits for 重试 when the keychain keeps the sign-ins shut", async () => {
      const user = userEvent.setup();
      const why =
        "钥匙串没有让兔啃读取「site.tuken.desktop」，登录信息暂时打不开。点重试，在钥匙串的弹窗里选「允许」";
      const invoke = show("in", { platform: "macos", vaultLocked: why });
      expect(
        await screen.findByRole("heading", { name: "登录信息暂时打不开" }),
      ).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent(why);
      // Not the sign-in form: signing in would only ask the keychain again.
      expect(screen.queryByLabelText("密码")).toBeNull();
      expect(calls(invoke, "vault_retry")).toHaveLength(0);
      await user.click(screen.getByRole("button", { name: "重试" }));
      expect(await screen.findByText("$28.50")).toBeInTheDocument();
      expect(calls(invoke, "vault_retry")).toHaveLength(1);
    });

    it("lets the window follow the system look while the app does", () => {
      const invoke = vi.fn(createMock("out", { platform: "macos" }));
      installMock(invoke);
      const sent = () =>
        invoke.mock.calls
          .filter(([cmd]) => cmd === "set_backdrop_theme")
          .map(([, args]) => args);
      applyTheme("system");
      applyTheme("dark");
      expect(sent()).toEqual([
        { dark: false, system: true },
        { dark: true, system: false },
      ]);
    });
  });
});
