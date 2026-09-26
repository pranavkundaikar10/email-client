import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../lib/api";
import { useAppStore } from "../store";

type MailAction = "archive" | "delete";
type ThreadLike = { id?: string; thread_id?: string };

function withoutThreads(data: unknown, ids: Set<string>): unknown {
  if (Array.isArray(data)) {
    return data.filter((item) => {
      const thread = item as ThreadLike;
      return !ids.has(thread.thread_id ?? thread.id ?? "");
    });
  }
  // React Query stores paginated inboxes as { pages, pageParams }.
  if (data && typeof data === "object" && "pages" in data) {
    const paged = data as { pages: unknown[]; pageParams: unknown[] };
    return { ...paged, pages: paged.pages.map((page) => withoutThreads(page, ids)) };
  }
  return data;
}

/**
 * The single client-side path for Gmail-changing actions. It removes mail from
 * every view immediately, while the Rust outbox owns reliable remote delivery
 * and eventual failure recovery.
 */
export function useMailActions() {
  const queryClient = useQueryClient();

  const undo = useCallback(async (ids: string[]) => {
    const restored = await api.cancelMailOperations(ids);
    if (restored === 0) {
      useAppStore.getState().addToast("This action is already being synced to Gmail.");
      return;
    }
    await Promise.all([
      queryClient.refetchQueries({ queryKey: ["threads"] }),
      queryClient.refetchQueries({ queryKey: ["review_queue"] }),
      queryClient.refetchQueries({ queryKey: ["search"] }),
      queryClient.refetchQueries({ queryKey: ["digest"] }),
    ]);
    useAppStore.getState().addToast(
      restored === 1 ? "Email restored" : `${restored} emails restored`
    );
  }, [queryClient]);

  const queue = useCallback(async (action: MailAction, threadIds: string[]) => {
    const ids = [...new Set(threadIds)];
    if (ids.length === 0) return;
    const idSet = new Set(ids);

    // All views have distinct query keys and shapes, so update them together
    // rather than making each feature reinvent an optimistic delete.
    for (const queryKey of [["threads"], ["review_queue"], ["search"], ["digest"]]) {
      queryClient.setQueriesData({ queryKey }, (old) => withoutThreads(old, idSet));
    }

    const state = useAppStore.getState();
    const remaining = state.threads.filter((thread) => !idSet.has(thread.id));
    if (state.selectedThreadId && idSet.has(state.selectedThreadId)) {
      const selectedIndex = state.threads.findIndex((thread) => thread.id === state.selectedThreadId);
      // Pick the next visible survivor in the *original* list, not the same
      // numeric index in the shortened list. The latter skips emails whenever
      // a bulk action removes entries before the active selection.
      const fallback = state.threads.slice(selectedIndex + 1).find((thread) => !idSet.has(thread.id))
        ?? state.threads.slice(0, selectedIndex).reverse().find((thread) => !idSet.has(thread.id))
        ?? null;
      state.setSelectedThread(fallback?.id ?? null);
    }
    state.setThreads(remaining);
    state.clearChecked();

    const request = action === "archive" ? api.archiveThread : api.deleteThread;
    const results = await Promise.allSettled(ids.map((id) => request(id)));
    const failures = results.filter((result) => result.status === "rejected");
    if (failures.length > 0) {
      // The local queue could not be created, so immediately restore from the
      // database. Later Gmail failures are handled by the global failure event.
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["threads"] }),
        queryClient.invalidateQueries({ queryKey: ["review_queue"] }),
        queryClient.invalidateQueries({ queryKey: ["search"] }),
        queryClient.invalidateQueries({ queryKey: ["digest"] }),
      ]);
      throw new Error(`${failures.length} email action${failures.length === 1 ? "" : "s"} could not be queued`);
    }

    // This validates our optimistic state against SQLite. It is local and does
    // not wait for Gmail; the pending-operation filter keeps the item hidden.
    void queryClient.invalidateQueries({ queryKey: ["threads"] });
    void queryClient.invalidateQueries({ queryKey: ["review_queue"] });
    void queryClient.invalidateQueries({ queryKey: ["search"] });
    void queryClient.invalidateQueries({ queryKey: ["digest"] });

    const label = action === "archive" ? "Archived" : "Moved to Gmail Trash";
    useAppStore.getState().addToast(
      ids.length === 1 ? label : `${label} · ${ids.length} emails`,
      { actionLabel: "Undo", onAction: () => undo(ids), duration: 8_000 }
    );
  }, [queryClient, undo]);

  return {
    archiveThreads: (threadIds: string[]) => queue("archive", threadIds),
    deleteThreads: (threadIds: string[]) => queue("delete", threadIds),
    archiveThread: (threadId: string) => queue("archive", [threadId]),
    deleteThread: (threadId: string) => queue("delete", [threadId]),
  };
}
