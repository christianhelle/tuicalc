//! Excel date serial numbers.

const SECONDS_PER_DAY: f64 = 86_400.0;
/// Days from 1970-01-01 back to serial 0 in each date system.
const EPOCH_1900: i64 = -25_569;
const EPOCH_1904: i64 = -24_107;
/// The serial of 1900-02-29, a date that only exists because of a Lotus 1-2-3 bug Excel kept.
const PHANTOM_LEAP_DAY: i64 = 60;

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn serial_to_civil(serial: i64, date1904: bool) -> (i64, i64, i64) {
    if date1904 {
        return civil_from_days(serial + EPOCH_1904);
    }
    match serial {
        PHANTOM_LEAP_DAY => (1900, 2, 29),
        // Serials before the phantom day sit one day later than the epoch implies.
        s if s < PHANTOM_LEAP_DAY => civil_from_days(s + EPOCH_1900 + 1),
        s => civil_from_days(s + EPOCH_1900),
    }
}

fn civil_to_serial(year: i64, month: i64, day: i64, date1904: bool) -> Option<i64> {
    if !date1904 && (year, month, day) == (1900, 2, 29) {
        return Some(PHANTOM_LEAP_DAY);
    }
    if !(1..=12).contains(&month) || civil_from_days(days_from_civil(year, month, day)) != (year, month, day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let serial = if date1904 {
        days - EPOCH_1904
    } else {
        let serial = days - EPOCH_1900;
        if serial <= PHANTOM_LEAP_DAY { serial - 1 } else { serial }
    };
    (serial >= i64::from(!date1904)).then_some(serial)
}

/// Renders a serial as `YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`, or `HH:MM:SS`.
pub fn format_serial(serial: f64, date1904: bool) -> String {
    let total_seconds = (serial * SECONDS_PER_DAY).round() as i64;
    let days = total_seconds.div_euclid(86_400);
    let seconds = total_seconds.rem_euclid(86_400);
    let time = format!("{:02}:{:02}:{:02}", seconds / 3_600, seconds / 60 % 60, seconds % 60);
    if days == 0 && !date1904 {
        return time;
    }
    let (year, month, day) = serial_to_civil(days, date1904);
    let date = format!("{year:04}-{month:02}-{day:02}");
    if seconds == 0 { date } else { format!("{date} {time}") }
}

fn parse_time(text: &str) -> Option<f64> {
    let mut parts = text.split(':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let seconds: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    Some(f64::from(hours * 3_600 + minutes * 60 + seconds) / SECONDS_PER_DAY)
}

fn parse_date(text: &str, date1904: bool) -> Option<f64> {
    let mut parts = text.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    civil_to_serial(year, month, day, date1904).map(|serial| serial as f64)
}

/// Parses the forms produced by [`format_serial`].
pub fn parse_serial(text: &str, date1904: bool) -> Option<f64> {
    if let Some((date, time)) = text.split_once(' ') {
        return Some(parse_date(date, date1904)? + parse_time(time)?);
    }
    if text.contains(':') {
        return parse_time(text);
    }
    parse_date(text, date1904)
}
