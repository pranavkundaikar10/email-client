import { create } from "zustand";
import type { Thread } from "../lib/api";

export interface Toast {
  id: string;
  message: string;
  actionLabel?: string;
  onAction?: () => void | Promise<void>;
  duration?: number;
}

interface AppState {
  selectedThreadId: string | null;
  threads: Thread[];
  commandPaletteOpen: boolean;
  activeSplitId: string | null;
  checkedThreadIds: Set<string>;
  toasts: Toast[];

  setSelectedThread: (id: string | null) => void;
  setThreads: (threads: Thread[]) => void;
  setCommandPaletteOpen: (open: boolean) => void;
  setActiveSplitId: (id: string | null) => void;
  toggleThreadCheck: (id: string) => void;
  clearChecked: () => void;
  addToast: (message: string, options?: Omit<Toast, "id" | "message">) => void;
  removeToast: (id: string) => void;
  undoToast: (id: string) => Promise<void>;
  undoLatestToast: () => Promise<void>;

  selectNextThread: () => void;
  selectPrevThread: () => void;
  selectNextOrPrev: () => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  selectedThreadId: null,
  threads: [],
  commandPaletteOpen: false,
  activeSplitId: null,
  checkedThreadIds: new Set<string>(),
  toasts: [],

  setSelectedThread: (id) => set({ selectedThreadId: id }),
  setThreads: (threads) => set({ threads }),
  setCommandPaletteOpen: (open) => set({ commandPaletteOpen: open }),
  setActiveSplitId: (id) => set({ activeSplitId: id }),

  toggleThreadCheck: (id) => set((state) => {
    const next = new Set(state.checkedThreadIds);
    if (next.has(id)) next.delete(id); else next.add(id);
    return { checkedThreadIds: next };
  }),
  clearChecked: () => set({ checkedThreadIds: new Set() }),

  addToast: (message, options) => set((state) => ({
    toasts: [...state.toasts, { id: crypto.randomUUID(), message, ...options }],
  })),
  removeToast: (id) => set((state) => ({
    toasts: state.toasts.filter((t) => t.id !== id),
  })),
  undoToast: async (id) => {
    const toast = get().toasts.find((item) => item.id === id);
    if (!toast?.onAction) return;
    // Remove first so a click and Z pressed at the same time cannot submit the
    // same cancellation twice.
    get().removeToast(id);
    await toast.onAction();
  },
  undoLatestToast: async () => {
    const toast = [...get().toasts].reverse().find((item) => item.onAction);
    if (toast) await get().undoToast(toast.id);
  },

  selectNextThread: () => {
    const { threads, selectedThreadId } = get();
    if (threads.length === 0) return;
    const idx = threads.findIndex((t) => t.id === selectedThreadId);
    const next = idx === -1 ? 0 : Math.min(idx + 1, threads.length - 1);
    set({ selectedThreadId: threads[next].id });
  },

  selectPrevThread: () => {
    const { threads, selectedThreadId } = get();
    if (threads.length === 0) return;
    const idx = threads.findIndex((t) => t.id === selectedThreadId);
    const prev = idx <= 0 ? 0 : idx - 1;
    set({ selectedThreadId: threads[prev].id });
  },

  selectNextOrPrev: () => {
    const { threads, selectedThreadId } = get();
    if (threads.length <= 1) { set({ selectedThreadId: null }); return; }
    const idx = threads.findIndex((t) => t.id === selectedThreadId);
    if (idx === -1) return;
    const next = idx < threads.length - 1 ? threads[idx + 1].id : threads[idx - 1].id;
    set({ selectedThreadId: next });
  },
}));
