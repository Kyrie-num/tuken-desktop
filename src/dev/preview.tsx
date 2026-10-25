/**
 * Design preview on mock data: /preview.html?s=out | in | ok (signed out,
 * signed in, Claude Desktop connected); &auth=site for a server without the
 * desktop sign-in API; &paid=never to keep an order waiting for payment;
 * &desktop=none for a computer without Claude Desktop, &credit=none for an
 * account without credit (the setup shows until it is finished once),
 * &account=old for an account made long ago (no setup, no guide).
 * The body stands in for the desktop wallpaper that Windows 11 Mica shows
 * through the window.
 */
import { createMock, installMock, type Scenario } from "./mock";

const params = new URLSearchParams(location.search);
const scenario = (params.get("s") ?? "in") as Scenario;
installMock(
  createMock(scenario, {
    nativeAuth: params.get("auth") !== "site",
    paidAfter: params.get("paid") === "never" ? Infinity : 2,
    noDesktop: params.get("desktop") === "none",
    noCredit: params.get("credit") === "none",
    oldAccount: params.get("account") === "old",
  }),
);

const style = document.createElement("style");
style.textContent = `
  html[data-backdrop="mica"] body {
    background: linear-gradient(135deg, #dce8e7 0%, #eef0ea 45%, #d9e4ee 100%);
  }
  html.dark[data-backdrop="mica"] body {
    background: linear-gradient(135deg, #0c2326 0%, #121b21 50%, #0a1923 100%);
  }
`;
document.head.append(style);

const { boot } = await import("@/main");
await boot();
