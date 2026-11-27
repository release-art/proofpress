//! What placing a text run needs from its font (ISO 32000-2, 9.2.4 and 9.6 to 9.7): how many bytes a character code
//! takes, each code's advance width, and the ascent and descent that bound a glyph's height. Read from the font
//! dictionaries only; no font program is parsed.
//!
//! A simple font's widths come from `/FirstChar` and `/Widths` (scaled by a Type 3 font's `/FontMatrix`), a composite
//! font's from its descendant's `/W` and `/DW`. A font that declares no widths falls back to a typical text face's,
//! and one that has no descriptor to the usual ascent and descent, so a run in an unembedded standard font is placed
//! approximately. A malformed font dictionary gives the same fallback font as a missing one.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{Dict, Document, Object, Value};

/// An advance width, in thousandths of the font size, for a font that gives none.
const FALLBACK_WIDTH: f64 = 500.0;
const DEFAULT_ASCENT: f64 = 800.0;
const DEFAULT_DESCENT: f64 = -200.0;
/// Adobe's GB1, CNS1 and Korea1 collections give CIDs 1 to 95 to ASCII 0x20 to 0x7E; Japan1 differs at a few.
const ADOBE_LATIN: [&[u8]; 3] = [b"GB1", b"CNS1", b"Korea1"];
const ADOBE_LATIN_CIDS: u32 = 95;

/// A font's metrics.
#[derive(Clone, Debug, PartialEq)]
#[expect(clippy::struct_excessive_bools, reason = "independent facts a font dictionary gives")]
pub(crate) struct Font {
    /// 1 for a simple font, 2 for a composite one.
    pub(crate) code_bytes: usize,
    default_width: f64,
    /// Widths by code, as ranges `first..=last`; a later range overrides an earlier one.
    widths: Vec<(u64, u64, f64)>,
    /// In thousandths of the font size.
    pub(crate) ascent: f64,
    pub(crate) descent: f64,
    /// The `/BaseFont` without a subset prefix (`ABCDEF+`); `?` without one.
    pub(crate) name: String,
    /// The `/Subtype` without its slash: `Type1`, `TrueType`, `Type3`, `Type0` and so on.
    pub(crate) subtype: String,
    /// Whether the font program is in the file: a Type 3 font, or a descriptor with `/FontFile`, `/FontFile2` or
    /// `/FontFile3` (a composite font's descendant's).
    pub(crate) embedded: bool,
    /// Whether the font dictionary has a `/ToUnicode` entry.
    pub(crate) to_unicode: bool,
    /// Its `/ToUnicode` `CMap`'s `bfchar` and `bfrange` mappings, code to text; empty when there is none or it cannot
    /// be read.
    unicode: BTreeMap<u32, String>,
    /// A composite font of one of Adobe's GB1, CNS1 or Korea1 collections with an Identity encoding, whose codes are
    /// CIDs and CIDs 1 to 95 ASCII.
    adobe_latin: bool,
    /// The codes a simple font's `/Differences` maps to `.notdef`.
    notdef: BTreeSet<i64>,
    /// For a composite font, whether its encoding is `Identity-H` or `Identity-V`, so that code 0 is CID 0, `.notdef`.
    identity: Option<bool>,
    /// Whether it was read from a font dictionary, rather than standing in for a missing or malformed one.
    pub(crate) known: bool,
    /// The font dictionary's object number, when the walk selected an indirect one; `load` leaves it `None`.
    pub(crate) object: Option<u32>,
}

impl Default for Font {
    /// The font a missing or malformed font dictionary gives.
    fn default() -> Self {
        Self {
            code_bytes: 1,
            default_width: FALLBACK_WIDTH,
            widths: Vec::new(),
            ascent: DEFAULT_ASCENT,
            descent: DEFAULT_DESCENT,
            name: "?".to_owned(),
            subtype: "unknown".to_owned(),
            embedded: false,
            to_unicode: false,
            unicode: BTreeMap::new(),
            adobe_latin: false,
            notdef: BTreeSet::new(),
            identity: None,
            known: false,
            object: None,
        }
    }
}

