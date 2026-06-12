import { Star } from "lucide-react";
import { cn } from "../../lib/utils";
import type { Thread } from "../../lib/api";

interface Props {
  thread: Thread;
  selected: boolean;
  checked: boolean;
  onClick: () => void;
  onCheck: () => void;
  onStar: () => void;
}

function formatDate(iso: string): string {
  const date = new Date(iso);
  const now = new Date();
  const diffDays = Math.floor((now.getTime() - date.getTime()) / 86_400_000);

  if (diffDays === 0)
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (diffDays < 7)
    return date.toLocaleDateString([], { weekday: "short" });
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

function firstRecipient(toEmailsJson: string): string {
  try {
    const arr = JSON.parse(toEmailsJson) as { name?: string; email?: string }[] | string[];
    if (!arr.length) return "";
    const first = arr[0];
    if (typeof first === "string") return first;
    return first.name || first.email || "";
  } catch {
    return toEmailsJson;
  }
}

export default function ThreadItem({ thread, selected, checked, onClick, onCheck, onStar }: Props) {
  const unread = thread.unread && !selected;
  const isSentOrDraft = thread.folder === "sent" || thread.folder === "drafts";
  const senderLabel = isSentOrDraft
    ? `To: ${firstRecipient(thread.to_emails) || thread.to_emails}`
    : thread.from_name || thread.from_email;

  return (
    <div
      className={cn(
        "group w-full flex border-b border-gray-100 transition-colors",
        selected
          ? "bg-indigo-50 border-l-2 border-l-indigo-500"
          : checked
          ? "bg-indigo-50/40 border-l-2 border-l-indigo-300"
          : "hover:bg-gray-50 border-l-2 border-l-transparent"
      )}
    >
      {/* Checkbox — visible on hover or when checked */}
      <div
        className={cn(
          "flex-shrink-0 w-8 flex items-center justify-center cursor-pointer transition-opacity",
          checked ? "opacity-100" : "opacity-0 group-hover:opacity-100"
        )}
        onClick={(e) => { e.stopPropagation(); onCheck(); }}
      >
        <div className={cn(
          "w-3.5 h-3.5 rounded border flex items-center justify-center transition-colors",
          checked ? "bg-indigo-500 border-indigo-500" : "border-gray-300 bg-white"
        )}>
          {checked && (
            <svg className="w-2.5 h-2.5 text-white" viewBox="0 0 10 10" fill="none">
              <path d="M1.5 5l2.5 2.5 4.5-4.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
            </svg>
          )}
        </div>
      </div>

      {/* Thread content */}
      <button
        onClick={onClick}
        className="flex-1 text-left pr-2 py-3 flex flex-col gap-0.5 min-w-0"
      >
      <div className="flex items-center justify-between gap-2">
        <span
          className={cn(
            "text-sm truncate",
            unread ? "font-semibold text-gray-900" : "font-normal text-gray-600"
          )}
        >
          {senderLabel}
        </span>
        <span className="text-xs text-gray-400 flex-shrink-0">
          {formatDate(thread.last_message_at)}
        </span>
      </div>

      <div className="flex items-center gap-1.5">
        {/* Always reserve space for the dot to prevent layout shift */}
        <span className={cn(
          "w-1.5 h-1.5 rounded-full flex-shrink-0",
          unread ? "bg-indigo-500" : "invisible"
        )} />
        <span
          className={cn(
            "text-xs truncate",
            unread ? "font-medium text-gray-800" : "text-gray-500"
          )}
        >
          {thread.subject || "(no subject)"}
        </span>
      </div>

      <p className="text-xs text-gray-400 truncate leading-relaxed">
        {thread.snippet}
      </p>
      </button>

      {/* Star — right */}
      <div
        className={cn(
          "flex-shrink-0 w-7 flex items-center justify-center cursor-pointer transition-opacity",
          thread.starred ? "opacity-100" : "opacity-0 group-hover:opacity-100"
        )}
        onClick={(e) => { e.stopPropagation(); onStar(); }}
      >
        <Star
          size={13}
          className={thread.starred ? "fill-yellow-400 text-yellow-400" : "text-gray-300"}
        />
      </div>
    </div>
  );
}
