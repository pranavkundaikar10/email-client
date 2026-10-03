# Testing-First and Backend Triage Worker Plan

## Purpose

Establish reliable automated coverage before moving automatic AI triage from
the frontend into a durable backend worker. The goal is to make Gmail-changing
actions, local workflow state, sync, and background analysis safe to evolve.

## Current boundary

The backend already owns OAuth, IMAP, SQLite, Gmail operation outboxes, and
the actual Ollama request. The frontend currently owns the periodic sync timer,
candidate selection, and one-at-a-time AI scheduling. That frontend scheduling
can stall while the app is unfocused or if an Ollama request never returns.

The target is a backend-owned, account-scoped triage job queue. The frontend
will render status and send explicit user intent only.

## Phase 1: Test harness

Build reusable test foundations before changing scheduler behavior.

- Rust tests use temporary SQLite databases with real migrations applied.
- Fixtures create accounts, threads, messages, analyses, follow-ups, and
  queued operations without depending on personal local data.
- IMAP/Gmail, Ollama, Keychain, and Calendar integrations are represented by
  mockable adapters. Tests must never contact a real mailbox or local model.
- Frontend tests use Vitest and React Testing Library, with fresh React Query
  state and mocked Tauri commands for each test.
- Keep successful test output quiet; detailed diagnostics should appear only
  on failures.

## Phase 2: Mail-action safety suite

Implement the regression coverage specified in
[`mail-action-contract.md`](./mail-action-contract.md):

- Keep, Complete, scheduling/rescheduling follow-up, review decisions, and AI
  analysis never create remote Gmail operation rows.
- Archive and Delete create the correct `mail_operations` entry.
- Read/unread and star/unstar create only coalesced `mail_flag_operations`.
- Undo affects only still-pending archive/delete operations.
- Failed remote delivery restores the standard visible state and reports the
  failure without changing the action type.

## Phase 3: Define the backend triage-worker contract in tests

Write these tests before implementing the worker:

- Syncing a new eligible email creates one account-scoped triage job.
- A worker runs at most one Ollama request per account at a time.
- Pending jobs drain serially without window focus or frontend timers.
- Model timeouts and failures become retryable, visible job states; they never
  leave a job permanently marked as analyzing.
- Archived, reviewed, already-analyzed, and ineligible emails are skipped.
- Restarting the app resumes pending work safely and does not duplicate jobs.
- Queue status reports pending, processing, failed, and completed counts.

## Phase 4: Implement in small vertical slices

1. Add a durable, account-scoped triage-jobs table and backend repository
   functions, covered by SQLite tests.
2. Add the serialized backend worker, model deadline/retry behavior, and
   status events, covered by mocked-Ollama tests.
3. Move the frontend to display backend queue status and provide a manual
   retry/error state.
4. Remove the frontend candidate-selection timer and in-memory scheduling
   lock only after the backend worker tests pass.

## Phase 5: Standard checks and CI

After the harness is established, document exact commands in `AGENTS.md`:

- run focused frontend and Rust tests for the edited behavior during work;
- run the complete test suite before commit/push;
- run the complete suite again in GitHub Actions for pull requests and release
  builds.

Pure styling/layout work may use proportionate manual verification. Changes to
Gmail actions, persistence, sync, account isolation, or the local/remote
boundary require regression coverage.
