import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type Message } from "../../lib/api";
import { useAppStore } from "../../store";
import { useEffect, useRef, useState } from "react";
import { Archive, MailOpen, Reply, Trash2, Sparkles } from "lucide-react";
import ReplyComposer from "./ReplyComposer";
import { openUrl } from "@tauri-apps/plugin-opener";

// Renders HTML email in an isolated iframe so its <style> tags cannot
// leak out and shift the host page layout.
// Uses allow-scripts (no allow-same-origin) + postMessage bridge so link clicks
// open in the system browser and height is reported without cross-origin access.
function IsolatedHtml({ html }: { html: string }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(150);

  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (e.source !== ref.current?.contentWindow) return;
      if (e.data?.type === "height") {
        setHeight((e.data.h as number) + 16);
      } else if (e.data?.type === "open-url") {
        openUrl(String(e.data.url)).catch(() => {});
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // Strip script tags from email content — we inject our own handler below.
  const safeHtml = html.replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, "");

  const srcdoc = `<!DOCTYPE html><html><head>
<style>body{margin:0;font-family:sans-serif;font-size:14px;color:#374151;word-break:break-word}img{max-width:100%}</style>
</head><body>${safeHtml}<script>(function(){
  function h(){window.parent.postMessage({type:'height',h:document.body.scrollHeight},'*');}
  h(); window.addEventListener('load',h);
  document.addEventListener('click',function(e){
    var el=e.target;
    while(el&&el.tagName!=='A')el=el.parentElement;
    if(el&&el.href&&/^(https?|mailto|tel):/.test(el.href)){
      e.preventDefault();
      window.parent.postMessage({type:'open-url',url:el.href},'*');
    }
  });
  document.addEventListener('keydown',function(e){
    if(/^[jkesux#]$/i.test(e.key)||e.key==='Escape'){
      window.parent.postMessage({type:'keydown',key:e.key},'*');
    }
  });
})();<\/script></body></html>`;

  return (
    <iframe
      ref={ref}
      srcDoc={srcdoc}
      sandbox="allow-scripts"
      style={{ width: "100%", height, border: "none", display: "block" }}
    />
  );
}

function formatFullDate(iso: string): string {
  return new Date(iso).toLocaleString([], {
    weekday: "short", month: "short", day: "numeric",
    hour: "2-digit", minute: "2-digit",
  });
}

