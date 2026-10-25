import type { ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/cn";

/** A floating glass sheet over a blurred scrim. */
export function Sheet({
  open,
  onOpenChange,
  title,
  description,
  width = 540,
  className,
  focusField = false,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  width?: number;
  className?: string;
  /** Starts in the first text field rather than on the close button. */
  focusField?: boolean;
  children: ReactNode;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="scrim" />
        <Dialog.Content
          className={cn(
            "sheet glass fixed left-1/2 top-1/2 z-50 flex max-h-[86vh] -translate-x-1/2 -translate-y-1/2 flex-col outline-none",
            className,
          )}
          style={{ width: `min(${width}px, calc(100vw - 40px))` }}
          onOpenAutoFocus={(e) => {
            const field = focusField
              ? (e.currentTarget as HTMLElement | null)?.querySelector("input")
              : null;
            if (field) {
              e.preventDefault();
              field.focus();
            }
          }}
          // Without a description, say so instead of letting Radix warn.
          {...(description ? {} : { "aria-describedby": undefined })}
        >
          <header className="flex items-start justify-between gap-4 px-6 pb-3 pt-5">
            <div className="min-w-0">
              <Dialog.Title className="text-section font-semibold">
                {title}
              </Dialog.Title>
              {description && (
                <Dialog.Description className="mt-1 text-small text-ink/65">
                  {description}
                </Dialog.Description>
              )}
            </div>
            <Dialog.Close className="icon-btn -mr-2 -mt-1" aria-label="关闭">
              <X size={16} />
            </Dialog.Close>
          </header>
          <div className="min-h-0 overflow-y-auto px-6 pb-6">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
