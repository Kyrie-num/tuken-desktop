import {
  dehydrate,
  hydrate,
  type DehydratedState,
  type QueryClient,
} from "@tanstack/react-query";

/**
 * Keeps the last known pages (balance, keys, bills…) across restarts, so the
 * window opens with data on screen and refreshes it quietly. Nothing secret
 * is in these queries: tokens and key secrets never leave the native side.
 */
const KEY = "tuken-cache-v1";
const MAX_AGE = 7 * 24 * 3600_000;
const SKIP = new Set(["quote", "desktop", "update"]);

export function restoreCache(client: QueryClient) {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as { at: number; state: DehydratedState };
    if (Date.now() - saved.at > MAX_AGE) return;
    hydrate(client, saved.state);
  } catch {
    localStorage.removeItem(KEY);
  }
}

export function persistCache(client: QueryClient) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const write = () => {
    const state = dehydrate(client, {
      shouldDehydrateQuery: (q) =>
        q.state.status === "success" && !SKIP.has(String(q.queryKey[0])),
    });
    try {
      localStorage.setItem(KEY, JSON.stringify({ at: Date.now(), state }));
    } catch {
      /* Storage full: the cache is a convenience. */
    }
  };
  return client.getQueryCache().subscribe(() => {
    clearTimeout(timer);
    timer = setTimeout(write, 800);
  });
}
