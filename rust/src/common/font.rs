//! What placing a text run needs from its font (ISO 32000-2, 9.2.4 and 9.6 to 9.7): how many bytes a character code
//! takes, each code's advance width, and the ascent and descent that bound a glyph's height. Read from the font
//! dictionaries only; no font program is parsed.
//!
//! A simple font's widths come from `/FirstChar` and `/Widths` (scaled by a Type 3 font's `/FontMatrix`), a composite
//! font's from its descendant's `/W` and `/DW`. A font that declares no widths falls back to a typical text face's,
//! and one that has no descriptor to the usual ascent and descent, so a run in an unembedded standard font is placed
//! approximately. A malformed font dictionary gives the same fallback font as a missing one.

use crate::document::{Dict, Document, Object, Value};

/// An advance width, in thousandths of the font size, for a font that gives none.
const FALLBACK_WIDTH: f64 = 500.0;
const DEFAULT_ASCENT: f64 = 800.0;
const DEFAULT_DESCENT: f64 = -200.0;

/// A font's metrics.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Font {
    /// 1 for a simple font, 2 for a composite one.
    pub(crate) code_bytes: usize,
    default_width: f64,
    /// Widths by code, as ranges `first..=last`; a later range overrides an earlier one.
    widths: Vec<(u64, u64, f64)>,
    /// In thousandths of the font size.
    pub(crate) ascent: f64,
    pub(crate) descent: f64,
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

fn number(doc: &Document, object: &Object) -> Option<f64> {
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
    if composite {
        return Some(Font {
            code_bytes: 2,
            default_width: number_or(doc, Some(source), b"DW", 1000.0)?,
            widths: cid_widths(doc, source.get(b"W"))?,
            ascent,
            descent,
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
    })
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
}
