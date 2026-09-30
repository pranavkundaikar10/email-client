import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type Message } from "../../lib/api";
import { useAppStore } from "../../store";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Archive, CalendarClock, Download, ExternalLink, MailOpen, Paperclip, Reply, Trash2, Sparkles, X } from "lucide-react";
import ReplyComposer from "./ReplyComposer";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useMailActions } from "../../hooks/useMailActions";
import { useMailFlags } from "../../hooks/useMailFlags";
import { useUpcomingBodyPrefetch } from "../../hooks/useUpcomingBodyPrefetch";
import JobCategoryBadge from "../ui/JobCategoryBadge";
import { recommendationLabel, recommendationTone } from "../../lib/recommendations";

// Renders HTML email in an isolated iframe so its <style> tags cannot
// leak out and shift the host page layout.
// Uses allow-scripts (no allow-same-origin) + postMessage bridge so link clicks
// open in the system browser and height is reported without cross-origin access.
function normalizeEmailHtml(html: string): string {
  // Marketing and applicant-tracking systems commonly emit malformed legacy
  // table markup. Parsing and serializing it once repairs broken attributes
  // before it reaches the isolated document, while retaining email styling.
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelectorAll("script, base, meta, link").forEach((element) => element.remove());
  document.querySelectorAll("*").forEach((element) => {
    for (const attribute of [...element.attributes]) {
      if (attribute.name.toLowerCase().startsWith("on")) element.removeAttribute(attribute.name);
    }
  });
  return document.body.innerHTML;
}

function scrollEmailContent(container: HTMLDivElement | null, direction: 1 | -1) {
  if (!container) return;
  // A near-page scroll preserves a little context, matching native reader
  // behavior. Smooth scrolling also makes repeated Space presses legible.
  const distance = Math.max(Math.round(container.clientHeight * 0.85), 240);
  container.scrollBy({ top: direction * distance, behavior: "smooth" });
}

function IsolatedHtml({ html }: { html: string }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(150);

  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (e.source !== ref.current?.contentWindow) return;
      if (e.data?.type === "height") {
        const measured = Number(e.data.h);
        if (Number.isFinite(measured)) setHeight(Math.max(80, measured + 16));
      } else if (e.data?.type === "open-url") {
        openUrl(String(e.data.url)).catch(() => {});
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  const safeHtml = useMemo(() => normalizeEmailHtml(html), [html]);

  const srcdoc = `<!DOCTYPE html><html><head><meta name="viewport" content="width=device-width, initial-scale=1">
<style>
html,body{margin:0;width:100%;max-width:100%;overflow-wrap:anywhere}
body{font-family:sans-serif;font-size:14px;color:#374151;word-break:break-word;min-width:0}
table{max-width:100% !important;overflow-wrap:anywhere}
table.fullWidth,.fullWidth{min-width:0 !important;max-width:100% !important;width:100% !important}
td,th{min-width:0 !important;overflow-wrap:anywhere}
img{max-width:100% !important;height:auto !important}
pre{max-width:100% !important;white-space:pre-wrap !important;overflow-wrap:anywhere}
</style>
</head><body>${safeHtml}<script>(function(){
  function h(){
    var body=document.body, bodyTop=body.getBoundingClientRect().top, height=0;
    // Do not use body/document scrollHeight here: when the iframe expands,
    // those values can include the viewport itself and recursively add blank
    // space. Measure the actual email elements instead.
    for(var i=0;i<body.children.length;i++){
      var rect=body.children[i].getBoundingClientRect();
      height=Math.max(height,rect.bottom-bodyTop);
    }
    if(!height){
      var range=document.createRange(); range.selectNodeContents(body);
      height=range.getBoundingClientRect().height;
    }
    window.parent.postMessage({type:'height',h:Math.ceil(height)},'*');
  }
  // Email tables frequently reflow after the initial document load. Observe
  // their final layout instead of measuring only the first rendered line.
  h(); requestAnimationFrame(h); window.addEventListener('load',h);
  window.addEventListener('resize',h);
  if(window.ResizeObserver){new ResizeObserver(h).observe(document.body);}
  setTimeout(h,50); setTimeout(h,250); setTimeout(h,1000);
  document.addEventListener('click',function(e){
    var el=e.target;
    while(el&&el.tagName!=='A')el=el.parentElement;
    if(el&&el.href&&/^(https?|mailto|tel):/.test(el.href)){
      e.preventDefault();
      window.parent.postMessage({type:'open-url',url:el.href},'*');
    }
  });
  document.addEventListener('keydown',function(e){
    if(/^[fijkresux#]$/i.test(e.key)||e.key==='Escape'||e.key==='Tab'||e.key===' '){
      if(e.key==='Tab')e.preventDefault();
      if(e.key===' ')e.preventDefault();
      window.parent.postMessage({type:'keydown',key:e.key,shiftKey:e.shiftKey},'*');
    }
  });
})();<\/script></body></html>`;

  return (
    <iframe
      ref={ref}
      srcDoc={srcdoc}
      sandbox="allow-scripts"
      scrolling="no"
      style={{ width: "100%", minWidth: 0, maxWidth: "100%", height, border: "none", display: "block", overflow: "hidden" }}
    />
  );
}

function hasMeaningfulHtml(html: string | null): boolean {
  if (!html) return false;
  if (/<img\b|background(?:-image)?\s*:/i.test(html)) return true;
  const visible = html
    .replace(/<style\b[^<]*(?:(?!<\/style>)<[^<]*)*<\/style>/gi, " ")
    .replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;|\s/g, "");
  return visible.length > 0;
}

