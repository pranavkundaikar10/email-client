import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type Message } from "../../lib/api";
import { useAppStore } from "../../store";
import { useEffect, useRef, useState } from "react";
import { Archive, Reply, Trash2 } from "lucide-react";
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

export default function EmailPreview({ email }: { email: string }) {
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
