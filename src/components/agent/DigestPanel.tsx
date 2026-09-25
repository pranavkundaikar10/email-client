import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X, RefreshCw, Sparkles } from "lucide-react";
import { api, type DigestItem } from "../../lib/api";
import { useAppStore } from "../../store";

const CATEGORY_LABEL: Record<string, string> = {
  interview: "Interview",
  assessment: "Assessment",
  offer: "Offer",
  rejection: "Rejection",
  application_update: "Update",
  networking: "Networking",
  deadline: "Deadline",
  newsletter: "Newsletter",
  other: "Other",
};

const CATEGORY_COLOR: Record<string, string> = {
  interview: "bg-emerald-50 text-emerald-700",
  assessment: "bg-amber-50 text-amber-700",
  offer: "bg-emerald-50 text-emerald-700",
  rejection: "bg-gray-100 text-gray-500",
  application_update: "bg-indigo-50 text-indigo-700",
  networking: "bg-sky-50 text-sky-700",
  deadline: "bg-red-50 text-red-700",
  newsletter: "bg-gray-100 text-gray-400",
  other: "bg-gray-100 text-gray-500",
};

function parseItems(json: string): string[] {
  try {
    const arr = JSON.parse(json);
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function ImportanceDots({ importance }: { importance: number }) {
  return (
    <div className="flex gap-0.5" title={`Importance ${importance}/5`}>
      {Array.from({ length: 5 }, (_, i) => (
        <span
          key={i}
          className={`w-1.5 h-1.5 rounded-full ${
            i < importance ? "bg-indigo-500" : "bg-gray-200"
          }`}
        />
      ))}
    </div>
  );
}

function DigestRow({ item, onOpen }: { item: DigestItem; onOpen: (threadId: string) => void }) {
  const items = parseItems(item.action_items);
  return (
    <button
      onClick={() => onOpen(item.thread_id)}
      className="w-full text-left border border-gray-200 rounded-lg px-4 py-3 hover:border-gray-300 hover:bg-gray-50 transition-colors"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span
              className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${
                CATEGORY_COLOR[item.category] ?? CATEGORY_COLOR.other
              }`}
            >
              {CATEGORY_LABEL[item.category] ?? item.category}
            </span>
            {item.unread && (
              <span className="w-1.5 h-1.5 rounded-full bg-indigo-500 flex-shrink-0" />
            )}
            {item.deadline && (
              <span className="text-[10px] text-red-500 font-medium">Due {item.deadline}</span>
            )}
          </div>
          <p className="text-sm font-medium text-gray-900 truncate mt-1">
            {item.from_name || item.from_email}
            <span className="text-gray-400 font-normal"> — {item.subject || "(no subject)"}</span>
          </p>
          <p className="text-xs text-gray-500 mt-0.5">{item.summary}</p>
          {items.length > 0 && (
            <ul className="mt-1.5 space-y-0.5">
              {items.map((it, i) => (
                <li key={i} className="text-xs text-gray-600 flex gap-1.5">
                  <span className="text-gray-300">•</span>
                  {it}
                </li>
              ))}
            </ul>
          )}
        </div>
        <ImportanceDots importance={item.importance} />
      </div>
    </button>
  );
}

interface Props {
  onClose: () => void;
}

export default function DigestPanel({ onClose }: Props) {
  const queryClient = useQueryClient();
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const addToast = useAppStore((s) => s.addToast);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  const { data: digest = [], isLoading } = useQuery({
    queryKey: ["digest"],
    queryFn: () => api.getDigest(100),
  });

  async function scan() {
    setScanning(true);
    setScanError(null);
    try {
      await api.analyzeInbox();
      queryClient.invalidateQueries({ queryKey: ["digest"] });
    } catch (e) {
      setScanError(String(e));
      addToast(`Digest scan failed: ${String(e)}`);
    } finally {
      setScanning(false);
    }
  }

  function openThread(threadId: string) {
    setSelectedThread(threadId);
    onClose();
  }

  const visible = showAll ? digest : digest.filter((d) => d.is_actionable || d.importance >= 3);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="bg-white rounded-xl shadow-2xl w-[640px] max-h-[80vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-100">
          <div className="flex items-center gap-2">
            <Sparkles size={15} className="text-indigo-500" />
            <h2 className="text-sm font-semibold text-gray-900">Digest</h2>
          </div>
          <div className="flex items-center gap-3">
            <button
              onClick={scan}
              disabled={scanning}
              className="flex items-center gap-1.5 text-xs text-gray-500 hover:text-gray-800 disabled:opacity-40"
            >
              <RefreshCw size={12} className={scanning ? "animate-spin" : ""} />
              {scanning ? "Scanning…" : "Rescan inbox"}
            </button>
            <button onClick={onClose} className="text-gray-400 hover:text-gray-600">
              <X size={15} />
            </button>
          </div>
        </div>

        <p className="px-6 pt-3 pb-1 text-xs text-gray-400">
          A local model reads new unread mail and pulls out what actually needs a response —
          newsletters and fluff are filtered out below by default.
        </p>

        {/* List */}
        <div className="flex-1 overflow-y-auto px-6 py-3 space-y-2">
          {scanError && (
            <p className="text-xs text-red-500 bg-red-50 px-3 py-2 rounded">
              {scanError} — is Ollama running locally?
            </p>
          )}
          {isLoading ? (
            <p className="text-xs text-gray-400">Loading…</p>
          ) : visible.length === 0 ? (
            <p className="text-xs text-gray-400 italic py-6 text-center">
              {digest.length === 0
                ? "Nothing scanned yet — click Rescan inbox to analyze your unread mail."
                : "Nothing urgent right now."}
            </p>
          ) : (
            visible.map((item) => (
              <DigestRow key={item.thread_id} item={item} onOpen={openThread} />
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t border-gray-100">
          <button
            onClick={() => setShowAll((s) => !s)}
            className="text-xs text-gray-400 hover:text-gray-600"
          >
            {showAll ? "Hide low-priority items" : `Show all analyzed (${digest.length})`}
          </button>
          <p className="text-xs text-gray-400">Runs on your own machine via Ollama</p>
        </div>
      </div>
    </div>
  );
}
