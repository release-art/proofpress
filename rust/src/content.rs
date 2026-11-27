//! Tier 6: content-stream operators, inline images, forms and resource lookup (ISO 32000-2, 7.8).
//!
//! `read` turns a page's (or a form's) content streams into operations, each with its operands, in order. The
//! streams are read as one, joined by white space, since 7.7.3.3 lets a page's content be divided between streams at any
//! token boundary, even inside an array. Offsets are given back per stream. A token split across two streams, which
//! 7.7.3.3 forbids, reads as two tokens and is not reported.
//!
//! An inline image (8.9.7) ends where its data ends, never where a byte sequence `EI` happens to appear: at `/L` bytes
//! when given, at the size width × height × components × bits when unfiltered, and otherwise at the first filter's
//! end-of-data marker. When none of these applies the end is unknown, and the rest of the content is a gap.
//!
//! Reading stops at the first damage, or where the content passes a limit (more states saved by `q` and not restored, or
//! more operands waiting for an operator, than [`Limits`] allows); what was read before it stands.

use crate::document::{Dict, Object, Value};
use crate::filters::{self, Filter};
use crate::pipeline::Limits;
use crate::syntax::{self, Damage, Mode, Note, Parser, Read};

/// The operators of the PDF 1.6 Reference, Appendix A, Table A.1, which PDF/X-4 6.19 holds content to.
pub(crate) const OPERATORS: [&[u8]; 73] = [
    b"b", b"B", b"b*", b"B*", b"BDC", b"BI", b"BMC", b"BT", b"BX", b"c", b"cm", b"CS", b"cs", b"d", b"d0", b"d1",
    b"Do", b"DP", b"EI", b"EMC", b"ET", b"EX", b"f", b"F", b"f*", b"G", b"g", b"gs", b"h", b"i", b"ID", b"j", b"J",
    b"K", b"k", b"l", b"m", b"M", b"MP", b"n", b"q", b"Q", b"re", b"RG", b"rg", b"ri", b"s", b"S", b"SC", b"sc",
    b"SCN", b"scn", b"sh", b"T*", b"Tc", b"Td", b"TD", b"Tf", b"Tj", b"TJ", b"TL", b"Tm", b"Tr", b"Ts", b"Tw", b"Tz",
    b"v", b"w", b"W", b"W*", b"y", b"'", b"\"",
];

/// Where something in the content is: which of the streams, and the offset in its decoded data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct At {
    pub(crate) stream: usize,
    pub(crate) offset: usize,
}

/// One operation: an operator and the operands before it.
#[derive(Debug, PartialEq)]
pub(crate) struct Op {
    pub(crate) name: Vec<u8>,
    pub(crate) operands: Vec<Object>,
    pub(crate) at: At,
    /// Whether it is inside a `BX`/`EX` section.
    pub(crate) compatible: bool,
}

/// An inline image: its dictionary with the abbreviated keys spelled out (8.9.7, Table 92), and where its data is:
/// `data` bytes long from `start`, which is where the image's `BI` is.
#[derive(Debug, PartialEq)]
pub(crate) struct InlineImage {
    pub(crate) dict: Dict,
    pub(crate) data: At,
    pub(crate) len: usize,
    pub(crate) at: At,
}

/// What the content holds, in order.
#[derive(Debug, PartialEq)]
pub(crate) enum Item {
    Op(Op),
    Image(InlineImage),
}

/// Something wrong with the content, found while reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// Tier 2 could not read on; the rest is a gap.
    Syntax(Damage),
    /// An inline image whose data's end cannot be determined; the rest is a gap.
    InlineEnd,
    /// An inline image whose `/L` disagrees with its measured length; the rest is a gap.
    InlineLength { given: usize, measured: usize },
    /// An inline image dictionary that is malformed: a key that is not a name, a reference, or no `ID`.
    InlineDict,
    /// After an inline image's data, something other than `EI`; the rest is a gap.
    NoEi,
    /// An operator the PDF 1.6 Reference does not define, outside `BX`/`EX` (inside, it is an [`Op`] marked
    /// compatible).
    UnknownOperator(Vec<u8>),
    /// An `EX` with no `BX` open, or a `BX` still open at the end.
    Compatibility,
    /// A `q` past [`Limits::saved_states`] states saved and not restored; the rest is a gap.
    SavedStates,
    /// An operand past [`Limits::operands`] waiting for an operator; the rest is a gap.
    Operands,
}

