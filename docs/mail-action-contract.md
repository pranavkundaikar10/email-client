# Mail Action Contract

This document defines the safety boundary between local productivity workflow
state and changes made to a connected Gmail mailbox. It is the reference for
implementation and regression tests.

## Remote Gmail actions

Only an explicit mail action may create a durable remote-operation record.

| User action | Local behavior | Gmail behavior | Durable queue |
| --- | --- | --- | --- |
| Archive | Hides the thread immediately; completes an active follow-up | Removes the thread from Gmail Inbox after the Undo window | `mail_operations` (`archive`) |
| Delete | Hides the thread immediately; completes an active follow-up | Moves the thread to Gmail Trash after the Undo window | `mail_operations` (`trash`) |
| Mark read / unread | Updates the local read state immediately | Updates Gmail's read state asynchronously | `mail_flag_operations` |
| Star / unstar | Updates the local star state immediately | Updates Gmail's star state asynchronously | `mail_flag_operations` |

Archive and Delete use the shared `useMailActions` path. They provide the
standard eight-second Undo window. A queued Gmail action must survive an app
restart and be retried or surfaced as a failure through the existing outbox.

## Local-only workflow actions

These actions must never create either remote-operation queue entry and must
not directly call IMAP or a Gmail API.

| User action | Local behavior | Gmail behavior |
| --- | --- | --- |
| Keep (`I` in Review Queue) | Records the review decision as `keep` | None |
| Complete (`I` on an active follow-up) | Marks the follow-up complete and records `keep` | None |
| Schedule / reschedule follow-up | Stores or updates the local due time | None |
| AI analysis | Stores local triage/review data | None |
| Review decision | Stores the local decision | None |

Completing a follow-up means “handled, but keep the email where it is.” It
therefore leaves Gmail Inbox, read state, and star state unchanged. Archiving
or deleting a thread intentionally completes an active follow-up because the
thread is no longer meant to resurface.

## Required regression coverage

Any action-path change must preserve these tests:

- Keep creates no `mail_operations` or `mail_flag_operations` row.
- Complete creates no remote-operation row, marks the follow-up completed, and
  stores a `keep` review decision.
- Schedule and reschedule create no remote-operation row.
- AI analysis creates no remote-operation row.
- Archive creates exactly one pending `mail_operations` row with operation
  `archive`, and no flag-operation row.
- Delete creates exactly one pending `mail_operations` row with operation
  `trash`, and no flag-operation row.
- Star/unstar and read/unread create only the expected coalesced
  `mail_flag_operations` row.
- Undo removes only a still-pending archive/delete operation; it must not
  alter local-only review or follow-up decisions.
- A failed Gmail delivery restores the standard visible state and reports the
  failure without converting the requested action into another action.

Tests should use a temporary SQLite database and mocked IMAP/Gmail transport;
they must never run against a real mailbox.
