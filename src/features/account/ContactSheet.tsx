import { useEffect } from "react";
import { Copy } from "lucide-react";
import { toast } from "sonner";
import { Markdown } from "@/components/Markdown";
import { Sheet } from "@/components/Sheet";
import { useSite } from "@/lib/queries";
import { message, useUi } from "@/app/context";

/** 联系我们: the contact details set in the site's admin. */
export function ContactSheet() {
  const { contactOpen, setContactOpen } = useUi();
  const site = useSite();
  const contact = site.data?.contact ?? "";
  // Each opening reads it afresh: an admin's edit shows at once.
  const refetch = site.refetch;
  useEffect(() => {
    if (contactOpen) void refetch();
  }, [contactOpen, refetch]);

  const copy = () =>
    void navigator.clipboard
      .writeText(contact)
      .then(() => toast.success("已复制"))
      .catch(() => toast.error("复制失败，请手动选中复制"));

  return (
    <Sheet
      open={contactOpen}
      onOpenChange={setContactOpen}
      title="联系我们"
      width={460}
    >
      {site.isPending ? (
        <div className="space-y-2">
          <div className="skeleton h-5 w-3/4" />
          <div className="skeleton h-5 w-1/2" />
        </div>
      ) : site.isError ? (
        <p className="text-body text-ink/65">{message(site.error)}</p>
      ) : contact ? (
        <div className="rounded-[16px] bg-ink/[0.04] px-4 py-3.5">
          <Markdown text={contact} />
        </div>
      ) : (
        <p className="text-body text-ink/65">客服联系方式还没公布。</p>
      )}
      {contact && (
        <button
          type="button"
          className="btn btn-glass btn-sm mt-5"
          onClick={copy}
        >
          <Copy size={14} />
          复制
        </button>
      )}
    </Sheet>
  );
}
