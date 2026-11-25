//! Tier 1: bytes to tokens (ISO 32000-2, 7.2). Private to tier 2, which drives it.
//!
//! A token borrows its bytes from the buffer and knows where it is. A defect that still leaves a token, such as an odd
//! number of hex digits, travels with the token as a [`Defect`]; one that does not, such as a string with no end, is a
//! [`Damage`], and the caller stops reading there.

/// What a token is. Bytes are the token's own slice of the buffer, with the delimiters removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind<'a> {
    /// An integer, and the text it was written as.
    Integer(i64),
    /// A real, read as f64; its text is the token's slice, which the writer copies.
    Real(f64),
    /// A name's bytes after the `/`, still escaped; [`name_bytes`] decodes them.
    Name(&'a [u8]),
    /// A literal string's bytes between the parentheses, still escaped; [`literal_bytes`] decodes them.
    Literal(&'a [u8]),
    /// A hex string's bytes between `<` and `>`; [`hex_bytes`] decodes them.
    Hex(&'a [u8]),
    /// `[`
    ArrayOpen,
    /// `]`
    ArrayClose,
    /// `<<`
    DictOpen,
    /// `>>`
    DictClose,
    /// `{`, only in PostScript calculator functions.
    BraceOpen,
    /// `}`
    BraceClose,
    /// Any other run of regular characters: `true`, `obj`, `R`, an operator.
    Keyword(&'a [u8]),
}

/// One token and where it sits in the buffer: `start..end` covers it whole, delimiters included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Token<'a> {
    pub(crate) kind: Kind<'a>,
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// A defect the token still survived; at most one is reported per token, the first.
    pub(crate) defect: Option<Defect>,
}

/// A byte-level defect inside a token that is still readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Defect {
    /// A hex string with an odd number of digits; ISO 32000 pads it with 0, PDF/A forbids it.
    OddHexDigits,
    /// An octal escape above 255 in a literal string; the high bit is dropped (7.3.4.2).
    OctalOverflow,
    /// A `#` in a name not followed by two hex digits, or `#00` (7.3.5).
    BadNameEscape,
}

/// Damage: the bytes at `at` cannot be read as a token, so nothing after them in this scope can be either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Damage {
    pub(crate) at: usize,
    pub(crate) reason: Reason,
}

/// Why reading stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A literal string whose parentheses never balance.
    UnterminatedLiteral,
    /// A hex string with no `>`.
    UnterminatedHex,
    /// A character in a hex string that is neither a hex digit nor white space.
    BadHexDigit,
    /// A `)` or a lone `>` where a token should start.
    UnexpectedDelimiter,
    /// An integer outside 64 bits; PDF's own limit is far smaller and is a check, not damage.
    IntegerOverflow,
}

pub(crate) const fn is_white(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

const fn is_delimiter(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

const fn is_regular(b: u8) -> bool {
    !is_white(b) && !is_delimiter(b)
}

/// The next token at or after `pos`, skipping white space and comments, or `None` at the end of the buffer.
pub(crate) fn next(buf: &[u8], mut pos: usize) -> Result<Option<Token<'_>>, Damage> {
    loop {
        match buf.get(pos) {
            None => return Ok(None),
            Some(b) if is_white(*b) => pos += 1,
            Some(b'%') => {
                pos = buf
                    .iter()
                    .skip(pos)
                    .position(|b| matches!(b, 10 | 13))
                    .map_or(buf.len(), |n| pos + n);
            }
            Some(_) => return token(buf, pos).map(Some),
        }
    }
}

fn token(buf: &[u8], start: usize) -> Result<Token<'_>, Damage> {
    let at = |kind, end| Token {
        kind,
        start,
        end,
        defect: None,
    };
    let rest = buf.get(start..).unwrap_or_default();
    match rest {
        [b'<', b'<', ..] => Ok(at(Kind::DictOpen, start + 2)),
        [b'>', b'>', ..] => Ok(at(Kind::DictClose, start + 2)),
        [b'<', ..] => hex(buf, start),
        [b'(', ..] => literal(buf, start),
        [b'[', ..] => Ok(at(Kind::ArrayOpen, start + 1)),
        [b']', ..] => Ok(at(Kind::ArrayClose, start + 1)),
        [b'{', ..] => Ok(at(Kind::BraceOpen, start + 1)),
        [b'}', ..] => Ok(at(Kind::BraceClose, start + 1)),
        [b'/', ..] => Ok(name(buf, start)),
        [b')' | b'>', ..] => Err(Damage {
            at: start,
            reason: Reason::UnexpectedDelimiter,
        }),
        _ => regular(buf, start),
    }
}

