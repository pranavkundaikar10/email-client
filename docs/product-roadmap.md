# Product Roadmap

This is a future-facing list for the productivity email workflow. The first
priority is safe, fast handling of job-application email without taking remote
actions automatically.

## Next UX priorities

### 1. Undo archive and delete

Show a short-lived Undo toast after queueing an archive or Gmail Trash action.
If the action has not reached Gmail yet, cancel the queued operation. If it has
already completed, restore the thread to Inbox as a separate explicit action.

**Done when:** accidental actions can be reversed from the UI without manually
opening Gmail.

### 2. Keyboard-first review decisions

In Review Queue, support obvious shortcuts and display them in the interface:

- `A` — Archive
- `K` — Keep in inbox
- `F` — Follow up
- `#` — Move to Gmail Trash

**Done when:** a user can review a sequence of emails without reaching for the
mouse, while buttons remain available and discoverable.

### 3. Application-status badges

Display compact, consistent badges for the AI category:

- Confirmation
- Rejection
- Assessment
- Recruiter / screening
- Interview
- Offer
- Other

**Done when:** the sidebar makes job-related email type apparent before opening
the message.

### 4. Explain why review is needed

Show a short evidence-based rationale below the AI summary, for example
“Assessment link detected” or “Interview scheduling language detected.”

**Done when:** users can understand the recommendation without having to trust
an unexplained score.

### 5. Follow-up and snooze

Allow “tomorrow,” “in 3 days,” and custom-date follow-up decisions. Remove the
thread from the current review queue and resurface it at the chosen time.

**Done when:** follow-up is a durable local workflow that survives app restarts
and never changes Gmail unexpectedly.

### 6. Attachment-first review support

Keep the current conservative behavior: attachment-bearing messages must not be
classified low-risk by default. Add a prominent attachment badge and later a
safe preview/download flow.

**Done when:** an assessment sent as a PDF or document is visually distinct and
cannot be casually archived.

### 7. User-defined safety rules

Let users add simple local rules, such as:

- Never suggest Archive for chosen senders or domains.
- Always require review for terms such as “assessment,” “interview,” “schedule,”
  or “action required.”
- Hide application confirmations only after the user has explicitly approved a
  rule.

**Done when:** automation remains transparent, reversible, and opt-in.

### 8. Job-search daily digest

Build on the existing digest with a concise job-search view, for example:
“2 rejections, 1 screening request, 1 assessment due Friday.”

**Done when:** the user can understand the day’s application activity at a
glance.

## Recommended delivery order

1. Undo archive/delete
2. Keyboard review decisions
3. Application-status badges
4. Follow-up/snooze
5. Review explanations and safety rules
6. Attachment preview
7. Job-search digest

## Engineering follow-ups

- Make Gmail archive/delete retries idempotent by verifying whether the thread
  has already reached Archive or Trash before retrying an uncertain operation.
- Add a persistent “actions needing attention” surface for failed Gmail
  operations, including Retry and Dismiss controls.
- Add local Trash retention and reconciliation: retain locally trashed threads
  for roughly 30–35 days, periodically confirm their presence in Gmail Trash,
  then permanently remove local thread, message, analysis, and review records
  once Gmail has removed them.
- Add regression tests for MIME body selection, the durable Gmail outbox, and
  shared optimistic mail actions.
