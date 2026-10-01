import "./App.css";
import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useHotkeys } from "react-hotkeys-hook";
import AccountSetup from "./pages/AccountSetup";
import Sidebar from "./components/layout/Sidebar";
import SearchBar from "./components/email/SearchBar";
import ThreadList from "./components/email/ThreadList";
import EmailPreview from "./components/email/EmailPreview";
import ComposeModal from "./components/email/ComposeModal";
import CommandPalette from "./components/ui/CommandPalette";
import KeyboardShortcutsDialog from "./components/ui/KeyboardShortcutsDialog";
import SplitsSettings from "./components/settings/SplitsSettings";
import DigestPanel from "./components/agent/DigestPanel";
import ReviewList from "./components/agent/ReviewList";
import FollowUpList from "./components/agent/FollowUpList";
import { ToastContainer } from "./components/ui/Toast";
import { useKeyboardNav } from "./hooks/useKeyboardNav";
import { useAppStore } from "./store";
import { api } from "./lib/api";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type View = "inbox" | "starred" | "archive" | "search" | "sent" | "drafts" | "review" | "follow_ups";

const ACCOUNT_KEY = "connected_email";

function SignOutDialog({ email, signingOut, onConfirm, onClose }: {
  email: string;
  signingOut: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || signingOut) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, signingOut]);

  return <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[20vh] backdrop-blur-sm" onMouseDown={(event) => { if (event.target === event.currentTarget && !signingOut) onClose(); }}>
    <div className="app-dialog w-full max-w-md overflow-hidden rounded-xl shadow-2xl" role="dialog" aria-modal="true" aria-label="Sign out">
      <div className="app-dialog-header border-b px-5 py-4">
        <h2 className="text-sm font-semibold text-gray-900">Sign out of {email}?</h2>
        <p className="mt-1 text-sm leading-5 text-gray-500">Your locally synced email and AI data will stay on this device.</p>
      </div>
      <div className="flex justify-end gap-2 px-5 py-3">
        <button type="button" onClick={onClose} disabled={signingOut} className="button-secondary rounded-lg px-3 py-1.5 text-sm disabled:opacity-40">Cancel</button>
        <button type="button" onClick={onConfirm} disabled={signingOut} className="rounded-lg bg-gray-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-gray-700 disabled:opacity-40">{signingOut ? "Signing out…" : "Sign out"}</button>
      </div>
      <div className="app-dialog-footer border-t px-5 py-2 text-xs text-gray-400">esc cancel</div>
    </div>
  </div>;
}

