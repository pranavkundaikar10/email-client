import { Inbox, Star, Archive, Search, LogOut, SlidersHorizontal, Send, FileEdit, SquarePen, Sparkles } from "lucide-react";
import { cn } from "../../lib/utils";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts";

interface Props {
  activeView: View;
  onViewChange: (view: View) => void;
  email: string;
  onLogout: () => void;
  onSplits: () => void;
  onCompose: () => void;
  onDigest: () => void;
  inboxUnread: number;
}

const navItems: { id: View; icon: typeof Inbox; label: string; shortcut: string }[] = [
  { id: "inbox",   icon: Inbox,    label: "Inbox",   shortcut: "G I" },
  { id: "starred", icon: Star,     label: "Starred",  shortcut: "G S" },
  { id: "sent",    icon: Send,     label: "Sent",     shortcut: "G N" },
  { id: "drafts",  icon: FileEdit, label: "Drafts",   shortcut: "G D" },
  { id: "archive", icon: Archive,  label: "Archive",  shortcut: "G A" },
  { id: "search",  icon: Search,   label: "Search",   shortcut: "/"   },
];

export default function Sidebar({ activeView, onViewChange, email, onLogout, onSplits, onCompose, onDigest, inboxUnread }: Props) {
  return (
    <aside className="w-14 flex flex-col items-center py-4 gap-1 bg-gray-950 border-r border-gray-800 flex-shrink-0">
      {/* Avatar */}
      <div className="w-8 h-8 rounded-full bg-indigo-500 flex items-center justify-center mb-3">
        <span className="text-white text-xs font-semibold">
          {email[0]?.toUpperCase() ?? "?"}
        </span>
      </div>

      {/* Compose */}
      <button
        onClick={onCompose}
        title="Compose (C)"
        className="w-9 h-9 flex items-center justify-center rounded-lg text-gray-400 hover:text-white hover:bg-gray-800 transition-colors mb-2"
      >
        <SquarePen size={17} />
      </button>

      {navItems.map(({ id, icon: Icon, label }) => (
        <button
          key={id}
          onClick={() => onViewChange(id)}
          title={label}
          className={cn(
            "relative w-9 h-9 flex items-center justify-center rounded-lg transition-colors",
            activeView === id
              ? "bg-gray-700 text-white"
              : "text-gray-500 hover:text-gray-300 hover:bg-gray-800"
          )}
        >
          <Icon size={17} />
          {id === "inbox" && inboxUnread > 0 && (
            <span className="absolute -top-0.5 -right-0.5 min-w-[14px] h-3.5 flex items-center justify-center bg-indigo-500 text-white rounded-full text-[9px] font-bold px-0.5 leading-none">
              {inboxUnread > 99 ? "99+" : inboxUnread}
            </span>
          )}
        </button>
      ))}

      {/* Spacer */}
      <div className="flex-1" />

      <button
        onClick={onDigest}
        title="Digest — action items from unread mail"
        className="w-9 h-9 flex items-center justify-center rounded-lg text-gray-600 hover:text-indigo-400 hover:bg-gray-800 transition-colors"
      >
        <Sparkles size={15} />
      </button>
      <button
        onClick={onSplits}
        title="Split Inbox"
        className="w-9 h-9 flex items-center justify-center rounded-lg text-gray-600 hover:text-gray-300 hover:bg-gray-800 transition-colors"
      >
        <SlidersHorizontal size={15} />
      </button>
      <button
        onClick={onLogout}
        title="Sign out"
        className="w-9 h-9 flex items-center justify-center rounded-lg text-gray-600 hover:text-gray-300 hover:bg-gray-800 transition-colors"
      >
        <LogOut size={15} />
      </button>
    </aside>
  );
}
