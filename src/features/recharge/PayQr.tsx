import { AnimatePresence, motion } from "framer-motion";
import { Check, Loader2 } from "lucide-react";
import type { Order } from "@/lib/api";

/** The quiet zone scanners need around the code, in modules. */
const QUIET = 3;

/**
 * The order's payment link as a QR code for the phone's Alipay, drawn by the
 * app on white (the native side sends only the module rectangles). Once paid
 * it steps back behind a check while the credit lands.
 */
export function PayQr({ order }: { order: Order }) {
  const qr = order.qr;
  const paid = order.status !== "awaiting_payment";
  const span = (qr?.size ?? 0) + QUIET * 2;
  // Dense codes get more room, so each module stays big enough to scan.
  const side = Math.min(288, Math.max(216, span * 3));
  return (
    <div
      className="relative shrink-0 overflow-hidden rounded-[20px] bg-white shadow-[0_0_0_1px_rgb(0_63_67/0.08),0_14px_32px_-18px_rgb(0_44_48/0.55)]"
      style={{ width: side, height: side }}
    >
      {qr ? (
        <svg
          role="img"
          aria-label="支付宝付款二维码"
          viewBox={`${-QUIET} ${-QUIET} ${span} ${span}`}
          shapeRendering="crispEdges"
          className="h-full w-full transition-[filter,opacity] duration-300"
          style={paid ? { filter: "blur(5px)", opacity: 0.22 } : undefined}
        >
          <path d={qr.path} fill="#003F43" />
        </svg>
      ) : (
        <div className="flex h-full items-center justify-center">
          <Loader2 size={20} className="animate-spin text-ink/40" />
        </div>
      )}
      <AnimatePresence>
        {paid && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ type: "spring", stiffness: 320, damping: 24 }}
            className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-[#003F43]"
          >
            <span className="flex h-12 w-12 items-center justify-center rounded-full bg-[#2BB7B3] text-white shadow-[0_8px_20px_-8px_rgb(43_183_179/0.9)]">
              <Check size={26} strokeWidth={3} />
            </span>
            <span className="text-strong font-semibold">已付款</span>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
