import { useState, useEffect } from "react";
import { X, Send } from "lucide-react";
import { api } from "../../lib/api";

interface Props {
  from: string;
  onClose: () => void;
}

export default function ComposeModal({ from, onClose }: Props) {
  const [to, setTo] = useState("");
  const [cc, setCc] = useState("");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  async function handleSend() {
    if (!to.trim() || !subject.trim()) return;
    setSending(true);
    setError(null);
    try {
      await api.sendEmail({
        from,
        to: to.split(",").map((s) => s.trim()).filter(Boolean),
        cc: cc.split(",").map((s) => s.trim()).filter(Boolean),
        subject,
        body,
      });
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-end justify-end p-6 pointer-events-none">
      <div className="pointer-events-auto w-full max-w-lg bg-white rounded-xl shadow-2xl border border-gray-200 flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 bg-gray-900 text-white flex-shrink-0">
          <span className="text-sm font-medium">New message</span>
          <button onClick={onClose} className="text-gray-400 hover:text-white transition-colors">
            <X size={15} />
          </button>
        </div>

        {/* Fields */}
        <div className="flex flex-col divide-y divide-gray-100 flex-shrink-0">
          <input
            autoFocus
            value={to}
            onChange={(e) => setTo(e.target.value)}
            placeholder="To"
            className="px-4 py-2.5 text-sm outline-none placeholder:text-gray-400"
          />
          <input
            value={cc}
            onChange={(e) => setCc(e.target.value)}
            placeholder="Cc"
            className="px-4 py-2.5 text-sm outline-none placeholder:text-gray-400"
          />
          <input
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
            placeholder="Subject"
            className="px-4 py-2.5 text-sm outline-none placeholder:text-gray-400 font-medium"
          />
        </div>

        {/* Body */}
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="Write your message…"
          className="flex-1 px-4 py-3 text-sm outline-none resize-none placeholder:text-gray-400 min-h-48"
        />

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-gray-100 flex-shrink-0">
          {error && <p className="text-xs text-red-500 truncate flex-1 mr-4">{error}</p>}
          {!error && <div className="flex-1" />}
          <button
            onClick={handleSend}
            disabled={sending || !to.trim() || !subject.trim()}
            className="flex items-center gap-2 px-4 py-2 bg-gray-900 text-white text-sm font-medium rounded-lg hover:bg-gray-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            <Send size={13} />
            {sending ? "Sending…" : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}
