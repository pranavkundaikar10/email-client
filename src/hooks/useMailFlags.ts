import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../lib/api";
import { useAppStore } from "../store";

type ThreadPatch = Partial<Pick<import("../lib/api").Thread, "unread" | "starred">>;
type ThreadLike = { id?: string; thread_id?: string };

function patchThreadData(data: unknown, threadId: string, patch: ThreadPatch): unknown {
  if (Array.isArray(data)) {
    return data.map((item) => patchThreadData(item, threadId, patch));
  }
  if (!data || typeof data !== "object") return data;

  // React Query stores Inbox pages as { pages, pageParams }; Review, Search,
  // and other list views use arrays. Patch both shapes through one path.
  if ("pages" in data) {
    const paged = data as { pages: unknown[]; pageParams: unknown[] };
    return { ...paged, pages: paged.pages.map((page) => patchThreadData(page, threadId, patch)) };
  }

  const thread = data as ThreadLike;
  return thread.id === threadId || thread.thread_id === threadId
    ? { ...thread, ...patch }
    : data;
}

/**
 * The shared optimistic UI path for read/unread/star state. Rust persists the
 * desired state locally and queues remote IMAP delivery; this hook keeps every
 * visible list in sync before that background delivery completes.
 */
export function useMailFlags() {
  const queryClient = useQueryClient();

  const patchEverywhere = useCallback((threadId: string, patch: ThreadPatch) => {
    for (const queryKey of [["threads"], ["review_queue"], ["follow_ups"], ["search"]]) {
      queryClient.setQueriesData({ queryKey }, (old) => patchThreadData(old, threadId, patch));
    }
    const state = useAppStore.getState();
    state.setThreads(state.threads.map((thread) => thread.id === threadId ? { ...thread, ...patch } : thread));
  }, [queryClient]);

  const refresh = useCallback(() => Promise.all([
    queryClient.invalidateQueries({ queryKey: ["threads"] }),
    queryClient.invalidateQueries({ queryKey: ["review_queue"] }),
    queryClient.invalidateQueries({ queryKey: ["follow_ups"] }),
    queryClient.invalidateQueries({ queryKey: ["search"] }),
    queryClient.invalidateQueries({ queryKey: ["unread_counts"] }),
  ]), [queryClient]);

  const update = useCallback(async (
    threadId: string,
    patch: ThreadPatch,
    operation: () => Promise<void>,
  ) => {
    patchEverywhere(threadId, patch);
    try {
      await operation();
      void refresh(); // Local SQLite validation; never waits for Gmail.
    } catch (error) {
      await refresh(); // Restore from SQLite if the local mutation failed.
      throw error;
    }
  }, [patchEverywhere, refresh]);

  const markRead = useCallback(
    (threadId: string) => update(threadId, { unread: false }, () => api.markThreadRead(threadId)),
    [update],
  );
  const markUnread = useCallback(
    (threadId: string) => update(threadId, { unread: true }, () => api.markThreadUnread(threadId)),
    [update],
  );
  const setStarred = useCallback(
    (threadId: string, starred: boolean) => update(threadId, { starred }, () => api.starThread(threadId, starred)),
    [update],
  );

  return { markRead, markUnread, setStarred };
}
