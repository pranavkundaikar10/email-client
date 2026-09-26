import { useEffect, useRef, useMemo, useState } from "react";
import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { api, type Thread } from "../../lib/api";
import ThreadItem from "./ThreadItem";
import { useAppStore } from "../../store";
import { useMailActions } from "../../hooks/useMailActions";
import { useVisibleThreadList } from "../../hooks/useVisibleThreadList";
import BulkActionBar from "./BulkActionBar";

const PAGE_SIZE = 50;

interface Props {
  activeView: string;
  effectiveSplitId?: string | null;
  searchResults?: Thread[];
  isSearching?: boolean;
}

export default function ThreadList({ activeView, effectiveSplitId, searchResults, isSearching }: Props) {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const checkedThreadIds = useAppStore((s) => s.checkedThreadIds);
  const toggleThreadCheck = useAppStore((s) => s.toggleThreadCheck);
  const clearChecked = useAppStore((s) => s.clearChecked);

  const addToast = useAppStore((s) => s.addToast);
  const queryClient = useQueryClient();
  const { archiveThreads, deleteThreads } = useMailActions();
  const [syncingOlder, setSyncingOlder] = useState(false);
  const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());

  const {
    data,
    isLoading,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useInfiniteQuery({
    queryKey: ["threads", activeView, effectiveSplitId ?? null],
    queryFn: ({ pageParam = 0 }) =>
      api.getThreads(PAGE_SIZE, pageParam as number, activeView, effectiveSplitId ?? undefined),
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) =>
      lastPage.length < PAGE_SIZE ? undefined : allPages.flat().length,
    refetchInterval: 60_000,
    enabled: !isSearching,
  });

  const inboxThreads = useMemo(() => data?.pages.flat() ?? [], [data]);

  const visibleThreads = useMemo(
    () => (isSearching ? searchResults ?? [] : inboxThreads),
    [isSearching, searchResults, inboxThreads]
  );

  useVisibleThreadList(visibleThreads);

  useEffect(() => {
    if (!selectedThreadId) return;
    const el = itemRefs.current.get(selectedThreadId);
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selectedThreadId]);

  function handleScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 200;
    if (!nearBottom) return;

    if (hasNextPage && !isFetchingNextPage) {
      fetchNextPage();
      return;
    }

    if (!hasNextPage && !syncingOlder && !isSearching && visibleThreads.length > 0) {
      const oldest = [...visibleThreads].sort((a, b) =>
        a.last_message_at < b.last_message_at ? -1 : 1
      )[0];
      if (!oldest) return;
      setSyncingOlder(true);
      api.syncOlder(oldest.account_id, oldest.last_message_at)
        .then((count) => {
          if (count > 0) {
            queryClient.invalidateQueries({ queryKey: ["threads", activeView, effectiveSplitId ?? null] });
          }
        })
        .finally(() => setSyncingOlder(false));
    }
  }

  async function handleBulkArchive() {
    const ids = Array.from(checkedThreadIds);
    try { await archiveThreads(ids); }
    catch (error) { addToast(`Could not queue archive: ${String(error)}`); }
  }

  async function handleBulkDelete() {
    const ids = Array.from(checkedThreadIds);
    try { await deleteThreads(ids); }
    catch (error) { addToast(`Could not queue delete: ${String(error)}`); }
  }

  function handleStar(thread: Thread) {
    const newStarred = !thread.starred;
    // Optimistic update across all cached thread pages
    queryClient.setQueriesData<{ pages: Thread[][]; pageParams: number[] }>(
      { queryKey: ["threads"] },
      (old) =>
        old
          ? {
              ...old,
              pages: old.pages.map((page) =>
                page.map((t) => (t.id === thread.id ? { ...t, starred: newStarred } : t))
              ),
            }
          : old
    );
    api.starThread(thread.id, newStarred).then(() => {
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    });
  }

  const renderThread = (thread: Thread) => (
    <div
      key={thread.id}
      ref={(el) => {
        if (el) itemRefs.current.set(thread.id, el);
        else itemRefs.current.delete(thread.id);
      }}
    >
      <ThreadItem
        thread={thread}
        selected={thread.id === selectedThreadId}
        checked={checkedThreadIds?.has(thread.id) ?? false}
        onClick={() => setSelectedThread(thread.id)}
        onCheck={() => toggleThreadCheck(thread.id)}
        onStar={() => handleStar(thread)}
      />
    </div>
  );

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <BulkActionBar
        count={checkedThreadIds?.size ?? 0}
        onArchive={handleBulkArchive}
        onDelete={handleBulkDelete}
        onClear={clearChecked}
      />

      {!isSearching && isLoading ? (
        <div className="flex-1 flex items-center justify-center text-sm text-gray-400">
          Loading…
        </div>
      ) : visibleThreads.length === 0 ? (
        <div className="flex-1 flex items-center justify-center text-sm text-gray-400">
          {isSearching ? "No results" : "No emails"}
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto" onScroll={handleScroll}>
          {visibleThreads.map(renderThread)}
          {(isFetchingNextPage || syncingOlder) && (
            <div className="py-2 flex items-center justify-center">
              <span className="text-xs text-gray-400">
                {syncingOlder ? "Fetching older emails from Gmail…" : "Loading…"}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
