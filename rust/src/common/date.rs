//! Dates as PDF writes them (PDF 1.6 Reference, 3.8.3: `D:YYYYMMDDHHmmSSOHH'mm'`) and as XMP does (a subset of
//! ISO 8601: `YYYY-MM-DDThh:mm:ss.sTZD`), parsed strictly and compared as instants.

use std::cmp::Ordering;

/// A date and time. Parts a writer left out take their earliest value (month 1, day 1, 00:00:00), as both formats say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Date {
    year: i64,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    /// Minutes east of UTC; `None` when the writer gave no zone.
    zone: Option<i32>,
}

impl Date {
    /// Seconds since 1970-01-01, in UTC when the zone is known.
    fn seconds(&self) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        let local = days * 86_400 + i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second);
        local - i64::from(self.zone.unwrap_or(0)) * 60
    }

    /// Whether two dates name the same second: in UTC when both have a zone, as written when neither has, and never
    /// when only one has, since that one's instant cannot be placed.
    pub(crate) fn same_instant(&self, other: &Self) -> bool {
        match (self.zone, other.zone) {
            (Some(_), Some(_)) => self.seconds().cmp(&other.seconds()) == Ordering::Equal,
            (None, None) => self == other,
            _ => false,
        }
    }

    fn checked(self) -> Option<Self> {
        let valid = (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60;
        valid.then_some(self)
    }
}

/// Days from 1970-01-01 to a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: u8) -> u8 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Reads fixed-width decimal fields from the front of a string.
struct Fields<'a>(&'a [u8]);

