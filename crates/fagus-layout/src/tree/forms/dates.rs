//! The text of date and time fields (`date`, `time`, `datetime-local`, `month`, `week`) as
//! Chrome shows it: the fields of the value (or their placeholders) and the text between them,
//! in the order and with the names and placeholders of a language ([`UiLocale`]), and the picker
//! icon at the end.
//!
//! The field widths follow `DateTimeEditElement` and the icons are those of Chromium's
//! `core/html/resources/images`:
//! Copyright 2012 The Chromium Authors. Use of this source code is governed by a BSD-style
//! license that can be found in the CHROMIUM-LICENSE file.

use std::fmt::Write as _;

use super::locale::{Piece, UiLocale, pieces};

/// The largest year a date field takes, which its year field has room for
/// (`DateComponents::MaximumYear`).
const MAX_YEAR: &str = "275760";

/// A part of the text of a date or time field.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Part {
    /// A field: the text it shows, its part of the value or a placeholder, and the texts it has
    /// room for (its placeholder, its largest value, its labels).
    Field { text: String, room: Vec<String> },
    /// Text between fields.
    Text(&'static str),
}

impl Part {
    fn field(value: Option<String>, placeholder: &str, room: &[&str]) -> Self {
        let mut texts: Vec<String> = room.iter().map(|text| (*text).to_owned()).collect();
        texts.push(placeholder.to_owned());
        let text = value.unwrap_or_else(|| placeholder.to_owned());
        texts.push(text.clone());
        Self::Field { text, room: texts }
    }
}

/// The placeholder of a field of names: as many dashes as the longest name has characters.
fn dashes(names: &[&str]) -> String {
    "-".repeat(names.iter().map(|name| name.chars().count()).max().unwrap_or(0))
}

/// The parts of a field of `kind` with `value`, its `step` and `max` attributes, written as in
/// `locale`; `None` if the type is not a date or time.
pub(super) fn parts(
    kind: &str,
    value: Option<&str>,
    step: Option<&str>,
    max: Option<&str>,
    locale: &UiLocale,
) -> Option<Vec<Part>> {
    let value = value.map(str::trim);
    let max_year = max
        .and_then(|max| year_of(kind, max.trim()))
        .unwrap_or_else(|| MAX_YEAR.to_owned());
    // A time has a seconds field where its value has seconds or its step is not whole minutes.
    let step_seconds = step
        .and_then(|step| step.trim().parse::<f64>().ok())
        .is_some_and(|step| step > 0.0 && step % 60.0 != 0.0);
    Some(match kind {
        "date" => date_parts(value.and_then(parse_date), &max_year, locale),
        "time" => {
            let time = value.and_then(parse_time);
            time_parts(time, step_seconds, locale)
        }
        "datetime-local" => {
            let parsed = value.and_then(|value| {
                let (day, clock) = value.split_once(['T', ' '])?;
                Some((parse_date(day)?, parse_time(clock)?))
            });
            let mut parts = date_parts(parsed.as_ref().map(|(date, _)| date.clone()), &max_year, locale);
            parts.push(Part::Text(locale.date_time));
            parts.extend(time_parts(parsed.map(|(_, time)| time), step_seconds, locale));
            parts
        }
        "month" => {
            let parsed = value.and_then(parse_month);
            let (year, month) = parsed.map_or((None, None), |(year, month)| (Some(year), Some(month)));
            let placeholder = dashes(&locale.months);
            pieces(locale.month)
                .map(|piece| match piece {
                    Piece::Field('M') => Part::field(
                        month
                            .and_then(|month| locale.months.get(month - 1))
                            .map(|name| (*name).to_owned()),
                        &placeholder,
                        &locale.months,
                    ),
                    Piece::Field(_) => year_part(year.clone(), "----", &max_year),
                    Piece::Text(text) => Part::Text(text),
                })
                .collect()
        }
        "week" => {
            let parsed = value.and_then(parse_week);
            let (year, week) = parsed.map_or((None, None), |(year, week)| (Some(year), Some(week)));
            pieces(locale.week)
                .map(|piece| match piece {
                    Piece::Field('w') => Part::field(week.map(|week| format!("{week:02}")), "--", &["53"]),
                    Piece::Field(_) => year_part(year.clone(), "----", &max_year),
                    Piece::Text(text) => Part::Text(text),
                })
                .collect()
        }
        _ => return None,
    })
}

