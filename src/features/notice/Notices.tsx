import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Bell } from "lucide-react";
import { Markdown } from "@/components/Markdown";
import { Sheet } from "@/components/Sheet";
import { tuken, type Announcements } from "@/lib/api";
import { day } from "@/lib/format";
import { qk, useAnnouncements } from "@/lib/queries";
import { useSession, useUi } from "@/app/context";

/** The bell in the title bar, with a dot while something is unread. */
export function NoticeBell() {
  const { uid } = useSession();
  const { setNoticesOpen } = useUi();
  const { data } = useAnnouncements(uid);
  if (!data?.supported) return null;
  const unread = data.items.filter((a) => !a.read).length;
  return (
    <button
      type="button"
      data-tour="notices"
      className="icon-btn relative"
      aria-label={unread ? `公告，${unread} 条未读` : "公告"}
      onClick={() => setNoticesOpen(true)}
    >
      <Bell size={18} />
      {unread > 0 && (
        <span
          aria-hidden="true"
          className="absolute right-[7px] top-[7px] h-2 w-2 rounded-full bg-orange shadow-[0_0_0_2px_rgb(var(--glass))]"
        />
      )}
    </button>
  );
}

/** The site's announcements; an important new one opens by itself once. */
export function NoticesSheet() {
  const client = useQueryClient();
  const { uid } = useSession();
  const { noticesOpen, setNoticesOpen } = useUi();
  const { data, refetch } = useAnnouncements(uid);
  const items = data?.items ?? [];

  // Opening the list asks for anything new.
  useEffect(() => {
    if (noticesOpen) void refetch();
  }, [noticesOpen, refetch]);

  const popped = useRef(false);
  const urgent = items.some((a) => a.popup && !a.read);
  useEffect(() => {
    if (popped.current || !urgent) return;
    popped.current = true;
    const t = window.setTimeout(() => {
      if (!document.querySelector('[role="dialog"]')) setNoticesOpen(true);
    }, 1200);
    return () => window.clearTimeout(t);
  }, [urgent, setNoticesOpen]);

  // What is on screen in the open list counts as read.
  useEffect(() => {
    if (!noticesOpen || !uid) return;
    const unread = items.filter((a) => !a.read);
    if (!unread.length) return;
    void Promise.all(
      unread.map((a) =>
        tuken("announcement_read", { id: a.id, as_user: uid }).catch(() => {}),
      ),
    ).then(() =>
      client.setQueryData<Announcements>(
        qk.announcements(uid),
        (old) =>
          old && {
            ...old,
            items: old.items.map((a) => ({ ...a, read: true })),
          },
      ),
    );
  }, [noticesOpen, uid, items, client]);

  return (
    <Sheet
      open={noticesOpen}
      onOpenChange={setNoticesOpen}
      title="公告"
      width={560}
    >
      {items.length ? (
        <ol className="space-y-3">
          {items.map((a) => (
            <li key={a.id} className="rounded-[18px] bg-ink/[0.04] px-4 py-3.5">
              <div className="flex items-baseline justify-between gap-3">
                <p className="flex min-w-0 items-center gap-2 text-strong font-semibold">
                  {!a.read && (
                    <>
                      <span
                        aria-hidden="true"
                        className="h-2 w-2 shrink-0 rounded-full bg-orange"
                      />
                      <span className="sr-only">未读：</span>
                    </>
                  )}
                  <span className="min-w-0">{a.title}</span>
                </p>
                <time className="shrink-0 text-caption text-ink/50">
                  {day(a.created_at)}
                </time>
              </div>
              {a.content && <Markdown text={a.content} className="mt-1.5" />}
            </li>
          ))}
        </ol>
      ) : (
        <p className="py-8 text-center text-body text-ink/60">暂时没有公告</p>
      )}
    </Sheet>
  );
}
