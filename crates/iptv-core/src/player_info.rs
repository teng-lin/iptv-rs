//! What the player's info card and top bar say: the stream badges
//! (resolution class, frame rate, audio layout), the minutes left of a
//! programme and the date with the time. Pure functions of the values the
//! caller passes (no clock access here).

use crate::tracks::{audio_layout, fps_label, resolution_class};

/// What the player reports of the stream that plays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StreamFormat {
    /// Picture size (`size` event); 0 while unknown.
    pub width: u32,
    pub height: u32,
    /// Frames per second (`format` event); 0 while unknown.
    pub fps: f32,
    /// Audio channels (`format` event); 0 while unknown.
    pub channels: u32,
}

/// The card's badges, in order: resolution class, frame rate, audio
/// layout; each only once the player knows it.
pub fn badges(f: &StreamFormat) -> Vec<String> {
    let mut out = Vec::new();
    if f.width > 0 && f.height > 0 {
        out.push(resolution_class(f.width, f.height).to_owned());
    }
    out.extend(fps_label(f.fps));
    if f.channels > 0 {
        out.push(audio_layout(f.channels));
    }
    out
}

/// "25 min left" for a programme ending at guide minute `end` at `now`
/// (`None` once it ended or before it starts).
pub fn minutes_left(start: i32, end: i32, now: i32) -> Option<String> {
    (start <= now && now < end).then(|| format!("{} min left", end - now))
}

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Month (1–12) and day of a day number since 1970-01-01 (proleptic
/// Gregorian; Howard Hinnant's `civil_from_days`).
fn month_day(days: i64) -> (usize, i64) {
    let z = days + 719_468;
    let doe = z - z.div_euclid(146_097) * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (month as usize, day)
}

/// "Tue 10 Nov  18:00": local date and time of `unix` at a UTC offset of
/// `offset_minutes`.
pub fn date_time(unix: i64, offset_minutes: i32) -> String {
    let t = unix + i64::from(offset_minutes) * 60;
    let (days, minute) = (t.div_euclid(86_400), t.rem_euclid(86_400) / 60);
    let (month, day) = month_day(days);
    // 1970-01-01 was a Thursday (index 3 from Monday).
    let weekday = DAYS[(days + 3).rem_euclid(7) as usize];
    format!(
        "{weekday} {day} {}  {:02}:{:02}",
        MONTHS[month - 1],
        minute / 60,
        minute % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::SYNTHETIC_EPOCH_UNIX as EPOCH;

    #[test]
    fn badges_show_what_the_player_knows() {
        assert!(badges(&StreamFormat::default()).is_empty());
        let f = StreamFormat {
            width: 1920,
            height: 1080,
            fps: 50.0,
            channels: 2,
        };
        assert_eq!(badges(&f), ["FHD", "50 fps", "Stereo"]);
        let audio_only = StreamFormat {
            channels: 6,
            ..StreamFormat::default()
        };
        assert_eq!(badges(&audio_only), ["5.1"]);
    }

    #[test]
    fn minutes_left_while_airing() {
        assert_eq!(minutes_left(0, 30, 5).as_deref(), Some("25 min left"));
        assert_eq!(minutes_left(0, 30, 29).as_deref(), Some("1 min left"));
        assert_eq!(minutes_left(0, 30, 30), None);
        assert_eq!(minutes_left(10, 30, 5), None);
    }

    #[test]
    fn dates_read_in_the_local_offset() {
        // The synthetic epoch is Tuesday 10 November 2026, 18:00 UTC.
        assert_eq!(date_time(EPOCH, 0), "Tue 10 Nov  18:00");
        assert_eq!(date_time(EPOCH, 7 * 60), "Wed 11 Nov  01:00");
        assert_eq!(date_time(EPOCH, -5 * 60 - 30), "Tue 10 Nov  12:30");
        // Month and year boundaries.
        assert_eq!(date_time(EPOCH + 51 * 86_400, 0), "Thu 31 Dec  18:00");
        assert_eq!(date_time(EPOCH + 52 * 86_400, 0), "Fri 1 Jan  18:00");
        assert_eq!(date_time(951_782_400, 0), "Tue 29 Feb  00:00");
    }
}
