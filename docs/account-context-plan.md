# Account Context Foundation Plan

## Purpose

Prepare Productive Email for multiple Gmail accounts and future providers such
as Outlook without mixing mail, local workflow state, credentials, or queued
remote actions between accounts.

This is groundwork for durable drafts and provider support. It is not a change
to the current one-account-at-a-time product workflow by itself.

## Core rule

Every operation involving mail must receive an explicit `account_id`. No
backend command, cache entry, queued operation, or UI action may infer an
account from a global email address or whichever account was most recently
used.

## Scope boundaries

| Data or setting | Intended scope |
| --- | --- |
| Local AI model, thinking defaults, visual settings | Device-wide |
| OAuth credentials and provider configuration | Per account and provider |
| Threads, messages, attachments, search index | Per account |
| Sync cursors, IMAP IDLE workers, mutation/outbox queues | Per account |
| Splits, triage preferences, review decisions, follow-ups, drafts | Per account |
| Active account selection | Device-local UI preference |

## Target model

An account represents one authenticated mailbox and provider:

```text
Account
  id              stable local UUID
  provider        gmail | outlook | future provider
  email           canonical mailbox address
  display_name    optional
  profile_picture optional
  status          connected | needs_reauth | disconnected
```

Provider-specific remote identifiers must be stored alongside the account. A
thread or message key must be namespaced by provider and account rather than
assuming a remote ID is globally unique.

## Implementation phases

### 1. Establish stable account identity

- Add a stable local account ID rather than using an email address as the
  implicit identity everywhere.
- Migrate the existing connected Gmail account and all its local data without
  loss.
- Link credentials to the stable account ID; retain the mailbox email as the
  display value and secure-keychain lookup key during this compatibility
  phase.
- Audit every database relation and index for account scoping.

**Done when:** existing users open the upgraded app with the same mail,
reviews, follow-ups, settings, and credentials, and no duplicate data.

### 2. Scope backend commands and durable queues

- Require `account_id` for sync, body fetch, send, archive, delete, flag,
  follow-up, analysis, and draft commands.
- Scope pending mail and flag operations to the owning account.
- Run sync and IDLE lifecycle independently per connected account.
- Ensure retry/failure events identify the account that needs attention.

**Done when:** one account cannot read, modify, retry, or authenticate as
another account through any backend command.

### 3. Scope frontend data and selection state

- Include `account_id` in React Query keys for threads, search, review queue,
  follow-ups, unread counts, analysis, drafts, and provider status.
- Clear thread selection, checked-thread state, and view-local cached data on
  account switch.
- Keep active-account selection in a local UI preference.
- Preserve the current shared list/action hooks; pass account context through
  them instead of creating account-specific variants.

**Done when:** switching accounts cannot display cached rows, counts, or
selection state from the previous account.

### 4. Add a minimal multi-account user flow

- Add an account menu to switch the active account and add another account.
- Keep the default experience account-at-a-time; do not introduce a unified
  inbox until its ordering, search, and action semantics are explicitly
  designed.
- Show account identity in compose and settings where it affects behaviour.
- Provide a clear re-authentication state without silently removing local
  mail data.

**Done when:** a user can connect, switch, and remove accounts safely, with
each account retaining an independent inbox and local workflow state.

### 5. Build account-aware compose and drafts

- Store every draft and outbox item with `account_id` and provider draft ID.
- Default compose to the active account; make the sending account visible.
- Route autosave, send, retry, Sent updates, and Drafts synchronization to the
  selected account/provider.

**Done when:** draft recovery and sending remain correct after switching
accounts or restarting the app.

### 6. Add provider adapters

- Introduce a focused provider interface for authentication, sync, message
  fetch, mutations, drafts, send, and notification lifecycle.
- Keep Gmail IMAP/SMTP behaviour behind the Gmail adapter while moving toward
  stable Gmail draft handling.
- Add Outlook through Microsoft Graph only after account context is complete.

**Done when:** adding a provider does not require view-specific conditionals
or changes to the shared account-scoped data model.

## Data safety rules

- Back up the SQLite database before any schema/data migration.
- Make migrations idempotent and resumable; never delete source rows during
  the identity migration.
- Preserve existing legacy-app migration markers and do not re-import older
  settings over current account-scoped settings.
- Do not retry a remote send blindly after an uncertain network outcome.
- Keep account removal explicit: remove credentials first only after user
  confirmation, and retain local mail only according to a documented choice.

## Verification checklist

- Upgrade an existing single-account database and verify all local workflow
  data is present exactly once.
- Connect two test accounts and alternate sync, body fetch, archive/delete,
  read/star, search, follow-up, and AI review actions.
- Confirm query caches and keyboard selection reset on account switch.
- Restart during a pending queued action and verify it resumes only under the
  owning account.
- Disconnect one account and verify the other account remains functional.
- Run frontend build and Rust checks after each implementation checkpoint.

## Deferred product decision

A unified multi-account inbox is intentionally out of scope for this
foundation. It should be designed separately, including cross-account search,
sorting, badges, action confirmation, and the visual source of each message.
