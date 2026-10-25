import { useEffect, useLayoutEffect, useRef, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { X } from "lucide-react";
import { Mascot } from "@/components/Mascot";
import { cn } from "@/lib/cn";
import { useUi } from "@/app/context";

/** What the guide points at (a `data-tour` anchor) and what it says there. */
const steps = [
  {
    target: "balance",
    title: "这是你的余额",
    body: "用 Claude 按用量扣费。不够了就点「充值」，手机支付宝扫码，付完自动到账。",
  },
  {
    target: "claude",
    title: "接入 Claude",
    body: "装好 Claude Desktop 后点「一键接入」，选好默认模型和推理程度就能打开用。Claude Code 也能一键接入。",
  },
  {
    target: "keys",
    title: "你的密钥",
    body: "接入时会自动建好。想在别的软件里用兔啃，在这里新建就行。",
  },
  {
    target: "pages",
    title: "账单和模型",
    body: "花了多少、充值记录看「账单」，能用哪些模型看「模型」。",
  },
  {
    target: "account",
    title: "账户和设置",
    body: "添加和切换账户、改资料、换深浅色都在这里，这份指引也能在这里重看。",
  },
] as const;

/** Room around the lit control, between it and the bubble, and at the edges. */
const PAD = 8;
const GAP = 14;
const EDGE = 16;
const WIDTH = 340;

type Box = { x: number; y: number; w: number; h: number; r: number };

/** The bubble goes below the control, else above it, else beside it. */
function placeBubble(b: Box, height: number) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const clampX = (x: number) => Math.max(EDGE, Math.min(vw - EDGE - WIDTH, x));
  const clampY = (y: number) => Math.max(EDGE, Math.min(vh - EDGE - height, y));
  const centered = clampX(b.x + b.w / 2 - WIDTH / 2);
  const below = b.y + b.h + PAD + GAP;
  if (below + height <= vh - EDGE) return { left: centered, top: below };
  const above = b.y - PAD - GAP - height;
  if (above >= EDGE) return { left: centered, top: above };
  const middle = clampY(b.y + b.h / 2 - height / 2);
  const right = b.x + b.w + PAD + GAP;
  if (right + WIDTH <= vw - EDGE) return { left: right, top: middle };
  const left = b.x - PAD - GAP - WIDTH;
  if (left >= EDGE) return { left, top: middle };
  // A control as big as the window: the bubble sits over its lower part.
  return { left: centered, top: vh - EDGE - height };
}

/**
 * The first-run guide: the window dims, a light moves from control to control
 * on the home page, and 兔啃 says in a bubble what each one is for.
 */
