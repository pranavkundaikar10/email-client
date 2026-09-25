import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { api } from "../../lib/api";
import ThreadItem from "../email/ThreadItem";
import { useAppStore } from "../../store";

export default function ReviewList() {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const setThreads = useAppStore((s) => s.setThreads);
  const itemRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const { data: queue = [], isLoading } = useQuery({
    queryKey: ["review_queue"],
    queryFn: () => api.getReviewQueue(),
    refetchInterval: 60_000,
  });

  useEffect(() => {
    setThreads(queue);
    if (!selectedThreadId && queue[0]) setSelectedThread(queue[0].thread_id);
    if (selectedThreadId && !queue.some((item) => item.thread_id === selectedThreadId)) {
      setSelectedThread(queue[0]?.thread_id ?? null);
    }
  }, [queue, selectedThreadId, setSelectedThread, setThreads]);

  // Match the Inbox list: keyboard navigation keeps the selected review item
  // visible as the selection moves beyond the current viewport.
  useEffect(() => {
    if (!selectedThreadId) return;
    itemRefs.current.get(selectedThreadId)?.scrollIntoView({
      block: "nearest",
      behavior: "smooth",
    });
  }, [selectedThreadId]);

  if (isLoading) return <div className="flex-1 flex items-center justify-center text-sm text-gray-400">Loading…</div>;
  if (queue.length === 0) {
    return <div className="flex-1 flex flex-col items-center justify-center px-6 text-center">
      <Check size={22} className="text-emerald-500" />
      <p className="mt-3 text-sm font-medium text-gray-700">You’re caught up</p>
      <p className="mt-1 text-xs text-gray-400">Today’s analyzed emails will appear here.</p>
    </div>;
  }

  return <div className="flex-1 overflow-y-auto">
    {queue.map((item) => <div
      key={item.thread_id}
      ref={(element) => {
        if (element) itemRefs.current.set(item.thread_id, element);
        else itemRefs.current.delete(item.thread_id);
      }}
    ><ThreadItem
      thread={item}
      selected={item.thread_id === selectedThreadId}
      checked={false}
      onClick={() => setSelectedThread(item.thread_id)}
      onCheck={() => {}}
      onStar={() => {}}
    /></div>)}
  </div>;
}
