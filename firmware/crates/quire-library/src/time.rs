//! Calendar arithmetic without a time-zone database: the device clock is kept in local
//! time, so every timestamp here is "local seconds since 1970-01-01".

/// Seconds in a day.
pub const DAY: u32 = 86_400;

/// Days since the epoch for a local timestamp.
pub fn day_of(ts: u32) -> u16 {
    (ts / DAY).min(u16::MAX as u32) as u16
}

/// Hour of day (0–23).
pub fn hour_of(ts: u32) -> u8 {
    ((ts % DAY) / 3600) as u8
}

/// Minute of day (0–1439).
pub fn minute_of_day(ts: u32) -> u16 {
    ((ts % DAY) / 60) as u16
}

/// Weekday of a day number: 0 = Monday … 6 = Sunday.
pub fn weekday(day: u16) -> u8 {
    // 1970-01-01 was a Thursday (3 with Monday = 0).
    ((day as u32 + 3) % 7) as u8
}

/// Civil date of a day number (proleptic Gregorian).
pub fn civil(day: u16) -> (u16, u8, u8) {
    let z = day as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = if m <= 2 { y + 1 } else { y };
    (y as u16, m, d)
}

/// Day number of a civil date.
pub fn from_civil(y: u16, m: u8, d: u8) -> u16 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468).clamp(0, u16::MAX as i64) as u16
}

/// Days in a month.
pub fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
    }
}

/// Short month name.
pub fn month_name(m: u8) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m.clamp(1, 12) - 1) as usize]
}

/// Full month name.
pub fn month_name_long(m: u8) -> &'static str {
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
        [(m.clamp(1, 12) - 1) as usize]
}

/// Short weekday name for 0 = Monday.
pub fn weekday_name(wd: u8) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(wd % 7) as usize]
}

/// Full weekday name for 0 = Monday.
pub fn weekday_name_long(wd: u8) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][(wd % 7) as usize]
}

/// "1 h 42", "42 min", "0 min" — the brief's duration style.
pub fn fmt_duration(secs: u32) -> alloc::string::String {
    let m = secs.div_ceil(60);
    if m >= 60 {
        alloc::format!("{} h {:02}", m / 60, m % 60)
    } else {
        alloc::format!("{m} min")
    }
}

/// "21:04" or "9:04 pm".
pub fn fmt_clock(ts: u32, h24: bool) -> alloc::string::String {
    let m = minute_of_day(ts);
    let (h, mm) = (m / 60, m % 60);
    if h24 {
        alloc::format!("{h:02}:{mm:02}")
    } else {
        let (h12, suffix) = match h {
            0 => (12, "am"),
            1..=11 => (h, "am"),
            12 => (12, "pm"),
            _ => (h - 12, "pm"),
        };
        alloc::format!("{h12}:{mm:02} {suffix}")
    }
}

/// "2 Sep".
pub fn fmt_date(day: u16) -> alloc::string::String {
    let (_, m, d) = civil(day);
    alloc::format!("{d} {}", month_name(m))
}

/// "2 Sep 2026".
pub fn fmt_date_year(day: u16) -> alloc::string::String {
    let (y, m, d) = civil(day);
    alloc::format!("{d} {} {y}", month_name(m))
}

/// "Sunday 7 Sep".
pub fn fmt_weekday_date(day: u16) -> alloc::string::String {
    let (_, m, d) = civil(day);
    alloc::format!("{} {d} {}", weekday_name_long(weekday(day)), month_name(m))
}

/// "9 pm" / "noon" / "midnight" style hour label.
pub fn fmt_hour(h: u8) -> alloc::string::String {
    match h % 24 {
        0 => "midnight".into(),
        12 => "noon".into(),
        h if h < 12 => alloc::format!("{h} am"),
        h => alloc::format!("{} pm", h - 12),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 9, 13), (2099, 12, 31)] {
            let day = from_civil(y, m, d);
            assert_eq!(civil(day), (y, m, d));
        }
        // 2026-09-13 is a Sunday.
        assert_eq!(weekday(from_civil(2026, 9, 13)), 6);
        assert_eq!(weekday(from_civil(2026, 9, 1)), 1);
        assert_eq!(fmt_duration(6120), "1 h 42");
        assert_eq!(fmt_duration(2520), "42 min");
        assert_eq!(fmt_hour(21), "9 pm");
        let ts = from_civil(2026, 9, 7) as u32 * DAY + 21 * 3600 + 4 * 60;
        assert_eq!(fmt_clock(ts, true), "21:04");
        assert_eq!(fmt_clock(ts, false), "9:04 pm");
        assert_eq!(fmt_date(from_civil(2026, 9, 2)), "2 Sep");
        assert_eq!(fmt_weekday_date(from_civil(2026, 9, 7)), "Monday 7 Sep");
    }
}
