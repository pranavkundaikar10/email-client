use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use regex::Regex;

#[derive(Debug, PartialEq, Eq)]
pub struct EventHints {
    pub date_clues: Vec<String>,
    pub time: String,
    pub timezone: String,
    pub duration_minutes: i64,
}

pub fn extract_common_event_hints(body: &str) -> Option<EventHints> {
    let date = Regex::new(r"(?i)\b(?:today|tomorrow)\b")
        .ok()?
        .find(body)?
        .as_str()
        .to_string();
    let weekday = Regex::new(r"(?i)\b(?:monday|tuesday|wednesday|thursday|friday|saturday|sunday),?\s+(?:january|february|march|april|may|june|july|august|september|october|november|december)\s+\d{1,2}\b").ok()?.find(body).map(|m| m.as_str().to_string());
    let time = Regex::new(r"(?i)\b\d{1,2}:\d{2}\s*(?:am|pm)\s*(?:pst|pdt|est|edt|utc|gmt)\b")
        .ok()?
        .find(body)?
        .as_str()
        .to_string();
    let parts: Vec<_> = time.split_whitespace().collect();
    let duration = Regex::new(r"(?i)\b(\d{1,3})\s*[- ]?minute\b")
        .ok()?
        .captures(body)?
        .get(1)?
        .as_str()
        .parse()
        .ok()?;
    let mut date_clues = vec![date];
    if let Some(weekday) = weekday {
        date_clues.push(weekday);
    }
    Some(EventHints {
        date_clues,
        time: format!("{} {}", parts[0], parts[1]),
        timezone: parts[2].to_string(),
        duration_minutes: duration,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedEventTime {
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    MissingTime,
    MissingTimezone,
    MissingDuration,
    InvalidDate,
    ConflictingDates,
}

/// Human-written abbreviations are often technically inaccurate (for example,
/// senders write "PST" year-round). Treat US regional abbreviations as a
/// region, so the offset is selected for the event's actual calendar date.
/// Explicit numeric offsets remain literal.
fn regional_timezone(value: &str) -> Option<Tz> {
    match value.trim().to_ascii_uppercase().as_str() {
        "PST" | "PDT" => Some(chrono_tz::America::Los_Angeles),
        "EST" | "EDT" => Some(chrono_tz::America::New_York),
        _ => None,
    }
}

fn fixed_timezone_offset(value: &str) -> Option<FixedOffset> {
    match value.trim().to_ascii_uppercase().as_str() {
        "UTC" | "GMT" => FixedOffset::east_opt(0),
        value if value.starts_with('+') || value.starts_with('-') => {
            let sign = if value.starts_with('-') { -1 } else { 1 };
            let parts: Vec<_> = value[1..].split(':').collect();
            if parts.len() != 2 {
                return None;
            }
            let hours = parts[0].parse::<i32>().ok()?;
            let minutes = parts[1].parse::<i32>().ok()?;
            FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
        }
        _ => None,
    }
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    for format in ["%I:%M %p", "%I %p", "%H:%M"] {
        if let Ok(time) = NaiveTime::parse_from_str(&value.trim().to_ascii_uppercase(), format) {
            return Some(time);
        }
    }
    None
}

fn month(value: &str) -> Option<u32> {
    [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ]
    .iter()
    .position(|name| name.starts_with(&value.to_ascii_lowercase()[..3.min(value.len())]))
    .map(|index| index as u32 + 1)
}

fn parse_date_clue(reference: DateTime<Utc>, clue: &str) -> Option<NaiveDate> {
    let clue = clue.trim().to_ascii_lowercase().replace(',', "");
    let reference_date = reference.date_naive();
    if clue == "today" {
        return Some(reference_date);
    }
    if clue == "tomorrow" {
        return Some(reference_date + Duration::days(1));
    }
    if let Ok(date) = NaiveDate::parse_from_str(&clue, "%Y-%m-%d") {
        return Some(date);
    }

    let tokens: Vec<_> = clue.split_whitespace().collect();
    let (weekday_token, month_index) = match tokens.as_slice() {
        [weekday, _, _] => (Some(*weekday), 1),
        [_, _] => (None, 0),
        _ => return None,
    };
    let target_month = month(tokens[month_index])?;
    let day = tokens[month_index + 1].parse::<u32>().ok()?;
    let mut year = reference_date.year();
    let mut date = NaiveDate::from_ymd_opt(year, target_month, day)?;
    if date < reference_date - Duration::days(1) {
        year += 1;
        date = NaiveDate::from_ymd_opt(year, target_month, day)?;
    }
    if let Some(weekday) = weekday_token {
        let expected = match weekday {
            "monday" => Weekday::Mon,
            "tuesday" => Weekday::Tue,
            "wednesday" => Weekday::Wed,
            "thursday" => Weekday::Thu,
            "friday" => Weekday::Fri,
            "saturday" => Weekday::Sat,
            "sunday" => Weekday::Sun,
            _ => return None,
        };
        if date.weekday() != expected {
            return None;
        }
    }
    Some(date)
}

pub fn resolve_free_text_event(
    received_at: DateTime<Utc>,
    date_clues: &[&str],
    time_text: Option<&str>,
    timezone_text: Option<&str>,
    duration_minutes: Option<i64>,
) -> Result<ResolvedEventTime, ResolveError> {
    let time = time_text
        .and_then(parse_time)
        .ok_or(ResolveError::MissingTime)?;
    let timezone = timezone_text.ok_or(ResolveError::MissingTimezone)?;
    let duration = duration_minutes
        .filter(|minutes| *minutes > 0)
        .ok_or(ResolveError::MissingDuration)?;
    let dates: Vec<_> = date_clues
        .iter()
        .map(|clue| parse_date_clue(received_at, clue).ok_or(ResolveError::InvalidDate))
        .collect::<Result<_, _>>()?;
    let date = *dates.first().ok_or(ResolveError::InvalidDate)?;
    if dates.iter().any(|candidate| *candidate != date) {
        return Err(ResolveError::ConflictingDates);
    }
    let local_time = date.and_time(time);
    let start = if let Some(region) = regional_timezone(timezone) {
        region
            .from_local_datetime(&local_time)
            .single()
            .ok_or(ResolveError::InvalidDate)?
            .fixed_offset()
    } else {
        let offset = fixed_timezone_offset(timezone).ok_or(ResolveError::MissingTimezone)?;
        offset
            .from_local_datetime(&local_time)
            .single()
            .ok_or(ResolveError::InvalidDate)?
    };
    let end = start + Duration::minutes(duration);
    Ok(ResolvedEventTime {
        start_at: start.to_rfc3339(),
        end_at: end.to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_free_text_event, ResolveError};
    use chrono::{DateTime, Utc};

    fn received() -> DateTime<Utc> {
        "2026-10-02T19:01:38Z".parse().unwrap()
    }

    #[test]
    fn resolves_tomorrow_from_the_email_received_date() {
        let event = resolve_free_text_event(
            received(),
            &["tomorrow", "Saturday, October 3"],
            Some("9:00 AM"),
            Some("PST"),
            Some(45),
        )
        .unwrap();
        assert_eq!(event.start_at, "2026-10-03T09:00:00-07:00");
        assert_eq!(event.end_at, "2026-10-03T09:45:00-07:00");
    }

    #[test]
    fn treats_pst_written_in_summer_as_pacific_regional_time() {
        let event = resolve_free_text_event(
            received(),
            &["tomorrow"],
            Some("9:00 AM"),
            Some("PST"),
            Some(45),
        )
        .unwrap();
        let eastern = event
            .start_at
            .parse::<DateTime<chrono::FixedOffset>>()
            .unwrap()
            .with_timezone(&chrono_tz::America::New_York);
        assert_eq!(eastern.format("%-I:%M %p").to_string(), "12:00 PM");
        assert_eq!(eastern.offset().to_string(), "EDT");
    }

    #[test]
    fn preserves_explicit_numeric_offsets() {
        let event = resolve_free_text_event(
            received(),
            &["tomorrow"],
            Some("9:00 AM"),
            Some("-08:00"),
            Some(45),
        )
        .unwrap();
        assert_eq!(event.start_at, "2026-10-03T09:00:00-08:00");
    }

    #[test]
    fn rejects_conflicting_or_incomplete_event_details() {
        assert_eq!(
            resolve_free_text_event(
                received(),
                &["tomorrow", "Sunday, October 4"],
                Some("9:00 AM"),
                Some("PST"),
                Some(45)
            ),
            Err(ResolveError::ConflictingDates)
        );
        assert_eq!(
            resolve_free_text_event(
                received(),
                &["tomorrow"],
                Some("9:00 AM"),
                Some("PST"),
                None
            ),
            Err(ResolveError::MissingDuration)
        );
    }

    #[test]
    fn extracts_complete_reminder_hints_without_an_llm() {
        let hints = super::extract_common_event_hints(
            "Join me live tomorrow. Saturday, October 3. 9:00 AM PST. Free 45-minute session.",
        )
        .unwrap();
        assert_eq!(hints.date_clues, ["tomorrow", "Saturday, October 3"]);
        assert_eq!(hints.time, "9:00 AM");
        assert_eq!(hints.timezone, "PST");
        assert_eq!(hints.duration_minutes, 45);
    }
}