/// The content read, with what went wrong and tier 2's notes, each where it was.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Content {
    pub(crate) items: Vec<Item>,
    pub(crate) problems: Vec<(At, Problem)>,
    pub(crate) notes: Vec<(usize, Note)>,
}

/// How many components the colour space a resource name names has, from the content's `/Resources`.
pub(crate) type Spaces<'a> = &'a dyn Fn(&[u8]) -> Option<usize>;

/// Read `streams`, the decoded data of one content in order, within `limits`; `spaces` answers for the colour spaces
/// inline images name from the resources.
pub(crate) fn read(streams: &[&[u8]], limits: &Limits, spaces: Spaces<'_>) -> Content {
    let mut joined = Vec::new();
    let mut starts = Vec::new();
    for data in streams {
        starts.push(joined.len());
        joined.extend_from_slice(data);
        joined.push(b'\n');
    }
    let locate = |offset: usize| -> At {
        let stream = starts.partition_point(|&s| s <= offset).saturating_sub(1);
        At {
            stream,
            offset: offset - starts.get(stream).copied().unwrap_or(0),
        }
    };
    let mut content = Content::default();
    let mut operands = Vec::new();
    let mut depth = 0_usize;
    let mut saved = 0_usize;
    let mut parser = Parser::new(&joined, 0, Mode::Content, limits.nesting);
    loop {
        let mut notes = Vec::new();
        let read = syntax::read(&mut parser, &mut notes);
        content
            .notes
            .extend(notes.into_iter().map(|n| (locate(note_at(&n)).stream, n)));
        let (word, offset) = match read {
            Ok(Read::Value(object)) if operands.len() >= limits.operands => {
                content.problems.push((locate(object.start), Problem::Operands));
                break;
            }
            Ok(Read::Value(object)) => {
                operands.push(object);
                continue;
            }
            Ok(Read::Keyword { word, start, .. }) => (word, start),
            Ok(Read::End) => break,
            Err(damage) => {
                content.problems.push((
                    locate(damage.at.min(joined.len().saturating_sub(1))),
                    Problem::Syntax(damage),
                ));
                break;
            }
        };
        let at = locate(offset);
        if word == b"BI" {
            operands.clear();
            match inline_image(&mut parser, &joined, limits, spaces) {
                Ok((dict, start, len)) => content.items.push(Item::Image(InlineImage {
                    dict,
                    data: locate(start),
                    len,
                    at,
                })),
                Err(problem) => {
                    content.problems.push((at, problem));
                    break;
                }
            }
            continue;
        }
        match word.as_slice() {
            b"q" if saved >= limits.saved_states => {
                content.problems.push((at, Problem::SavedStates));
                break;
            }
            b"q" => saved += 1,
            b"Q" => saved = saved.saturating_sub(1),
            b"BX" => depth += 1,
            b"EX" if depth == 0 => content.problems.push((at, Problem::Compatibility)),
            b"EX" => depth -= 1,
            _ => {}
        }
        let known = OPERATORS.contains(&word.as_slice());
        if !known && depth == 0 {
            content.problems.push((at, Problem::UnknownOperator(word.clone())));
        }
        let operands = std::mem::take(&mut operands);
        content.items.push(Item::Op(Op {
            name: word,
            operands,
            at,
            compatible: depth > 0,
        }));
    }
    if depth > 0 {
        content
            .problems
            .push((locate(joined.len().saturating_sub(1)), Problem::Compatibility));
    }
    content
}

/// Where a tier-2 note is.
fn note_at(note: &Note) -> usize {
    match *note {
        Note::Defect(at, _) | Note::DuplicateKey(at) | Note::NoEolBeforeEndstream(at) => at,
    }
}

/// The full name of an inline image key (Table 92) or value (Table 93), as written when it has no abbreviation.
fn spelled_out(name: &[u8]) -> &[u8] {
    match name {
        b"BPC" => b"BitsPerComponent",
        b"CS" => b"ColorSpace",
        b"D" => b"Decode",
        b"DP" => b"DecodeParms",
        b"F" => b"Filter",
        b"H" => b"Height",
        b"IM" => b"ImageMask",
        b"I" => b"Interpolate",
        b"W" => b"Width",
        b"L" => b"Length",
        other => other,
    }
}

