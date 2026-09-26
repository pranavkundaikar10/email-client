import { useEffect } from "react";
import { X } from "lucide-react";
import { useAppStore, type Toast } from "../../store";

function ToastItem({ toast }: { toast: Toast }) {
  const removeToast = useAppStore((s) => s.removeToast);
  const undoToast = useAppStore((s) => s.undoToast);

  useEffect(() => {
    const timer = setTimeout(() => removeToast(toast.id), toast.duration ?? 4000);
    return () => clearTimeout(timer);
  }, [toast.id]);

  return (
    <div className="toast-in flex items-start gap-3 px-4 py-3 bg-gray-900 text-white text-xs rounded-lg shadow-xl max-w-sm w-full pointer-events-auto">
      <span className="flex-1 leading-relaxed">{toast.message}</span>
      {toast.actionLabel && toast.onAction && (
        <button
          onClick={() => {
            void undoToast(toast.id);
          }}
          className="font-semibold text-indigo-300 hover:text-indigo-100 transition-colors"
        >
          {toast.actionLabel}
        </button>
      )}
      <button
        onClick={() => removeToast(toast.id)}
        className="text-gray-400 hover:text-white flex-shrink-0 mt-0.5 transition-colors"
      >
        <X size={13} />
      </button>
    </div>
  );
}

export function ToastContainer() {
  const toasts = useAppStore((s) => s.toasts);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2 pointer-events-none">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