function InboxApp({ email, onLogout }: { email: string; onLogout: () => void }) {
  const [activeView, setActiveView] = useState<View>("review");
  const [initialSyncComplete, setInitialSyncComplete] = useState(false);
  const startupViewResolved = useRef(false);
  const [, setSyncing] = useState(false);
  const syncedFolders = useRef(new Set<string>());
  const backgroundAnalysisRunning = useRef(false);
  const inboxSyncInProgress = useRef(false);
  const addToast = useAppStore((s) => s.addToast);
  const [composeOpen, setComposeOpen] = useState(false);
  const [splitsOpen, setSplitsOpen] = useState(false);
  const [digestOpen, setDigestOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [simpleFullscreen, setSimpleFullscreen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const commandPaletteOpen = useAppStore((s) => s.commandPaletteOpen);
  const setCommandPaletteOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const activeSplitId = useAppStore((s) => s.activeSplitId);
  const setActiveSplitId = useAppStore((s) => s.setActiveSplitId);
  const queryClient = useQueryClient();

  const toggleSimpleFullscreen = useCallback(async () => {
    const appWindow = getCurrentWindow();
    if (await appWindow.isFullscreen()) {
      addToast("Exit macOS fullscreen first, then use Control-Shift-F.");
      return;
    }
    const next = !simpleFullscreen;
    try {
      await appWindow.setSimpleFullscreen(next);
      setSimpleFullscreen(next);
    } catch (error) {
      addToast(`Could not change fullscreen: ${String(error)}`);
    }
  }, [addToast, simpleFullscreen]);

  useEffect(() => {
    const unlisten = listen<{ operation: string; error: string }>("mail-operation-failed", ({ payload }) => {
      if (payload.operation === "flags") {
        addToast("Could not sync this email’s read/star status to Gmail.");
      } else {
        const action = payload.operation === "trash" ? "move the email to Gmail Trash" : "archive the email";
        addToast(`Could not ${action}. It has been returned to your inbox.`);
      }
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      console.warn("Queued Gmail operation failed:", payload.error);
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, [addToast, queryClient]);

  const { data: rawSplits = [] } = useQuery({
    queryKey: ["splits"],
    queryFn: api.getSplits,
  });
  const { data: accountProfile } = useQuery({
    queryKey: ["account_profile", email],
    queryFn: () => api.getAccountProfile(email),
  });
  const { data: accountContext } = useQuery({
    queryKey: ["account_context", email],
    queryFn: () => api.getAccountContext(email),
  });
  const { data: aiAssistanceSettings = { enabled: false } } = useQuery({
    queryKey: ["ai_assistance_settings"],
    queryFn: api.getAiAssistanceSettings,
  });
  const { data: ollamaModels = [], isSuccess: ollamaAvailable } = useQuery({
    queryKey: ["ollama_models"],
    queryFn: api.getOllamaModels,
    enabled: aiAssistanceSettings.enabled,
    retry: false,
    refetchInterval: 60_000,
  });

  const { data: unreadCounts = {} } = useQuery({
    queryKey: ["unread_counts"],
    queryFn: api.getUnreadCounts,
    refetchInterval: 60_000,
  });
  const { data: followUps = [] } = useQuery({
    queryKey: ["follow_ups"],
    queryFn: api.getFollowUps,
    refetchInterval: 60_000,
  });
  const followUpDueCount = useMemo(() => {
    const endToday = new Date();
    endToday.setHours(23, 59, 59, 999);
    return followUps.filter((item) => new Date(item.due_at) <= endToday).length;
  }, [followUps]);

  useEffect(() => {
    if (startupViewResolved.current || !initialSyncComplete) return;
    let cancelled = false;
    // Read a fresh startup snapshot rather than relying on whichever list
    // cache happened to render first during sync.
    Promise.all([api.getReviewQueue(50, "priority"), api.getAutoAnalysisPendingCount(), api.getFollowUps()])
      .then(([queue, pendingCount, startupFollowUps]) => {
        if (cancelled || startupViewResolved.current) return;
        startupViewResolved.current = true;
        const endToday = new Date();
        endToday.setHours(23, 59, 59, 999);
        const hasDueFollowUp = startupFollowUps.some((item) => new Date(item.due_at) <= endToday);
        if (queue.length === 0 && pendingCount === 0 && !hasDueFollowUp) setActiveView("inbox");
      })
      .catch(() => {
        // Keep Review as the safe default if the local status lookup fails.
        if (!cancelled) startupViewResolved.current = true;
      });
    return () => { cancelled = true; };
  }, [initialSyncComplete]);

  const splitDefs = useMemo(
    () => rawSplits.map((s) => ({ id: s.id, label: s.name })),
    [rawSplits]
  );

  // Search results
  const { data: searchResults = [] } = useQuery({
    queryKey: ["search", accountContext?.id ?? "", searchQuery],
    queryFn: () => api.searchThreads(accountContext!.id, searchQuery),
    enabled: searchQuery.length > 1 && Boolean(accountContext?.id),
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
    startupViewResolved.current = true;
    if (id !== effectiveSplitId) {
      const state = useAppStore.getState();
      state.clearChecked();
      state.setSelectedThread(null);
    }
    setActiveSplitId(id);
  }

  function changeView(view: View) {
    // Search is a global workspace overlay, not a separate mailbox. Keep the
    // current view intact so Escape restores exactly where the user started.
    if (view === "search") {
      searchInputRef.current?.focus();
      return;
    }
    startupViewResolved.current = true;
    if (view !== activeView) {
      const state = useAppStore.getState();
      state.clearChecked();
      state.setSelectedThread(null);
    }
    setActiveView(view);
  }

  useKeyboardNav({
    onViewChange: changeView,
    splits: splitDefs,
    onSplitChange: switchTab,
    activeSplitId: effectiveSplitId,
    splitNavigationEnabled: showTabs && !composeOpen && !splitsOpen && !digestOpen && !commandPaletteOpen && !shortcutsOpen,
  });

  // / — global mail search. A native listener is more reliable than the
  // hotkey wrapper for this punctuation key in the desktop WebView. It is
  // deliberately scoped to the mail workspace, never text entry or dialogs.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) return;
      if (composeOpen || splitsOpen || digestOpen || commandPaletteOpen || shortcutsOpen) return;
      const target = event.target;
      if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true'], [role='dialog']")) return;
      event.preventDefault();
      searchInputRef.current?.focus();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [commandPaletteOpen, composeOpen, digestOpen, shortcutsOpen, splitsOpen]);

  // App-controlled fullscreen keeps Escape available for email actions. On
  // macOS this deliberately uses Tauri's simple fullscreen mode instead of
  // the green-button native fullscreen space.
  useHotkeys("ctrl+shift+f", (event) => {
    event.preventDefault();
    void toggleSimpleFullscreen();
  }, { enableOnFormTags: false });

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "?" || event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target;
      if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true'], [role='dialog']")) return;
      event.preventDefault();
      setShortcutsOpen(true);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // C — compose
  useHotkeys("c", (e) => {
    e.preventDefault();
    setComposeOpen(true);
  }, { enableOnFormTags: false });

  const processOneRecentEmail = useCallback(async () => {
    // Keep local inference deliberately low-impact: one email at a time,
    // never in parallel, and only for mail received today.
    if (!aiAssistanceSettings.enabled || !ollamaAvailable || ollamaModels.length === 0 || backgroundAnalysisRunning.current) return;
    backgroundAnalysisRunning.current = true;
    try {
      const [candidate] = await api.getAutoAnalysisCandidates(1);
      if (!candidate) return;

      // BODY.PEEK fetches the content without changing Gmail's read state.
      await api.fetchMessageBody(email, candidate.message_id);
      await api.analyzeThread(candidate.thread_id, undefined, undefined, "background");
      queryClient.invalidateQueries({ queryKey: ["thread_analysis", candidate.thread_id] });
      queryClient.invalidateQueries({ queryKey: ["digest"] });
      queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      queryClient.invalidateQueries({ queryKey: ["follow_ups"] });
      queryClient.invalidateQueries({ queryKey: ["search"] });
    } catch (err) {
      console.warn("Background email analysis skipped:", err);
    } finally {
      backgroundAnalysisRunning.current = false;
    }
  }, [aiAssistanceSettings.enabled, email, ollamaAvailable, ollamaModels.length, queryClient]);

  // Both the periodic fallback and IMAP IDLE events use this single sync path.
  // It owns cache invalidation and keeps local LLM work serialized.
  const syncInbox = useCallback(async () => {
    if (inboxSyncInProgress.current) return;
    inboxSyncInProgress.current = true;
    setSyncing(true);
    try {
      await api.processPendingMailOperations();
      await api.syncInbox(email);
      queryClient.invalidateQueries({ queryKey: ["threads"] });
      queryClient.invalidateQueries({ queryKey: ["unread_counts"] });
      queryClient.invalidateQueries({ queryKey: ["review_queue"] });
      queryClient.invalidateQueries({ queryKey: ["auto_analysis_pending_count"] });
      await processOneRecentEmail();
      queryClient.invalidateQueries({ queryKey: ["auto_analysis_pending_count"] });
    } catch (err) {
      addToast(`Sync failed: ${String(err)}`);
    } finally {
      inboxSyncInProgress.current = false;
      setSyncing(false);
      setInitialSyncComplete(true);
    }
  }, [addToast, email, processOneRecentEmail, queryClient]);

  useEffect(() => {
    void syncInbox();
    const interval = setInterval(() => { void syncInbox(); }, 60_000);
    return () => clearInterval(interval);
  }, [syncInbox]);

  useEffect(() => {
    void api.startInboxIdle(email).catch((error) => console.warn("Could not start IMAP IDLE:", error));
    return () => { void api.stopInboxIdle(email); };
  }, [email]);

  useEffect(() => {
    let debounce: number | undefined;
    const unlisten = listen<string>("inbox-idle-changed", ({ payload }) => {
      if (payload !== email) return;
      if (debounce !== undefined) window.clearTimeout(debounce);
      debounce = window.setTimeout(() => { void syncInbox(); }, 1200);
    });
    return () => {
      if (debounce !== undefined) window.clearTimeout(debounce);
      void unlisten.then((dispose) => dispose());
    };
  }, [email, syncInbox]);

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
    <div className="app-shell flex h-screen w-screen overflow-hidden">
      <Sidebar
        activeView={activeView}
        onViewChange={changeView}
        email={email}
        onLogout={onLogout}
        onSplits={() => setSplitsOpen(true)}
        onCompose={() => setComposeOpen(true)}
        onDigest={() => setDigestOpen(true)}
        profilePicture={accountProfile?.profile_picture ?? null}
        inboxUnread={Object.values(unreadCounts).reduce((a, b) => a + b, 0)}
        followUpDueCount={followUpDueCount}
        searchActive={searchQuery.length > 0}
      />

      {/* Right content: tabs on top, then thread list + email preview below */}
      <div className="flex-1 min-w-0 flex flex-col overflow-hidden">

        {/* Split tabs — spans full width above both panels */}
        {showTabs && (
          <div className="app-tabs flex flex-shrink-0 border-b">
            {splitDefs.map((split, idx) => {
              const active = split.id === effectiveSplitId;
              return (
                <button
                  key={split.id}
                  onClick={() => switchTab(split.id)}
                  title={`G ${idx + 1}`}
                  className={`relative px-6 py-2.5 text-sm font-medium whitespace-nowrap transition-colors flex items-center gap-1.5 ${
                    active
                      ? "split-tab-active after:absolute after:bottom-0 after:inset-x-0 after:h-0.5"
                      : "split-tab-idle"
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
        <div className="flex min-w-0 flex-1 overflow-hidden">

          {/* Thread list panel */}
          <div className="thread-panel w-72 flex flex-shrink-0 flex-col border-r">
            <SearchBar
              value={searchQuery}
              onChange={setSearchQuery}
              onClear={() => setSearchQuery("")}
              inputRef={searchInputRef}
            />

            {isSearching ? <ThreadList
              accountId={accountContext?.id ?? ""}
              activeView={activeView}
              effectiveSplitId={effectiveSplitId}
              searchResults={searchResults}
              isSearching
            /> : activeView === "review" ? <ReviewList /> : activeView === "follow_ups" ? <FollowUpList /> : <ThreadList
              accountId={accountContext?.id ?? ""}
              activeView={activeView}
              effectiveSplitId={effectiveSplitId}
              searchResults={searchResults}
              isSearching={isSearching}
            />}
          </div>

          {/* Email preview */}
          <EmailPreview email={email} accountId={accountContext?.id ?? ""} reviewMode={activeView === "review" && !isSearching} followUpMode={activeView === "follow_ups" && !isSearching} />

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

      {/* Command palette */}
      {commandPaletteOpen && (
        <CommandPalette
          accountId={accountContext?.id ?? ""}
          onViewChange={changeView}
          onClose={() => setCommandPaletteOpen(false)}
          splits={splitDefs}
          onSplitChange={switchTab}
          onSplitsSettings={() => setSplitsOpen(true)}
          onToggleFullscreen={() => { void toggleSimpleFullscreen(); }}
          isSimpleFullscreen={simpleFullscreen}
        />
      )}

      {shortcutsOpen && <KeyboardShortcutsDialog onClose={() => setShortcutsOpen(false)} />}

      <ToastContainer />
    </div>
  );
}

export default function App() {
  const [email, setEmail] = useState<string | null | undefined>(undefined);
  const [signOutOpen, setSignOutOpen] = useState(false);
  const [signingOut, setSigningOut] = useState(false);

  useEffect(() => {
    const savedEmail = localStorage.getItem(ACCOUNT_KEY);
    if (savedEmail) {
      setEmail(savedEmail);
      return;
    }

    let cancelled = false;
    api.getAccounts()
      .then((accounts) => {
        if (cancelled) return;
        if (accounts.length === 0) {
          setEmail(null);
          return;
        }
        // LocalStorage belongs to the old WebView profile and is not carried
        // across an identifier change. The migrated account is authoritative.
        localStorage.setItem(ACCOUNT_KEY, accounts[0]);
        setEmail(accounts[0]);
      })
      .catch(() => {
        // Keep the setup screen available if account discovery is unavailable.
        if (!cancelled) setEmail(null);
      });
    return () => { cancelled = true; };
  }, []);

  function handleConnected(connectedEmail: string) {
    localStorage.setItem(ACCOUNT_KEY, connectedEmail);
    setEmail(connectedEmail);
  }

  async function handleLogout() {
    if (!email) return;
    setSigningOut(true);
    try {
      // Signing out must remove the stored credential as well; otherwise the
      // next launch would silently restore the account from local storage.
      await api.removeAccount(email);
    } catch (error) {
      console.error("Could not remove the stored account:", error);
      window.alert("Could not sign out. Please try again.");
      setSigningOut(false);
      return;
    }
    localStorage.removeItem(ACCOUNT_KEY);
    setEmail(null);
    setSignOutOpen(false);
    setSigningOut(false);
  }

  if (email === undefined) return null;
  if (!email) return <AccountSetup onConnected={handleConnected} />;
  return <>
    <InboxApp email={email} onLogout={() => setSignOutOpen(true)} />
    {signOutOpen && <SignOutDialog
      email={email}
      signingOut={signingOut}
      onConfirm={() => { void handleLogout(); }}
      onClose={() => setSignOutOpen(false)}
    />}
  </>;
}
