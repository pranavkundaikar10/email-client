import { invoke } from "@tauri-apps/api/core";

export interface Thread {
  id: string;
  account_id: string;
  subject: string;
  snippet: string;
  unread: boolean;
  starred: boolean;
  archived: boolean;
  last_message_at: string;
  label_ids: string;
  from_name: string;
  from_email: string;
  to_emails: string;
  category: string;
  folder: string;
}

export interface Message {
  id: string;
  thread_id: string;
  from_email: string;
  from_name: string;
  to_emails: string;
  cc_emails: string;
  subject: string;
  body_html: string | null;
  body_text: string | null;
  sent_at: string;
  unread: boolean;
  body_fetched: boolean;
}

export interface SplitRule {
  type: "is_newsletter" | "from_pattern" | "from_email" | "from_domain" | "subject_contains";
  value?: string;
}

export interface Split {
  id: string;
  name: string;
  position: number;
  rules: SplitRule[];
}

export type AnalysisCategory =
  | "interview"
  | "assessment"
  | "offer"
  | "rejection"
  | "application_update"
  | "networking"
  | "deadline"
  | "newsletter"
  | "other";

export interface ThreadAnalysis {
  thread_id: string;
  is_actionable: boolean;
  importance: number; // 1-5
  category: AnalysisCategory | string;
  summary: string;
  action_items: string; // JSON-encoded string[]
  deadline: string | null;
  model: string;
  analyzed_at: string;
}

export interface DigestItem {
  thread_id: string;
  subject: string;
  from_name: string;
  from_email: string;
  unread: boolean;
  is_actionable: boolean;
  importance: number;
  category: AnalysisCategory | string;
  summary: string;
  action_items: string; // JSON-encoded string[]
  deadline: string | null;
}

export const api = {
  addAccount: (email: string, password: string) =>
    invoke<string>("add_account", { email, password }),

  removeAccount: (email: string) =>
    invoke<void>("remove_account", { email }),

  getThreads: (limit = 50, offset = 0, view = "inbox", category?: string) =>
    invoke<Thread[]>("get_threads", { limit, offset, view, category }),

  getMessages: (threadId: string) =>
    invoke<Message[]>("get_messages", { threadId: threadId }),

  markThreadRead: (threadId: string) =>
    invoke<void>("mark_thread_read", { threadId: threadId }),

  markThreadUnread: (threadId: string) =>
    invoke<void>("mark_thread_unread", { threadId: threadId }),

  archiveThread: (threadId: string) =>
    invoke<void>("archive_thread", { threadId: threadId }),

  deleteThread: (threadId: string) =>
    invoke<void>("delete_thread", { threadId: threadId }),

  starThread: (threadId: string, starred: boolean) =>
    invoke<void>("star_thread", { threadId, starred }),

  searchThreads: (query: string) =>
    invoke<Thread[]>("search_threads", { query }),

  getUnreadCounts: () =>
    invoke<Record<string, number>>("get_unread_counts"),

  syncInbox: (email: string) =>
    invoke<number>("sync_inbox", { email }),

  syncOlder: (email: string, beforeDate: string) =>
    invoke<number>("sync_older", { email, beforeDate }),

  syncSent: (email: string) =>
    invoke<number>("sync_sent", { email }),

  syncDrafts: (email: string) =>
    invoke<number>("sync_drafts", { email }),

  fetchMessageBody: (email: string, messageId: string) =>
    invoke<Message>("fetch_message_body", { email, messageId }),

  getSplits: () =>
    invoke<Array<Split & { rules: string }>>("get_splits"),

  createSplit: (name: string) =>
    invoke<Split & { rules: string }>("create_split", { name }),

  updateSplit: (id: string, name: string, rules: SplitRule[]) =>
    invoke<void>("update_split", { id, name, rules: JSON.stringify(rules) }),

  deleteSplit: (id: string) =>
    invoke<void>("delete_split", { id }),

  reorderSplits: (ids: string[]) =>
    invoke<void>("reorder_splits", { ids }),

  recategorizeThreads: () =>
    invoke<void>("recategorize_threads"),

  // Agent — local-LLM email triage. `model`/`baseUrl` are optional overrides
  // for the Ollama model name / server URL (defaults live on the Rust side).
  analyzeThread: (threadId: string, model?: string, baseUrl?: string) =>
    invoke<ThreadAnalysis>("analyze_thread", { threadId, model, baseUrl }),

  analyzeInbox: (model?: string, baseUrl?: string, limit?: number) =>
    invoke<ThreadAnalysis[]>("analyze_inbox", { model, baseUrl, limit }),

  getDigest: (limit = 50) =>
    invoke<DigestItem[]>("get_digest", { limit }),

  getThreadAnalysis: (threadId: string) =>
    invoke<ThreadAnalysis | null>("get_thread_analysis", { threadId }),

  sendEmail: (req: {
    from: string;
    to: string[];
    cc: string[];
    subject: string;
    body: string;
    inReplyTo?: string;
    references?: string;
  }) => invoke<{ ok: boolean }>("send_email", { req }),
};