export function Tour() {
  const { tour, moveTour, endTour } = useUi();
  const reduce = useReducedMotion();
  const [box, setBox] = useState<Box | null>(null);
  const [bubble, setBubble] = useState<HTMLDivElement | null>(null);
  const [height, setHeight] = useState(176);
  const next = useRef<HTMLButtonElement>(null);
  const open = tour !== null;
  const step = tour ?? 0;
  const last = step === steps.length - 1;

  // Follows the step's control: into view, and wherever it moves after.
  useLayoutEffect(() => {
    if (tour === null) {
      setBox(null);
      return;
    }
    const el = document.querySelector<HTMLElement>(
      `[data-tour="${steps[tour].target}"]`,
    );
    if (!el) {
      // Not on screen in this layout: on to the next one.
      if (tour + 1 < steps.length) moveTour(tour + 1);
      else endTour();
      return;
    }
    el.scrollIntoView({
      block: "nearest",
      behavior: reduce ? "auto" : "smooth",
    });
    const update = () => {
      const r = el.getBoundingClientRect();
      const radius = parseFloat(getComputedStyle(el).borderTopLeftRadius);
      setBox({
        x: r.left,
        y: r.top,
        w: r.width,
        h: r.height,
        r: Number.isFinite(radius) ? radius : 16,
      });
    };
    update();
    const watch = new ResizeObserver(update);
    watch.observe(el);
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      watch.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [tour, reduce, moveTour, endTour]);

  // Whether the bubble fits below the control depends on its own height.
  useLayoutEffect(() => {
    if (!bubble) return;
    const update = () => setHeight(bubble.offsetHeight || 176);
    update();
    const watch = new ResizeObserver(update);
    watch.observe(bubble);
    return () => watch.disconnect();
  }, [bubble]);

  useEffect(() => {
    if (open) next.current?.focus();
  }, [open, step]);

  const finish = () => {
    endTour();
    // Back to the top of the home page, where the balance is.
    document
      .querySelector("main")
      ?.scrollTo({ top: 0, behavior: reduce ? "auto" : "smooth" });
  };
  const goTo = (n: number) => {
    if (n < 0) return;
    if (n >= steps.length) finish();
    else moveTour(n);
  };

  const motionOf = reduce
    ? { duration: 0 }
    : ({ type: "spring", stiffness: 260, damping: 30 } as const);
  const spot = box && {
    left: box.x - PAD,
    top: box.y - PAD,
    width: box.w + PAD * 2,
    height: box.h + PAD * 2,
    borderRadius: Math.min(box.r + PAD, (box.h + PAD * 2) / 2),
  };
  const at = box && placeBubble(box, height);

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && finish()}>
      <AnimatePresence>
        {open && spot && at && (
          <Dialog.Portal forceMount>
            <motion.div
              aria-hidden="true"
              className="pointer-events-none fixed z-[60]"
              style={{
                boxShadow:
                  "0 0 0 9999px rgb(var(--shade) / 0.5), 0 0 0 2px rgb(var(--rim) / 0.85)",
              }}
              initial={{ opacity: 0, ...spot }}
              animate={{ opacity: 1, ...spot }}
              exit={{ opacity: 0 }}
              transition={motionOf}
            />
            <Dialog.Content
              forceMount
              asChild
              onOpenAutoFocus={(e) => {
                e.preventDefault();
                next.current?.focus();
              }}
              // Only the bubble's own buttons and Esc end the guide.
              onInteractOutside={(e) => e.preventDefault()}
              onKeyDown={(e) => {
                if (e.key === "ArrowRight") goTo(step + 1);
                if (e.key === "ArrowLeft") goTo(step - 1);
              }}
            >
              <motion.div
                ref={setBubble}
                className="glass fixed z-[61] p-5 outline-none"
                // Nearly solid: it sits on the dimmed window, not on a wallpaper.
                style={{ width: WIDTH, background: "rgb(var(--glass) / 0.94)" }}
                initial={{ opacity: 0, scale: 0.96, ...at }}
                animate={{ opacity: 1, scale: 1, ...at }}
                exit={{ opacity: 0, scale: 0.96 }}
                transition={motionOf}
              >
                <div className="flex items-start gap-3">
                  <Mascot mood={last ? "cheer" : "bunny"} size={44} />
                  <AnimatePresence mode="wait" initial={false}>
                    <motion.div
                      key={step}
                      className="min-w-0 flex-1 pt-0.5"
                      initial={{ opacity: 0, y: 4 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, y: -4 }}
                      transition={{ duration: reduce ? 0 : 0.16 }}
                    >
                      <Dialog.Title className="pr-7 text-strong font-semibold">
                        {steps[step].title}
                      </Dialog.Title>
                      <Dialog.Description className="mt-1 text-body text-ink/75">
                        {steps[step].body}
                      </Dialog.Description>
                    </motion.div>
                  </AnimatePresence>
                </div>
                <Dialog.Close
                  className="icon-btn absolute right-2.5 top-2.5"
                  aria-label="关闭指引"
                >
                  <X size={16} />
                </Dialog.Close>
                <div className="mt-4 flex items-center justify-between gap-3">
                  <div className="flex items-center gap-1.5">
                    {steps.map((s, i) => (
                      <span
                        key={s.target}
                        aria-hidden="true"
                        className={cn(
                          "h-1.5 rounded-full transition-all duration-300",
                          i === step ? "w-4 bg-orange" : "w-1.5 bg-ink/15",
                        )}
                      />
                    ))}
                    <span className="sr-only">
                      第 {step + 1} 步，共 {steps.length} 步
                    </span>
                  </div>
                  <div className="flex gap-2">
                    {step > 0 && (
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={() => goTo(step - 1)}
                      >
                        上一步
                      </button>
                    )}
                    <button
                      ref={next}
                      type="button"
                      className="btn btn-primary btn-sm min-w-[84px]"
                      onClick={() => goTo(step + 1)}
                    >
                      {last ? "开始使用" : "下一步"}
                    </button>
                  </div>
                </div>
              </motion.div>
            </Dialog.Content>
          </Dialog.Portal>
        )}
      </AnimatePresence>
    </Dialog.Root>
  );
}
