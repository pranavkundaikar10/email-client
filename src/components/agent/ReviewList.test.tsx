import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ReviewList from "./ReviewList";
import { useAppStore } from "../../store";

const { apiMocks } = vi.hoisted(() => ({
  apiMocks: {
    getReviewQueue: vi.fn(),
    getAiAssistanceSettings: vi.fn(),
    getOllamaModels: vi.fn(),
    getAutoAnalysisPendingCount: vi.fn(),
    getFollowUps: vi.fn(),
    enqueueBacklogTriage: vi.fn(),
  },
}));

vi.mock("../../lib/api", () => ({ api: apiMocks }));
vi.mock("../../hooks/useMailActions", () => ({
  useMailActions: () => ({ archiveThreads: vi.fn(), deleteThreads: vi.fn() }),
}));
vi.mock("../../hooks/useMailFlags", () => ({
  useMailFlags: () => ({ setStarred: vi.fn() }),
}));
vi.mock("../../hooks/useVisibleThreadList", () => ({ useVisibleThreadList: () => undefined }));
vi.mock("../email/BulkActionBar", () => ({ default: () => null }));
vi.mock("../email/ThreadItem", () => ({
  default: ({ thread }: { thread: { subject: string } }) => <div>{thread.subject}</div>,
}));

function renderReviewList() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ReviewList accountId="account-1" />
    </QueryClientProvider>,
  );
}

describe("ReviewList older batches", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    useAppStore.setState({ selectedThreadId: null, checkedThreadIds: new Set<string>() });
    apiMocks.getReviewQueue.mockResolvedValue({ items: [{
      id: "thread-1",
      thread_id: "thread-1",
      account_id: "account-1",
      subject: "Ready for review",
      snippet: "",
      unread: true,
      starred: false,
      archived: false,
      last_message_at: new Date().toISOString(),
      label_ids: "[]",
      from_name: "Sender",
      from_email: "sender@example.com",
      to_emails: "[]",
      folder: "inbox",
      importance: 3,
      category: "other",
      summary: "",
      action_items: "[]",
      deadline: null,
      is_actionable: false,
      recommended_action: "review",
      analysis_available: true,
      triage_failed: false,
      triage_error: "",
    }], total: 101, next_offset: 50 });
    apiMocks.getAiAssistanceSettings.mockResolvedValue({ enabled: true });
    apiMocks.getOllamaModels.mockResolvedValue([]);
    apiMocks.getAutoAnalysisPendingCount.mockResolvedValue(0);
    apiMocks.getFollowUps.mockResolvedValue([]);
    apiMocks.enqueueBacklogTriage.mockResolvedValue({ queued: 50, headers_discovered: 0 });
  });

  it("keeps the older-email batch control available while queue items are present", async () => {
    renderReviewList();

    const button = await screen.findByRole("button", { name: "Review next 50 older emails" });
    fireEvent.click(button);

    await waitFor(() => expect(apiMocks.enqueueBacklogTriage).toHaveBeenCalledWith("account-1"));
  });

  it("shows the full review total rather than the number currently loaded", async () => {
    renderReviewList();

    expect(await screen.findByText("101 to review")).toBeInTheDocument();
    expect(apiMocks.getReviewQueue).toHaveBeenCalledWith("account-1", 50, "priority", 0);
  });
});
