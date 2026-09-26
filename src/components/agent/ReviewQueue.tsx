import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, ArrowRight, Bookmark, CalendarClock, Check, Eye, X } from "lucide-react";
import { api, type ReviewItem } from "../../lib/api";
import { useAppStore } from "../../store";
import { useMailActions } from "../../hooks/useMailActions";

function actionItems(json: string): string[] {
  try {
    const value = JSON.parse(json);
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

function severity(importance: number): string {
  if (importance >= 5) return "Critical";
  if (importance >= 4) return "High";
  if (importance >= 3) return "Medium";
  return "Low";
}

function recommendation(item: ReviewItem): "archive" | "keep" | "review" {
  if (item.category === "assessment" || item.category === "interview" || item.category === "offer" || item.category === "deadline") return "keep";
  if (item.is_actionable || item.importance >= 3) return "review";
  if (item.category === "rejection" || item.category === "newsletter" || item.importance <= 2) return "archive";
  return "review";
}

export default function ReviewQueue({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const addToast = useAppStore((s) => s.addToast);
  const [offset, setOffset] = useState(0);
  const [saving, setSaving] = useState(false);
  const { archiveThread } = useMailActions();
  const { data: queue = [], isLoading } = useQuery({
    queryKey: ["review_queue"],
    queryFn: () => api.getReviewQueue(),
  });
  const item = queue[offset] ?? queue[0];

  async function decide(decision: "keep" | "follow_up" | "archived") {
    if (!item) return;
    setSaving(true);
    try {
      if (decision === "archived") await archiveThread(item.thread_id);
      await api.recordReviewDecision(item.thread_id, decision);
      await queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      await queryClient.invalidateQueries({ queryKey: ["threads"] });
      addToast(decision === "archived" ? "Archive queued" : decision === "keep" ? "Kept in inbox" : "Marked for follow-up");
      setOffset(0);
    } catch (error) {
      addToast(`Could not save decision: ${String(error)}`);
    } finally {
      setSaving(false);
    }
  }

  function openEmail() {
    if (!item) return;
    setSelectedThread(item.thread_id);
    onClose();
  }

  const suggested = item && recommendation(item);
  const items = item ? actionItems(item.action_items) : [];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-[640px] max-h-[80vh] overflow-hidden rounded-xl bg-white shadow-2xl flex flex-col">
        <div className="flex items-center justify-between border-b border-gray-100 px-6 py-4">
          <div>
            <h2 className="text-sm font-semibold text-gray-900">Review queue</h2>
            <p className="mt-0.5 text-xs text-gray-400">Today’s analyzed inbox emails · you choose every action</p>
          </div>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600"><X size={16} /></button>
        </div>

        <div className="flex-1 overflow-y-auto px-6 py-5">
          {isLoading ? <p className="py-10 text-center text-xs text-gray-400">Loading review queue…</p>
            : !item ? (
              <div className="py-12 text-center">
                <Check size={22} className="mx-auto text-emerald-500" />
                <p className="mt-3 text-sm font-medium text-gray-700">You’re caught up</p>
                <p className="mt-1 text-xs text-gray-400">New analyzed emails from today will appear here.</p>
              </div>
            ) : (
              <>
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <p className="text-sm font-semibold text-gray-900">{item.from_name || item.from_email}</p>
                    <p className="mt-0.5 text-xs text-gray-400 truncate">{item.subject || "(no subject)"}</p>
                  </div>
                  <span className="flex-shrink-0 rounded-full bg-indigo-50 px-2 py-1 text-[10px] font-semibold text-indigo-700">
                    {severity(item.importance)} · {item.importance}/5
                  </span>
                </div>

                <div className="mt-5 rounded-lg border border-indigo-100 bg-indigo-50/60 px-4 py-3">
                  <p className="text-[10px] font-semibold uppercase tracking-wide text-indigo-500">AI analysis</p>
                  <p className="mt-1 text-sm leading-relaxed text-indigo-900">{item.summary || "No summary available."}</p>
                  {item.deadline && <p className="mt-2 text-xs font-medium text-red-600">Due {item.deadline}</p>}
                  {items.length > 0 && (
                    <ul className="mt-3 space-y-1">
                      {items.map((action, index) => <li key={index} className="text-xs text-indigo-800 flex gap-2"><span>•</span>{action}</li>)}
                    </ul>
                  )}
                </div>

                <div className="mt-4 flex items-center justify-between rounded-lg bg-gray-50 px-3 py-2.5">
                  <span className="text-xs text-gray-500">Suggested decision</span>
                  <span className={`text-xs font-semibold ${suggested === "archive" ? "text-gray-600" : suggested === "keep" ? "text-emerald-700" : "text-amber-700"}`}>
                    {suggested === "archive" ? "Archive — low risk" : suggested === "keep" ? "Keep in inbox" : "Needs your review"}
                  </span>
                </div>
              </>
            )}
        </div>

        {item && <div className="border-t border-gray-100 px-6 py-4">
          <div className="flex items-center justify-between gap-3">
            <button onClick={openEmail} className="flex items-center gap-1.5 text-xs text-gray-500 hover:text-gray-800"><Eye size={14} /> Open email</button>
            <div className="flex items-center gap-2">
              <button disabled={saving} onClick={() => decide("follow_up")} className="flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs text-amber-700 hover:bg-amber-50 disabled:opacity-40"><CalendarClock size={13} /> Follow up</button>
              <button disabled={saving} onClick={() => decide("keep")} className="flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs text-indigo-700 hover:bg-indigo-50 disabled:opacity-40"><Bookmark size={13} /> Keep</button>
              <button disabled={saving} onClick={() => decide("archived")} className="flex items-center gap-1 rounded-lg bg-gray-900 px-3 py-1.5 text-xs text-white hover:bg-gray-700 disabled:opacity-40"><Archive size={13} /> Archive</button>
            </div>
          </div>
          {queue.length > 1 && <button disabled={saving} onClick={() => setOffset((current) => (current + 1) % queue.length)} className="mt-3 flex items-center gap-1 text-xs text-gray-400 hover:text-gray-600">Skip for now <ArrowRight size={13} /></button>}
        </div>}
      </div>
    </div>
  );
}
