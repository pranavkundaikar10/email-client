import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../lib/api";
import { useAppStore } from "../store";

const PREFETCH_AHEAD = 3;
const IMMEDIATE_PREFETCH_COUNT = 2;
const IDLE_DELAY_MS = 75;

/** Warm SQLite and TanStack caches for the next visible emails without changing Gmail state. */
export function useUpcomingBodyPrefetch(email: string) {
  const selectedThreadId = useAppStore((state) => state.selectedThreadId);
  const threads = useAppStore((state) => state.threads);
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!selectedThreadId) return;
    const index = threads.findIndex((thread) => thread.id === selectedThreadId);
    if (index < 0) return;
    const threadIds = threads.slice(index + 1, index + 1 + PREFETCH_AHEAD).map((thread) => thread.id);
    if (threadIds.length === 0) return;
    const immediateIds = threadIds.slice(0, IMMEDIATE_PREFETCH_COUNT);
    const followUpIds = threadIds.slice(IMMEDIATE_PREFETCH_COUNT);

    let cancelled = false;
    async function warmBodies(ids: string[]) {
      if (ids.length === 0) return;
      await api.prefetchThreadBodies(email, ids);
      if (cancelled) return;
      await Promise.all(ids.map((threadId) => queryClient.prefetchQuery({
        queryKey: ["messages", threadId],
        queryFn: () => api.getMessages(threadId),
        staleTime: 5 * 60_000,
      })));
    }

    const timer = window.setTimeout(async () => {
      try {
        // Start the closest two while the selected email is loading. The
        // selected message remains the UI's first request; this is one
        // background batch, not a burst of individual IMAP connections.
        await warmBodies(immediateIds);
        if (cancelled) return;
        // Finish the small look-ahead window only if the user is still moving
        // through this same part of the list.
        await warmBodies(followUpIds);
      } catch (error) {
        // Prefetching is opportunistic; opening the email still uses the
        // normal body fetch and should not expose background failures.
        console.warn("Email body prefetch skipped:", error);
      }
    }, IDLE_DELAY_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [email, selectedThreadId, threads, queryClient]);
}
