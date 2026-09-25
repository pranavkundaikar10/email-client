import "./App.css";
import { useState, useEffect, useRef, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useHotkeys } from "react-hotkeys-hook";
import AccountSetup from "./pages/AccountSetup";
import Sidebar from "./components/layout/Sidebar";
import SearchBar from "./components/email/SearchBar";
import ThreadList from "./components/email/ThreadList";
import EmailPreview from "./components/email/EmailPreview";
import ComposeModal from "./components/email/ComposeModal";
import CommandPalette from "./components/ui/CommandPalette";
import SplitsSettings from "./components/settings/SplitsSettings";
import DigestPanel from "./components/agent/DigestPanel";
import ReviewQueue from "./components/agent/ReviewQueue";
import { ToastContainer } from "./components/ui/Toast";
import { useKeyboardNav } from "./hooks/useKeyboardNav";
import { useAppStore } from "./store";
import { api } from "./lib/api";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts";

const ACCOUNT_KEY = "connected_email";

function InboxApp({ email, onLogout }: { email: string; onLogout: () => void }) {
  const [activeView, setActiveView] = useState<View>("inbox");
  const [, setSyncing] = useState(false);
  const syncedFolders = useRef(new Set<string>());
  const backgroundAnalysisRunning = useRef(false);
  const addToast = useAppStore((s) => s.addToast);
  const [composeOpen, setComposeOpen] = useState(false);
  const [splitsOpen, setSplitsOpen] = useState(false);
  const [digestOpen, setDigestOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const commandPaletteOpen = useAppStore((s) => s.commandPaletteOpen);
  const setCommandPaletteOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const activeSplitId = useAppStore((s) => s.activeSplitId);
  const setActiveSplitId = useAppStore((s) => s.setActiveSplitId);
  const setSelectedThread = useAppStore((s) => s.setSelectedThread);
  const queryClient = useQueryClient();

  const { data: rawSplits = [] } = useQuery({
    queryKey: ["splits"],
    queryFn: api.getSplits,
  });

  const { data: unreadCounts = {} } = useQuery({
    queryKey: ["unread_counts"],
    queryFn: api.getUnreadCounts,
    refetchInterval: 60_000,
  });

  const splitDefs = useMemo(
    () => rawSplits.map((s) => ({ id: s.id, label: s.name })),
    [rawSplits]
  );

  // Search results
  const { data: searchResults = [] } = useQuery({
    queryKey: ["search", searchQuery],
    queryFn: () => api.searchThreads(searchQuery),
    enabled: searchQuery.length > 1,
    staleTime: 5_000,
  });

  const isSearching = searchQuery.length > 1;

  const showTabs = activeView === "inbox" && !isSearching && splitDefs.length > 0;
  const effectiveSplitId = useMemo(() => {
    if (!showTabs) return null;
    return activeSplitId && splitDefs.some((s) => s.id === activeSplitId)
      ? activeSplitId
      : splitDefs[0]?.id ?? null;
  }, [showTabs, activeSplitId, splitDefs]);

  function switchTab(id: string) {
    setActiveSplitId(id);
    setSelectedThread(null);
  }

  useKeyboardNav({
    onViewChange: setActiveView,
    onSearchFocus: () => { searchInputRef.current?.focus(); },
    splits: splitDefs,
    onSplitChange: (id) => { setActiveView("inbox"); setActiveSplitId(id); },
  });

  // C — compose
  useHotkeys("c", (e) => {
    e.preventDefault();
    setComposeOpen(true);
  }, { enableOnFormTags: false });

  useEffect(() => {
    async function processOneRecentEmail() {
      // Keep local inference deliberately low-impact: one email per sync
      // cycle, never in parallel, and only for mail received today.
      if (backgroundAnalysisRunning.current) return;
      backgroundAnalysisRunning.current = true;
      try {
        const [candidate] = await api.getAutoAnalysisCandidates(1);
        if (!candidate) return;

        // BODY.PEEK fetches the content without changing Gmail's read state.
        await api.fetchMessageBody(email, candidate.message_id);
        await api.analyzeThread(candidate.thread_id);
        queryClient.invalidateQueries({ queryKey: ["thread_analysis", candidate.thread_id] });
        queryClient.invalidateQueries({ queryKey: ["digest"] });
      } catch (err) {
        // Background triage is opportunistic. Manual Analyze remains available
        // and avoids interrupting the user with repeated transient errors.
        console.warn("Background email analysis skipped:", err);
      } finally {
        backgroundAnalysisRunning.current = false;
      }
    }

    async function sync() {
      setSyncing(true);
      try {
        await api.syncInbox(email);
        queryClient.invalidateQueries({ queryKey: ["threads"] });
        queryClient.invalidateQueries({ queryKey: ["unread_counts"] });
        await processOneRecentEmail();
      } catch (err) {
        addToast(`Sync failed: ${String(err)}`);
      } finally {
        setSyncing(false);
      }
    }
    sync();
    const interval = setInterval(sync, 60_000);
    return () => clearInterval(interval);
  }, [email]);

  // Lazy sync for sent/drafts — fire once per session when the view is first visited
  useEffect(() => {
    if (activeView !== "sent" && activeView !== "drafts") return;
    if (syncedFolders.current.has(activeView)) return;
    syncedFolders.current.add(activeView);
    setSyncing(true);
    const fn = activeView === "sent" ? api.syncSent : api.syncDrafts;
    fn(email)
      .then(() => queryClient.invalidateQueries({ queryKey: ["threads"] }))
      .catch((err) => addToast(`Sync failed: ${String(err)}`))
      .finally(() => setSyncing(false));
  }, [activeView, email]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-white">
      <Sidebar
        activeView={activeView}
        onViewChange={setActiveView}
        email={email}
        onLogout={onLogout}
        onSplits={() => setSplitsOpen(true)}
        onCompose={() => setComposeOpen(true)}
        onDigest={() => setDigestOpen(true)}
        onReview={() => setReviewOpen(true)}
        inboxUnread={Object.values(unreadCounts).reduce((a, b) => a + b, 0)}
      />

      {/* Right content: tabs on top, then thread list + email preview below */}
      <div className="flex-1 flex flex-col overflow-hidden">

        {/* Split tabs — spans full width above both panels */}
        {showTabs && (
          <div className="flex-shrink-0 flex border-b border-gray-100">
            {splitDefs.map((split, idx) => {
              const active = split.id === effectiveSplitId;
              return (
                <button
                  key={split.id}
                  onClick={() => switchTab(split.id)}
                  title={`G ${idx + 1}`}
                  className={`relative px-6 py-2.5 text-sm font-medium whitespace-nowrap transition-colors flex items-center gap-1.5 ${
                    active
                      ? "text-gray-900 after:absolute after:bottom-0 after:inset-x-0 after:h-0.5 after:bg-indigo-500"
                      : "text-gray-400 hover:text-gray-600"
                  }`}
                >
                  {split.label}
                  {(unreadCounts[split.id] ?? 0) > 0 && (
                    <span className="text-xs bg-indigo-500 text-white rounded-full px-1.5 py-0.5 leading-none font-medium">
                      {unreadCounts[split.id]}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}

        {/* Thread list + email preview side by side */}
        <div className="flex flex-1 overflow-hidden">

          {/* Thread list panel */}
          <div className="w-72 flex flex-col border-r border-gray-100 flex-shrink-0">
            <SearchBar
              value={searchQuery}
              onChange={setSearchQuery}
              onClear={() => setSearchQuery("")}
              inputRef={searchInputRef}
            />

            <ThreadList
              activeView={activeView}
              effectiveSplitId={effectiveSplitId}
              searchResults={searchResults}
              isSearching={isSearching}
            />
          </div>

          {/* Email preview */}
          <EmailPreview email={email} />

        </div>{/* end thread list + email preview row */}
      </div>{/* end right content column */}

      {/* Compose modal */}
      {composeOpen && (
        <ComposeModal from={email} onClose={() => setComposeOpen(false)} />
      )}

      {/* Split inbox settings */}
      {splitsOpen && <SplitsSettings onClose={() => setSplitsOpen(false)} />}

      {/* Digest — AI action items across unread inbox */}
      {digestOpen && <DigestPanel onClose={() => setDigestOpen(false)} />}

      {/* Review queue — user-approved decisions for analyzed inbox mail */}
      {reviewOpen && <ReviewQueue onClose={() => setReviewOpen(false)} />}

      {/* Command palette */}
      {commandPaletteOpen && (
        <CommandPalette
          onViewChange={setActiveView}
          onClose={() => setCommandPaletteOpen(false)}
          splits={splitDefs}
          onSplitChange={(id) => { setActiveView("inbox"); setActiveSplitId(id); }}
          onSplitsSettings={() => setSplitsOpen(true)}
        />
      )}

      <ToastContainer />
    </div>
  );
}

export default function App() {
  const [email, setEmail] = useState<string | null>(
    () => localStorage.getItem(ACCOUNT_KEY)
  );

  function handleConnected(connectedEmail: string) {
    localStorage.setItem(ACCOUNT_KEY, connectedEmail);
    setEmail(connectedEmail);
  }

  function handleLogout() {
    localStorage.removeItem(ACCOUNT_KEY);
    setEmail(null);
  }

  if (!email) return <AccountSetup onConnected={handleConnected} />;
  return <InboxApp email={email} onLogout={handleLogout} />;
}
