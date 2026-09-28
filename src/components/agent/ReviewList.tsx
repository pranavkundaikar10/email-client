import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, LoaderCircle } from "lucide-react";
import { api } from "../../lib/api";
import ThreadItem from "../email/ThreadItem";
import BulkActionBar from "../email/BulkActionBar";
import { useAppStore } from "../../store";
import { useMailActions } from "../../hooks/useMailActions";
import { useMailFlags } from "../../hooks/useMailFlags";
import { useVisibleThreadList } from "../../hooks/useVisibleThreadList";
import type { FollowUpItem, ReviewItem } from "../../lib/api";

type ReviewSidebarItem = ReviewItem | FollowUpItem;

function dueLabel(dueAt: string) {
  const due = new Date(dueAt);
  const now = new Date();
  if (due < now) {
    const hours = Math.max(1, Math.floor((now.getTime() - due.getTime()) / 3_600_000));
    return hours >= 24 ? `Overdue · ${Math.floor(hours / 24)}d` : `Overdue · ${hours}h`;
  }
  return `Due today · ${due.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`;
}

export default function ReviewList() {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const checkedThreadIds = useAppStore((s) => s.checkedThreadIds);
  const toggleThreadCheck = useAppStore((s) => s.toggleThreadCheck);
  const clearChecked = useAppStore((s) => s.clearChecked);
  const addToast = useAppStore((s) => s.addToast);
  const { archiveThreads, deleteThreads } = useMailActions();
  const { setStarred } = useMailFlags();
  const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [sort, setSort] = useState<"priority" | "newest" | "oldest">("priority");
  const { data: queue = [], isLoading } = useQuery({
    queryKey: ["review_queue", sort],
    queryFn: () => api.getReviewQueue(50, sort),
    refetchInterval: 60_000,
  });
  const { data: pendingCount = 0 } = useQuery({
    queryKey: ["auto_analysis_pending_count"],
    queryFn: api.getAutoAnalysisPendingCount,
    refetchInterval: 60_000,
  });
  const { data: followUps = [] } = useQuery({
    queryKey: ["follow_ups"],
    queryFn: api.getFollowUps,
    refetchInterval: 60_000,
  });

  const [dueFollowUps, today, earlier] = useMemo(() => {
    const startOfToday = new Date();
    startOfToday.setHours(0, 0, 0, 0);
    const endToday = new Date(startOfToday);
    endToday.setDate(endToday.getDate() + 1);
    const due = followUps
      .filter((item) => new Date(item.due_at) < endToday)
      .sort((a, b) => new Date(a.due_at).getTime() - new Date(b.due_at).getTime());
    const dueIds = new Set(due.map((item) => item.thread_id));
    const reviewItems = queue.filter((item) => !dueIds.has(item.thread_id));
    return [
      due,
      reviewItems.filter((item) => new Date(item.last_message_at) >= startOfToday),
      reviewItems.filter((item) => new Date(item.last_message_at) < startOfToday),
    ];
  }, [followUps, queue]);
  // This is also the literal render order below, so J/K cannot jump between
  // groups based on the API's priority order.
  const visibleReviewThreads = useMemo(() => [...dueFollowUps, ...today, ...earlier], [dueFollowUps, today, earlier]);
  useVisibleThreadList(visibleReviewThreads);

  useEffect(() => {
    if (!selectedThreadId && visibleReviewThreads[0]) setSelectedThread(visibleReviewThreads[0].thread_id);
    if (selectedThreadId && !visibleReviewThreads.some((item) => item.thread_id === selectedThreadId)) {
      setSelectedThread(visibleReviewThreads[0]?.thread_id ?? null);
    }
  }, [visibleReviewThreads, selectedThreadId, setSelectedThread]);

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

  function renderItems(items: ReviewSidebarItem[], followUp = false) {
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
      importance={"importance" in item ? item.importance : undefined}
      footer={followUp && "due_at" in item
        ? <p className="px-3 pb-2 text-[10px] font-medium text-violet-600">{dueLabel(item.due_at)}</p>
        : undefined}
      onClick={() => setSelectedThread(item.thread_id)}
      onCheck={() => toggleThreadCheck(item.thread_id)}
      onStar={() => { void setStarred(item.thread_id, !item.starred); }}
    /></div>);
  }

  if (isLoading) return <div className="flex-1 flex items-center justify-center text-sm text-gray-400">Loading…</div>;
  if (visibleReviewThreads.length === 0) {
    return <div className="flex-1 flex flex-col items-center justify-center px-6 text-center">
      {pendingCount > 0 ? <>
        <LoaderCircle size={22} className="animate-spin text-violet-500" />
        <p className="mt-3 text-sm font-medium text-gray-700">Preparing {pendingCount} {pendingCount === 1 ? "email" : "emails"}</p>
      </> : <>
        <Check size={22} className="text-emerald-500" />
        <p className="mt-3 text-sm font-medium text-gray-700">You’re caught up</p>
        <p className="mt-1 text-xs text-gray-400">New analyzed emails will appear here automatically.</p>
      </>}
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
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">{visibleReviewThreads.length} to review</span>
        {pendingCount > 0 && <span
          className="flex items-center gap-1 text-[10px] font-medium tabular-nums text-violet-500"
          title={`${pendingCount} ${pendingCount === 1 ? "email is" : "emails are"} awaiting AI analysis`}
          aria-label={`${pendingCount} ${pendingCount === 1 ? "email" : "emails"} awaiting AI analysis`}
        >
          <LoaderCircle size={10} className="animate-spin" />
          {pendingCount}
        </span>}
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
    {dueFollowUps.length > 0 && <>
      <p className="border-b border-violet-100 bg-violet-50/50 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-violet-700">Due follow-ups · {dueFollowUps.length}</p>
      {renderItems(dueFollowUps, true)}
    </>}
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