impl Font {
    /// The advance of `code`, in thousandths of the font size.
    pub(crate) fn width(&self, code: u32) -> f64 {
        self.widths
            .iter()
            .rev()
            .find(|(first, last, _)| (*first..=*last).contains(&u64::from(code)))
            .map_or(self.default_width, |(_, _, w)| *w)
    }

    /// Whether `code` shows `.notdef`: code 0 in a composite font with an identity encoding, or a code a simple font's
    /// `/Differences` maps there.
    /// The characters `codes` show: from `/ToUnicode`, else for an Adobe collection its Latin CIDs, else Latin-1 for a
    /// simple font; `None` when nothing tells. A code neither gives is U+FFFD.
    pub(crate) fn text(&self, codes: &[u32]) -> Option<String> {
        if self.unicode.is_empty() && !self.adobe_latin {
            return (self.code_bytes == 1).then(|| codes.iter().map(|&c| char::from((c & 0xFF) as u8)).collect());
        }
        let latin = |code: u32| {
            (self.adobe_latin && (1..=ADOBE_LATIN_CIDS).contains(&code))
                .then(|| char::from_u32(code + 31).map(String::from))
                .flatten()
        };
        let shown: Vec<String> = codes
            .iter()
            .map(|&code| {
                self.unicode
                    .get(&code)
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .or_else(|| latin(code))
                    .unwrap_or_else(|| "\u{FFFD}".to_owned())
            })
            .collect();
        (!self.unicode.is_empty() || shown.iter().any(|c| c != "\u{FFFD}")).then(|| shown.concat())
    }

    pub(crate) fn is_notdef(&self, code: u32) -> bool {
        match self.identity {
            Some(identity) => identity && code == 0,
            None => self.notdef.contains(&i64::from(code)),
        }
    }

    /// Whether the font declares any glyph widths, so a run's extent is measured rather than estimated.
    pub(crate) fn measured(&self) -> bool {
        self.widths.iter().any(|(first, last, _)| first <= last)
    }

    /// The character codes in a shown string; a composite font's odd last byte is padded with a zero.
    pub(crate) fn codes(&self, data: &[u8]) -> Vec<u32> {
        if self.code_bytes == 1 {
            return data.iter().map(|&b| u32::from(b)).collect();
        }
        let mut padded = data.to_vec();
        if padded.len() % 2 == 1 {
            padded.push(0);
        }
        padded
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u32::from(u16::from_be_bytes(*pair)))
            .collect()
    }
}

/// The metrics of the font `dict`.
pub(crate) fn load(doc: &Document, dict: &Dict) -> Font {
    read(doc, dict).unwrap_or_default()
}

pub(crate) fn number(doc: &Document, object: &Object) -> Option<f64> {
    match doc.resolve(object)?.value {
        #[expect(clippy::cast_precision_loss, reason = "font metrics are far below 2^53")]
        Value::Integer(n) => Some(n as f64),
        Value::Real(r) => Some(r),
        _ => None,
    }
}

/// A code or CID written as a number, from 0 to 2^32 - 1: a real is truncated.
fn code(doc: &Document, object: &Object) -> Option<u64> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "truncation is the reading"
    )]
    number(doc, object)
        .filter(|n| (0.0..=f64::from(u32::MAX)).contains(n))
        .map(|n| n as u64)
}

/// `key` in `dict` as a number, `default` when it is absent; `None` when it is there but not a number.
fn number_or(doc: &Document, dict: Option<&Dict>, key: &[u8], default: f64) -> Option<f64> {
    match dict.and_then(|d| d.get(key)) {
        None => Some(default),
        Some(value) => number(doc, value),
    }
}

