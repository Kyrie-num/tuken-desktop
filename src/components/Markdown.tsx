import { Fragment, type ReactNode } from "react";
import { toast } from "sonner";
import { tuken } from "@/lib/api";
import { cn } from "@/lib/cn";

const openLink = (url: string) =>
  void tuken("open_link", { url }).catch((e) => toast.error(String(e)));

/** **bold**, `code`, [text](https://…) and bare https links. */
function inline(text: string, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re =
    /\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\((https:\/\/[^\s)]+)\)|(https:\/\/[^\s<>()，。；！？、]+)/g;
  let last = 0;
  let n = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const k = `${key}-${n++}`;
    if (m[1]) {
      out.push(
        <strong key={k} className="font-semibold text-ink">
          {m[1]}
        </strong>,
      );
    } else if (m[2]) {
      out.push(
        <code
          key={k}
          className="rounded bg-ink/[0.07] px-1 font-mono text-[0.92em]"
        >
          {m[2]}
        </code>,
      );
    } else {
      const url = m[4] ?? m[5];
      out.push(
        // A button, not an anchor: the app's own window never navigates away.
        <button
          key={k}
          type="button"
          className="text-cyan-ink underline decoration-cyan-ink/40 underline-offset-2 hover:decoration-cyan-ink"
          onClick={() => openLink(url)}
        >
          {m[3] ?? url}
        </button>,
      );
    }
    last = re.lastIndex;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

const listItem = /^\s*([-*]|\d+[.)])\s+/;

/**
 * The bits of Markdown the site's announcements and contact details use,
 * drawn as the app's own text: never as HTML.
 */
export function Markdown({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  const blocks = text
    .replace(/\r\n?/g, "\n")
    .trim()
    .split(/\n{2,}/);
  return (
    <div
      className={cn("selectable space-y-2 text-body text-ink/80", className)}
    >
      {blocks.map((block, b) => {
        const lines = block.split("\n");
        if (lines.every((l) => listItem.test(l))) {
          const ordered = /^\s*\d/.test(lines[0]);
          const List = ordered ? "ol" : "ul";
          return (
            <List
              key={b}
              className={cn(
                "space-y-1 pl-5",
                ordered ? "list-decimal" : "list-disc",
              )}
            >
              {lines.map((l, i) => (
                <li key={i}>{inline(l.replace(listItem, ""), `${b}-${i}`)}</li>
              ))}
            </List>
          );
        }
        return (
          <p key={b}>
            {lines.map((l, i) => {
              const heading = /^#{1,6}\s+(.*)$/.exec(l);
              return (
                <Fragment key={i}>
                  {i > 0 && <br />}
                  {heading ? (
                    <strong className="font-semibold text-ink">
                      {inline(heading[1], `${b}-${i}`)}
                    </strong>
                  ) : (
                    inline(l, `${b}-${i}`)
                  )}
                </Fragment>
              );
            })}
          </p>
        );
      })}
    </div>
  );
}
