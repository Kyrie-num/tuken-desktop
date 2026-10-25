import { useState } from "react";
import { motion } from "framer-motion";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2, RotateCw } from "lucide-react";
import { toast } from "sonner";
import { Mascot } from "@/components/Mascot";
import { tuken, type AppState } from "@/lib/api";
import { qk } from "@/lib/queries";
import { message } from "@/app/context";

/**
 * macOS, when the keychain didn't let 兔啃 open the saved sign-ins. Nothing
 * asks the keychain again by itself; 重试 does, once.
 */
export function VaultLocked({ why }: { why: string }) {
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const retry = async () => {
    setBusy(true);
    try {
      client.setQueryData(qk.state, await tuken<AppState>("vault_retry"));
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-1 items-center justify-center px-6 py-4">
      <motion.div
        initial={{ opacity: 0, y: 14, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ type: "spring", stiffness: 260, damping: 26 }}
        className="relative w-full max-w-[392px] pt-[44px]"
      >
        <Mascot
          mood="oops"
          size={88}
          className="absolute left-1/2 top-0 z-10 -translate-x-1/2"
        />
        <section className="glass px-8 pb-7 pt-[56px] text-center">
          <h1 className="font-display text-title font-semibold">
            登录信息暂时打不开
          </h1>
          <p role="alert" className="mt-3 text-body text-ink/70">
            {why}
          </p>
          <button
            type="button"
            className="btn btn-primary btn-lg mt-6 w-full"
            disabled={busy}
            onClick={() => void retry()}
          >
            {busy ? (
              <Loader2 size={17} className="animate-spin" />
            ) : (
              <RotateCw size={17} />
            )}
            重试
          </button>
        </section>
      </motion.div>
    </div>
  );
}
