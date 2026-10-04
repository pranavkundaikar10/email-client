# Calendar Event Extraction Plan

## Goal

Offer a user-confirmed calendar candidate when an email contains a trustworthy
event. Never let an LLM invent a year, timezone, duration, or remote calendar
action.

## Candidate contract

Each candidate is provider-neutral and records:

- `source`: `ics`, `structured`, or `free_text`
- `title`
- raw date, time, timezone, location, and duration text
- the email received timestamp used as the reference point
- resolved start/end only after deterministic validation
- confidence and an ambiguity reason when no event can be safely resolved

Google Calendar links and `.ics` files are output adapters only. They receive
validated resolved values; neither parses raw email text nor writes a remote
calendar event.

## Resolution order

1. Parse `text/calendar` / `.ics` MIME parts. Their DTSTART, DTEND, TZID, UID,
   update sequence, and cancellation data are authoritative.
2. Use sender-provided structured event data when it has complete date/time
   data.
3. For free text, use the model only to identify relevant raw event phrases.
   Resolve them deterministically against the email received timestamp.

The model must not emit final RFC3339 timestamps for free-text events.

## Free-text safety rules

- `tomorrow`, weekdays, and month/day values are resolved from the received
  timestamp, never from the current machine date.
- Independent clues must agree. For example, “tomorrow” and “Saturday,
  October 3” must resolve to the same date.
- Preserve an explicit timezone. Ambiguous abbreviations are displayed to the
  user and do not silently become an IANA timezone.
- Use an explicit duration/end only. If neither is available, the candidate is
  not exportable rather than inventing a default meeting length.
- Any concrete event with a complete date, time, timezone, and duration/end
  may be shown as an “Event mentioned” suggestion. It never claims the user
  registered or must attend, and export remains an explicit user action.
- Quoted historical dates, conflicting date clues, and missing timezone data
  produce no exportable candidate.

## Required test corpus

- `tomorrow` anchored to the delivered date
- weekday + month/day agreement, including year boundaries
- explicit numeric dates and ISO timestamps
- DST boundaries and explicit timezone abbreviations
- explicit duration versus no duration
- multiple events, quoted prior events, and conflicting clues
- registered event versus generic promotion
- `.ics` invite, update, and cancellation

## Delivery sequence

1. Add resolver unit tests and implement the free-text deterministic resolver.
2. Change the model schema to raw event phrases and persist only validated
   candidates.
3. Add MIME `text/calendar` parsing and tests.
4. Update the event card to show source and resolved details before export.
5. Reanalyze or invalidate candidates created by the old LLM-timestamp schema.
