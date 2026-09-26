import { useEffect } from "react";
import type { Thread } from "../lib/api";
import { useAppStore } from "../store";

/**
 * Keeps keyboard navigation in the exact same order as the currently rendered
 * sidebar. Every thread-list view must pass its final, display-ordered list.
 */
export function useVisibleThreadList(threads: Thread[]) {
  const setThreads = useAppStore((state) => state.setThreads);

  useEffect(() => {
    setThreads(threads);
  }, [threads, setThreads]);
}
