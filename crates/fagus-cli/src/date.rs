//! Creation dates as the command line and the worker take them.

/// The date of `text` in UTC: six numbers (year, month, day, hour, minute, second), such as
/// `2026-11-13T12:00:00Z`, optionally with a fraction of a second, which is dropped, and an offset
/// from UTC (`Z`, `+02:00`, `-0530`), which is applied. A date without an offset is taken as UTC.
pub(crate) fn parse(text: &str) -> Option<fagus::Date> {
    let text = text.trim();
    // The six numbers, separated by anything but digits, and where they end.
    let mut numbers = [0_u32; 6];
    let mut end = 0;
    for number in &mut numbers {
        let start = end + text[end..].find(|character: char| character.is_ascii_digit())?;
        let length = text[start..]
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(text.len() - start);
        *number = text[start..start + length].parse().ok()?;
        end = start + length;
    }
    let [year, month, day, hour, minute, second] = numbers;
    let valid =
        (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 61;
    if !valid {
        return None;
    }
    let date = fagus::Date::new(
        u16::try_from(year).ok()?,
        month as u8,
        day as u8,
        hour as u8,
        minute as u8,
        second as u8,
    );
    match offset(&text[end..])? {
        0 => Some(date),
        minutes => {
            let local = days_from_civil(i64::from(year), month, day) * 86_400
                + i64::from(hour * 3600 + minute * 60 + second);
            let utc = u64::try_from(local - i64::from(minutes) * 60).ok()?;
            Some(fagus::Date::from_unix(utc))
        }
    }
}

/// The offset from UTC in minutes of what follows the seconds of a date: nothing or `Z` (0),
/// `+02:00`, `+0200` or `+02`, after a fraction of a second (`.250`); `None` for anything else.
fn offset(rest: &str) -> Option<i32> {
    let rest = rest
        .strip_prefix(['.', ','])
        .map_or(rest, |fraction| {
            fraction.trim_start_matches(|character: char| character.is_ascii_digit())
        })
        .trim_start();
    if rest.is_empty() || rest.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = match rest.chars().next()? {
        '+' => 1,
        '-' | '\u{2212}' => -1,
        _ => return None,
    };
    let digits: String = rest[rest.chars().next()?.len_utf8()..]
        .chars()
        .filter(|character| *character != ':')
        .collect();
    if !digits.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes) = match digits.len() {
        2 => (digits.parse::<i32>().ok()?, 0),
        4 => (digits[..2].parse::<i32>().ok()?, digits[2..].parse::<i32>().ok()?),
        _ => return None,
    };
    (hours < 24 && minutes < 60).then_some(sign * (hours * 60 + minutes))
}

/// The days of a date of the civil calendar since 1970-01-01 (H. Hinnant, `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn fields(text: &str) -> Option<(u16, u8, u8, u8, u8, u8)> {
        parse(text).map(|date| {
            (
                date.year,
                date.month,
                date.day,
                date.hour,
                date.minute,
                date.second,
            )
        })
    }

    #[test]
    fn applies_offsets_from_utc() {
        assert_eq!(fields("2026-11-13T12:00:00Z"), Some((2026, 11, 13, 12, 0, 0)));
        assert_eq!(fields("2026-11-13T12:00:00"), Some((2026, 11, 13, 12, 0, 0)));
        assert_eq!(
            fields("2026-11-13T12:00:00+02:00"),
            Some((2026, 11, 13, 10, 0, 0))
        );
        assert_eq!(
            fields("2026-11-13T12:00:00.250+0200"),
            Some((2026, 11, 13, 10, 0, 0))
        );
        assert_eq!(
            fields("2026-11-13T12:00:00+00:00"),
            Some((2026, 11, 13, 12, 0, 0))
        );
        // Across days, months and years.
        assert_eq!(
            fields("2026-11-13T01:30:00+05:30"),
            Some((2026, 11, 12, 20, 0, 0))
        );
        assert_eq!(fields("2026-12-31T22:00:00-03"), Some((2027, 1, 1, 1, 0, 0)));
        assert_eq!(
            fields("2028-03-01T00:15:00+01:00"),
            Some((2028, 2, 29, 23, 15, 0))
        );
        // What is no offset makes the date invalid rather than being ignored.
        assert_eq!(fields("2026-11-13T12:00:00 Berlin"), None);
        assert_eq!(fields("2026-11-13T12:00:00+2:00"), None);
        assert_eq!(fields("2026-11-13T12:00:00+25:00"), None);
        assert_eq!(fields("2026-11-13"), None);
    }
}
