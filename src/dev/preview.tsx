/**
 * Design preview on mock data: /preview.html?s=out | in | ok (signed out,
 * signed in, Claude Desktop connected); &auth=site for a server without the
 * desktop sign-in API; &paid=never to keep an order waiting for payment;
 * &desktop=none for a computer without Claude Desktop, &credit=none for an
 * account without credit (the setup shows until it is finished once);
 * &account=old for an account made long ago (no setup, no guide);
 * &first=1 for an account that hasn't recharged yet (the first-recharge offer);
 * &credit=none for a server without time-limited credit;
 * &lapse=many for check-in credit lapsing today, tomorrow and later;
 * &os=mac for macOS (traffic lights drawn in, vibrancy over a wallpaper);
 * &quiet without the setup, the first-run guide, reward popups and the day's
 * announcement, for screenshots.
 * The body stands in for the desktop wallpaper that Windows 11 Mica shows
 * through the window.
 */
import { setupDone, tourSeen, TOUR } from "@/app/context";
import { noticeDay } from "@/features/notice/Notices";
import { beijingDate } from "@/lib/format";
import { createMock, installMock, type Scenario } from "./mock";

const params = new URLSearchParams(location.search);
const scenario = (params.get("s") ?? "in") as Scenario;
if (params.has("quiet")) {
  for (const uid of [1, 2]) {
    localStorage.setItem(tourSeen(uid), TOUR);
    localStorage.setItem(setupDone(uid), "done");
    localStorage.setItem(`tuken-rewards-${uid}`, "999");
    localStorage.setItem(noticeDay(uid), beijingDate(0));
  }
}
installMock(
  createMock(scenario, {
    nativeAuth: params.get("auth") !== "site",
    paidAfter: params.get("paid") === "never" ? Infinity : 2,
    noDesktop: params.get("desktop") === "none",
    noCredit: params.get("credit") === "none",
    platform: params.get("os") === "mac" ? "macos" : "windows",
    oldAccount: params.get("account") === "old",
    firstRecharge: params.get("first") === "1",
    credit: params.get("credit") === "none" ? "none" : "on",
    manyLapses: params.get("lapse") === "many",
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
  /* Vibrancy lets more of a (default, blue) macOS wallpaper through. */
  html[data-backdrop="vibrancy"] {
    background: linear-gradient(160deg, #6f8fd8 0%, #b4c6ee 45%, #e7c9d9 100%);
  }
  html.dark[data-backdrop="vibrancy"] {
    background: linear-gradient(160deg, #152447 0%, #23304f 50%, #3a2a3f 100%);
  }
  /* The traffic lights macOS draws over the title bar. */
  html[data-platform="macos"] body::before {
    content: "";
    position: fixed;
    z-index: 100;
    left: 20px;
    top: 25px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #ff5f57;
    box-shadow: 20px 0 0 #febc2e, 40px 0 0 #28c840;
  }
`;
document.head.append(style);

const { boot } = await import("@/main");
await boot();
