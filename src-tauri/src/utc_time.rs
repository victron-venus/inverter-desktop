//! UTC timestamps shared by MQTT and gateway observations.
//!
//! Inputs are POSIX seconds or the system clock, never civil-time strings or
//! leap-second fractions. Preserve the existing +00:00 spelling, fractional
//! precision, millisecond floor and supported year range during the migration.
use time::{OffsetDateTime, UtcOffset};

pub(crate) fn unix_seconds_now() -> f64 {
    unix_seconds(OffsetDateTime::now_utc())
}

fn unix_seconds(datetime: OffsetDateTime) -> f64 {
    datetime.unix_timestamp_nanos().div_euclid(1_000_000) as f64 / 1000.0
}

pub(crate) fn rfc3339_now() -> String {
    format_utc(OffsetDateTime::now_utc())
}

fn from_unix_seconds(seconds: i64) -> Option<OffsetDateTime> {
    let datetime = OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    // Preserve the former calendar's bounds rather than accepting new values
    // because time's large-dates feature supports a wider range.
    (-262_143..=262_142)
        .contains(&datetime.year())
        .then_some(datetime)
}

pub(crate) fn notification_timestamp(seconds: i64) -> Option<String> {
    if seconds <= 0 {
        return None;
    }
    from_unix_seconds(seconds).map(format_utc)
}

fn format_utc(datetime: OffsetDateTime) -> String {
    let datetime = datetime.to_offset(UtcOffset::UTC);
    let (year, month, day) = datetime.to_calendar_date();
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+05}")
    };
    let (hour, minute, second, nanos) = datetime.to_hms_nano();
    let fraction = if nanos == 0 {
        String::new()
    } else if nanos.is_multiple_of(1_000_000) {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        format!(".{:06}", nanos / 1_000)
    } else {
        format!(".{nanos:09}")
    };
    format!(
        "{year}-{:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{fraction}+00:00",
        month as u8
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month};

    #[test]
    fn notification_times_keep_the_existing_utc_spelling() {
        assert_eq!(
            notification_timestamp(1_700_000_000).as_deref(),
            Some("2023-11-14T22:13:20+00:00")
        );
        assert_eq!(
            notification_timestamp(1).as_deref(),
            Some("1970-01-01T00:00:01+00:00")
        );
    }

    #[test]
    fn incomplete_nonpositive_and_overflowing_event_times_remain_invalid() {
        for seconds in [i64::MIN, -1, 0, i64::MAX] {
            assert_eq!(notification_timestamp(seconds), None);
        }
    }

    #[test]
    fn fractions_keep_zero_three_six_or_nine_digits() {
        for (nanos, suffix) in [
            (0, ""),
            (1_000_000, ".001"),
            (120_000_000, ".120"),
            (1_000, ".000001"),
            (123_400_000, ".123400"),
            (1, ".000000001"),
            (123_456_789, ".123456789"),
        ] {
            let datetime = OffsetDateTime::from_unix_timestamp_nanos(nanos).unwrap();
            assert_eq!(
                format_utc(datetime),
                format!("1970-01-01T00:00:00{suffix}+00:00")
            );
        }
    }

    #[test]
    fn formatting_normalizes_offsets_to_the_same_utc_instant() {
        let utc = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        for seconds in [-8 * 3600, 0, 5 * 3600 + 30 * 60] {
            let offset = UtcOffset::from_whole_seconds(seconds).unwrap();
            assert_eq!(format_utc(utc.to_offset(offset)), format_utc(utc));
        }
    }

    #[test]
    fn freshness_floors_to_milliseconds_including_before_epoch() {
        for (nanos, expected) in [
            (0, 0.0),
            (999_999, 0.0),
            (1_999_999, 0.001),
            (-1, -0.001),
            (-999_999, -0.001),
            (-1_000_001, -0.002),
            (1_700_000_000_123_999_999, 1_700_000_000.123),
        ] {
            let datetime = OffsetDateTime::from_unix_timestamp_nanos(nanos).unwrap();
            assert_eq!(unix_seconds(datetime), expected);
        }
    }

    #[test]
    fn signed_calendar_range_and_extended_year_strings_are_preserved() {
        let minimum = Date::from_calendar_date(-262_143, Month::January, 1)
            .unwrap()
            .midnight()
            .assume_utc();
        let maximum = Date::from_calendar_date(262_142, Month::December, 31)
            .unwrap()
            .with_hms(23, 59, 59)
            .unwrap()
            .assume_utc();
        for datetime in [minimum, maximum] {
            assert_eq!(from_unix_seconds(datetime.unix_timestamp()), Some(datetime));
        }
        assert_eq!(from_unix_seconds(minimum.unix_timestamp() - 1), None);
        assert_eq!(from_unix_seconds(maximum.unix_timestamp() + 1), None);
        assert_eq!(format_utc(minimum), "-262143-01-01T00:00:00+00:00");
        assert_eq!(
            notification_timestamp(maximum.unix_timestamp()).as_deref(),
            Some("+262142-12-31T23:59:59+00:00")
        );
        let year_ten_thousand = Date::from_calendar_date(10_000, Month::January, 1)
            .unwrap()
            .midnight()
            .assume_utc();
        assert_eq!(format_utc(year_ten_thousand), "+10000-01-01T00:00:00+00:00");
        assert_eq!(
            format_utc(from_unix_seconds(-1).unwrap()),
            "1969-12-31T23:59:59+00:00"
        );
    }

    #[test]
    fn posix_seconds_do_not_invent_a_leap_second() {
        assert_eq!(
            notification_timestamp(1_483_228_799).as_deref(),
            Some("2016-12-31T23:59:59+00:00")
        );
        assert_eq!(
            notification_timestamp(1_483_228_800).as_deref(),
            Some("2017-01-01T00:00:00+00:00")
        );
    }
}
