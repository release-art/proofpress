//! The glyph names a simple font's codes select (ISO 32000-1 9.6.5 and 9.6.6): a named base encoding, overridden by an
//! encoding dictionary's `/Differences`.
//!
//! `StandardEncoding` and `MacExpertEncoding` come from `read-fonts`. `WinAnsiEncoding` and `MacRomanEncoding` are built
//! from the characters their code pages map to (Windows code page 1252 and Mac OS Roman, from the Unicode Consortium's
//! mapping files) and the Adobe Glyph List names of those characters, with the differences of PDF's own tables (Annex
//! D): codes PDF leaves unassigned (`WinAnsiEncoding`'s unused codes are its bullet), `space` for the no-break space and `hyphen` for the soft hyphen, and Mac OS Roman's
//! currency sign where Apple later put the euro.

use read_fonts::ps::{PredefinedEncoding, agl};

use crate::document::{Dict, Document, Object, Value};

/// A named base encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    Standard,
    MacRoman,
    WinAnsi,
    MacExpert,
}

impl Base {
    pub(crate) fn named(name: &[u8]) -> Option<Self> {
        match name {
            b"StandardEncoding" => Some(Self::Standard),
            b"MacRomanEncoding" => Some(Self::MacRoman),
            b"WinAnsiEncoding" => Some(Self::WinAnsi),
            b"MacExpertEncoding" => Some(Self::MacExpert),
            _ => None,
        }
    }

    /// The glyph name `code` selects, `None` for a code the encoding leaves unassigned.
    pub(crate) fn name(self, code: u8) -> Option<String> {
        let named = |n: &str| (n != ".notdef").then(|| n.to_owned());
        match self {
            Self::Standard => named(PredefinedEncoding::Standard.name(code)),
            Self::MacExpert => named(PredefinedEncoding::Expert.name(code)),
            Self::WinAnsi | Self::MacRoman => {
                let ch = self.char(code)?;
                if ch == '\u{a0}' {
                    return Some("space".to_owned());
                }
                if ch == '\u{ad}' {
                    return Some("hyphen".to_owned());
                }
                let mut buf = [0u8; 64];
                agl::char_to_name(ch, &mut buf).map(str::to_owned)
            }
        }
    }

    /// The character `code` stands for in Windows 1252 or Mac OS Roman, as PDF's tables assign it.
    fn char(self, code: u8) -> Option<char> {
        let table: &[u16] = match self {
            Self::WinAnsi if (0x20..0x7F).contains(&code) || code >= 0xA0 => return Some(char::from(code)),
            Self::WinAnsi if code == 0x7F => return Some('\u{2022}'),
            Self::MacRoman if (0x20..0x7F).contains(&code) => return Some(char::from(code)),
            Self::WinAnsi => &WIN_1252,
            _ => &MAC_ROMAN,
        };
        let high = usize::from(code)
            .checked_sub(0x80)
            .and_then(|i| table.get(i))
            .copied()
            .unwrap_or(0);
        char::from_u32(u32::from(high)).filter(|_| high != 0)
    }
}

/// Windows 1252 for codes 0x80 to 0x9F; the codes it leaves unused are the bullet, as Annex D's note on
/// `WinAnsiEncoding` says of every unused code above octal 40 (0x7F too).
const WIN_1252: [u16; 32] = [
    0x20AC, 0x2022, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x2022,
    0x017D, 0x2022, 0x2022, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x2022, 0x017E, 0x0178,
];

/// Mac OS Roman for codes 0x80 to 0xFF; 0 where PDF assigns no glyph (the mathematical and Greek characters, and the
/// Apple logo, which PDF's table leaves out).
const MAC_ROMAN: [u16; 128] = [
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4, 0x00E3, 0x00E5, 0x00E7,
    0x00E9, 0x00E8, 0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6, 0x00F5,
    0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020, 0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9,
    0x2122, 0x00B4, 0x00A8, 0, 0x00C6, 0x00D8, 0, 0x00B1, 0, 0, 0x00A5, 0x00B5, 0, 0, 0, 0, 0, 0x00AA, 0x00BA, 0,
    0x00E6, 0x00F8, 0x00BF, 0x00A1, 0x00AC, 0, 0x0192, 0, 0, 0x00AB, 0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5,
    0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0, 0x00FF, 0x0178, 0x2044, 0x00A4, 0x2039,
    0x203A, 0xFB01, 0xFB02, 0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD,
    0x00CE, 0x00CF, 0x00CC, 0x00D3, 0x00D4, 0, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF, 0x02D8,
    0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
];

/// A simple font's encoding as its dictionary gives it: the base, if named, and the `/Differences` by code.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Encoding {
    pub(crate) base: Option<Base>,
    pub(crate) differences: Vec<(u8, String)>,
}

