//! Calendar arithmetic without a time-zone database or platform clock.
//!
//! Callers pass "now" explicitly. Server-local times are handled as a UTC
//! offset in minutes; resolving an IANA zone name with DST rules is the
//! host's job (see [`crate::xtream::ServerClock`]).

/// Wall-clock fields of an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallClock {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's algorithm).
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

/// Wall clock of `unix_secs` at a fixed UTC offset.
pub fn wall_clock(unix_secs: i64, offset_minutes: i32) -> WallClock {
    let local = unix_secs + i64::from(offset_minutes) * 60;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    WallClock {
        year,
        month,
        day,
        hour: secs / 3600,
        minute: secs / 60 % 60,
        second: secs % 60,
    }
}

/// Parses `YYYY-MM-DD HH:MM[:SS]` (or with `T`) as a naive wall clock and
/// returns it as Unix seconds assuming the clock is at `offset_minutes`.
pub fn parse_local_datetime(text: &str, offset_minutes: i32) -> Option<i64> {
    let t = text.trim();
    let b = t.as_bytes();
    if b.len() < 16
        || b[4] != b'-'
        || b[7] != b'-'
        || !(b[10] == b' ' || b[10] == b'T')
        || b[13] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| t.get(r)?.parse::<u32>().ok();
    let (year, month, day) = (num(0..4)? as i32, num(5..7)?, num(8..10)?);
    let (hour, minute) = (num(11..13)?, num(14..16)?);
    let second = if b.len() >= 19 && b[16] == b':' {
        num(17..19)?
    } else {
        0
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let naive =
        days_from_civil(year, month, day) * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
    Some(naive - i64::from(offset_minutes) * 60)
}

/// Xtream timeshift duration: `max(1, round((end − start) / 60))` minutes.
pub(crate) fn catchup_duration_minutes(start: i64, end: i64) -> u32 {
    let minutes = ((end - start) as f64 / 60.0).round();
    minutes.clamp(1.0, f64::from(u32::MAX)) as u32
}

/// Xtream timeshift `{start}`: `Y-m-d:H-M` in the panel's clock.
pub(crate) fn format_catchup_start(unix_secs: i64, offset_minutes: i32) -> String {
    let w = wall_clock(unix_secs, offset_minutes);
    format!(
        "{:04}-{:02}-{:02}:{:02}-{:02}",
        w.year, w.month, w.day, w.hour, w.minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        let w = wall_clock(1_700_000_000, 0);
        assert_eq!(
            (w.year, w.month, w.day, w.hour, w.minute, w.second),
            (2023, 11, 14, 22, 13, 20)
        );
        let w = wall_clock(1_700_000_000, 120);
        assert_eq!((w.day, w.hour), (15, 0));
        let w = wall_clock(0, -30);
        assert_eq!(
            (w.year, w.month, w.day, w.hour, w.minute),
            (1969, 12, 31, 23, 30)
        );
    }

    #[test]
    fn parses_local_datetimes() {
        assert_eq!(
            parse_local_datetime("2023-11-14 22:13:20", 0),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_local_datetime("2023-11-15T00:13:20", 120),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_local_datetime("2023-11-14 22:13", 0),
            Some(1_699_999_980)
        );
        assert_eq!(parse_local_datetime("2023-13-14 22:13", 0), None);
        assert_eq!(parse_local_datetime("garbage", 0), None);
        assert_eq!(parse_local_datetime("", 0), None);
    }
}
