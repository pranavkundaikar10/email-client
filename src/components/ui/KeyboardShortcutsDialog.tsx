import { useEffect } from "react";
import { Keyboard, X } from "lucide-react";

const sections = [
  {
    title: "Navigate",
    rows: [
      ["J / K", "Next / previous email"],
      ["Tab / Shift Tab", "Next / previous split"],
      ["G I", "Inbox"],
      ["G R", "Review queue"],
      ["G F", "Follow-ups"],
      ["G S / A / N / D", "Starred / Archive / Sent / Drafts"],
      ["G 1–9", "Open a split"],
    ],
  },
  {
    title: "Email actions",
    rows: [
      ["C", "Compose"],
      ["E", "Archive"],
      ["#", "Move to Gmail Trash"],
      ["S", "Star or unstar"],
      ["X", "Select email for bulk actions"],
      ["Z", "Undo archive or delete"],
      ["I", "Keep in inbox (Review)"],
      ["F", "Schedule follow-up (Review / Follow-ups)"],
    ],
  },
  {
    title: "Tools",
    rows: [
      ["/", "Search"],
      ["⌘ K", "Command palette"],
      ["Esc", "Close or deselect"],
      ["?", "Show keyboard shortcuts"],
    ],
  },
];

export default function KeyboardShortcutsDialog({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[16vh] backdrop-blur-sm" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="w-full max-w-lg overflow-hidden rounded-xl bg-white shadow-2xl" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
      <div className="flex items-center gap-3 border-b border-gray-100 px-4 py-3">
        <Keyboard size={16} className="text-indigo-500" />
        <h2 className="flex-1 text-sm font-medium text-gray-800">Keyboard shortcuts</h2>
        <button type="button" onClick={onClose} className="text-gray-300 hover:text-gray-500" aria-label="Close keyboard shortcuts"><X size={15} /></button>
      </div>
      <div className="max-h-[60vh] overflow-y-auto px-4 py-3">
        {sections.map((section) => <section key={section.title} className="mb-4 last:mb-0">
          <h3 className="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-gray-400">{section.title}</h3>
          <div className="space-y-0.5">
            {section.rows.map(([keys, action]) => <div key={keys} className="flex items-center justify-between gap-5 rounded-md px-2 py-1.5 hover:bg-gray-50">
              <span className="text-sm text-gray-600">{action}</span>
              <kbd className="whitespace-nowrap rounded border border-gray-200 bg-gray-50 px-1.5 py-0.5 font-mono text-[11px] text-gray-500">{keys}</kbd>
            </div>)}
          </div>
        </section>)}
      </div>
      <div className="border-t border-gray-50 px-4 py-2 text-xs text-gray-400">esc close</div>
    </div>
  </div>;
}
