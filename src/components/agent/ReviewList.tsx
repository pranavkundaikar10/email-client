import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { api } from "../../lib/api";
import ThreadItem from "../email/ThreadItem";
import BulkActionBar from "../email/BulkActionBar";
import { useAppStore } from "../../store";
import { useMailActions } from "../../hooks/useMailActions";
import { useVisibleThreadList } from "../../hooks/useVisibleThreadList";

export default function ReviewList() {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const checkedThreadIds = useAppStore((s) => s.checkedThreadIds);
  const toggleThreadCheck = useAppStore((s) => s.toggleThreadCheck);
  const clearChecked = useAppStore((s) => s.clearChecked);
  const addToast = useAppStore((s) => s.addToast);
  const { archiveThreads, deleteThreads } = useMailActions();
  const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [sort, setSort] = useState<"priority" | "newest" | "oldest">("priority");
  const { data: queue = [], isLoading } = useQuery({
    queryKey: ["review_queue", sort],
    queryFn: () => api.getReviewQueue(50, sort),
    refetchInterval: 60_000,
  });

  const [today, earlier] = useMemo(() => {
    const startOfToday = new Date();
    startOfToday.setHours(0, 0, 0, 0);
    return [
      queue.filter((item) => new Date(item.last_message_at) >= startOfToday),
      queue.filter((item) => new Date(item.last_message_at) < startOfToday),
    ];
  }, [queue]);
  // This is also the literal render order below, so J/K cannot jump between
  // groups based on the API's priority order.
  const visibleReviewThreads = useMemo(() => [...today, ...earlier], [today, earlier]);
  useVisibleThreadList(visibleReviewThreads);

  useEffect(() => {
    if (!selectedThreadId && visibleReviewThreads[0]) setSelectedThread(visibleReviewThreads[0].thread_id);
    if (selectedThreadId && !visibleReviewThreads.some((item) => item.thread_id === selectedThreadId)) {
      setSelectedThread(visibleReviewThreads[0]?.thread_id ?? null);
    }
  }, [queue, visibleReviewThreads, selectedThreadId, setSelectedThread]);

  // Match the Inbox list: keyboard navigation keeps the selected review item
  // visible as the selection moves beyond the current viewport.
  useEffect(() => {
    if (!selectedThreadId) return;
    itemRefs.current.get(selectedThreadId)?.scrollIntoView({
      block: "nearest",
      behavior: "smooth",
    });
  }, [selectedThreadId]);

  async function handleBulkArchive() {
    try { await archiveThreads(Array.from(checkedThreadIds)); }
    catch (error) { addToast(`Could not queue archive: ${String(error)}`); }
  }

  async function handleBulkDelete() {
    try { await deleteThreads(Array.from(checkedThreadIds)); }
    catch (error) { addToast(`Could not queue delete: ${String(error)}`); }
  }

  function renderItems(items: typeof queue) {
    return items.map((item) => <div
      key={item.thread_id}
      ref={(element) => {
        if (element) itemRefs.current.set(item.thread_id, element);
        else itemRefs.current.delete(item.thread_id);
      }}
    ><ThreadItem
      thread={item}
      selected={item.thread_id === selectedThreadId}
      checked={checkedThreadIds.has(item.thread_id)}
      importance={item.importance}
      onClick={() => setSelectedThread(item.thread_id)}
      onCheck={() => toggleThreadCheck(item.thread_id)}
      onStar={() => {}}
    /></div>);
  }

  if (isLoading) return <div className="flex-1 flex items-center justify-center text-sm text-gray-400">Loading…</div>;
  if (queue.length === 0) {
    return <div className="flex-1 flex flex-col items-center justify-center px-6 text-center">
      <Check size={22} className="text-emerald-500" />
      <p className="mt-3 text-sm font-medium text-gray-700">You’re caught up</p>
      <p className="mt-1 text-xs text-gray-400">New analyzed emails will appear here automatically.</p>
    </div>;
  }

  return <div className="flex-1 flex flex-col overflow-hidden">
    <BulkActionBar
      count={checkedThreadIds.size}
      onArchive={handleBulkArchive}
      onDelete={handleBulkDelete}
      onClear={clearChecked}
    />
    <div className="flex items-center justify-between border-b border-gray-100 px-3 py-2">
      <div>
        <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">{queue.length} to review</span>
      </div>
      <div className="flex items-center gap-2">
        <select
          value={sort}
          onChange={(event) => setSort(event.target.value as typeof sort)}
          className="bg-transparent text-[11px] text-gray-500 outline-none"
          aria-label="Sort review queue"
        >
          <option value="priority">Priority</option>
          <option value="newest">Newest first</option>
          <option value="oldest">Oldest first</option>
        </select>
      </div>
    </div>
    <div className="flex-1 overflow-y-auto">
    {today.length > 0 && <>
      <p className="border-b border-gray-100 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-gray-400">Today · {today.length}</p>
      {renderItems(today)}
    </>}
    {earlier.length > 0 && <>
      <p className="border-y border-gray-100 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-gray-400">Earlier · {earlier.length}</p>
      {renderItems(earlier)}
    </>}
    </div>
  </div>;
}
