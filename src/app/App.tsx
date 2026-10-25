import { useEffect } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Backdrop } from "@/components/Backdrop";
import { cn } from "@/lib/cn";
import { refreshCredit, useProfile } from "@/lib/queries";
import { Bills } from "@/features/bills/Bills";
import { Home } from "@/features/home/Home";
import { CheckinSheet } from "@/features/home/CheckinSheet";
import { ModelSheet } from "@/features/home/ModelSheet";
import { RestartSheet } from "@/features/home/RestartSheet";
import { CreateKeySheet } from "@/features/keys/CreateKeySheet";
import { Models } from "@/features/models/Models";
import { Profile } from "@/features/account/Profile";
import { RechargeSheet } from "@/features/recharge/RechargeSheet";
import { ContactSheet } from "@/features/account/ContactSheet";
import { InviteSheet } from "@/features/account/InviteSheet";
import { AuthSheet } from "@/features/auth/AuthSheet";
import { GoogleInviteSheet } from "@/features/auth/GoogleInviteSheet";
import { UpdateNotice } from "@/features/update/Update";
import { NoticesSheet } from "@/features/notice/Notices";
import { LoginSheet } from "@/features/LoginSheet";
import { LoginBanner, Welcome } from "@/features/Welcome";
import { Tour } from "@/features/tour/Tour";
import { Setup } from "@/features/setup/Setup";
import { RewardNotice } from "@/features/rewards/RewardNotice";
import { Toolbar } from "./Toolbar";
import { isNewAccount, tourDone, useSession, useSetup, useUi } from "./context";

const pages = { home: Home, bills: Bills, models: Models, profile: Profile };

export function App() {
  const client = useQueryClient();
  const { uid, loading } = useSession();
  const { page, rechargeOpen, startTour } = useUi();
  const setup = useSetup();

  // The native worker can finish a payment while the recharge sheet is closed.
  useEffect(() => {
    let stop: (() => void) | undefined;
    void listen("tuken://credited", () => {
      refreshCredit(client, uid);
      if (!rechargeOpen) toast.success("充值的额度已到账");
    })
      .then((u) => (stop = u))
      .catch(() => {});
    return () => stop?.();
  }, [client, uid, rechargeOpen]);

  // The guide shows itself to a new account once on each computer, after
  // its setup, unless something else is already open by then.
  const { data: profile } = useProfile(uid);
  const isNew = isNewAccount(profile?.created_at);
  useEffect(() => {
    if (!uid || setup !== "done" || tourDone(uid) || !isNew) return;
    const t = window.setTimeout(() => {
      if (!document.querySelector('[role="dialog"]')) startTour();
    }, 900);
    return () => window.clearTimeout(t);
  }, [uid, isNew, startTour, setup]);

  const Page = pages[page];
  return (
    <div className="relative flex h-full flex-col overflow-hidden">
      <Backdrop />
      <Toolbar />
      <main className="relative z-10 flex min-h-0 flex-1 flex-col overflow-y-auto">
        {loading ? null : !uid ? (
          <Welcome />
        ) : setup === "unknown" ? null : setup === "show" ? (
          <Setup />
        ) : (
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={page}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.16, ease: [0.22, 0.9, 0.24, 1] }}
              className={cn(
                "flex flex-col pt-1",
                page === "home" && "min-h-0 flex-1",
              )}
            >
              <Page />
            </motion.div>
          </AnimatePresence>
        )}
      </main>
      <LoginSheet />
      <ContactSheet />
      <GoogleInviteSheet />
      <UpdateNotice />
      {uid && (
        <>
          <RechargeSheet />
          <CreateKeySheet />
          <ModelSheet />
          <CheckinSheet />
          <RestartSheet />
          <AuthSheet />
          <NoticesSheet />
          <InviteSheet />
          <RewardNotice />
          <LoginBanner />
          <Tour />
        </>
      )}
    </div>
  );
}
