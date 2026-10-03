import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import EmailPreview from "./EmailPreview";
import { useAppStore } from "../../store";

const { apiMocks, mailActionMocks, mailFlagMocks } = vi.hoisted(() => ({
  apiMocks: {
    getMessages: vi.fn(),
    getAiAssistanceSettings: vi.fn(),
    getReviewQueue: vi.fn(),
    getFollowUps: vi.fn(),
    getThreadAnalysis: vi.fn(),
    recordReviewDecision: vi.fn(),
    completeFollowUp: vi.fn(),
  },
  mailActionMocks: {
    archiveThread: vi.fn(),
    deleteThread: vi.fn(),
  },
  mailFlagMocks: {
    markRead: vi.fn(),
    markUnread: vi.fn(),
    setStarred: vi.fn(),
  },
}));

vi.mock("../../lib/api", () => ({ api: apiMocks }));
vi.mock("../../hooks/useMailActions", () => ({ useMailActions: () => mailActionMocks }));
vi.mock("../../hooks/useMailFlags", () => ({ useMailFlags: () => mailFlagMocks }));
vi.mock("../../hooks/useUpcomingBodyPrefetch", () => ({ useUpcomingBodyPrefetch: () => undefined }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn(), openUrl: vi.fn() }));

const selectedThread = {
  id: "thread-1",
  account_id: "account-1",
  subject: "Interview details",
  snippet: "",
  unread: true,
  starred: false,
  archived: false,
  last_message_at: "2026-10-03T12:00:00Z",
  label_ids: "[]",
  from_name: "Recruiter",
  from_email: "recruiter@example.com",
  to_emails: "[]",
  category: "inbox",
  folder: "INBOX",
};

function renderPreview({ reviewMode = true, followUpMode = false } = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <EmailPreview email="me@example.com" accountId="account-1" reviewMode={reviewMode} followUpMode={followUpMode} />
    </QueryClientProvider>,
  );
}

describe("EmailPreview review shortcuts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAppStore.setState({
      selectedThreadId: selectedThread.id,
      threads: [selectedThread],
      checkedThreadIds: new Set<string>(),
      toasts: [],
    });
    apiMocks.getMessages.mockResolvedValue([]);
    apiMocks.getAiAssistanceSettings.mockResolvedValue({ enabled: false });
    apiMocks.getThreadAnalysis.mockResolvedValue(null);
    apiMocks.getReviewQueue.mockResolvedValue([]);
    apiMocks.getFollowUps.mockResolvedValue([]);
    apiMocks.recordReviewDecision.mockResolvedValue(undefined);
    apiMocks.completeFollowUp.mockResolvedValue(undefined);
    mailActionMocks.archiveThread.mockResolvedValue(undefined);
    mailActionMocks.deleteThread.mockResolvedValue(undefined);
    mailFlagMocks.markRead.mockResolvedValue(undefined);
    mailFlagMocks.markUnread.mockResolvedValue(undefined);
    mailFlagMocks.setStarred.mockResolvedValue(undefined);
  });

  afterEach(() => {
    useAppStore.setState({ selectedThreadId: null, threads: [], checkedThreadIds: new Set<string>(), toasts: [] });
  });

  it("I keeps a reviewed email locally without queuing archive or delete", async () => {
    apiMocks.getReviewQueue.mockResolvedValue([{
      thread_id: selectedThread.id,
      analysis_available: true,
      recommended_action: "keep",
    }]);
    renderPreview();

    await screen.findByRole("button", { name: "Keep" });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "i", bubbles: true }));

    await waitFor(() => expect(apiMocks.recordReviewDecision).toHaveBeenCalledWith(selectedThread.id, "keep"));
    expect(apiMocks.completeFollowUp).not.toHaveBeenCalled();
    expect(mailActionMocks.archiveThread).not.toHaveBeenCalled();
    expect(mailActionMocks.deleteThread).not.toHaveBeenCalled();
  });

  it("I completes a due follow-up and keeps the email without changing Gmail mail state", async () => {
    apiMocks.getFollowUps.mockResolvedValue([{
      thread_id: selectedThread.id,
      due_at: "2026-10-03T11:00:00Z",
      subject: selectedThread.subject,
    }]);
    renderPreview({ reviewMode: false, followUpMode: true });

    await screen.findByRole("button", { name: "Complete" });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "i", bubbles: true }));

    await waitFor(() => expect(apiMocks.completeFollowUp).toHaveBeenCalledWith(selectedThread.id));
    expect(apiMocks.recordReviewDecision).toHaveBeenCalledWith(selectedThread.id, "keep");
    expect(mailActionMocks.archiveThread).not.toHaveBeenCalled();
    expect(mailActionMocks.deleteThread).not.toHaveBeenCalled();
  });
});