/// Whether a font descriptor holds a font program: `/FontFile`, `/FontFile2` or `/FontFile3`, a null entry or one
/// referring to nothing being as good as absent (7.3.7).
fn embeds(doc: &Document, descriptor: &Dict) -> bool {
    [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"].iter().any(|key| {
        descriptor
            .get(key)
            .and_then(|v| doc.resolve(v))
            .is_some_and(|v| v.value != Value::Null)
    })
}

/// A font's `/BaseFont` without a subset prefix (six capitals and a plus sign, 9.6.4); `?` without one.
/// Whether CID font `source` belongs to Adobe's GB1, CNS1 or Korea1 collection.
fn adobe_latin(doc: &Document, source: &Dict) -> bool {
    let info = source
        .get(b"CIDSystemInfo")
        .and_then(|i| doc.resolve(i))
        .and_then(Object::dict);
    let string = |key: &[u8]| match info
        .and_then(|i| i.get(key))
        .and_then(|v| doc.resolve(v))
        .map(|v| &v.value)
    {
        Some(Value::String(bytes, _)) => Some(bytes.as_slice()),
        _ => None,
    };
    string(b"Registry") == Some(b"Adobe") && string(b"Ordering").is_some_and(|o| ADOBE_LATIN.contains(&o))
}

/// The `/ToUnicode` `CMap` of `font`, read as the Python inventory read it: every `bfchar` block, then every `bfrange`
/// block, later mappings winning; destinations are UTF-16BE, decoded with U+FFFD for what does not decode. A `CMap`
/// with a malformed entry gives no mappings at all.
fn unicode(doc: &Document, font: &Dict) -> BTreeMap<u32, String> {
    let data = match font.get(b"ToUnicode").map(|t| &t.value) {
        Some(Value::Reference(num, _)) => doc.stream_data(*num),
        _ => None,
    };
    data.and_then(|d| bf_mappings(&d)).unwrap_or_default()
}

