//! Wall-clock labels for guide times. The UTC offset comes from the
//! platform per instant (`offset_minutes`), so labels across a daylight
//! saving change stay right; the core never reads a clock or a time zone.

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Local day number (days since 1970-01-01 in local time) and minute of day.
fn local(unix: i64, offset_minutes: i32) -> (i64, i64) {
    let t = unix + i64::from(offset_minutes) * 60;
    (t.div_euclid(86_400), t.rem_euclid(86_400) / 60)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Day of the month (1-based) and month (0-based) of a day number
/// (proleptic Gregorian).
fn month_day(days: i64) -> (i64, usize) {
    let z = days + 719_468;
    let doe = z - z.div_euclid(146_097) * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 2 } else { mp - 10 };
    (doy - (153 * mp + 2) / 5 + 1, month as usize)
}

fn day_of_month(days: i64) -> i64 {
    month_day(days).0
}

fn weekday(days: i64) -> &'static str {
    // 1970-01-01 was a Thursday (index 3 from Monday).
    DAYS[(days + 3).rem_euclid(7) as usize]
}

/// The local date of `unix`: weekday, day and month (`Tue 10 Nov`).
pub fn date(unix: i64, offset_minutes: i32) -> String {
    let (day, _) = local(unix, offset_minutes);
    let (dom, month) = month_day(day);
    format!("{} {dom} {}", weekday(day), MONTHS[month])
}

/// `HH:MM` in local time for `unix`; on another local day than `now_unix`,
/// prefixed with the weekday and day of the month (`Tue 10 · 06:30`), as
/// the guide shows times while browsing the past or the future.
pub fn label(unix: i64, now_unix: i64, offset_minutes: impl Fn(i64) -> i32) -> String {
    let (day, minute) = local(unix, offset_minutes(unix));
    let (today, _) = local(now_unix, offset_minutes(now_unix));
    let time = format!("{:02}:{:02}", minute / 60, minute % 60);
    if day == today {
        time
    } else {
        format!("{} {} · {time}", weekday(day), day_of_month(day))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::SYNTHETIC_EPOCH_UNIX as EPOCH;

    #[test]
    fn today_shows_the_time_in_the_given_offset() {
        assert_eq!(label(EPOCH, EPOCH, |_| 0), "18:00");
        assert_eq!(label(EPOCH + 90 * 60, EPOCH, |_| 0), "19:30");
        assert_eq!(label(EPOCH, EPOCH, |_| -5 * 60), "13:00");
        assert_eq!(label(EPOCH, EPOCH, |_| 5 * 60 + 45), "23:45");
    }

    #[test]
    fn other_days_carry_their_weekday_and_date() {
        // The epoch is Tuesday 10 November 18:00 UTC; six hours later is Wednesday.
        assert_eq!(label(EPOCH + 6 * 3600, EPOCH, |_| 0), "Wed 11 · 00:00");
        assert_eq!(label(EPOCH - 24 * 3600, EPOCH, |_| 0), "Mon 9 · 18:00");
        // At UTC+7 the epoch is already Wednesday 01:00: today is local.
        assert_eq!(label(EPOCH, EPOCH, |_| 7 * 60), "01:00");
        assert_eq!(label(EPOCH - 2 * 3600, EPOCH, |_| 7 * 60), "Tue 10 · 23:00");
    }

    #[test]
    fn dates_name_the_weekday_day_and_month() {
        assert_eq!(date(EPOCH, 0), "Tue 10 Nov");
        assert_eq!(date(EPOCH, 7 * 60), "Wed 11 Nov");
        assert_eq!(date(0, 0), "Thu 1 Jan");
        assert_eq!(date(951_782_400, 0), "Tue 29 Feb", "2000-02-29");
        assert_eq!(date(EPOCH + 51 * 86_400, 0), "Thu 31 Dec");
    }

    #[test]
    fn the_offset_is_taken_per_instant() {
        // A daylight saving change between the two instants.
        let offset = |t: i64| if t < EPOCH + 3600 { 60 } else { 0 };
        assert_eq!(label(EPOCH, EPOCH, offset), "19:00");
        assert_eq!(label(EPOCH + 2 * 3600, EPOCH, offset), "20:00");
    }
}
