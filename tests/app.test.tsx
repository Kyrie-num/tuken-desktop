import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { Toaster } from "sonner";
import { App } from "@/app/App";
import { LoginProvider, TOUR_DONE, UiProvider } from "@/app/context";
import {
  createMock,
  installMock,
  MOCK_EMAIL_CODE,
  MOCK_PASSWORD,
  MOCK_TOTP,
  type Scenario,
} from "@/dev/mock";
import { persistCache, restoreCache } from "@/lib/persist";
import { qk, queryClient } from "@/lib/queries";

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
  localStorage.clear();
  // The first-run guide has its own test.
  localStorage.setItem(TOUR_DONE, "done");
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
    expect(await screen.findByText("$0.00")).toBeInTheDocument();
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
    expect(await screen.findByText("$0.00")).toBeInTheDocument();
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

  it("guides a new user once, and again from the account menu", async () => {
    localStorage.removeItem(TOUR_DONE);
    const user = userEvent.setup();
    show("in");
    const titles = ["这是你的余额", "接入 Claude", "你的密钥", "账单和模型"];
    for (const title of titles) {
      expect(
        await screen.findByRole("dialog", { name: title }, { timeout: 3000 }),
      ).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "下一步" }));
    }
    expect(
      await screen.findByRole("dialog", { name: "账户和设置" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "开始使用" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(localStorage.getItem(TOUR_DONE)).toBe("done");

    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /新手指引/ }));
    expect(
      await screen.findByRole("dialog", { name: "这是你的余额" }),
    ).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
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
    expect(await screen.findByText("$0.00")).toBeInTheDocument();
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
    expect(await screen.findByText("$0.00")).toBeInTheDocument();
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

  it("invites friends from the account menu", async () => {
    const user = userEvent.setup();
    show("in");
    await screen.findByText("$28.50");
    await user.click(screen.getByRole("button", { name: "账户" }));
    await user.click(await screen.findByRole("menuitem", { name: /邀请好友/ }));
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
    // The app's own QR code, no web page.
    expect(
      await screen.findByRole("img", { name: "支付宝付款二维码" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("打开手机支付宝扫一扫，付完额度自动到账"),
    ).toBeInTheDocument();
    expect(invoke.mock.calls.some(([cmd]) => cmd === "panel_show")).toBe(false);
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
});
