# Engineering Guidance

## Reuse before adding behavior

Before creating a new component, hook, command, or mutation path, search the
repository for an existing implementation of the same user-visible behavior.
Extend or reuse that implementation when it fits instead of duplicating it.

In particular:

- Thread-list views must reuse shared selection state and bulk-action UI.
- Gmail-changing actions must go through `src/hooks/useMailActions.ts`; do not
  call archive/delete API methods directly from a view.
- When a behavior is shared by more than one view, extract it into a focused
  reusable component or hook before adding another view-specific version.
- Keep cache updates, optimistic UI behavior, failure recovery, and keyboard
  behavior consistent across every view that exposes the same action.
- Every thread-list view must pass its final rendered ordering through
  `useVisibleThreadList` so keyboard navigation matches what the user sees.
- Upcoming message-body prefetching must use `useUpcomingBodyPrefetch` and the
  bounded `prefetch_thread_bodies` command. It prefetches the nearest two
  first, then the remaining look-ahead item only when still relevant; it must
  remain read-only and must not trigger AI analysis.

## Before handoff

Run the relevant frontend and Rust checks after changes:

```sh
npm run build
(cd src-tauri && cargo check)
```