/// The blocks of `data` between `begin` and `end`, each as its hex strings and brackets.
fn blocks<'a>(data: &'a [u8], begin: &[u8], end: &[u8]) -> Vec<Vec<&'a [u8]>> {
    let mut out = Vec::new();
    let mut rest = data;
    while let Some(start) = find(rest, begin) {
        let after = rest.get(start + begin.len()..).unwrap_or_default();
        let Some(stop) = find(after, end) else { break };
        let block = after.get(..stop).unwrap_or_default();
        let mut tokens = Vec::new();
        let mut i = 0;
        while let Some(&b) = block.get(i) {
            match b {
                b'<' => {
                    let close = block.get(i..).and_then(|t| t.iter().position(|&c| c == b'>'));
                    let Some(close) = close else { break };
                    tokens.push(block.get(i..=i + close).unwrap_or_default());
                    i += close + 1;
                }
                b'[' | b']' => {
                    tokens.push(block.get(i..=i).unwrap_or_default());
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out.push(tokens);
        rest = after.get(stop + end.len()..).unwrap_or_default();
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A hex string's bytes, white space ignored; `None` for a digit that is not hex. An odd count ends in a 0 digit.
fn hex(token: &[u8]) -> Option<Vec<u8>> {
    let digits: Vec<u8> = token
        .iter()
        .filter(|b| !matches!(b, b'<' | b'>') && !b.is_ascii_whitespace())
        .map(|&b| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok()))
        .collect::<Option<_>>()?;
    // Python's bytes.fromhex refuses an odd count, so the whole CMap is unread.
    digits
        .len()
        .is_multiple_of(2)
        .then(|| digits.chunks(2).map(|p| p.iter().fold(0, |a, d| a * 16 + d)).collect())
}

/// A source code's bytes as a number, big-endian.
fn code_of(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |a, &b| a.wrapping_shl(8) | u32::from(b))
}

/// UTF-16BE `bytes` as text, U+FFFD for an unpaired surrogate or an odd last byte.
fn utf16(bytes: &[u8]) -> String {
    let (pairs, odd) = bytes.as_chunks::<2>();
    let mut text = String::from_utf16_lossy(&pairs.iter().map(|p| u16::from_be_bytes(*p)).collect::<Vec<_>>());
    if !odd.is_empty() {
        text.push('\u{FFFD}');
    }
    text
}

fn bf_mappings(data: &[u8]) -> Option<BTreeMap<u32, String>> {
    let mut map = BTreeMap::new();
    for tokens in blocks(data, b"beginbfchar", b"endbfchar") {
        for [source, target] in tokens.as_chunks::<2>().0 {
            map.insert(code_of(&hex(source)?), utf16(&hex(target)?));
        }
    }
    for tokens in blocks(data, b"beginbfrange", b"endbfrange") {
        let mut rest = tokens.as_slice();
        while let [low, high, target, after @ ..] = rest {
            let (low, high) = (code_of(&hex(low)?), code_of(&hex(high)?));
            if *target == b"[" {
                let listed: Vec<&[u8]> = after.iter().copied().take_while(|t| *t != b"]").collect();
                // An array that is not closed leaves the whole `CMap` unread.
                if listed.len() == after.len() {
                    return None;
                }
                for (code, dst) in (low..=high).zip(&listed) {
                    map.insert(code, utf16(&hex(dst)?));
                }
                rest = after.get(listed.len() + 1..).unwrap_or_default();
            } else {
                let base: Vec<char> = utf16(&hex(target)?).chars().collect();
                for offset in 0..=high.saturating_sub(low).min(0xFFFF) {
                    // Past U+10FFFF Python's `chr` fails, and with it the whole `CMap`; a surrogate it keeps, and
                    // text cannot hold one, so it is U+FFFD.
                    let text = match base.split_last() {
                        Some((last, head)) => {
                            let next = u32::from(*last) + offset;
                            if next > 0x10_FFFF {
                                return None;
                            }
                            let next = char::from_u32(next).unwrap_or('\u{FFFD}');
                            head.iter().chain(std::iter::once(&next)).collect()
                        }
                        None => String::new(),
                    };
                    map.insert(low.wrapping_add(offset), text);
                }
                rest = after;
            }
        }
    }
    Some(map)
}

fn base_name(doc: &Document, font: &Dict) -> String {
    let Some(name) = font
        .get(b"BaseFont")
        .and_then(|n| doc.resolve(n))
        .and_then(Object::name)
    else {
        return "?".to_owned();
    };
    let prefixed = name
        .get(..7)
        .is_some_and(|p| p.iter().take(6).all(u8::is_ascii_uppercase) && p.get(6) == Some(&b'+'));
    String::from_utf8_lossy(if prefixed {
        name.get(7..).unwrap_or_default()
    } else {
        name
    })
    .into_owned()
}

fn read(doc: &Document, font: &Dict) -> Option<Font> {
    let composite = font.get(b"Subtype").and_then(Object::name) == Some(b"Type0");
    let source = if composite {
        let descendants = doc.resolve(font.get(b"DescendantFonts")?)?.array()?;
        doc.resolve(descendants.first()?)?.dict()?
    } else {
        font
    };
    let descriptor = match source.get(b"FontDescriptor") {
        Some(d) => Some(doc.resolve(d)?.dict()?),
        None => None,
    };
    let ascent = number_or(doc, descriptor, b"Ascent", DEFAULT_ASCENT)?;
    let descent = number_or(doc, descriptor, b"Descent", DEFAULT_DESCENT)?;
    let program = descriptor.is_some_and(|d| embeds(doc, d));
    let name = base_name(doc, font);
    let subtype = font
        .get(b"Subtype")
        .and_then(Object::name)
        .map_or_else(String::new, |s| String::from_utf8_lossy(s).into_owned());
    if composite {
        let encoding = font
            .get(b"Encoding")
            .and_then(|e| doc.resolve(e))
            .and_then(Object::name);
        return Some(Font {
            code_bytes: 2,
            default_width: number_or(doc, Some(source), b"DW", 1000.0)?,
            widths: cid_widths(doc, source.get(b"W"))?,
            ascent,
            descent,
            name,
            subtype,
            embedded: program,
            to_unicode: font.get(b"ToUnicode").is_some(),
            unicode: unicode(doc, font),
            adobe_latin: matches!(encoding, Some(b"Identity-H" | b"Identity-V")) && adobe_latin(doc, source),
            notdef: BTreeSet::new(),
            identity: Some(matches!(encoding, Some(b"Identity-H" | b"Identity-V"))),
            known: true,
            object: None,
        });
    }
    let scale = match font.get(b"FontMatrix") {
        Some(matrix) => number(doc, doc.resolve(matrix)?.array()?.first()?)? * 1000.0,
        None => 1.0,
    };
    let first = match font.get(b"FirstChar") {
        Some(first) => code(doc, first)?,
        None => 0,
    };
    let listed = match font.get(b"Widths") {
        Some(widths) => doc.resolve(widths)?.array()?,
        None => &[],
    };
    let mut widths = Vec::new();
    for (code, width) in (first..).zip(listed) {
        widths.push((code, code, number(doc, width)? * scale));
    }
    let missing = number_or(doc, descriptor, b"MissingWidth", 0.0)?;
    let default_width = if missing != 0.0 {
        missing
    } else if widths.is_empty() {
        FALLBACK_WIDTH
    } else {
        0.0
    };
    Some(Font {
        code_bytes: 1,
        default_width,
        widths,
        ascent,
        descent,
        embedded: subtype == "Type3" || program,
        to_unicode: font.get(b"ToUnicode").is_some(),
        unicode: unicode(doc, font),
        adobe_latin: false,
        name,
        subtype,
        notdef: notdef_codes(doc, font.get(b"Encoding"))?,
        identity: None,
        known: true,
        object: None,
    })
}

/// The codes an encoding dictionary's `/Differences` (9.6.5.1) maps to `.notdef`; `None` when an entry is neither a
/// code nor a name.
fn notdef_codes(doc: &Document, encoding: Option<&Object>) -> Option<BTreeSet<i64>> {
    let mut out = BTreeSet::new();
    let Some(differences) = encoding
        .and_then(|e| doc.resolve(e))
        .and_then(Object::dict)
        .and_then(|e| e.get(b"Differences"))
        .and_then(|d| doc.resolve(d))
        .and_then(Object::array)
    else {
        return Some(out);
    };
    let mut code = 0i64;
    for item in differences {
        let item = doc.resolve(item)?;
        match &item.value {
            Value::Name(name) => {
                if name == b".notdef" {
                    out.insert(code);
                }
                code = code.saturating_add(1);
            }
            #[expect(clippy::cast_possible_truncation, reason = "a code is truncated to a whole number")]
            Value::Real(r) => code = *r as i64,
            Value::Integer(n) => code = *n,
            _ => return None,
        }
    }
    Some(out)
}

/// A CID font's `/W` (9.7.4.3): `c [w1 w2 ...]` and `c_first c_last w` entries; `None` when it is malformed.
fn cid_widths(doc: &Document, w: Option<&Object>) -> Option<Vec<(u64, u64, f64)>> {
    let Some(w) = w else { return Some(Vec::new()) };
    let items = doc.resolve(w)?.array()?;
    let mut out = Vec::new();
    let mut rest = items;
    while let [first, next, tail @ ..] = rest {
        let first = code(doc, first)?;
        if let Some(list) = doc.resolve(next)?.array() {
            for (cid, width) in (first..).zip(list) {
                out.push((cid, cid, number(doc, width)?));
            }
            rest = tail;
        } else {
            let [width, tail @ ..] = tail else { return None };
            out.push((first, code(doc, next)?, number(doc, width)?));
            rest = tail;
        }
    }
    rest.is_empty().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// The font object 10 of a file whose other objects are `more`.
    fn font(dict: &str, more: &[(u32, &str)]) -> Font {
        let mut objects: Vec<(u32, &[u8])> = vec![(1, b"<< /Font 10 0 R >>"), (10, dict.as_bytes())];
        objects.extend(more.iter().map(|(n, s)| (*n, s.as_bytes())));
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let dict = doc
            .resolve(&Object {
                value: Value::Reference(10, 0),
                start: 0,
                end: 0,
            })
            .unwrap()
            .dict()
            .unwrap();
        load(&doc, dict)
    }

    #[test]
    fn simple_fonts() {
        let f = font(
            "<< /Subtype /Type1 /FirstChar 32 /Widths [250 9 0 R] /FontDescriptor 8 0 R >>",
            &[(8, "<< /Ascent 700 /Descent -150.5 /MissingWidth 300 >>"), (9, "333.5")],
        );
        assert_eq!(
            (f.width(32), f.width(33), f.width(34), f.code_bytes),
            (250.0, 333.5, 300.0, 1)
        );
        assert_eq!((f.ascent, f.descent), (700.0, -150.5));
        assert_eq!(f.codes(b"ab"), [97, 98]);
        let none = font("<< /Subtype /TrueType >>", &[]);
        assert_eq!(
            (none.width(65), none.ascent, none.descent),
            (500.0, 800.0, -200.0),
            "no widths, no descriptor"
        );
        let listed = font("<< /Subtype /Type1 /Widths [600] >>", &[]);
        assert_eq!(
            (listed.width(0), listed.width(1)),
            (600.0, 0.0),
            "codes outside /Widths: no MissingWidth"
        );
        let type3 = font(
            "<< /Subtype /Type3 /FontMatrix [0.01 0 0 0.01 0 0] /FirstChar 65.7 /Widths [50] >>",
            &[],
        );
        assert_eq!(type3.width(65), 500.0, "a Type 3 width in glyph space, scaled");
    }

    #[test]
    fn composite_fonts() {
        let f = font(
            "<< /Subtype /Type0 /DescendantFonts [8 0 R] >>",
            &[(
                8,
                "<< /DW 900 /W [1 [100 200] 5 7 300 6 [400]] /FontDescriptor << /Ascent 1000 >> >>",
            )],
        );
        assert_eq!(
            [1, 2, 3, 5, 6, 7].map(|c| f.width(c)),
            [100.0, 200.0, 900.0, 300.0, 400.0, 300.0]
        );
        assert_eq!((f.code_bytes, f.ascent, f.descent), (2, 1000.0, -200.0));
        assert_eq!(f.codes(b"\x01\x02\x03"), [0x0102, 0x0300], "an odd byte padded");
        assert_eq!(f.codes(b"\x01\x02"), [0x0102]);
        let plain = font("<< /Subtype /Type0 /DescendantFonts [<< >>] >>", &[]);
        assert_eq!((plain.width(1), plain.code_bytes), (1000.0, 2), "DW defaults to 1000");
    }

    #[test]
    fn malformed_fonts_fall_back() {
        let unknown = Font::default();
        for dict in [
            "<< /Subtype /Type0 >>",
            "<< /Subtype /Type0 /DescendantFonts 5 >>",
            "<< /Subtype /Type0 /DescendantFonts [] >>",
            "<< /Subtype /Type0 /DescendantFonts [5] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /DW /X >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W 5 >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [/X] >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1] >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1 [/X]] >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1 /X 3] >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1 2] >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1 2 /X] >>] >>",
            "<< /FontDescriptor 5 >>",
            "<< /FontDescriptor << /Ascent /X >> >>",
            "<< /FontDescriptor << /Descent /X >> >>",
            "<< /FontDescriptor << /MissingWidth /X >> >>",
            "<< /FontMatrix 5 >>",
            "<< /FontMatrix [] >>",
            "<< /FontMatrix [/X] >>",
            "<< /FirstChar -1 >>",
            "<< /Widths 5 >>",
            "<< /Widths [/X] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [/X 5 6] >>] >>",
            "<< /Widths 99 0 R >>",
            "<< /Widths [99 0 R] >>",
            "<< /FontMatrix 99 0 R >>",
            "<< /FontDescriptor 99 0 R >>",
            "<< /Subtype /Type0 /DescendantFonts 99 0 R >>",
            "<< /Subtype /Type0 /DescendantFonts [99 0 R] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W 99 0 R >>] >>",
            "<< /Subtype /Type0 /DescendantFonts [<< /W [1 99 0 R] >>] >>",
        ] {
            assert_eq!(font(dict, &[]), unknown, "{dict}");
        }
    }

    #[test]
    fn names_embedding_and_notdef() {
        assert!(
            !font(
                "<< /Subtype /Type1 /FontDescriptor << /FontFile3 null /FontFile 99 0 R >> >>",
                &[]
            )
            .embedded,
            "null and dangling"
        );
        let simple = font(
            "<< /Subtype /TrueType /BaseFont /ABCDEF+Nunito /FontDescriptor << /FontFile2 5 >> \
             /Encoding << /Differences [32 /.notdef /a 40.0 /.notdef] >> /FirstChar 3 /Widths [] >>",
            &[],
        );
        assert_eq!(
            (
                simple.name.as_str(),
                simple.subtype.as_str(),
                simple.embedded,
                simple.known
            ),
            ("Nunito", "TrueType", true, true)
        );
        assert_eq!(
            [32, 33, 40, 41].map(|c| simple.is_notdef(c)),
            [true, false, true, false]
        );
        assert!(!simple.measured(), "an empty /Widths");
        let plain = font(
            "<< /Subtype /Type1 /BaseFont /Abcdef+Not /Encoding /WinAnsiEncoding /Widths [500] >>",
            &[],
        );
        assert_eq!(
            (plain.name.as_str(), plain.embedded, plain.measured()),
            ("Abcdef+Not", false, true)
        );
        let type3 = font("<< /Subtype /Type3 /BaseFont 5 >>", &[]);
        assert_eq!((type3.name.as_str(), type3.embedded), ("?", true));
        let unnamed = font("<< >>", &[]);
        assert_eq!((unnamed.subtype.as_str(), unnamed.name.as_str()), ("", "?"));
        let identity = font(
            "<< /Subtype /Type0 /BaseFont /Zed /Encoding /Identity-V /DescendantFonts [<< /FontDescriptor << /FontFile3 5 >> >>] >>",
            &[],
        );
        assert_eq!(
            (identity.is_notdef(0), identity.is_notdef(1), identity.embedded),
            (true, false, true)
        );
        let cmap = font(
            "<< /Subtype /Type0 /Encoding /UniJIS-UCS2-H /DescendantFonts [<< /W [5 1 9] >>] >>",
            &[],
        );
        assert_eq!(
            (cmap.is_notdef(0), cmap.measured()),
            (false, false),
            "a range with its ends reversed declares nothing"
        );
        assert_eq!(
            font("<< /Encoding << /Differences [(x)] >> >>", &[]),
            Font::default(),
            "a malformed /Differences"
        );
        assert_eq!(
            font("<< /Encoding << /Differences [99 0 R] >> >>", &[]),
            Font::default(),
            "a dangling entry"
        );
        assert!(!Font::default().known);
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// The font object 10 of a file whose object 11 is a stream of `cmap`.
    fn font(dict: &str, cmap: &str) -> Font {
        let stream = format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len());
        let objects: Vec<(u32, &[u8])> = vec![
            (1, b"<< /Font 10 0 R >>"),
            (10, dict.as_bytes()),
            (11, stream.as_bytes()),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let dict = doc
            .resolve(&Object {
                value: Value::Reference(10, 0),
                start: 0,
                end: 0,
            })
            .unwrap()
            .dict()
            .unwrap();
        load(&doc, dict)
    }

    const SIMPLE: &str = "<< /Subtype /Type1 /ToUnicode 11 0 R >>";

    fn shown(cmap: &str, codes: &[u32]) -> Option<String> {
        font(SIMPLE, cmap).text(codes)
    }

    #[test]
    fn to_unicode_cmaps() {
        let cmap = "1 beginbfchar <41> <0042> <42> <00430044> endbfchar \
            2 beginbfrange <50> <52> <0061> <60> <61> [<0078> <0079>] <70> <70> <> endbfrange";
        assert_eq!(
            shown(cmap, &[0x41, 0x42, 0x50, 0x52, 0x60, 0x61, 0x70]).as_deref(),
            Some("BCDacxy\u{FFFD}")
        );
        // A code the CMap does not map is U+FFFD; an odd last byte and a lone surrogate are U+FFFD too.
        assert_eq!(shown(cmap, &[0x99]).as_deref(), Some("\u{FFFD}"));
        assert_eq!(
            shown("beginbfchar <41> <004100> <42> <D800> endbfchar", &[0x41, 0x42]).as_deref(),
            Some("A\u{FFFD}\u{FFFD}")
        );
        // A range into the surrogates gives U+FFFD (past U+10FFFF the whole CMap is unread, below); a later mapping
        // wins.
        assert_eq!(
            shown("beginbfrange <01> <02> <D7FF> endbfrange", &[1, 2]).as_deref(),
            Some("\u{D7FF}\u{FFFD}")
        );
        assert_eq!(
            shown(
                "beginbfchar <41> <0041> endbfchar beginbfchar <41> <0042> endbfchar",
                &[0x41]
            )
            .as_deref(),
            Some("B")
        );
        // A malformed CMap maps nothing, and the font reads as Latin-1: a digit that is not hex, an odd count of
        // digits, an array that is not closed, an unclosed string, a block without its end.
        for broken in [
            "beginbfchar <4G> <0041> endbfchar",
            "beginbfchar <041> <0041> endbfchar",
            "beginbfrange <41> <42> [<0041> endbfrange",
            "beginbfchar <41 endbfchar",
            "beginbfchar <41> <0042>",
            "beginbfchar <41> <004G> endbfchar",
            "beginbfrange <4G> <42> <0041> endbfrange",
            "beginbfrange <41> <4G> <0041> endbfrange",
            "beginbfrange <41> <42> <004G> endbfrange",
            "beginbfrange <41> <42> [<004G>] endbfrange",
            "beginbfrange <41> <42> <DBFFDFFF> endbfrange",
        ] {
            assert_eq!(shown(broken, &[0x41]).as_deref(), Some("A"), "{broken}");
        }
        // A /ToUnicode that is not a stream maps nothing either.
        assert_eq!(
            font("<< /Subtype /Type1 /ToUnicode /Identity-H >>", "")
                .text(&[0x41])
                .as_deref(),
            Some("A")
        );
    }

    #[test]
    fn composite_fonts_tell_text_only_through_a_cmap_or_an_adobe_collection() {
        let composite = |info: &str| {
            format!(
                "<< /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 {info} >>] >>"
            )
        };
        assert_eq!(font(&composite(""), "").text(&[1]), None);
        let gb1 = composite("/CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 2 >>");
        assert_eq!(font(&gb1, "").text(&[34, 95, 500]).as_deref(), Some("A~\u{FFFD}"));
        assert_eq!(font(&gb1, "").text(&[500]), None);
        let japan = composite("/CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) >>");
        assert_eq!(font(&japan, "").text(&[34]), None);
        let named = composite("/CIDSystemInfo << /Registry /Adobe /Ordering /GB1 >>");
        // Through another CMap the codes are not CIDs.
        let cmapped = gb1.replace("Identity-H", "UniGB-UCS2-H");
        assert_eq!(font(&cmapped, "").text(&[34]), None);
        assert_eq!(font(&named, "").text(&[34]), None);
        // With a CMap every code is told, a gap as U+FFFD.
        let mapped = format!("<< /Subtype /Type0 /ToUnicode 11 0 R {}", &composite("")[2..]);
        assert_eq!(
            font(&mapped, "beginbfchar <0001> <0041> endbfchar")
                .text(&[1, 2])
                .as_deref(),
            Some("A\u{FFFD}")
        );
    }
}