function formatFullDate(iso: string): string {
  return new Date(iso).toLocaleString([], {
    weekday: "short", month: "short", day: "numeric",
    hour: "2-digit", minute: "2-digit",
  });
}

function formatAttachmentSize(sizeBytes: number) {
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${Math.round(sizeBytes / 1024)} KB`;
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`;
}

function MessageCard({
  message, email, isLast, replyOpen, onReplyOpen, onReplyClose, replyAnchorRef,
}: {
  message: Message;
  email: string;
  isLast: boolean;
  replyOpen: boolean;
  onReplyOpen: () => void;
  onReplyClose: () => void;
  replyAnchorRef?: { current: HTMLDivElement | null };
}) {
  const queryClient = useQueryClient();
  const addToast = useAppStore((s) => s.addToast);

  const { mutate: fetchBody, isPending, error: fetchError } = useMutation<Message, Error, boolean>({
    mutationFn: (force) => api.fetchMessageBody(email, message.id, force),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["messages", message.thread_id] });
      queryClient.invalidateQueries({ queryKey: ["message_attachments", message.id] });
    },
  });

  const refreshedEmptyHtml = useRef(false);
  const refreshedInlineImages = useRef(false);
  const refreshedAttachmentMetadata = useRef(false);
  const [downloadedAttachments, setDownloadedAttachments] = useState<Record<string, string>>({});
  const { data: attachments = [], isFetching: loadingAttachments } = useQuery({
    queryKey: ["message_attachments", message.id],
    queryFn: () => api.getMessageAttachments(message.id),
    enabled: message.body_fetched && message.has_attachments,
  });
  const { mutate: downloadAttachment, isPending: downloadingAttachment } = useMutation({
    mutationFn: (attachmentId: string) => api.downloadAttachment(email, attachmentId),
    onSuccess: (path, attachmentId) => setDownloadedAttachments((current) => ({ ...current, [attachmentId]: path })),
  });
  useEffect(() => {
    if (!isLast || isPending) return;
    if (!message.body_fetched) {
      fetchBody(false);
    } else if (!refreshedEmptyHtml.current && !hasMeaningfulHtml(message.body_html)) {
      // Re-fetch old cached emails that were parsed before the richer MIME
      // selection existed. This happens once and preserves the plain-text
      // fallback if the sender genuinely supplied no useful HTML.
      refreshedEmptyHtml.current = true;
      fetchBody(true);
    } else if (!refreshedInlineImages.current && /\bcid:/i.test(message.body_html ?? "")) {
      // Cached messages from before inline MIME images were supported still
      // contain unresolved cid: URLs. Re-fetch once to convert them locally.
      refreshedInlineImages.current = true;
      fetchBody(true);
    }
  }, [isLast, isPending, message.body_fetched, message.body_html, message.body_text, fetchBody]);
  useEffect(() => {
    if (!message.body_fetched || !message.has_attachments || loadingAttachments || attachments.length > 0 || refreshedAttachmentMetadata.current) return;
    // Older cached bodies predate attachment metadata. Refresh only this one
    // message once so its on-demand download controls become available.
    refreshedAttachmentMetadata.current = true;
    fetchBody(true);
  }, [attachments.length, fetchBody, loadingAttachments, message.body_fetched, message.has_attachments]);

  const toList = (() => {
    try { return (JSON.parse(message.to_emails) as string[]).join(", "); }
    catch { return message.to_emails; }
  })();

  return (
    <div ref={isLast ? replyAnchorRef : undefined} className="mb-3 min-w-0 overflow-hidden rounded-xl border border-gray-100">
      {/* Header */}
      <div className="bg-white px-4 py-4 sm:px-5">
        <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
          <div className="min-w-0">
            <p className="text-sm font-semibold text-gray-900">
              {message.from_name || message.from_email}
            </p>
            <p className="mt-0.5 break-all text-xs text-gray-400">{message.from_email}</p>
            {toList && <p className="mt-0.5 break-words text-xs text-gray-400">To: {toList}</p>}
          </div>
          <span className="mt-0.5 shrink-0 text-xs text-gray-400">
            {formatFullDate(message.sent_at)}
          </span>
        </div>
      </div>

      {/* Body */}
      <div className="min-w-0 overflow-x-hidden border-t border-gray-50 bg-white px-4 py-4 sm:px-5">
        {fetchError ? (
          <p className="text-xs text-red-500 bg-red-50 px-3 py-2 rounded">
            Failed to load: {String(fetchError)}
          </p>
        ) : isPending && !message.body_fetched ? (
          <p className="text-xs text-gray-400">Loading…</p>
        ) : message.body_html && hasMeaningfulHtml(message.body_html) ? (
          <IsolatedHtml html={message.body_html} />
        ) : message.body_text ? (
          <pre className="text-sm text-gray-700 whitespace-pre-wrap font-sans leading-relaxed">
            {message.body_text}
          </pre>
        ) : (
          <p className="text-xs text-gray-400 italic">No content</p>
        )}
        {(attachments.length > 0 || loadingAttachments) && (
          <section className="mt-4 border-t border-gray-100 pt-3" aria-label="Attachments">
            <p className="mb-2 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-gray-500"><Paperclip size={13} /> Attachments</p>
            {loadingAttachments ? <p className="text-xs text-gray-400">Loading attachments…</p> : (
              <div className="space-y-1.5">
                {attachments.map((attachment) => {
                  const path = downloadedAttachments[attachment.id];
                  return <div key={attachment.id} className="flex min-w-0 items-center gap-3 rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2">
                    <Paperclip size={15} className="shrink-0 text-gray-400" />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-gray-700">{attachment.filename}</p>
                      <p className="text-xs text-gray-400">{formatAttachmentSize(attachment.size_bytes)}</p>
                    </div>
                    {path ? (
                      <button type="button" onClick={() => void openPath(path).catch(() => addToast("Could not open the downloaded attachment"))} className="inline-flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-indigo-700 hover:bg-indigo-50"><ExternalLink size={13} /> Open</button>
                    ) : (
                      <button type="button" disabled={downloadingAttachment} onClick={() => downloadAttachment(attachment.id)} className="inline-flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium text-indigo-700 hover:bg-indigo-50 disabled:opacity-50"><Download size={13} /> {downloadingAttachment ? "Downloading…" : "Download"}</button>
                    )}
                  </div>;
                })}
              </div>
            )}
          </section>
        )}
      </div>

      {/* Reply button — only on last message */}
      {isLast && (
        <div className="border-t border-gray-100 bg-gray-50 px-4 py-3 sm:px-5">
          {replyOpen ? (
            <ReplyComposer
              from={email}
              replyTo={message}
              onClose={onReplyClose}
            />
          ) : (
            <button
              onClick={onReplyOpen}
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
    <div className="mx-6 mt-4 mb-1 rounded-lg border border-gray-200 border-l-2 border-l-indigo-500 bg-white px-4 py-3">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-indigo-500">
            <Sparkles size={12} />
            AI analysis
            <JobCategoryBadge
              isJobRelated={analysis.is_job_related}
              category={analysis.job_category}
            />
          </div>
          <p className="mt-1.5 text-sm font-medium leading-relaxed text-gray-800">
            {analysis.summary || (analysis.is_actionable ? "Action needed" : "No action needed")}
          </p>
        </div>
        <div
          title={`Importance ${analysis.importance} out of 5 — ${severity}`}
          className={`flex flex-shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-semibold ring-1 ${severityStyle}`}
        >
          <span>{severity}</span>
          <span className="h-3 w-px bg-current opacity-20" />
          <span>{analysis.importance}/5</span>
        </div>
      </div>
      {analysis.deadline && (
        <p className="mt-2 text-xs font-medium text-red-600">Due {analysis.deadline}</p>
      )}
      {items.length > 0 && (
        <ul className="mt-1.5 space-y-0.5">
          {items.map((it, i) => (
            <li key={i} className="flex gap-1.5 text-sm text-gray-700">
              <span className="text-indigo-400">•</span>
              {it}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function withTime(date: Date, hour = 9, minute = 0) {
  const result = new Date(date);
  result.setHours(hour, minute, 0, 0);
  return result;
}

function parseFollowUpTime(expression: string): Date | null {
  const normalized = expression.trim().toLowerCase().replace(/,/g, " ").replace(/\s+/g, " ");
  if (!normalized) return null;
  const now = new Date();
  let timeHour = 9;
  let timeMinute = 0;
  const timeMatch = normalized.match(/(?:\s+at)?\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)$/);
  const dateExpression = timeMatch ? normalized.slice(0, timeMatch.index).trim() : normalized;
  if (timeMatch) {
    timeHour = Number(timeMatch[1]);
    timeMinute = Number(timeMatch[2] ?? 0);
    const meridiem = timeMatch[3];
    if (timeHour < 1 || timeHour > 12 || timeMinute > 59) return null;
    if (meridiem === "pm" && timeHour !== 12) timeHour += 12;
    if (meridiem === "am" && timeHour === 12) timeHour = 0;
  }

  let date: Date | null = null;
  if (dateExpression === "today") {
    date = new Date(now);
  } else if (dateExpression === "tomorrow") {
    date = new Date(now);
    date.setDate(date.getDate() + 1);
  } else {
    const relative = dateExpression.match(/^in (\d+) (day|days|week|weeks)$/);
    if (relative) {
      date = new Date(now);
      date.setDate(date.getDate() + Number(relative[1]) * (relative[2].startsWith("week") ? 7 : 1));
    }
  }

  if (!date) {
    const weekdays = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
    // Accept the forms people naturally type into a compact command-style
    // field. Convert only a complete weekday token, so partial input remains
    // neutral until it is unambiguous.
    const weekdayAliases: Record<string, string> = {
      sun: "sunday", mon: "monday", tue: "tuesday", tues: "tuesday",
      wed: "wednesday", thu: "thursday", thur: "thursday", thurs: "thursday",
      fri: "friday", sat: "saturday",
    };
    const weekdayExpression = dateExpression.replace(
      /^(next )?([a-z]+)$/,
      (_match, prefix: string | undefined, name: string) => `${prefix ?? ""}${weekdayAliases[name] ?? name}`,
    );
    const weekday = weekdayExpression.match(/^(?:next )?(sunday|monday|tuesday|wednesday|thursday|friday|saturday)$/);
    if (weekday) {
      const target = weekdays.indexOf(weekday[1]);
      let days = (target - now.getDay() + 7) % 7;
      if (weekdayExpression.startsWith("next ")) days += 7;
      else if (days === 0) days = 7;
      date = new Date(now);
      date.setDate(date.getDate() + days);
    }
  }

  if (!date) {
    const months = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    const calendar = dateExpression.match(/^(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:tember)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\s+(\d{1,2})$/);
    if (calendar) {
      const month = months.findIndex((name) => name.startsWith(calendar[1].slice(0, 3)));
      const day = Number(calendar[2]);
      if (month < 0 || day < 1 || day > 31) return null;
      date = new Date(now.getFullYear(), month, day);
      if (date.getMonth() !== month || date.getDate() !== day) return null;
      if (withTime(date, timeHour, timeMinute) <= now) date.setFullYear(date.getFullYear() + 1);
    }
  }

  if (!date) return null;
  const dueAt = withTime(date, timeHour, timeMinute);
  // "Today" is intentionally literal: never turn a missed time into a
  // surprise tomorrow follow-up.
  return dueAt > now ? dueAt : null;
}

type FollowUpSuggestion = {
  expression: string;
  dueAt: Date;
  label: string;
};

function formatFollowUpDueAt(dueAt: Date) {
  return dueAt.toLocaleString([], {
    weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
  });
}

function followUpTone(dueAt: string) {
  const due = new Date(dueAt);
  const now = new Date();
  if (due < now) {
    return { panel: "border-red-100 bg-red-50/40", label: "text-red-600", text: "text-red-900", action: "text-red-700 hover:bg-red-100", button: "bg-red-600 hover:bg-red-700" };
  }
  const endToday = new Date(now);
  endToday.setHours(24, 0, 0, 0);
  if (due < endToday) {
    return { panel: "border-amber-200 bg-amber-50/40", label: "text-amber-700", text: "text-amber-900", action: "text-amber-700 hover:bg-amber-100", button: "bg-amber-600 hover:bg-amber-700" };
  }
  return { panel: "border-gray-200 bg-white", label: "text-gray-500", text: "text-gray-800", action: "text-indigo-700 hover:bg-indigo-50", button: "bg-indigo-600 hover:bg-indigo-700" };
}

function getFollowUpSuggestions(expression: string): FollowUpSuggestion[] {
  const parsed = parseFollowUpTime(expression);
  const normalized = expression.trim().toLowerCase().replace(/\s+/g, " ");
  const weekdayNames = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
  const weekdayPrefix = normalized.match(/^(next )?([a-z]{1,8})$/);
  const weekdaySuggestions = weekdayPrefix
    ? weekdayNames
        .filter((weekday) => weekday.startsWith(weekdayPrefix[2]))
        .flatMap((weekday) => {
          const suggestionExpression = `${weekdayPrefix[1] ?? ""}${weekday}`;
          const dueAt = parseFollowUpTime(suggestionExpression);
          return dueAt ? [{
            expression: suggestionExpression,
            dueAt,
            label: `${weekdayPrefix[1] ? "Next " : ""}${weekday[0].toUpperCase()}${weekday.slice(1)}`,
          }] : [];
        })
    : [];
  const presets = [
    ["tomorrow 9am", "Tomorrow morning"],
    ["tomorrow 2pm", "Tomorrow afternoon"],
    ["in 3 days", "In 3 days"],
    ["in 1 week", "In one week"],
    ["next monday 9am", "Next Monday morning"],
  ] as const;
  const queryTerms = expression
    .toLowerCase()
    .replace(/\bat\b/g, " ")
    .split(/\s+/)
    .filter(Boolean);
  const presetSuggestions = presets.flatMap(([preset, label]) => {
    const dueAt = parseFollowUpTime(preset);
    if (!dueAt) return [];
    const searchable = `${preset} ${label}`.toLowerCase();
    if (queryTerms.length > 0 && !queryTerms.every((term) => searchable.includes(term))) return [];
    return [{ expression: preset, dueAt, label }];
  });

  if (!parsed || !expression.trim()) return [...weekdaySuggestions, ...presetSuggestions];
  return [
    { expression, dueAt: parsed, label: "Schedule" },
    ...weekdaySuggestions.filter((suggestion) => suggestion.dueAt.getTime() !== parsed.getTime()),
    ...presetSuggestions.filter((suggestion) => suggestion.dueAt.getTime() !== parsed.getTime()),
  ];
}

function FollowUpPicker({ onSchedule, onClose, scheduling }: {
  onSchedule: (dueAt: string) => void;
  onClose: () => void;
  scheduling: boolean;
}) {
  const [expression, setExpression] = useState("");
  const [activeSuggestion, setActiveSuggestion] = useState(0);
  const parsedDueAt = useMemo(() => parseFollowUpTime(expression), [expression]);
  const suggestions = useMemo(() => getFollowUpSuggestions(expression), [expression]);
  useEffect(() => setActiveSuggestion(0), [expression]);

  const schedule = (dueAt = parsedDueAt ?? suggestions[activeSuggestion]?.dueAt) => {
    if (dueAt && !scheduling) onSchedule(dueAt.toISOString());
  };
  return <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[20vh] backdrop-blur-sm" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="app-dialog w-full max-w-lg overflow-hidden rounded-xl shadow-2xl" role="dialog" aria-modal="true" aria-label="Schedule follow-up">
      <div className="app-dialog-header flex items-center gap-3 border-b px-4 py-3">
        <CalendarClock size={16} className="flex-shrink-0 text-indigo-500" />
        <input
          autoFocus
          value={expression}
          onChange={(event) => setExpression(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && suggestions.length > 0) {
              event.preventDefault();
              setActiveSuggestion((index) => Math.min(index + 1, suggestions.length - 1));
            }
            if (event.key === "ArrowUp" && suggestions.length > 0) {
              event.preventDefault();
              setActiveSuggestion((index) => Math.max(index - 1, 0));
            }
            if (event.key === "Enter") { event.preventDefault(); schedule(); }
            if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
          }}
          placeholder="tomorrow at 2pm…"
          className="flex-1 text-sm text-gray-800 outline-none placeholder:text-gray-400"
          role="combobox"
          aria-expanded={suggestions.length > 0}
          aria-controls="follow-up-suggestions"
          aria-activedescendant={suggestions[activeSuggestion] ? `follow-up-suggestion-${activeSuggestion}` : undefined}
        />
        <button type="button" onClick={onClose} disabled={scheduling} className="text-gray-300 hover:text-gray-500" aria-label="Close follow-up scheduling"><X size={15} /></button>
      </div>
      {suggestions.length > 0 && <div id="follow-up-suggestions" role="listbox" className="border-b border-gray-50 px-2 py-2">
        {suggestions.slice(0, 4).map((suggestion, index) => (
          <button
            key={`${suggestion.expression}-${suggestion.dueAt.toISOString()}`}
            id={`follow-up-suggestion-${index}`}
            type="button"
            role="option"
            aria-selected={index === activeSuggestion}
            onMouseDown={(event) => {
              event.preventDefault();
              setExpression(suggestion.expression);
              setActiveSuggestion(index);
            }}
            className={`flex w-full items-center justify-between rounded-md px-2.5 py-2 text-left text-sm ${index === activeSuggestion ? "bg-indigo-50 text-indigo-800" : "text-gray-600 hover:bg-gray-50"}`}
          >
            <span className="font-medium">{suggestion.label}</span>
            <span className="text-xs text-gray-400">{formatFollowUpDueAt(suggestion.dueAt)}</span>
          </button>
        ))}
      </div>}
      <div className="px-4 py-3 text-sm">
        {parsedDueAt ? <span className="text-indigo-700">Schedules {formatFollowUpDueAt(parsedDueAt)}</span> : <span className="text-gray-400">Type naturally, or choose a suggestion</span>}
      </div>
      <div className="app-dialog-footer flex gap-4 border-t px-4 py-2 text-xs text-gray-400">
        <span>↑↓ choose</span><span>↵ schedule</span><span>esc cancel</span>
      </div>
    </div>
  </div>;
}

export default function EmailPreview({ email, reviewMode = false, followUpMode = false }: { email: string; reviewMode?: boolean; followUpMode?: boolean }) {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const selectNextThread = useAppStore((s) => s.selectNextThread);
  const selectPrevThread = useAppStore((s) => s.selectPrevThread);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const addToast = useAppStore((s) => s.addToast);
  const queryClient = useQueryClient();
  const { archiveThread: queueArchiveThread, deleteThread: queueDeleteThread } = useMailActions();
  const { markRead, markUnread: queueMarkUnread, setStarred } = useMailFlags();
  const [followUpPickerOpen, setFollowUpPickerOpen] = useState(false);
  const [replyOpen, setReplyOpen] = useState(false);
  const messageListRef = useRef<HTMLDivElement>(null);
  const replyAnchorRef = useRef<HTMLDivElement>(null);

  const { data: messages = [], isLoading } = useQuery({
    queryKey: ["messages", selectedThreadId],
    queryFn: () => api.getMessages(selectedThreadId!),
    enabled: !!selectedThreadId,
  });
  const { data: aiAssistanceSettings = { enabled: true } } = useQuery({
    queryKey: ["ai_assistance_settings"],
    queryFn: api.getAiAssistanceSettings,
  });
  useUpcomingBodyPrefetch(email);

  const { data: reviewQueue = [] } = useQuery({
    queryKey: ["review_queue", "priority"],
    queryFn: () => api.getReviewQueue(50, "priority"),
    enabled: reviewMode,
  });
  const reviewItem = reviewQueue.find((item) => item.thread_id === selectedThreadId);
  const { data: followUps = [] } = useQuery({
    queryKey: ["follow_ups"],
    queryFn: api.getFollowUps,
    enabled: followUpMode || reviewMode,
  });
  const followUpItem = followUps.find((item) => item.thread_id === selectedThreadId);

  const { mutate: archive, isPending: archiving } = useMutation({
    mutationFn: (threadId: string) => queueArchiveThread(threadId),
    onError: (err) => addToast(`Archive failed: ${String(err)}`),
  });

  const { mutate: deleteThread, isPending: deleting } = useMutation({
    mutationFn: (threadId: string) => queueDeleteThread(threadId),
    onError: (err) => addToast(`Delete failed: ${String(err)}`),
  });

  const { mutate: markUnread, isPending: markingUnread } = useMutation({
    mutationFn: (threadId: string) => queueMarkUnread(threadId),
    onSuccess: () => {
      addToast("Marked unread locally");
    },
    onError: (err) => addToast(`Could not mark unread: ${String(err)}`),
  });

  const { mutate: analyzeThread, isPending: analyzing } = useMutation({
    mutationFn: (threadId: string) => api.analyzeThread(threadId),
    onSuccess: async () => {
      // Keep the visible Analyze state until every surface backed by this
      // result has refreshed. In review mode the recommendation comes from
      // the queue item, not the thread-analysis banner.
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["thread_analysis", selectedThreadId] }),
        queryClient.invalidateQueries({ queryKey: ["digest"] }),
        queryClient.invalidateQueries({ queryKey: ["review_queue"] }),
        queryClient.invalidateQueries({ queryKey: ["threads"] }),
        queryClient.invalidateQueries({ queryKey: ["follow_ups"] }),
        queryClient.invalidateQueries({ queryKey: ["search"] }),
      ]);
      addToast("Email analyzed");
    },
    onError: (err) => addToast(`Analysis failed: ${String(err)}`),
  });

  const { mutate: recordReview, isPending: savingReview } = useMutation({
    mutationFn: async ({ threadId, decision }: { threadId: string; decision: "keep" | "follow_up" | "archived" }) => {
      if (decision === "archived") await queueArchiveThread(threadId);
      await api.recordReviewDecision(threadId, decision);
    },
    onSuccess: (_, { decision }) => {
      // Review decisions remove this item from the current list. Choose its
      // adjacent visible neighbour before that list refreshes, so focus never
      // snaps back to the top of the queue.
      if (decision === "keep") useAppStore.getState().selectNextOrPrev();
      void queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      void queryClient.invalidateQueries({ queryKey: ["threads"] });
      addToast(decision === "archived" ? "Archive queued" : decision === "keep" ? "Kept in inbox" : "Marked for follow-up");
    },
    onError: (err) => addToast(`Could not save review decision: ${String(err)}`),
  });

  const { mutate: deleteFromReview, isPending: deletingFromReview } = useMutation({
    mutationFn: (threadId: string) => queueDeleteThread(threadId),
    onSuccess: () => {
      addToast("Move to Gmail Trash queued");
    },
    onError: (err) => addToast(`Could not delete email: ${String(err)}`),
  });

  const { mutate: scheduleFollowUp, isPending: schedulingFollowUp } = useMutation({
    mutationFn: async ({ threadId, dueAt }: { threadId: string; dueAt: string }) => {
      await api.scheduleFollowUp(threadId, dueAt);
      await api.recordReviewDecision(threadId, "follow_up");
    },
    onSuccess: () => {
      // Scheduling from Review moves this email to Follow-ups. Advance using
      // the shared visible-list selection state before the Review query drops
      // the scheduled item.
      if (reviewMode) useAppStore.getState().selectNextOrPrev();
      setFollowUpPickerOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["follow_ups"] });
      void queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      void queryClient.invalidateQueries({ queryKey: ["threads"] });
      addToast("Follow-up scheduled");
    },
    onError: (err) => addToast(`Could not schedule follow-up: ${String(err)}`),
  });

  const { mutate: completeFollowUp, isPending: completingFollowUp } = useMutation({
    mutationFn: (threadId: string) => api.completeFollowUp(threadId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["follow_ups"] });
      addToast("Follow-up completed");
    },
    onError: (err) => addToast(`Could not complete follow-up: ${String(err)}`),
  });

  const openReply = useCallback(() => {
    if (messages.length === 0) return;
    setReplyOpen(true);
    requestAnimationFrame(() => {
      replyAnchorRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    });
  }, [messages.length]);

  useEffect(() => {
    setReplyOpen(false);
  }, [selectedThreadId]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === " " && selectedThreadId && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const target = e.target;
        if (target instanceof Element && target.closest("input, textarea, select, button, a, [contenteditable='true'], [role='dialog']")) return;
        e.preventDefault();
        scrollEmailContent(messageListRef.current, e.shiftKey ? -1 : 1);
        return;
      }
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      // Plain-letter mail actions must not fire as a side effect of app or
      // system shortcuts such as Control-Shift-F for fullscreen.
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "r" && selectedThreadId) {
        e.preventDefault();
        openReply();
        return;
      }
      if (key === "f" && ((reviewMode && (reviewItem || followUpItem)) || (followUpMode && followUpItem))) {
        e.preventDefault();
        setFollowUpPickerOpen(true);
        return;
      }
      if (key === "i" && reviewMode && reviewItem && !savingReview) {
        e.preventDefault();
        recordReview({ threadId: reviewItem.thread_id, decision: "keep" });
        return;
      }
      if (key === "#" && selectedThreadId) {
        if (useAppStore.getState().checkedThreadIds.size > 0) return; // bulk handled by useKeyboardNav
        e.preventDefault();
        deleteThread(selectedThreadId);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [deleteThread, followUpItem, followUpMode, openReply, recordReview, reviewItem, reviewMode, savingReview, selectedThreadId]);

  // Handle keyboard shortcuts forwarded from the email iframe via postMessage.
  // useHotkeys is bypassed because hotkeys-js checks keyCode which synthetic events lack.
  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (e.data?.type !== "keydown") return;
      const key = String(e.data.key).toLowerCase();
      const threadId = useAppStore.getState().selectedThreadId;
      if (key === "tab") {
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: Boolean(e.data.shiftKey), bubbles: true }));
        return;
      }

      if (key === " ") {
        scrollEmailContent(messageListRef.current, e.data.shiftKey ? -1 : 1);
        return;
      }

      if (key === "r") { openReply(); return; }
      if (key === "j") { selectNextThread(); return; }
      if (key === "k") { selectPrevThread(); return; }
      if (key === "escape") {
        const state = useAppStore.getState();
        if (state.checkedThreadIds.size > 0) state.clearChecked();
        else setSelectedThread(null);
        return;
      }
      if (!threadId) return;
      if (key === "f" && ((reviewMode && (reviewItem || followUpItem)) || (followUpMode && followUpItem))) {
        setFollowUpPickerOpen(true);
        return;
      }
      if (key === "i" && reviewMode && reviewItem && !savingReview) {
        recordReview({ threadId: reviewItem.thread_id, decision: "keep" });
        return;
      }
      if (key === "e") { archive(threadId); return; }
      if (key === "#") {
        if (useAppStore.getState().checkedThreadIds.size > 0) return;
        deleteThread(threadId);
        return;
      }
      if (key === "s") {
        const thread = useAppStore.getState().threads.find((t) => t.id === threadId);
        if (thread) void setStarred(threadId, !thread.starred);
        return;
      }
      if (key === "u") {
        void markRead(threadId);
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [selectNextThread, selectPrevThread, setSelectedThread, archive, deleteThread, followUpItem, followUpMode, markRead, openReply, recordReview, reviewItem, reviewMode, savingReview, setStarred]);

  useEffect(() => {
    if (!selectedThreadId) return;
    void markRead(selectedThreadId);
  }, [selectedThreadId, markRead]);

  if (!selectedThreadId) {
    return (
      <div className="flex flex-1 select-none flex-col items-center justify-center gap-1.5 text-center">
        <div className="mb-1 flex h-10 w-10 items-center justify-center rounded-xl bg-gray-100 text-gray-500">
          <MailOpen size={20} strokeWidth={1.75} />
        </div>
        <p className="text-[clamp(1rem,1.5vw,1.5rem)] font-medium text-gray-700">Select an email to read</p>
        <p className="text-[clamp(0.75rem,0.9vw,0.95rem)] text-gray-500">Press <kbd className="rounded border border-gray-300 bg-gray-100 px-1.5 py-0.5 font-mono text-[0.85em] font-medium text-gray-600">J</kbd> or click an email to open it</p>
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
  const activeFollowUpTone = followUpItem ? followUpTone(followUpItem.due_at) : null;

  return (
    <div className="preview-surface flex min-w-0 flex-1 flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="preview-toolbar flex shrink-0 items-center justify-between gap-3 border-b px-4 py-3 sm:px-6">
        <h2 className="min-w-0 truncate text-sm font-semibold text-gray-900">{subject}</h2>
        <div className="flex shrink-0 items-center gap-1">
          {aiAssistanceSettings.enabled && <button
            onClick={() => { if (selectedThreadId) analyzeThread(selectedThreadId); }}
            disabled={analyzing || archiving || deleting}
            title="Analyze this email"
            className="flex items-center gap-1 px-2 py-1.5 rounded-lg text-xs text-indigo-600 hover:text-indigo-700 hover:bg-indigo-50 transition-colors disabled:opacity-40"
          >
            <Sparkles size={14} />
            {analyzing ? "Analyzing…" : "Analyze"}
          </button>}
          <button
            onClick={openReply}
            disabled={messages.length === 0}
            title="Reply (R)"
            className="rounded-lg p-1.5 text-gray-400 transition-colors hover:bg-gray-100 hover:text-gray-700 disabled:opacity-40"
          >
            <Reply size={15} />
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

      {/* AI recommendations enrich the usual Review Queue decision controls. */}
      {reviewMode && reviewItem && (
        <div className="mx-6 mt-4 rounded-lg border border-gray-200 border-l-2 border-l-indigo-500 bg-white px-4 py-3">
          <div className="flex items-center justify-between gap-4">
            <div>
              {reviewItem.analysis_available ? <>
                <p className="text-xs font-semibold uppercase tracking-wide text-indigo-500">AI recommendation</p>
                <p className={`mt-1.5 text-sm font-medium ${recommendationTone(reviewItem.recommended_action)}`}>
                  {recommendationLabel(reviewItem.recommended_action)}
                </p>
              </> : <>
                <p className="text-xs font-semibold uppercase tracking-wide text-gray-400">Review this email</p>
                <p className="mt-1.5 text-sm font-medium text-gray-700">Choose what happens next</p>
              </>}
            </div>
            <div className="flex items-center gap-2">
              <button
                disabled={savingReview || deletingFromReview || schedulingFollowUp}
                onClick={() => setFollowUpPickerOpen(true)}
                className="rounded-md px-3 py-2 text-sm text-gray-600 hover:bg-gray-100 disabled:opacity-40"
              >
                Follow up
              </button>
              <button
                disabled={savingReview || deletingFromReview}
                onClick={() => recordReview({ threadId: reviewItem.thread_id, decision: "keep" })}
                className="rounded-md px-3 py-2 text-sm text-indigo-700 hover:bg-indigo-50 disabled:opacity-40"
              >
                Keep
              </button>
              <button
                disabled={savingReview || deletingFromReview}
                onClick={() => recordReview({ threadId: reviewItem.thread_id, decision: "archived" })}
                className="rounded-md bg-gray-900 px-3 py-2 text-sm text-white hover:bg-gray-700 disabled:opacity-40"
              >
                Archive
              </button>
              <button
                disabled={savingReview || deletingFromReview}
                onClick={() => deleteFromReview(reviewItem.thread_id)}
                title="Move email to Gmail Trash"
                className="rounded-md px-2.5 py-2 text-sm text-red-500 hover:bg-red-50 disabled:opacity-40"
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>
          {followUpPickerOpen && <FollowUpPicker
            scheduling={schedulingFollowUp}
            onClose={() => setFollowUpPickerOpen(false)}
            onSchedule={(dueAt) => scheduleFollowUp({ threadId: reviewItem.thread_id, dueAt })}
          />}
        </div>
      )}
      {(followUpMode || reviewMode) && followUpItem && activeFollowUpTone && (
        <div className={`mx-6 mt-4 rounded-lg border px-4 py-3 ${activeFollowUpTone.panel}`}>
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className={`text-xs font-semibold uppercase tracking-wide ${activeFollowUpTone.label}`}>Follow-up</p>
              <p className={`mt-1 text-sm font-medium ${activeFollowUpTone.text}`}>Due {new Date(followUpItem.due_at).toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" })}</p>
            </div>
            <div className="flex items-center gap-2">
              <button disabled={completingFollowUp || schedulingFollowUp} onClick={() => setFollowUpPickerOpen(true)} className={`rounded-md px-3 py-2 text-sm disabled:opacity-40 ${activeFollowUpTone.action}`}>Reschedule</button>
              <button disabled={completingFollowUp || schedulingFollowUp} onClick={() => completeFollowUp(followUpItem.thread_id)} className={`rounded-md px-3 py-2 text-sm text-white disabled:opacity-40 ${activeFollowUpTone.button}`}>Complete</button>
            </div>
          </div>
          {followUpPickerOpen && <FollowUpPicker
            scheduling={schedulingFollowUp}
            onClose={() => setFollowUpPickerOpen(false)}
            onSchedule={(dueAt) => scheduleFollowUp({ threadId: followUpItem.thread_id, dueAt })}
          />}
        </div>
      )}
      <AnalysisBanner threadId={selectedThreadId} />

      {/* Messages */}
      <div ref={messageListRef} className="min-w-0 flex-1 overflow-y-auto px-3 py-4 sm:px-6 sm:py-5">
        {messages.map((msg, i) => (
          <MessageCard
            key={msg.id}
            message={msg}
            email={email}
            isLast={i === messages.length - 1}
            replyOpen={i === messages.length - 1 && replyOpen}
            onReplyOpen={openReply}
            onReplyClose={() => setReplyOpen(false)}
            replyAnchorRef={i === messages.length - 1 ? replyAnchorRef : undefined}
          />
        ))}
      </div>
    </div>
  );
}
