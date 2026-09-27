import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { CalendarClock, Check } from "lucide-react";
import { api, type FollowUpItem } from "../../lib/api";
import ThreadItem from "../email/ThreadItem";
import BulkActionBar from "../email/BulkActionBar";
import { useAppStore } from "../../store";
import { useMailActions } from "../../hooks/useMailActions";
import { useMailFlags } from "../../hooks/useMailFlags";
import { useVisibleThreadList } from "../../hooks/useVisibleThreadList";

function dueLabel(dueAt: string) {
  return new Date(dueAt).toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

export default function FollowUpList() {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const checkedThreadIds = useAppStore((s) => s.checkedThreadIds);
  const toggleThreadCheck = useAppStore((s) => s.toggleThreadCheck);
  const clearChecked = useAppStore((s) => s.clearChecked);
  const addToast = useAppStore((s) => s.addToast);
  const { archiveThreads, deleteThreads } = useMailActions();
  const { setStarred } = useMailFlags();
  const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const { data: followUps = [], isLoading } = useQuery({
    queryKey: ["follow_ups"],
    queryFn: api.getFollowUps,
    refetchInterval: 60_000,
  });
  useVisibleThreadList(followUps);

  const [overdue, today, upcoming] = useMemo(() => {
    const now = new Date();
    const startToday = new Date(now);
    startToday.setHours(0, 0, 0, 0);
    const endToday = new Date(startToday);
    endToday.setDate(endToday.getDate() + 1);
    return [
      followUps.filter((item) => new Date(item.due_at) < now),
      followUps.filter((item) => new Date(item.due_at) >= now && new Date(item.due_at) < endToday),
      followUps.filter((item) => new Date(item.due_at) >= endToday),
    ];
  }, [followUps]);

  useEffect(() => {
    if (!selectedThreadId && followUps[0]) setSelectedThread(followUps[0].thread_id);
    if (selectedThreadId && !followUps.some((item) => item.thread_id === selectedThreadId)) {
      setSelectedThread(followUps[0]?.thread_id ?? null);
    }
  }, [followUps, selectedThreadId, setSelectedThread]);

  useEffect(() => {
    if (selectedThreadId) itemRefs.current.get(selectedThreadId)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selectedThreadId]);

  async function archiveSelected() {
    try { await archiveThreads(Array.from(checkedThreadIds)); }
    catch (error) { addToast(`Could not queue archive: ${String(error)}`); }
  }
  async function deleteSelected() {
    try { await deleteThreads(Array.from(checkedThreadIds)); }
    catch (error) { addToast(`Could not queue delete: ${String(error)}`); }
  }
  function render(items: FollowUpItem[]) {
    return items.map((item) => <div key={item.thread_id} ref={(element) => {
      if (element) itemRefs.current.set(item.thread_id, element);
      else itemRefs.current.delete(item.thread_id);
    }}>
      <ThreadItem
        thread={item}
        selected={item.thread_id === selectedThreadId}
        checked={checkedThreadIds.has(item.thread_id)}
        onClick={() => setSelectedThread(item.thread_id)}
        onCheck={() => toggleThreadCheck(item.thread_id)}
        onStar={() => { void setStarred(item.thread_id, !item.starred); }}
      />
      <p className="-mt-1 border-b border-gray-50 px-3 pb-2 text-[10px] font-medium text-violet-500">Due {dueLabel(item.due_at)}</p>
    </div>);
  }

  if (isLoading) return <div className="flex flex-1 items-center justify-center text-sm text-gray-400">Loading…</div>;
  if (!followUps.length) return <div className="flex flex-1 flex-col items-center justify-center px-6 text-center">
    <Check size={22} className="text-emerald-500" />
    <p className="mt-3 text-sm font-medium text-gray-700">No follow-ups</p>
    <p className="mt-1 text-xs text-gray-400">Scheduled follow-ups will appear here.</p>
  </div>;

  return <div className="flex flex-1 flex-col overflow-hidden">
    <BulkActionBar count={checkedThreadIds.size} onArchive={archiveSelected} onDelete={deleteSelected} onClear={clearChecked} />
    <div className="flex items-center gap-1.5 border-b border-gray-100 px-3 py-2">
      <CalendarClock size={12} className="text-violet-500" />
      <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">{followUps.length} follow-up{followUps.length === 1 ? "" : "s"}</span>
    </div>
    <div className="flex-1 overflow-y-auto">
      {overdue.length > 0 && <><p className="border-b border-red-100 bg-red-50/40 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-red-500">Overdue · {overdue.length}</p>{render(overdue)}</>}
      {today.length > 0 && <><p className="border-y border-amber-100 bg-amber-50/40 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-amber-600">Today · {today.length}</p>{render(today)}</>}
      {upcoming.length > 0 && <><p className="border-y border-gray-100 px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-gray-400">Upcoming · {upcoming.length}</p>{render(upcoming)}</>}
    </div>
  </div>;
}