/// The end of the run of regular characters starting at `from`.
fn run_end(buf: &[u8], from: usize) -> usize {
    buf.iter()
        .skip(from)
        .position(|b| !is_regular(*b))
        .map_or(buf.len(), |n| from + n)
}

fn name(buf: &[u8], start: usize) -> Token<'_> {
    let end = run_end(buf, start + 1);
    let raw = buf.get(start + 1..end).unwrap_or_default();
    let defect = (!name_escapes_ok(raw)).then_some(Defect::BadNameEscape);
    Token {
        kind: Kind::Name(raw),
        start,
        end,
        defect,
    }
}

/// Whether every `#` in a raw name is followed by two hex digits that are not `00`.
fn name_escapes_ok(raw: &[u8]) -> bool {
    let mut i = 0;
    while let Some(off) = raw.iter().skip(i).position(|&b| b == b'#') {
        let at = i + off;
        match (
            raw.get(at + 1).copied().and_then(hex_value),
            raw.get(at + 2).copied().and_then(hex_value),
        ) {
            (Some(hi), Some(lo)) if hi | lo != 0 => i = at + 3,
            _ => return false,
        }
    }
    true
}

/// A name's bytes with `#xx` escapes decoded. A bad escape is kept as written.
pub(crate) fn name_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        let escaped = (b == b'#')
            .then(|| Some(hex_value(*raw.get(i + 1)?)? << 4 | hex_value(*raw.get(i + 2)?)?))
            .flatten()
            .filter(|&v| v != 0);
        if let Some(v) = escaped {
            out.push(v);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

fn regular(buf: &[u8], start: usize) -> Result<Token<'_>, Damage> {
    let end = run_end(buf, start);
    let text = buf.get(start..end).unwrap_or_default();
    let kind = match number(text) {
        Number::Integer(i) => Kind::Integer(i),
        Number::Real(r) => Kind::Real(r),
        Number::Overflow => {
            return Err(Damage {
                at: start,
                reason: Reason::IntegerOverflow,
            });
        }
        Number::Word => Kind::Keyword(text),
    };
    Ok(Token {
        kind,
        start,
        end,
        defect: None,
    })
}

enum Number {
    Integer(i64),
    Real(f64),
    Overflow,
    /// Not a number: a keyword.
    Word,
}

/// A number as 7.3.3 writes them: an optional sign, digits, at most one point, no exponent.
fn number(text: &[u8]) -> Number {
    let body = text
        .strip_prefix(b"+")
        .or_else(|| text.strip_prefix(b"-"))
        .unwrap_or(text);
    let point = body.iter().position(|&b| b == b'.');
    let one_point = point == body.iter().rposition(|&b| b == b'.');
    let digits_only = body.iter().all(|b| b.is_ascii_digit() || *b == b'.');
    if !one_point || !digits_only || body.len() == usize::from(point.is_some()) {
        return Number::Word;
    }
    // The text is ASCII digits, a sign and a point, so it is UTF-8 and Rust's parsers accept every form left: a
    // real always parses, and an integer fails only by overflowing.
    let text = std::str::from_utf8(text).unwrap_or_default();
    if point.is_some() {
        return text.parse::<f64>().map_or(Number::Word, Number::Real);
    }
    text.parse::<i64>().map_or(Number::Overflow, Number::Integer)
}

