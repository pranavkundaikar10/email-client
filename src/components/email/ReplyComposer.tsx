import { useState } from "react";
import { Send, X } from "lucide-react";
import { api, type Message } from "../../lib/api";

interface Props {
  from: string;
  replyTo: Message;
  onClose: () => void;
}

export default function ReplyComposer({ from, replyTo, onClose }: Props) {
  const [body, setBody] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSend() {
    if (!body.trim()) return;
    setSending(true);
    setError(null);
    try {
      await api.sendEmail({
        from,
        to: [replyTo.from_email],
        cc: [],
        subject: replyTo.subject.startsWith("Re:")
          ? replyTo.subject
          : `Re: ${replyTo.subject}`,
        body,
        inReplyTo: replyTo.id,
        references: replyTo.id,
      });
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="border-t border-gray-100 bg-white flex flex-col flex-shrink-0">
      {/* Reply header */}
      <div className="flex items-center justify-between px-5 py-2.5 border-b border-gray-100">
        <span className="text-xs text-gray-500">
          Reply to <span className="font-medium text-gray-700">{replyTo.from_email}</span>
        </span>
        <button onClick={onClose} className="text-gray-300 hover:text-gray-500">
          <X size={13} />
        </button>
      </div>

      {/* Body */}
      <textarea
        autoFocus
        value={body}
        onChange={(e) => setBody(e.target.value)}
        placeholder="Write your reply…"
        className="px-5 py-3 text-sm outline-none resize-none placeholder:text-gray-400 min-h-32"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) handleSend();
        }}
      />

      {/* Footer */}
      <div className="flex items-center justify-between px-5 py-3 border-t border-gray-100">
        {error && <p className="text-xs text-red-500 truncate flex-1 mr-4">{error}</p>}
        {!error && (
          <p className="text-xs text-gray-400">
            <kbd className="font-mono">⌘↵</kbd> to send
          </p>
        )}
        <button
          onClick={handleSend}
          disabled={sending || !body.trim()}
          className="flex items-center gap-2 px-4 py-2 bg-gray-900 text-white text-sm font-medium rounded-lg hover:bg-gray-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          <Send size={13} />
          {sending ? "Sending…" : "Reply"}
        </button>
      </div>
    </div>
  );
}