impl Encoding {
    /// The font dictionary's `/Encoding`: a name, or a dictionary with `/BaseEncoding` and `/Differences`. A value that
    /// is neither, and codes outside 0 to 255, are ignored.
    pub(crate) fn of(doc: &Document, font: &Dict) -> Self {
        let Some(value) = font.get(b"Encoding").and_then(|e| doc.resolve(e)) else {
            return Self::default();
        };
        match &value.value {
            Value::Name(n) => Self {
                base: Base::named(n),
                differences: Vec::new(),
            },
            Value::Dict(d) => Self {
                base: d.get(b"BaseEncoding").and_then(Object::name).and_then(Base::named),
                differences: differences(doc, d),
            },
            _ => Self::default(),
        }
    }

    /// The name `code` selects through the `/Differences`, else the base, else `fallback` (the font's own encoding).
    pub(crate) fn name(&self, code: u8, fallback: impl FnOnce(u8) -> Option<String>) -> Option<String> {
        if let Some((_, name)) = self.differences.iter().rev().find(|(c, _)| *c == code) {
            return Some(name.clone());
        }
        match self.base {
            Some(base) => base.name(code),
            None => fallback(code),
        }
    }
}

fn differences(doc: &Document, dict: &Dict) -> Vec<(u8, String)> {
    let items = dict
        .get(b"Differences")
        .and_then(|d| doc.resolve(d))
        .and_then(Object::array)
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut code: Option<i64> = None;
    for item in items {
        match &item.value {
            Value::Integer(n) => code = Some(*n),
            Value::Name(name) => {
                if let Some(c) = code.and_then(|c| u8::try_from(c).ok()) {
                    out.push((c, String::from_utf8_lossy(name).into_owned()));
                }
                code = code.map(|c| c + 1);
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Base, Encoding};
    use crate::document::Document;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    #[test]
    fn base_encodings_name_their_codes() {
        let name = |base: Base, code| base.name(code);
        assert_eq!(name(Base::Standard, b'A').as_deref(), Some("A"));
        assert_eq!(name(Base::Standard, 0x27).as_deref(), Some("quoteright"));
        assert_eq!(name(Base::Standard, 0).as_deref(), None);
        assert_eq!(name(Base::MacExpert, 0x30).as_deref(), Some("zerooldstyle"));
        assert_eq!(name(Base::WinAnsi, 0x27).as_deref(), Some("quotesingle"));
        assert_eq!(name(Base::WinAnsi, 0x80).as_deref(), Some("Euro"));
        assert_eq!(name(Base::WinAnsi, 0x81).as_deref(), Some("bullet"));
        assert_eq!(name(Base::WinAnsi, 0x7F).as_deref(), Some("bullet"));
        assert_eq!(name(Base::WinAnsi, 0xA0).as_deref(), Some("space"));
        assert_eq!(name(Base::WinAnsi, 0xAD).as_deref(), Some("hyphen"));
        assert_eq!(name(Base::WinAnsi, 0xE9).as_deref(), Some("eacute"));
        assert_eq!(name(Base::WinAnsi, 0x10).as_deref(), None);
        assert_eq!(name(Base::MacRoman, b'a').as_deref(), Some("a"));
        assert_eq!(name(Base::MacRoman, 0x8E).as_deref(), Some("eacute"));
        assert_eq!(name(Base::MacRoman, 0xCA).as_deref(), Some("space"));
        assert_eq!(name(Base::MacRoman, 0xDB).as_deref(), Some("currency"));
        assert_eq!(name(Base::MacRoman, 0xAD).as_deref(), None);
        assert_eq!(name(Base::MacRoman, 0x7F).as_deref(), None);
        assert_eq!(Base::named(b"Symbol"), None);
        assert_eq!(Base::named(b"StandardEncoding"), Some(Base::Standard));
        assert_eq!(Base::named(b"MacExpertEncoding"), Some(Base::MacExpert));
    }

    #[test]
    fn font_encodings_with_differences() {
        let fonts: &[u8] = b"<< /A 10 0 R /B << /Encoding << /BaseEncoding /WinAnsiEncoding \
            /Differences [65 /x /y (s) 300 /z -1 /w] >> >> /C << /Encoding 5 >> /D << >> >>";
        let objects: [(u32, &[u8]); 2] = [(1, fonts), (10, b"<< /Encoding /MacRomanEncoding >>")];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let catalog = doc.objects().find(|(n, ..)| *n == 1).unwrap().1.dict().unwrap();
        let font = |key: &[u8]| {
            let font = catalog
                .get(key)
                .and_then(|f| doc.resolve(f))
                .and_then(|f| f.dict())
                .unwrap();
            Encoding::of(&doc, font)
        };
        assert_eq!(font(b"A").base, Some(Base::MacRoman));
        let b = font(b"B");
        assert_eq!(b.differences, [(65, "x".to_owned()), (66, "y".to_owned())]);
        assert_eq!(b.name(65, |_| None).as_deref(), Some("x"));
        assert_eq!(b.name(67, |_| None).as_deref(), Some("C"));
        assert_eq!(font(b"C"), Encoding::default());
        let own = font(b"D");
        assert_eq!(own.name(65, |c| Some(format!("own{c}"))).as_deref(), Some("own65"));
    }
}