fn hex(buf: &[u8], start: usize) -> Result<Token<'_>, Damage> {
    let close = buf
        .iter()
        .skip(start + 1)
        .position(|&b| b == b'>')
        .map(|n| start + 1 + n);
    let close = close.ok_or(Damage {
        at: start,
        reason: Reason::UnterminatedHex,
    })?;
    let raw = buf.get(start + 1..close).unwrap_or_default();
    if let Some(bad) = raw.iter().position(|&b| hex_value(b).is_none() && !is_white(b)) {
        return Err(Damage {
            at: start + 1 + bad,
            reason: Reason::BadHexDigit,
        });
    }
    let odd = raw.iter().filter(|&&b| !is_white(b)).count() % 2 == 1;
    Ok(Token {
        kind: Kind::Hex(raw),
        start,
        end: close + 1,
        defect: odd.then_some(Defect::OddHexDigits),
    })
}

/// A hex string's bytes; an odd last digit is padded with 0, as 7.3.4.3 says.
pub(crate) fn hex_bytes(raw: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = raw.iter().filter_map(|&b| hex_value(b)).collect();
    digits
        .chunks(2)
        .map(|pair| pair.iter().chain([&0]).take(2).fold(0, |acc, d| acc << 4 | d))
        .collect()
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn literal(buf: &[u8], start: usize) -> Result<Token<'_>, Damage> {
    let mut depth = 0_usize;
    let mut i = start;
    let mut defect = None;
    while let Some(&b) = buf.get(i) {
        match b {
            b'\\' => {
                let octal = buf
                    .iter()
                    .skip(i + 1)
                    .take(3)
                    .take_while(|b| (b'0'..=b'7').contains(*b));
                let value = octal.clone().fold(0_u32, |acc, d| acc * 8 + u32::from(d - b'0'));
                if value > 255 {
                    defect = defect.or(Some(Defect::OctalOverflow));
                }
                i += 1 + octal.count().max(1);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let raw = buf.get(start + 1..i).unwrap_or_default();
                    return Ok(Token {
                        kind: Kind::Literal(raw),
                        start,
                        end: i + 1,
                        defect,
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Damage {
        at: start,
        reason: Reason::UnterminatedLiteral,
    })
}

/// A literal string's bytes with its escapes and line ends decoded as 7.3.4.2 says.
pub(crate) fn literal_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        i += 1;
        match b {
            b'\\' => {
                let Some(&e) = raw.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'0'..=b'7' => {
                        let mut value = u32::from(e - b'0');
                        for _ in 0..2 {
                            match raw.get(i) {
                                Some(&d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value.to_le_bytes()[0]);
                    }
                    // A backslash before an end of line joins the lines.
                    b'\r' => i += usize::from(raw.get(i) == Some(&b'\n')),
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            // A bare end of line, of any of the three kinds, reads as one line feed.
            b'\r' => {
                i += usize::from(raw.get(i) == Some(&b'\n'));
                out.push(b'\n');
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn kinds(buf: &[u8]) -> Vec<Kind<'_>> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(t) = next(buf, pos).unwrap() {
            out.push(t.kind);
            pos = t.end;
        }
        out
    }

    fn damage(buf: &[u8]) -> Damage {
        let mut pos = 0;
        while let Ok(Some(t)) = next(buf, pos) {
            pos = t.end;
        }
        next(buf, pos).unwrap_err()
    }

    #[test]
    fn every_kind_of_token() {
        assert_eq!(
            kinds(b"<< /Type /Page >> [1 -2 +3 4. -.5 0.0] (a(b)c) <414> { } true R % comment\n T*"),
            [
                Kind::DictOpen,
                Kind::Name(b"Type"),
                Kind::Name(b"Page"),
                Kind::DictClose,
                Kind::ArrayOpen,
                Kind::Integer(1),
                Kind::Integer(-2),
                Kind::Integer(3),
                Kind::Real(4.0),
                Kind::Real(-0.5),
                Kind::Real(0.0),
                Kind::ArrayClose,
                Kind::Literal(b"a(b)c"),
                Kind::Hex(b"414"),
                Kind::BraceOpen,
                Kind::BraceClose,
                Kind::Keyword(b"true"),
                Kind::Keyword(b"R"),
                Kind::Keyword(b"T*"),
            ]
        );
    }

    #[test]
    fn tokens_know_where_they_are() {
        let t = next(b"  /A#20B ", 0).unwrap().unwrap();
        assert_eq!((t.start, t.end, t.kind), (2, 8, Kind::Name(b"A#20B")));
        assert_eq!(next(b"   % only a comment", 0), Ok(None));
        assert_eq!(next(b"% comment\r(x)", 0).unwrap().unwrap().kind, Kind::Literal(b"x"));
    }

    #[test]
    fn not_numbers_are_keywords() {
        for text in [&b"1.2.3"[..], b"+", b"-", b".", b"1e5", b"--1", b"4-2"] {
            assert_eq!(kinds(text), [Kind::Keyword(text)], "{text:?}");
        }
    }

    #[test]
    fn damage_stops_reading() {
        let d = |buf: &[u8], at, reason| assert_eq!(damage(buf), Damage { at, reason }, "{buf:?}");
        d(b"(open", 0, Reason::UnterminatedLiteral);
        d(b"x <41", 2, Reason::UnterminatedHex);
        d(b"<4G>", 2, Reason::BadHexDigit);
        d(b"a )", 2, Reason::UnexpectedDelimiter);
        d(b"> ", 0, Reason::UnexpectedDelimiter);
        d(b"99999999999999999999", 0, Reason::IntegerOverflow);
    }

    #[test]
    fn defects_travel_with_their_token() {
        let defect = |buf: &[u8]| next(buf, 0).unwrap().unwrap().defect;
        assert_eq!(defect(b"<414>"), Some(Defect::OddHexDigits));
        assert_eq!(defect(b"< 41 42 >"), None);
        assert_eq!(defect(b"(\\777)"), Some(Defect::OctalOverflow));
        assert_eq!(defect(b"(\\377)"), None);
        for bad in [&b"/A#G1"[..], b"/A#", b"/A#4", b"/A#00"] {
            assert_eq!(defect(bad), Some(Defect::BadNameEscape), "{bad:?}");
        }
        assert_eq!(defect(b"/A#41#42"), None);
    }

    #[test]
    fn decoding() {
        assert_eq!(name_bytes(b"A#20B#23"), b"A B#");
        assert_eq!(name_bytes(b"A#G1#00#"), b"A#G1#00#");
        assert_eq!(name_bytes(b"#4A#4a#4"), b"JJ#4");
        assert_eq!(hex_bytes(b"41 42 4"), b"AB@");
        assert_eq!(hex_bytes(b""), b"");
        assert_eq!(
            literal_bytes(b"a\\n\\r\\t\\b\\f\\(\\)\\\\\\101\\7x\\8\\\r\nb\\\nc\r\nd\re\n\\"),
            b"a\n\r\t\x08\x0c()\\A\x07x8bc\nd\ne\n"
        );
        assert_eq!(literal_bytes(b"\\0011"), b"\x011");
    }

    proptest! {
        #[test]
        fn never_panics_and_always_advances(buf in proptest::collection::vec(any::<u8>(), 0..200)) {
            let mut pos = 0;
            while let Ok(Some(t)) = next(&buf, pos) {
                prop_assert!(t.end > pos && t.end <= buf.len());
                pos = t.end;
            }
        }
    }

    #[test]
    fn name_escapes_cut_short() {
        assert_eq!(name_bytes(b"#4A#4a#4"), b"JJ#4");
    }

    #[test]
    fn a_name_escape_with_a_bad_second_digit() {
        assert_eq!(name_bytes(b"#4G"), b"#4G");
    }
}
