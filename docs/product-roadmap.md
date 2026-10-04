# Product Roadmap

This tracks work that is genuinely still pending. The current product already
has Undo for archive/delete, keyboard-first review, AI job-category badges,
durable Follow-ups, attachment downloads, configurable triage preferences,
and a backend-owned local AI triage worker.

## Next priorities

### 1. Triage failure visibility

Show terminal AI-analysis failures in Review with a clear reason and explicit
Retry/Dismiss controls. The worker must expose pending, processing, failed,
and completed states without treating a failed job as still processing.

### 2. Gmail action recovery

Add an “actions needing attention” surface for failed Gmail delivery with
Retry and Dismiss. Before retrying uncertain archive/delete delivery, verify
whether Gmail already applied the requested state.

### 3. Local Trash retention

Retain locally trashed threads for roughly 30–35 days, reconcile against Gmail
Trash, then safely remove expired local thread, message, analysis, review, and
follow-up data.

### 4. Review clarity

Add a short evidence-based rationale such as “Assessment link detected” below
the AI recommendation. Make attachment-bearing threads more prominent in list
views so they are not casually archived.

### 5. Job-search digest

Turn the existing analysis data into a concise daily job-search summary, such
as “2 rejections, 1 screening request, 1 assessment due Friday.”

### 6. Compose and drafts

Build durable autosaved drafts, attachment support, visible sending-account
selection, send/retry states, and safe handling of uncertain delivery.

### 7. Multi-account and providers

Finish the account-context plan: account switching, fully account-scoped local
workflow data and caches, two-account regression tests, and account-aware
compose/drafts. Add Outlook through Microsoft Graph only afterward.

### 8. Calendar prototype evaluation

Use the current Calendar link/ICS export prototype before deciding whether to
keep it. If retained, improve its event card and test export/validation. It
must remain user-confirmed rather than creating remote events automatically.

## Engineering follow-ups

- Run the frontend and Rust suites in GitHub Actions for pull requests and
  release builds.
- Add mocked-Ollama coverage for the full calendar-free fallback path.
- Refine rapid-navigation body prefetching with active-thread priority,
  bounded look-ahead, and stale-work cancellation.
- Keep README and this roadmap aligned with shipped behavior.