function MessageCard({
  message, email, isLast,
}: {
  message: Message; email: string; isLast: boolean;
}) {
  const queryClient = useQueryClient();
  const [replyOpen, setReplyOpen] = useState(false);

  const { mutate: fetchBody, isPending, error: fetchError } = useMutation({
    mutationFn: () => api.fetchMessageBody(email, message.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["messages", message.thread_id] });
    },
  });

  useEffect(() => {
    if (isLast && !message.body_fetched) fetchBody();
  }, [isLast, message.body_fetched]);

  const toList = (() => {
    try { return (JSON.parse(message.to_emails) as string[]).join(", "); }
    catch { return message.to_emails; }
  })();

  return (
    <div className="border border-gray-100 rounded-xl mb-3 overflow-hidden">
      {/* Header */}
      <div className="px-5 py-4 bg-white">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-sm font-semibold text-gray-900">
              {message.from_name || message.from_email}
            </p>
            <p className="text-xs text-gray-400 mt-0.5">{message.from_email}</p>
            {toList && <p className="text-xs text-gray-400 mt-0.5">To: {toList}</p>}
          </div>
          <span className="text-xs text-gray-400 flex-shrink-0 mt-0.5">
            {formatFullDate(message.sent_at)}
          </span>
        </div>
      </div>

      {/* Body */}
      <div className="px-5 py-4 bg-white border-t border-gray-50">
        {fetchError ? (
          <p className="text-xs text-red-500 bg-red-50 px-3 py-2 rounded">
            Failed to load: {String(fetchError)}
          </p>
        ) : isPending && !message.body_fetched ? (
          <p className="text-xs text-gray-400">Loading…</p>
        ) : message.body_html ? (
          <IsolatedHtml html={message.body_html} />
        ) : message.body_text ? (
          <pre className="text-sm text-gray-700 whitespace-pre-wrap font-sans leading-relaxed">
            {message.body_text}
          </pre>
        ) : (
          <p className="text-xs text-gray-400 italic">No content</p>
        )}
      </div>

      {/* Reply button — only on last message */}
      {isLast && (
        <div className="px-5 py-3 bg-gray-50 border-t border-gray-100">
          {replyOpen ? (
            <ReplyComposer
              from={email}
              replyTo={message}
              onClose={() => setReplyOpen(false)}
            />
          ) : (
            <button
              onClick={() => setReplyOpen(true)}
              className="flex items-center gap-1.5 text-xs text-gray-500 hover:text-gray-800 transition-colors"
            >
              <Reply size={13} />
              Reply
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function parseActionItems(json: string): string[] {
  try {
    const arr = JSON.parse(json);
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function severityForImportance(importance: number): string {
  if (importance >= 5) return "Critical";
  if (importance >= 4) return "High";
  if (importance >= 3) return "Medium";
  return "Low";
}

// Shows the agent's extracted summary/action items for the open thread, if
// it's already been analyzed (via the Digest scan). Doesn't trigger analysis
// itself — that stays a deliberate, batched action so an 8B local model
// isn't called on every click.
function AnalysisBanner({ threadId }: { threadId: string }) {
  const { data: analysis } = useQuery({
    queryKey: ["thread_analysis", threadId],
    queryFn: () => api.getThreadAnalysis(threadId),
  });

  if (!analysis) return null;
  const items = parseActionItems(analysis.action_items);
  const severity = severityForImportance(analysis.importance);
  const severityStyle = analysis.importance >= 5
    ? "text-red-600 bg-red-50 ring-red-100"
    : analysis.importance >= 4
      ? "text-amber-700 bg-amber-50 ring-amber-100"
      : analysis.importance >= 3
        ? "text-blue-700 bg-blue-50 ring-blue-100"
        : "text-gray-500 bg-gray-100 ring-gray-200";

  return (
    <div className={`mx-6 mt-4 mb-1 px-4 py-3 rounded-lg border ${
      analysis.is_actionable
        ? "bg-indigo-50/60 border-indigo-100"
        : "bg-gray-50 border-gray-100"
    }`}>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-indigo-500">
            <Sparkles size={12} />
            AI analysis
          </div>
          <p className={`mt-1 text-xs font-medium leading-relaxed ${
            analysis.is_actionable ? "text-indigo-800" : "text-gray-700"
          }`}>
            {analysis.summary || (analysis.is_actionable ? "Action needed" : "No action needed")}
          </p>
        </div>
        <div
          title={`Importance ${analysis.importance} out of 5 — ${severity}`}
          className={`flex flex-shrink-0 items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold ring-1 ${severityStyle}`}
        >
          <span>{severity}</span>
          <span className="h-3 w-px bg-current opacity-20" />
          <span>{analysis.importance}/5</span>
        </div>
      </div>
      {analysis.deadline && (
        <p className="mt-1.5 text-[11px] font-medium text-red-600">Due {analysis.deadline}</p>
      )}
      {items.length > 0 && (
        <ul className="mt-1.5 space-y-0.5">
          {items.map((it, i) => (
            <li key={i} className="text-xs text-gray-700 flex gap-1.5">
              <span className="text-indigo-300">•</span>
              {it}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function reviewRecommendation(category: string, actionable: boolean, importance: number) {
  if (category === "rejection" || category === "newsletter" || (!actionable && importance <= 2)) return "Archive — low risk";
  if (["assessment", "interview", "offer", "deadline"].includes(category)) return "Keep in inbox";
  return "Needs your review";
}

export default function EmailPreview({ email, reviewMode = false }: { email: string; reviewMode?: boolean }) {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const selectNextThread = useAppStore((s) => s.selectNextThread);
  const selectPrevThread = useAppStore((s) => s.selectPrevThread);
  const selectNextOrPrev = useAppStore((s) => s.selectNextOrPrev);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const addToast = useAppStore((s) => s.addToast);
  const queryClient = useQueryClient();

  const { data: messages = [], isLoading } = useQuery({
    queryKey: ["messages", selectedThreadId],
    queryFn: () => api.getMessages(selectedThreadId!),
    enabled: !!selectedThreadId,
  });

  const { data: reviewQueue = [] } = useQuery({
    queryKey: ["review_queue"],
    queryFn: () => api.getReviewQueue(),
    enabled: reviewMode,
  });
  const reviewItem = reviewQueue.find((item) => item.thread_id === selectedThreadId);

  const { mutate: archive, isPending: archiving } = useMutation({
    mutationFn: (threadId: string) => api.archiveThread(threadId),
    onSuccess: () => {
      selectNextOrPrev();
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    },
    onError: (err) => addToast(`Archive failed: ${String(err)}`),
  });

  const { mutate: deleteThread, isPending: deleting } = useMutation({
    mutationFn: (threadId: string) => api.deleteThread(threadId),
    onSuccess: () => {
      selectNextOrPrev();
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    },
    onError: (err) => addToast(`Delete failed: ${String(err)}`),
  });

  const { mutate: markUnread, isPending: markingUnread } = useMutation({
    mutationFn: (threadId: string) => api.markThreadUnread(threadId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      queryClient.invalidateQueries({ queryKey: ["unread_counts"] });
      addToast("Marked unread locally");
    },
    onError: (err) => addToast(`Could not mark unread: ${String(err)}`),
  });

  const { mutate: analyzeThread, isPending: analyzing } = useMutation({
    mutationFn: (threadId: string) => api.analyzeThread(threadId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["thread_analysis", selectedThreadId] });
      queryClient.invalidateQueries({ queryKey: ["digest"] });
      addToast("Email analyzed");
    },
    onError: (err) => addToast(`Analysis failed: ${String(err)}`),
  });

  const { mutate: recordReview, isPending: savingReview } = useMutation({
    mutationFn: async ({ threadId, decision }: { threadId: string; decision: "keep" | "follow_up" | "archived" }) => {
      if (decision === "archived") await api.archiveThread(threadId);
      await api.recordReviewDecision(threadId, decision);
    },
    onSuccess: (_, { decision }) => {
      queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      addToast(decision === "archived" ? "Archived" : decision === "keep" ? "Kept in inbox" : "Marked for follow-up");
      selectNextOrPrev();
    },
    onError: (err) => addToast(`Could not save review decision: ${String(err)}`),
  });

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      if (e.key === "#" && selectedThreadId) {
        if (useAppStore.getState().checkedThreadIds.size > 0) return; // bulk handled by useKeyboardNav
        e.preventDefault();
        deleteThread(selectedThreadId);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [deleteThread, selectedThreadId]);

  // Handle keyboard shortcuts forwarded from the email iframe via postMessage.
  // useHotkeys is bypassed because hotkeys-js checks keyCode which synthetic events lack.
  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (e.data?.type !== "keydown") return;
      const key = String(e.data.key).toLowerCase();
      const threadId = useAppStore.getState().selectedThreadId;

      if (key === "j") { selectNextThread(); return; }
      if (key === "k") { selectPrevThread(); return; }
      if (key === "escape") { setSelectedThread(null); return; }
      if (!threadId) return;
      if (key === "e") { archive(threadId); return; }
      if (key === "#") {
        if (useAppStore.getState().checkedThreadIds.size > 0) return;
        deleteThread(threadId);
        return;
      }
      if (key === "s") {
        const thread = useAppStore.getState().threads.find((t) => t.id === threadId);
        if (thread) api.starThread(threadId, !thread.starred).then(() =>
          queryClient.invalidateQueries({ queryKey: ["threads"] })
        );
        return;
      }
      if (key === "u") {
        api.markThreadRead(threadId).then(() =>
          queryClient.invalidateQueries({ queryKey: ["threads"] })
        );
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [selectNextThread, selectPrevThread, setSelectedThread, archive, deleteThread, queryClient]);

  useEffect(() => {
    if (!selectedThreadId) return;
    // Optimistically flip unread→false in the cache immediately so the
    // font-weight change happens in sync with selection, not after the API call.
    queryClient.setQueryData<import("../../lib/api").Thread[]>(["threads"], (old) =>
      old?.map((t) => t.id === selectedThreadId ? { ...t, unread: false } : t)
    );
    api.markThreadRead(selectedThreadId).then(() => {
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    });
  }, [selectedThreadId]);

  if (!selectedThreadId) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-gray-300 select-none">
        Select an email to read
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-gray-400">
        Loading…
      </div>
    );
  }

  const subject = messages[0]?.subject || "(no subject)";

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-gray-50">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-6 py-3 bg-white border-b border-gray-100 flex-shrink-0">
        <h2 className="text-sm font-semibold text-gray-900 truncate">{subject}</h2>
        <div className="flex items-center gap-1">
          <button
            onClick={() => { if (selectedThreadId) analyzeThread(selectedThreadId); }}
            disabled={analyzing || archiving || deleting}
            title="Analyze this email"
            className="flex items-center gap-1 px-2 py-1.5 rounded-lg text-xs text-indigo-600 hover:text-indigo-700 hover:bg-indigo-50 transition-colors disabled:opacity-40"
          >
            <Sparkles size={14} />
            {analyzing ? "Analyzing…" : "Analyze"}
          </button>
          <button
            onClick={() => { if (selectedThreadId) markUnread(selectedThreadId); }}
            disabled={markingUnread || archiving || deleting}
            title="Mark unread locally"
            className="p-1.5 rounded-lg text-gray-400 hover:text-gray-700 hover:bg-gray-100 transition-colors disabled:opacity-40"
          >
            <MailOpen size={15} />
          </button>
          <button
            onClick={() => { if (selectedThreadId) archive(selectedThreadId); }}
            disabled={archiving || deleting}
            title="Archive (E)"
            className="p-1.5 rounded-lg text-gray-400 hover:text-gray-700 hover:bg-gray-100 transition-colors disabled:opacity-40"
          >
            <Archive size={15} />
          </button>
          <button
            onClick={() => { if (selectedThreadId) deleteThread(selectedThreadId); }}
            disabled={archiving || deleting}
            title="Delete (#)"
            className="p-1.5 rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 transition-colors disabled:opacity-40"
          >
            <Trash2 size={15} />
          </button>
        </div>
      </div>

      {/* AI-extracted action items, if this thread has been analyzed */}
      {reviewMode && reviewItem && (
        <div className="mx-6 mt-4 rounded-lg border border-gray-200 bg-white px-4 py-3">
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">Review decision</p>
              <p className="mt-1 text-xs font-medium text-gray-700">
                {reviewRecommendation(reviewItem.category, reviewItem.is_actionable, reviewItem.importance)}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <button
                disabled={savingReview}
                onClick={() => recordReview({ threadId: reviewItem.thread_id, decision: "follow_up" })}
                className="rounded-md px-2.5 py-1.5 text-xs text-amber-700 hover:bg-amber-50 disabled:opacity-40"
              >
                Follow up
              </button>
              <button
                disabled={savingReview}
                onClick={() => recordReview({ threadId: reviewItem.thread_id, decision: "keep" })}
                className="rounded-md px-2.5 py-1.5 text-xs text-indigo-700 hover:bg-indigo-50 disabled:opacity-40"
              >
                Keep
              </button>
              <button
                disabled={savingReview}
                onClick={() => recordReview({ threadId: reviewItem.thread_id, decision: "archived" })}
                className="rounded-md bg-gray-900 px-3 py-1.5 text-xs text-white hover:bg-gray-700 disabled:opacity-40"
              >
                Archive
              </button>
            </div>
          </div>
        </div>
      )}
      <AnalysisBanner threadId={selectedThreadId} />

      {/* Messages */}
      <div className="flex-1 overflow-y-auto px-6 py-5">
        {messages.map((msg, i) => (
          <MessageCard
            key={msg.id}
            message={msg}
            email={email}
            isLast={i === messages.length - 1}
          />
        ))}
      </div>
    </div>
  );
}
