//! When something happened, in the time zone the system is set to.

use std::time::SystemTime;

use fluent_bundle::FluentValue;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::i18n::Translator;

/// "Today 14:12", "Yesterday 23:30" or "3 days ago". Days are counted by the calendar where the
/// user is, so a run from last night is yesterday's however few hours ago it was.
pub fn when(at: SystemTime, now: SystemTime, tr: &Translator) -> String {
    let (days, time) = local(at, now, &TimeZone::system());
    tr.with(
        "session-age",
        &[
            ("days", FluentValue::from(days)),
            ("time", FluentValue::from(time)),
        ],
    )
}

fn local(at: SystemTime, now: SystemTime, zone: &TimeZone) -> (i64, String) {
    let (Ok(at), Ok(now)) = (Timestamp::try_from(at), Timestamp::try_from(now)) else {
        return (0, String::new());
    };
    let (at, now) = (at.to_zoned(zone.clone()), now.to_zoned(zone.clone()));
    // A clock set back can put `at` after `now`.
    let days = now
        .date()
        .since(at.date())
        .map_or(0, |span| i64::from(span.get_days()).max(0));
    (days, format!("{:02}:{:02}", at.hour(), at.minute()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> SystemTime {
        SystemTime::from(text.parse::<Timestamp>().unwrap())
    }

    #[test]
    fn days_are_counted_by_the_local_calendar() {
        let seoul = TimeZone::fixed(jiff::tz::offset(9));
        let now = at("2026-10-27T08:00:00+09:00");
        assert_eq!(
            local(at("2026-10-27T07:05:00+09:00"), now, &seoul),
            (0, "07:05".to_owned())
        );
        assert_eq!(
            local(at("2026-10-26T23:30:00+09:00"), now, &seoul),
            (1, "23:30".to_owned()),
            "last night is yesterday though it is under a day ago"
        );
        assert_eq!(local(at("2026-10-23T12:00:00+09:00"), now, &seoul).0, 4);
        assert_eq!(
            local(at("2026-10-27T09:00:00+09:00"), now, &seoul).0,
            0,
            "a time after now is today"
        );
    }
}
