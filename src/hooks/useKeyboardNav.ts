import { useEffect } from "react";
import { useHotkeys } from "react-hotkeys-hook";
import { useAppStore } from "../store";
import { useMailActions } from "./useMailActions";
import { useMailFlags } from "./useMailFlags";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts" | "review" | "follow_ups";

interface Options {
  onViewChange: (view: View) => void;
  onSearchFocus: () => void;
  splits?: { id: string; label: string }[];
  onSplitChange?: (id: string) => void;
  activeSplitId?: string | null;
  splitNavigationEnabled?: boolean;
}

export function useKeyboardNav({
  onViewChange,
  onSearchFocus,
  splits,
  onSplitChange,
  activeSplitId,
  splitNavigationEnabled = false,
}: Options) {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const selectNextThread = useAppStore((s) => s.selectNextThread);
  const selectPrevThread = useAppStore((s) => s.selectPrevThread);
  const setCommandPaletteOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const { archiveThreads, deleteThreads } = useMailActions();
  const { markRead, setStarred } = useMailFlags();

  // J — next thread
  useHotkeys("j", (e) => {
    e.preventDefault();
    selectNextThread();
  }, { enableOnFormTags: false });

  // K — previous thread
  useHotkeys("k", (e) => {
    e.preventDefault();
    selectPrevThread();
  }, { enableOnFormTags: false });

  // E — archive (bulk if any checked, otherwise single selected)
  useHotkeys("e", (e) => {
    e.preventDefault();
    const { checkedThreadIds } = useAppStore.getState();
    if (checkedThreadIds.size > 0) {
      const ids = Array.from(checkedThreadIds);
      archiveThreads(ids).catch(() => {});
      return;
    }
    if (!selectedThreadId) return;
    const threadToArchive = selectedThreadId;
    archiveThreads([threadToArchive]).catch(() => {});
  }, { enableOnFormTags: false });

  // Z — undo the latest still-available archive/delete action.
  useHotkeys("z", (e) => {
    e.preventDefault();
    void useAppStore.getState().undoLatestToast();
  }, { enableOnFormTags: false });

  // X — toggle check on selected thread
  useHotkeys("x", (e) => {
    e.preventDefault();
    if (!selectedThreadId) return;
    useAppStore.getState().toggleThreadCheck(selectedThreadId);
  }, { enableOnFormTags: false });

  // # — bulk delete (raw listener: useHotkeys doesn't reliably fire for Shift+3)
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "#") return;
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      const { checkedThreadIds } = useAppStore.getState();
      if (checkedThreadIds.size === 0) return; // let EmailPreview handle single-thread
      e.preventDefault();
      const ids = Array.from(checkedThreadIds);
      deleteThreads(ids).catch(() => {});
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [deleteThreads]);

  // S — star/unstar selected thread
  useHotkeys("s", (e) => {
    e.preventDefault();
    if (!selectedThreadId) return;
    const thread = useAppStore.getState().threads.find((t) => t.id === selectedThreadId);
    if (!thread) return;
    void setStarred(selectedThreadId, !thread.starred);
  }, { enableOnFormTags: false });

  // U — mark read
  useHotkeys("u", (e) => {
    e.preventDefault();
    if (!selectedThreadId) return;
    void markRead(selectedThreadId);
  }, { enableOnFormTags: false });

  // Match Superhuman's Split Inbox navigation: Tab moves right through Split
  // Inboxes and Shift+Tab moves left. Scoped to the email workspace so text
  // fields and open dialogs retain normal native focus traversal.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "Tab" || !splitNavigationEnabled || !splits?.length || !onSplitChange) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target;
      if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true'], [role='dialog']")) return;

      e.preventDefault();
      const currentIndex = splits.findIndex((split) => split.id === activeSplitId);
      const start = currentIndex === -1 ? (e.shiftKey ? 0 : -1) : currentIndex;
      const nextIndex = (start + (e.shiftKey ? -1 : 1) + splits.length) % splits.length;
      onViewChange("inbox");
      onSplitChange(splits[nextIndex].id);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeSplitId, onSplitChange, onViewChange, splitNavigationEnabled, splits]);

  // Escape — clear bulk selection first, then close the active preview.
  useHotkeys("escape", (e) => {
    e.preventDefault();
    const state = useAppStore.getState();
    if (state.checkedThreadIds.size > 0) {
      state.clearChecked();
      return;
    }
    setSelectedThread(null);
  }, { enableOnFormTags: false });

  // / — search
  useHotkeys("/", (e) => {
    e.preventDefault();
    onSearchFocus();
  }, { enableOnFormTags: false });

  // Cmd+K — command palette
  useHotkeys("meta+k", (e) => {
    e.preventDefault();
    setCommandPaletteOpen(true);
  }, { enableOnFormTags: true });

  // G-sequences: G then I/R/F/S/A or 1/2/3 within 1 second
  useEffect(() => {
    let gPressed = false;
    let timer: ReturnType<typeof setTimeout>;

    function onKeyDown(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;

      if (e.key === "g" || e.key === "G") {
        gPressed = true;
        clearTimeout(timer);
        timer = setTimeout(() => { gPressed = false; }, 1000);
        return;
      }

      if (gPressed) {
        gPressed = false;
        clearTimeout(timer);
        // G-sequences take precedence over a single-key review action such as
        // I = Keep, so G then I remains a reliable "go to Inbox" shortcut.
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === "i" || e.key === "I") { e.preventDefault(); onViewChange("inbox"); }
        else if (e.key === "r" || e.key === "R") { e.preventDefault(); onViewChange("review"); }
        else if (e.key === "f" || e.key === "F") { e.preventDefault(); onViewChange("follow_ups"); }
        else if (e.key === "s" || e.key === "S") { e.preventDefault(); onViewChange("starred"); }
        else if (e.key === "a" || e.key === "A") { e.preventDefault(); onViewChange("archive"); }
        else if (e.key === "n" || e.key === "N") { e.preventDefault(); onViewChange("sent"); }
        else if (e.key === "d" || e.key === "D") { e.preventDefault(); onViewChange("drafts"); }
        else {
          const num = parseInt(e.key, 10);
          if (!isNaN(num) && num >= 1 && splits && onSplitChange) {
            const split = splits[num - 1];
            if (split) { e.preventDefault(); onViewChange("inbox"); onSplitChange(split.id); }
          }
        }
      }
    }

    // Capture lets an established G-sequence consume its second key before a
    // view-level single-key action observes it.
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onViewChange, splits, onSplitChange]);
}
