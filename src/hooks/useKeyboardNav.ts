import { useEffect } from "react";
import { useHotkeys } from "react-hotkeys-hook";
import { useQueryClient } from "@tanstack/react-query";
import { useAppStore } from "../store";
import { api } from "../lib/api";
import { useMailActions } from "./useMailActions";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts";

interface Options {
  onViewChange: (view: View) => void;
  onSearchFocus: () => void;
  splits?: { id: string; label: string }[];
  onSplitChange?: (id: string) => void;
}

export function useKeyboardNav({ onViewChange, onSearchFocus, splits, onSplitChange }: Options) {
  const selectedThreadId = useAppStore((s) => s.selectedThreadId);
  const selectNextThread = useAppStore((s) => s.selectNextThread);
  const selectPrevThread = useAppStore((s) => s.selectPrevThread);
  const setCommandPaletteOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const queryClient = useQueryClient();
  const { archiveThreads, deleteThreads } = useMailActions();

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
    api.starThread(selectedThreadId, !thread.starred).then(() => {
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    });
  }, { enableOnFormTags: false });

  // U — mark read
  useHotkeys("u", (e) => {
    e.preventDefault();
    if (!selectedThreadId) return;
    api.markThreadRead(selectedThreadId).then(() => {
      queryClient.invalidateQueries({ queryKey: ["threads"] });
    });
  }, { enableOnFormTags: false });

  // Escape — deselect
  useHotkeys("escape", (e) => {
    e.preventDefault();
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

  // G-sequences: G then I/S/A or 1/2/3 within 1 second
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
        if (e.key === "i" || e.key === "I") { e.preventDefault(); onViewChange("inbox"); }
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

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onViewChange, splits, onSplitChange]);
}
