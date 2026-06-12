import { useState, useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { useHotkeys } from "react-hotkeys-hook";
import { Search, Inbox, Star, Archive, X, Layers, Send, FileEdit, SlidersHorizontal } from "lucide-react";
import { cn } from "../../lib/utils";
import { api } from "../../lib/api";
import { useAppStore } from "../../store";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts";

interface Action {
  id: string;
  label: string;
  shortcut?: string;
  icon: typeof Search;
  onSelect: () => void;
}

interface Props {
  onViewChange: (view: View) => void;
  onClose: () => void;
  splits?: { id: string; label: string }[];
  onSplitChange?: (id: string) => void;
  onSplitsSettings: () => void;
}

export default function CommandPalette({ onViewChange, onClose, splits, onSplitChange, onSplitsSettings }: Props) {
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const { setSelectedThread } = useAppStore();
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const { data: searchResults = [] } = useQuery({
    queryKey: ["search", query],
    queryFn: () => api.searchThreads(query),
    enabled: query.length > 1,
    staleTime: 5_000,
  });

  const splitActions: Action[] = (splits ?? []).map((s, i) => ({
    id: `split-${s.id}`,
    label: s.label,
    shortcut: `G ${i + 1}`,
    icon: Layers,
    onSelect: () => { onViewChange("inbox"); onSplitChange?.(s.id); onClose(); },
  }));

  const staticActions: Action[] = [
    ...splitActions,
    {
      id: "inbox",
      label: "Go to Inbox",
      shortcut: "G I",
      icon: Inbox,
      onSelect: () => { onViewChange("inbox"); onClose(); },
    },
    {
      id: "starred",
      label: "Go to Starred",
      shortcut: "G S",
      icon: Star,
      onSelect: () => { onViewChange("starred"); onClose(); },
    },
    {
      id: "sent",
      label: "Go to Sent",
      shortcut: "G N",
      icon: Send,
      onSelect: () => { onViewChange("sent"); onClose(); },
    },
    {
      id: "drafts",
      label: "Go to Drafts",
      shortcut: "G D",
      icon: FileEdit,
      onSelect: () => { onViewChange("drafts"); onClose(); },
    },
    {
      id: "archive",
      label: "Go to Archive",
      shortcut: "G A",
      icon: Archive,
      onSelect: () => { onViewChange("archive"); onClose(); },
    },
    {
      id: "splits-settings",
      label: "Split Inbox Settings",
      icon: SlidersHorizontal,
      onSelect: () => { onSplitsSettings(); onClose(); },
    },
  ];

  const threadActions: Action[] = searchResults.map((t) => ({
    id: t.id,
    label: t.subject || "(no subject)",
    icon: Search,
    onSelect: () => {
      setSelectedThread(t.id);
      onClose();
    },
  }));

  const filteredStatic = query.length > 1
    ? staticActions.filter((a) => a.label.toLowerCase().includes(query.toLowerCase()))
    : staticActions;

  const items = [...filteredStatic, ...threadActions];

  useEffect(() => {
    setActiveIdx(0);
  }, [query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Scroll active item into view
  useEffect(() => {
    const el = listRef.current?.children[activeIdx] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIdx]);

  useHotkeys("arrowdown", (e) => {
    e.preventDefault();
    setActiveIdx((i) => Math.min(i + 1, items.length - 1));
  }, { enableOnFormTags: true });

  useHotkeys("arrowup", (e) => {
    e.preventDefault();
    setActiveIdx((i) => Math.max(i - 1, 0));
  }, { enableOnFormTags: true });

  useHotkeys("enter", (e) => {
    e.preventDefault();
    items[activeIdx]?.onSelect();
  }, { enableOnFormTags: true });

  useHotkeys("escape", (e) => {
    e.preventDefault();
    onClose();
  }, { enableOnFormTags: true });

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh] bg-black/30 backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="w-full max-w-lg bg-white rounded-xl shadow-2xl overflow-hidden">
        {/* Input */}
        <div className="flex items-center gap-3 px-4 py-3 border-b border-gray-100">
          <Search size={15} className="text-gray-400 flex-shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search emails or jump to…"
            className="flex-1 text-sm outline-none placeholder:text-gray-400"
          />
          <button onClick={onClose} className="text-gray-300 hover:text-gray-500">
            <X size={14} />
          </button>
        </div>

        {/* Results */}
        <div ref={listRef} className="max-h-72 overflow-y-auto py-1">
          {items.length === 0 && query.length > 1 && (
            <p className="px-4 py-3 text-sm text-gray-400">No results for "{query}"</p>
          )}
          {items.map((item, i) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                onClick={item.onSelect}
                onMouseEnter={() => setActiveIdx(i)}
                className={cn(
                  "w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors",
                  i === activeIdx ? "bg-indigo-50 text-indigo-700" : "text-gray-700 hover:bg-gray-50"
                )}
              >
                <Icon size={14} className="flex-shrink-0 opacity-60" />
                <span className="flex-1 text-sm truncate">{item.label}</span>
                {item.shortcut && (
                  <span className="text-xs text-gray-400 font-mono">{item.shortcut}</span>
                )}
              </button>
            );
          })}
        </div>

        {/* Footer hint */}
        <div className="px-4 py-2 border-t border-gray-50 flex gap-4 text-xs text-gray-400">
          <span>↑↓ navigate</span>
          <span>↵ select</span>
          <span>esc close</span>
        </div>
      </div>
    </div>
  );
}