/// An inline image's dictionary, from after `BI` to `ID`, then its data and `EI`: the dictionary, and where the data
/// starts in `data` and how long it is.
fn inline_image(
    parser: &mut Parser<'_>,
    data: &[u8],
    limits: &Limits,
    spaces: Spaces<'_>,
) -> Result<(Dict, usize, usize), Problem> {
    let mut dict = Dict::default();
    loop {
        let mut notes = Vec::new();
        let key = match syntax::read(parser, &mut notes).map_err(Problem::Syntax)? {
            Read::Value(Object {
                value: Value::Name(key),
                ..
            }) => key,
            Read::Keyword { word, .. } if word == b"ID" => break,
            _ => return Err(Problem::InlineDict),
        };
        let Read::Value(value) = syntax::read(parser, &mut notes).map_err(Problem::Syntax)? else {
            return Err(Problem::InlineDict);
        };
        dict.push(spelled_out(&key).to_vec(), value);
    }
    let after_id = data.get(parser.position()..).unwrap_or_default();
    // A single white-space byte follows ID (8.9.7). Writers also put CR LF there, one end of line; that reading is taken
    // only when the data then ends exactly at EI and the one-byte reading does not.
    let skips: &[usize] = if after_id.starts_with(b"\r\n") { &[1, 2] } else { &[1] };
    let given = dict
        .get(b"Length")
        .and_then(Object::integer)
        .and_then(|l| usize::try_from(l).ok());
    let mut problem = Problem::InlineEnd;
    for &skip in skips {
        let rest = after_id.get(skip..).unwrap_or_default();
        let len = match (given, measure(&dict, rest, limits, spaces)) {
            (Some(given), Some(measured)) if given != measured => {
                problem = Problem::InlineLength { given, measured };
                continue;
            }
            (Some(len), _) | (None, Some(len)) => len,
            (None, None) => continue,
        };
        // Try the reading on a copy of the parser: it stands only when EI comes right after the data.
        let mut trial = parser.clone();
        let mut notes = Vec::new();
        if let Ok(raw) = trial.raw(len + skip - 1)
            && let Ok(Read::Keyword { word, .. }) = syntax::read(&mut trial, &mut notes)
            && word == b"EI"
        {
            *parser = trial;
            return Ok((dict, raw.start + skip - 1, len));
        }
        problem = Problem::NoEi;
    }
    Err(problem)
}

/// The length of an inline image's data, measured: the first filter's encoded length, or for unfiltered data the
/// size its dimensions give.
fn measure(dict: &Dict, data: &[u8], limits: &Limits, spaces: Spaces<'_>) -> Option<usize> {
    let first = match dict.get(b"Filter").map(|f| &f.value) {
        None => None,
        Some(Value::Name(name)) => Some(name.as_slice()),
        Some(Value::Array(items)) => match items.first() {
            None => None,
            Some(item) => Some(item.name()?),
        },
        Some(_) => return None,
    };
    let int = |key: &[u8]| {
        dict.get(key)
            .and_then(Object::integer)
            .and_then(|v| usize::try_from(v).ok())
    };
    if let Some(name) = first {
        let parms = match dict.get(b"DecodeParms").map(|p| &p.value) {
            Some(Value::Dict(p)) => Some(p),
            Some(Value::Array(items)) => items.first().and_then(Object::dict),
            _ => None,
        };
        let parm = |key: &[u8]| parms.and_then(|p| p.get(key));
        let flag = |key: &[u8], default: bool| match parm(key).map(|v| &v.value) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        };
        let number = |key: &[u8], default: i64| parm(key).and_then(Object::integer).unwrap_or(default);
        return match name {
            b"DCT" | b"DCTDecode" => filters::dct_len(data),
            b"CCF" | b"CCITTFaxDecode" => {
                let rows = match number(b"Rows", 0) {
                    0 => int(b"Height")?,
                    rows => usize::try_from(rows).ok()?,
                };
                let fax = filters::Fax {
                    k: number(b"K", 0),
                    columns: u32::try_from(number(b"Columns", 1728)).ok()?,
                    rows: u32::try_from(rows).ok()?,
                    end_of_block: flag(b"EndOfBlock", true),
                    byte_align: flag(b"EncodedByteAlign", false),
                    end_of_line: flag(b"EndOfLine", false),
                };
                filters::fax_len(data, &fax)
            }
            _ => filters::encoded_len(
                data,
                Filter::from_name(name)?,
                number(b"EarlyChange", 1) != 0,
                limits.decoded_bytes,
            ),
        };
    }
    let mask = matches!(dict.get(b"ImageMask").map(|v| &v.value), Some(Value::Bool(true)));
    let (bits, components) = if mask {
        (1, 1)
    } else {
        (int(b"BitsPerComponent")?, components(dict.get(b"ColorSpace")?, spaces)?)
    };
    let row = int(b"Width")?.checked_mul(bits)?.checked_mul(components)?.div_ceil(8);
    row.checked_mul(int(b"Height")?)
}