impl Fields<'_> {
    /// The next `width` bytes (at most 4) as a number, when they are all digits.
    fn number(&mut self, width: usize) -> Option<u16> {
        let (head, rest) = self.0.split_at_checked(width)?;
        if !head.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.0 = rest;
        Some(head.iter().fold(0, |n, d| n * 10 + u16::from(d - b'0')))
    }

    /// The next two digits.
    fn two(&mut self) -> Option<u8> {
        // Two digits are at most 99.
        self.number(2).map(|n| u8::try_from(n).unwrap_or(u8::MAX))
    }

    /// The next two-digit field, or `default` when the string has ended or a zone begins; `None` on anything else.
    fn optional(&mut self, default: u8) -> Option<u8> {
        if matches!(self.0.first(), None | Some(b'Z' | b'+' | b'-')) {
            return Some(default);
        }
        self.two()
    }

    /// Whether the next byte is `b`, consuming it when it is.
    fn eat(&mut self, b: u8) -> bool {
        let next = self.0.first() == Some(&b);
        if next {
            self.0 = self.0.get(1..).unwrap_or_default();
        }
        next
    }

    /// `+` as 1, `-` as -1, consumed; `None` for anything else.
    fn sign(&mut self) -> Option<i32> {
        if self.eat(b'+') {
            Some(1)
        } else if self.eat(b'-') {
            Some(-1)
        } else {
            None
        }
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

/// A zone of `hours` and `minutes`, in minutes east of UTC, when both are in range.
fn zone(sign: i32, hours: u8, minutes: u8) -> Option<i32> {
    (hours < 24 && minutes < 60).then_some(sign * (i32::from(hours) * 60 + i32::from(minutes)))
}

/// A PDF date: `D:` (optional in PDF 1.6), then YYYY and optional MM, DD, HH, mm, SS, and a zone `Z`, `+HH'mm'` or
/// `-HH'mm'` whose minutes and apostrophes may be left out.
pub(crate) fn parse_pdf(text: &str) -> Option<Date> {
    let mut f = Fields(text.strip_prefix("D:").unwrap_or(text).as_bytes());
    let year = i64::from(f.number(4)?);
    let mut fields = [1, 1, 0, 0, 0];
    for field in &mut fields {
        *field = f.optional(*field)?;
    }
    let [month, day, hour, minute, second] = fields;
    let zone = if f.done() {
        None
    } else if f.eat(b'Z') {
        // Some writers add 00'00' after Z.
        (f.done() || f.0 == b"00'00'" || f.0 == b"00'00").then_some(Some(0))?
    } else {
        let sign = f.sign()?;
        let hours = f.two()?;
        let minutes = if f.eat(b'\'') && !f.done() { f.two()? } else { 0 };
        f.eat(b'\'');
        f.done().then_some(())?;
        Some(zone(sign, hours, minutes)?)
    };
    Date {
        year,
        month,
        day,
        hour,
        minute,
        second,
        zone,
    }
    .checked()
}

/// An XMP date: `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, or that with `Thh:mm`, optional `:ss` and fraction, and an optional
/// zone `Z` or `±hh:mm`. Fractions of a second are dropped, since PDF dates cannot carry them.
pub(crate) fn parse_xmp(text: &str) -> Option<Date> {
    let mut f = Fields(text.as_bytes());
    let year = i64::from(f.number(4)?);
    let mut date = Date {
        year,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        zone: None,
    };
    if f.eat(b'-') {
        date.month = f.two()?;
        if f.eat(b'-') {
            date.day = f.two()?;
            if f.eat(b'T') {
                date.hour = f.two()?;
                f.eat(b':').then_some(())?;
                date.minute = f.two()?;
                if f.eat(b':') {
                    date.second = f.two()?;
                    if f.eat(b'.') {
                        let digits = f.0.iter().take_while(|b| b.is_ascii_digit()).count();
                        f.0 = f.0.get(digits..).filter(|_| digits > 0)?;
                    }
                }
                date.zone = if f.done() { None } else { Some(xmp_zone(&mut f)?) };
            }
        }
    }
    f.done().then_some(())?;
    date.checked()
}

/// `Z`, `+hh:mm` or `-hh:mm`, in minutes east of UTC; `None` when it is malformed.
fn xmp_zone(f: &mut Fields<'_>) -> Option<i32> {
    if f.eat(b'Z') {
        return Some(0);
    }
    let sign = f.sign()?;
    let hours = f.two()?;
    f.eat(b':').then_some(())?;
    zone(sign, hours, f.two()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(pdf: &str, xmp: &str) -> bool {
        parse_pdf(pdf).unwrap().same_instant(&parse_xmp(xmp).unwrap())
    }

    #[test]
    fn pdf_dates() {
        let full = parse_pdf("D:20261125120000+01'30'").unwrap();
        assert_eq!(full.zone, Some(90));
        assert_eq!(parse_pdf("D:20261125120000-05'00").unwrap().zone, Some(-300));
        assert_eq!(parse_pdf("20261125120000+01").unwrap().zone, Some(60));
        assert_eq!(parse_pdf("D:20261125120000Z").unwrap().zone, Some(0));
        assert_eq!(parse_pdf("D:20261125120000Z00'00'").unwrap().zone, Some(0));
        assert_eq!(
            parse_pdf("D:2026").unwrap(),
            Date {
                year: 2026,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                zone: None
            }
        );
        for bad in [
            "D:202",
            "D:2026x",
            "D:20261325",
            "D:20260230",
            "D:2026112524",
            "D:20261125126000",
            "D:20261125120060",
            "D:20261125120000Z01'00'",
            "D:20261125120000*01'00'",
            "D:20261125120000+24'00'",
            "D:20261125120000+01'60'",
            "D:20261125120000+01'00'x",
            "D:20261125120000+0",
            "D:99999999999999999999",
            "D:20261125120000+01'x",
            "D:202611x",
            "D:20261125x",
            "D:2026112512x",
            "D:202611251200x",
        ] {
            assert_eq!(parse_pdf(bad), None, "{bad}");
        }
        assert!(
            parse_pdf("D:20240229").is_some() && parse_pdf("D:21000229").is_none() && parse_pdf("D:20000229").is_some()
        );
        assert!(parse_pdf("D:20230229").is_none() && parse_pdf("D:20260431").is_none());
    }

    #[test]
    fn xmp_dates() {
        for good in [
            "2026",
            "2026-11",
            "2026-11-25",
            "2026-11-25T12:00",
            "2026-11-25T12:00Z",
            "2026-11-25T12:00:30.25+01:00",
            "2026-11-25T07:00-05:00",
        ] {
            assert!(parse_xmp(good).is_some(), "{good}");
        }
        for bad in [
            "26",
            "2026-1",
            "2026-11-25T12",
            "2026-11-25T12:00:",
            "2026-11-25T12:00:00.",
            "2026-11-25T12:00:00+0100",
            "2026-11-25T12:00:00+24:00",
            "2026-11-25T12:00:00+01:60",
            "2026-11-25T12:00:00*01:00",
            "2026-11-25 ",
            "2026-11-25T12:00:00Zx",
            "2026-13",
            "2026-11-25T12:60",
            "2026-11-25T12:00:00+1:00",
            "2026-11-25T12:00:00+01:0",
            "2026-11-25T1a:00",
            "2026-11-25T12:0a",
            "2026-11-25T12:00:0a",
            "2026-11-3a",
            "2026-1a",
        ] {
            assert_eq!(parse_xmp(bad), None, "{bad}");
        }
    }

    #[test]
    fn instants_compare_in_utc_when_both_have_zones() {
        assert!(same("D:20261125120000Z", "2026-11-25T12:00:00Z"));
        assert!(same("D:20261125130000+01'00'", "2026-11-25T12:00:00Z"));
        assert!(same("D:20260101003000+01'00'", "2025-12-31T23:30:00Z"));
        assert!(same("D:20261125120000", "2026-11-25T12:00:00.9"));
        assert!(!same("D:20261125120000", "2026-11-25T12:00:00Z"));
        assert!(!same("D:20261125120001Z", "2026-11-25T12:00:00Z"));
        assert!(same("D:19000301", "1900-03-01") && same("D:16000101Z", "1600-01-01T00:00Z"));
    }
}
