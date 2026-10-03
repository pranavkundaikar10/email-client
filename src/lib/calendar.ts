export interface CalendarEventDraft {
  title: string;
  start_at: string;
  end_at: string;
  location: string;
  description: string;
}

export function googleCalendarUrl(event: CalendarEventDraft) {
  const format = (value: string) => new Date(value).toISOString().replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z");
  const params = new URLSearchParams({
    action: "TEMPLATE", text: event.title, dates: `${format(event.start_at)}/${format(event.end_at)}`,
    details: event.description, location: event.location,
  });
  return `https://calendar.google.com/calendar/render?${params}`;
}

export function calendarIcs(event: CalendarEventDraft) {
  const escape = (value: string) => value.replace(/\\/g, "\\\\").replace(/\n/g, "\\n").replace(/,/g, "\\,").replace(/;/g, "\\;");
  const format = (value: string) => new Date(value).toISOString().replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z");
  return ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Productive Email//EN", "BEGIN:VEVENT", `UID:${crypto.randomUUID()}@productive-email`, `DTSTAMP:${format(new Date().toISOString())}`, `DTSTART:${format(event.start_at)}`, `DTEND:${format(event.end_at)}`, `SUMMARY:${escape(event.title)}`, `LOCATION:${escape(event.location)}`, `DESCRIPTION:${escape(event.description)}`, "END:VEVENT", "END:VCALENDAR", ""].join("\r\n");
}