/// The number of colour components of an inline image's colour space; a name the standard does not define is a
/// resource, which `spaces` answers for.
fn components(space: &Object, spaces: Spaces<'_>) -> Option<usize> {
    let name = match &space.value {
        Value::Name(name) => name.as_slice(),
        // [/Indexed base hival lookup]: one index per sample.
        Value::Array(items) => match items.first().and_then(Object::name)? {
            b"I" | b"Indexed" => return Some(1),
            _ => return None,
        },
        _ => return None,
    };
    match name {
        b"G" | b"DeviceGray" | b"CalGray" => Some(1),
        b"RGB" | b"DeviceRGB" | b"CalRGB" | b"Lab" => Some(3),
        b"CMYK" | b"DeviceCMYK" => Some(4),
        resource => spaces(resource),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn content(streams: &[&[u8]]) -> Content {
        read(streams, &Limits::default(), &|name| (name == b"CS1").then_some(3))
    }

    fn names(c: &Content) -> Vec<String> {
        c.items
            .iter()
            .map(|i| match i {
                Item::Op(op) => String::from_utf8_lossy(&op.name).into_owned(),
                Item::Image(_) => "<image>".to_owned(),
            })
            .collect()
    }

    #[test]
    fn operations_and_their_operands_across_streams() {
        let c = content(&[b"q 1 0 0 1 10 20", b" cm /F1 12 Tf [(a) -3 (b)] TJ Q"]);
        assert_eq!(names(&c), ["q", "cm", "Tf", "TJ", "Q"]);
        assert!(
            matches!(&c.items[1], Item::Op(cm) if cm.operands.len() == 6 && cm.at == At { stream: 1, offset: 1 }),
            "operands carried from the first stream"
        );
        assert_eq!(c.problems, []);
    }

    #[test]
    fn compatibility_sections() {
        let c = content(&[b"BX 1 2 foo EX bar EX BX"]);
        assert_eq!(names(&c), ["BX", "foo", "EX", "bar", "EX", "BX"]);
        assert!(matches!(&c.items[1], Item::Op(foo) if foo.compatible));
        assert_eq!(
            c.problems.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
            [
                Problem::UnknownOperator(b"bar".to_vec()),
                Problem::Compatibility,
                Problem::Compatibility
            ]
        );
    }

    #[test]
    fn inline_images_end_where_their_data_ends() {
        // 2 x 2 gray at 8 bits: 4 bytes, one of them "EI".
        let c = content(&[b"BI /W 2 /H 2 /BPC 8 /CS /G ID xEIx EI Q"]);
        assert_eq!(names(&c), ["<image>", "Q"]);
        assert!(
            matches!(&c.items[0], Item::Image(image) if (image.data, image.len) == (At { stream: 0, offset: 30 }, 4)
            && image.dict.get(b"Width").is_some() && image.dict.get(b"ColorSpace").is_some())
        );
        // An image mask at one bit: 9 pixels a row is 2 bytes, 2 rows.
        assert_eq!(names(&content(&[b"BI /IM true /W 9 /H 2 ID abcd EI"])), ["<image>"]);
        // Indexed, CMYK and RGB.
        assert_eq!(
            names(&content(&[
                b"BI /W 1 /H 1 /BPC 8 /CS [/I /RGB 1 <000000FFFFFF>] ID a EI"
            ])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /CMYK ID abcd EI"])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /DeviceRGB ID abc EI"])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /CS1 ID abc EI"])),
            ["<image>"],
            "a resource"
        );
        // Filtered: the filter's end-of-data marker; /L when given.
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G /F /AHx ID 41> EI"])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G /F [/AHx] ID 41> EI"])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G /F [] ID a EI"])),
            ["<image>"]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G /F /DCT /L 3 ID abc EI"])),
            ["<image>"]
        );
        // CR LF after ID, then data that is itself an end of line.
        let crlf = content(&[
            b"q BI /W 1 /H 1 /BPC 8 /CS /G ID\r\nx EI",
            b"BI /W 1 /H 1 /BPC 8 /CS /G ID\r\n EI",
        ]);
        let images: Vec<(At, usize)> = crlf
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Image(im) => Some((im.data, im.len)),
                Item::Op(_) => None,
            })
            .collect();
        // The second reads as the specified one byte after ID, the data a LF, since that also ends at EI.
        assert_eq!(
            images,
            [(At { stream: 0, offset: 33 }, 1), (At { stream: 1, offset: 30 }, 1)]
        );
        assert_eq!(
            names(&content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G ID aEIb EI"])),
            Vec::<String>::new(),
            "EI inside a word"
        );
        let lzw: &[u8] = b"BI /W 1 /H 1 /BPC 8 /CS /G /F /LZW /DP << /EarlyChange 0 >> ID \x80\x10\x60\x20 EI";
        assert_eq!(names(&content(&[lzw])), ["<image>"]);
    }

    #[test]
    fn fax_and_jpeg_inline_images() {
        let fax: &[u8] = b"BI /IM true /W 8 /H 8 /F /CCF /DP << /K -1 /Columns 8 /EndOfBlock false >> ID \xFF EI";
        assert_eq!(names(&content(&[fax])), ["<image>"]);
        let rows: &[u8] = b"BI /IM true /W 8 /H 99 /F [/CCF] /DP [<< /K -1 /Columns 8 /Rows 8 /EndOfBlock 5 >>] ID \xFF\x00\x10\x01 EI";
        assert_eq!(
            names(&content(&[rows])),
            ["<image>"],
            "Rows over Height; a flag that is not a boolean keeps its default"
        );
        let jpeg: &[u8] = b"BI /W 1 /H 1 /BPC 8 /CS /G /F /DCT ID \xFF\xD8\xFF\xD9 EI";
        assert_eq!(names(&content(&[jpeg])), ["<image>"]);
        let bad_rows: &[u8] = b"BI /IM true /W 8 /H 8 /F /CCF /DP << /Rows -1 >> ID \xFF EI";
        assert_eq!(names(&content(&[bad_rows])), Vec::<String>::new());
        let bad_columns: &[u8] = b"BI /IM true /W 8 /H 8 /F /CCF /DP << /Columns -1 >> ID \xFF EI";
        assert_eq!(names(&content(&[bad_columns])), Vec::<String>::new());
        let no_height: &[u8] = b"BI /IM true /W 8 /F /CCF ID \xFF EI";
        assert_eq!(names(&content(&[no_height])), Vec::<String>::new());
        let huge_rows: &[u8] = b"BI /IM true /W 8 /H 8 /F /CCF /DP << /Rows 9999999999 >> ID \xFF EI";
        assert_eq!(names(&content(&[huge_rows])), Vec::<String>::new());
    }

    #[test]
    fn inline_images_whose_end_is_unknown_are_gaps() {
        let problem = |data: &[u8]| content(&[data]).problems.first().map(|(_, p)| p.clone());
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /CS0 ID a EI"),
            Some(Problem::InlineEnd),
            "a named colour space"
        );
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /F /DCT ID a EI"),
            Some(Problem::InlineEnd),
            "an image filter"
        );
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS [/Separation] ID a EI"),
            Some(Problem::InlineEnd)
        );
        assert_eq!(problem(b"BI /W 1 /H 1 /BPC 8 /CS 5 ID a EI"), Some(Problem::InlineEnd));
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /F /Nope ID a EI"),
            Some(Problem::InlineEnd)
        );
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /F 5 ID a EI"),
            Some(Problem::InlineEnd)
        );
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /F [5] ID a EI"),
            Some(Problem::InlineEnd)
        );
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /L 2 ID a EI"),
            Some(Problem::InlineLength { given: 2, measured: 1 })
        );
        assert_eq!(problem(b"BI /W 1 /H 1 /BPC 8 /CS /G ID ab EI"), Some(Problem::NoEi));
        assert_eq!(problem(b"BI /W 1 /H 1 /BPC 8 /CS /G ID a"), Some(Problem::NoEi));
        assert_eq!(problem(b"BI (W) 1 ID a EI"), Some(Problem::InlineDict));
        assert_eq!(problem(b"BI /W ID a EI"), Some(Problem::InlineDict));
        assert_eq!(problem(b"BI /W 1 /H 1 /BPC 8 /CS /G"), Some(Problem::InlineDict));
        assert_eq!(
            problem(b"BI /W 1 /H 1 /BPC 8 /CS /G /F /DCT /L 9 ID a"),
            Some(Problem::NoEi),
            "data shorter than /L"
        );
        assert!(matches!(problem(b"BI /W ) ID"), Some(Problem::Syntax(_))));
        assert!(matches!(problem(b"BI ) ID"), Some(Problem::Syntax(_))));
        for missing in [
            b"BI /H 1 /BPC 8 /CS /G ID a EI".as_slice(),
            b"BI /W 1 /BPC 8 /CS /G ID a EI",
            b"BI /W 1 /H 1 /CS /G ID a EI",
            b"BI /W 1 /H 1 /BPC 8 ID a EI",
            b"BI /W 1 /H 1 /BPC 8 /CS [5] ID a EI",
            b"BI /W 9999999999999 /H 9999999999999 /BPC 8 /CS /G ID a EI",
            b"BI /W 99999999999999999 /H 1 /BPC 99 /CS /CMYK ID a EI",
            b"BI /W -1 /H 1 /BPC 8 /CS /G ID a EI",
            b"BI /W 4000000000000000000 /H 1 /BPC 8 /CS /G ID a EI",
        ] {
            assert_eq!(
                problem(missing),
                Some(Problem::InlineEnd),
                "{}",
                String::from_utf8_lossy(missing)
            );
        }
        let spelled = content(&[b"BI /W 1 /H 1 /BPC 8 /CS /G /D [1 0] /I true ID a EI"]);
        assert!(
            matches!(&spelled.items[0], Item::Image(i) if i.dict.get(b"Decode").is_some() && i.dict.get(b"Interpolate").is_some())
        );
        assert!(matches!(problem(b"BI /W 1 /H ) ID"), Some(Problem::Syntax(_))));
        let c = content(&[b"q BI /W 1 ID a EI", b"Q"]);
        assert_eq!(names(&c), ["q"], "nothing after the gap is read");
    }

    #[test]
    fn syntax_damage_stops_the_reading() {
        let c = content(&[b"q (unterminated", b"Q"]);
        assert_eq!(names(&c), ["q"]);
        assert!(matches!(
            c.problems.as_slice(),
            [(At { stream: 0, offset: 2 }, Problem::Syntax(_))]
        ));
        let split = content(&[b"[(a) ", b"(b)] TJ"]);
        assert_eq!(names(&split), ["TJ"], "an array may span two streams (7.7.3.3)");
        let noted = content(&[b"n", b"/A#zz n <<\n/K 1 /K 2>> BDC"]);
        assert_eq!(noted.notes.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [1, 1]);
        assert_eq!(content(&[]), Content::default());
    }

    #[test]
    fn limits_stop_the_reading() {
        let limits = Limits {
            saved_states: 2,
            operands: 3,
            ..Limits::default()
        };
        let within = |data: &[u8]| read(&[data], &limits, &|_| None);
        // A restored state frees its place; the third unrestored one passes the limit, however many follow.
        let flood = b"q Q q q ".repeat(1_000_000);
        let c = within(&flood);
        assert_eq!(names(&c), ["q", "Q", "q", "q"]);
        assert_eq!(c.problems, [(At { stream: 0, offset: 8 }, Problem::SavedStates)]);
        let c = within(b"1 2 3 n 1 2 3 4 n");
        assert_eq!(names(&c), ["n"]);
        assert_eq!(c.problems, [(At { stream: 0, offset: 14 }, Problem::Operands)]);
    }
}