/// The year, month and day of a valid date string (HTML, §2.3.5.2).
fn parse_date(value: &str) -> Option<(String, usize, usize)> {
    let mut parts = value.splitn(3, '-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    let (month_number, day_number) = (number(month, 2)?, number(day, 2)?);
    (year_valid(year) && (1..=12).contains(&month_number) && (1..=31).contains(&day_number))
        .then(|| (year.trim_start_matches('0').to_owned(), month_number, day_number))
}

/// The year and month of a valid month string.
fn parse_month(value: &str) -> Option<(String, usize)> {
    let (year, month) = value.split_once('-')?;
    let month = number(month, 2)?;
    (year_valid(year) && (1..=12).contains(&month)).then(|| (year.trim_start_matches('0').to_owned(), month))
}

/// The year and week of a valid week string.
fn parse_week(value: &str) -> Option<(String, usize)> {
    let (year, week) = value.split_once("-W")?;
    let week = number(week, 2)?;
    (year_valid(year) && (1..=53).contains(&week)).then(|| (year.trim_start_matches('0').to_owned(), week))
}

/// The hours, minutes and seconds (if given and not zero) of a valid time string.
fn parse_time(value: &str) -> Option<(usize, usize, Option<usize>)> {
    let mut parts = value.splitn(3, ':');
    let (hours, minutes) = (number(parts.next()?, 2)?, number(parts.next()?, 2)?);
    let seconds = match parts.next() {
        Some(seconds) => {
            let whole = seconds.split_once('.').map_or(seconds, |(whole, _)| whole);
            Some(number(whole, 2)?)
        }
        None => None,
    };
    (hours < 24 && minutes < 60 && seconds.is_none_or(|seconds| seconds < 60)).then_some((
        hours,
        minutes,
        seconds.filter(|seconds| *seconds > 0),
    ))
}

/// A number of exactly `digits` ASCII digits.
fn number(text: &str, digits: usize) -> Option<usize> {
    (text.len() == digits && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn year_valid(year: &str) -> bool {
    year.len() >= 4
        && year.bytes().all(|byte| byte.is_ascii_digit())
        && !year.trim_start_matches('0').is_empty()
}

/// The year of a `max` attribute of a field of `kind`.
fn year_of(kind: &str, max: &str) -> Option<String> {
    match kind {
        "date" => parse_date(max).map(|(year, ..)| year),
        "datetime-local" => parse_date(max.split_once(['T', ' '])?.0).map(|(year, ..)| year),
        "month" => parse_month(max).map(|(year, _)| year),
        "week" => parse_week(max).map(|(year, _)| year),
        _ => None,
    }
}

/// A year field: `yyyy` (or its translation) its placeholder in a date, `----` in a month or
/// week.
fn year_part(year: Option<String>, placeholder: &str, max_year: &str) -> Part {
    Part::field(year, placeholder, &[max_year])
}

fn date_parts(date: Option<(String, usize, usize)>, max_year: &str, locale: &UiLocale) -> Vec<Part> {
    let (year, month, day) = date.map_or((None, None, None), |(year, month, day)| {
        (Some(year), Some(month), Some(day))
    });
    let [day_placeholder, month_placeholder, year_placeholder] = locale.placeholders;
    pieces(locale.date)
        .map(|piece| match piece {
            Piece::Field('d') => Part::field(day.map(|day| format!("{day:02}")), day_placeholder, &["31"]),
            Piece::Field('M') => Part::field(
                month.map(|month| format!("{month:02}")),
                month_placeholder,
                &["12"],
            ),
            Piece::Field(_) => year_part(year.clone(), year_placeholder, max_year),
            Piece::Text(text) => Part::Text(text),
        })
        .collect()
}

/// The parts of a time, in twelve or 24-hour notation as `locale` has it, with seconds where the
/// time has them or `seconds` asks for them.
fn time_parts(time: Option<(usize, usize, Option<usize>)>, seconds: bool, locale: &UiLocale) -> Vec<Part> {
    let seconds = seconds || time.is_some_and(|(_, _, seconds)| seconds.is_some());
    let hours = time.map(|(hours, ..)| match locale.am_pm {
        Some(_) => {
            let twelve = if hours % 12 == 0 { 12 } else { hours % 12 };
            format!("{twelve:02}")
        }
        None => format!("{hours:02}"),
    });
    let largest = if locale.am_pm.is_some() { "12" } else { "23" };
    let mut parts = vec![
        Part::field(hours, "--", &[largest]),
        Part::Text(":"),
        Part::field(time.map(|(_, minutes, _)| format!("{minutes:02}")), "--", &["59"]),
    ];
    if seconds {
        parts.push(Part::Text(":"));
        parts.push(Part::field(
            time.map(|(.., seconds)| format!("{:02}", seconds.unwrap_or(0))),
            "--",
            &["59"],
        ));
    }
    if let Some(labels) = locale.am_pm {
        parts.push(Part::Text(" "));
        parts.push(Part::field(
            time.map(|(hours, ..)| labels[usize::from(hours >= 12)].to_owned()),
            &dashes(&labels),
            &labels,
        ));
    }
    parts
}

/// The picker icon at the end of a field of `kind`, for a font size of `em`: the size of its
/// content box, its padding and its margin before it, and the SVG of the icon in its content
/// box (Chrome's `::-webkit-calendar-picker-indicator`, its icon drawn as a background image
/// sized to `contain`).
pub(super) struct Indicator {
    pub side: f32,
    pub padding: f32,
    pub margin: f32,
    pub body: String,
}

pub(super) fn indicator(kind: &str, em: f32) -> Indicator {
    if kind == "time" {
        // A clock, 14 × 14 with a view box of 24, in a box 1.05em on a side.
        let side = 1.05 * em;
        let mut body = String::new();
        let _ = write!(
            body,
            concat!(
                r#"<g transform="scale({scale})" fill="rgb(0,0,0)">"#,
                r#"<path d="M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8z"/>"#,
                r#"<path d="M12.5 7H11v6l5.25 3.15.75-1.23-4.5-2.67z"/></g>"#
            ),
            scale = side / 24.0
        );
        Indicator {
            side,
            padding: 3.0,
            margin: 8.0,
            body,
        }
    } else {
        // A calendar, 16 × 15 with a view box of 24 (so 15 × 15 in its middle), in a box 1em on
        // a side: as wide as the box, 15/16 of it high, at its top.
        let side = em;
        let height = side * 15.0 / 16.0;
        let mut body = String::new();
        let _ = write!(
            body,
            concat!(
                r#"<path transform="translate({x} 0) scale({scale})" fill="rgb(0,0,0)" "#,
                r#"d="M20 3h-1V1h-2v2H7V1H5v2H4c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 18H4V8h16v13z"/>"#
            ),
            x = (side - height) / 2.0,
            scale = height / 24.0
        );
        Indicator {
            side,
            padding: 2.0,
            margin: 0.0,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::locale::{EN_US, resolve};
    use super::*;

    fn text(parts: &[Part]) -> String {
        parts
            .iter()
            .map(|part| match part {
                Part::Field { text, .. } => text.as_str(),
                Part::Text(text) => text,
            })
            .collect()
    }

    #[test]
    fn values_are_written_in_american_english() {
        let date = parts("date", Some("2026-11-04"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&date), "11/04/2026");
        let time = parts("time", Some("14:05"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&time), "02:05 PM");
        let time = parts("time", Some("00:05:30"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&time), "12:05:30 AM");
        let month = parts("month", Some("2026-11"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&month), "November 2026");
        let both = parts("datetime-local", Some("2026-11-04T14:05"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&both), "11/04/2026, 02:05 PM");
    }

    #[test]
    fn empty_and_invalid_fields_show_placeholders() {
        let date = parts("date", Some("2026-1-4"), None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&date), "mm/dd/yyyy");
        let month = parts("month", None, None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&month), "--------- ----");
        let week = parts("week", None, None, None, &EN_US).unwrap_or_default();
        assert_eq!(text(&week), "Week --, ----");
        let time = parts("time", None, Some("1"), None, &EN_US).unwrap_or_default();
        assert_eq!(text(&time), "--:--:-- --");
        assert!(parts("text", None, None, None, &EN_US).is_none());
    }

    #[test]
    fn year_has_room_for_the_largest_year() {
        let date = parts("date", None, None, None, &EN_US).unwrap_or_default();
        let Some(Part::Field { room, .. }) = date.last() else {
            panic!("no year field");
        };
        assert!(room.iter().any(|text| text == MAX_YEAR));
        let date = parts("date", None, None, Some("2030-12-31"), &EN_US).unwrap_or_default();
        let Some(Part::Field { room, .. }) = date.last() else {
            panic!("no year field");
        };
        assert!(!room.iter().any(|text| text == MAX_YEAR));
    }

    #[test]
    fn values_are_written_in_the_language() {
        let write = |tag: &str, kind: &str, value: Option<&str>| {
            text(&parts(kind, value, None, None, resolve(Some(tag), None)).unwrap_or_default())
        };
        assert_eq!(write("de", "date", None), "tt.mm.jjjj");
        assert_eq!(write("de", "date", Some("2026-11-04")), "04.11.2026");
        assert_eq!(write("de", "time", Some("14:05")), "14:05");
        assert_eq!(write("de", "time", Some("09:05")), "09:05");
        assert_eq!(write("de", "time", None), "--:--");
        assert_eq!(write("de", "datetime-local", None), "tt.mm.jjjj, --:--");
        assert_eq!(write("de", "month", Some("2026-03")), "März 2026");
        assert_eq!(write("de", "week", Some("2026-W07")), "Woche 07, 2026");
        assert_eq!(write("en-GB", "date", Some("2026-11-04")), "04/11/2026");
        assert_eq!(write("en-GB", "time", Some("00:05:30")), "00:05:30");
        assert_eq!(
            write("fr", "datetime-local", Some("2026-11-04T14:05")),
            "04/11/2026 14:05"
        );
        assert_eq!(write("fr", "date", None), "jj/mm/aaaa");
        assert_eq!(write("es", "month", None), "---------- de ----");
        assert_eq!(write("es", "month", Some("2026-03")), "marzo de 2026");
        assert_eq!(write("it", "date", None), "gg/mm/aaaa");
        assert_eq!(write("nl", "date", Some("2026-11-04")), "04-11-2026");
        assert_eq!(write("pt", "month", None), "--------- de ----");
        assert_eq!(write("pt-PT", "week", None), "Semana --, de ----");
        assert_eq!(write("pl", "date", None), "dd.mm.rrrr");
        assert_eq!(write("pl", "month", None), "----------- ----");
        assert_eq!(write("pl", "month", Some("2026-03")), "marzec 2026");
        assert_eq!(write("pl", "week", None), "Tydzień --, ----");
    }

    #[test]
    fn regional_formats_follow_macos() {
        let write = |tag: &str, kind: &str, value: Option<&str>| {
            text(&parts(kind, value, None, None, resolve(Some(tag), None)).unwrap_or_default())
        };
        assert_eq!(write("es-419", "time", Some("14:05")), "02:05 p.m.");
        assert_eq!(write("es-MX", "time", Some("00:05:30")), "12:05:30 a.m.");
        assert_eq!(write("es-419", "time", None), "--:-- ----");
        assert_eq!(
            write("es-MX", "datetime-local", Some("2026-11-04T14:05")),
            "04/11/2026, 02:05 p.m."
        );
        assert_eq!(write("es-419", "date", None), "dd/mm/aaaa");
        assert_eq!(write("fr-CA", "date", Some("2026-11-04")), "2026-11-04");
        assert_eq!(write("fr-CA", "date", None), "aaaa-mm-jj");
        assert_eq!(
            write("fr-CA", "datetime-local", Some("2026-11-04T14:05")),
            "2026-11-04 14:05"
        );
        assert_eq!(write("en-AU", "date", Some("2026-11-04")), "04/11/2026");
        assert_eq!(write("en-AU", "time", Some("14:05")), "02:05 pm");
        assert_eq!(write("en-AU", "time", None), "--:-- --");
        assert_eq!(write("en-IN", "date", None), "dd/mm/yyyy");
        assert_eq!(write("en-IN", "time", Some("09:30")), "09:30 AM");
    }

    #[test]
    fn hours_have_room_for_their_largest_value() {
        let room = |tag: &str| {
            let time = parts("time", None, None, None, resolve(Some(tag), None)).unwrap_or_default();
            match time.first() {
                Some(Part::Field { room, .. }) => room.clone(),
                _ => Vec::new(),
            }
        };
        assert!(room("en").contains(&"12".to_owned()));
        assert!(room("de").contains(&"23".to_owned()));
    }
}
