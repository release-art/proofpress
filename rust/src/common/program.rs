//! An embedded font program (ISO 32000-1 9.9): where a font descriptor keeps it and what format it is in.

use crate::document::{Dict, Document, Object, Value};

/// The formats of Table 126.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// `/FontFile`: Type 1, cleartext and `eexec` portions.
    Type1,
    /// `/FontFile2`, or `/FontFile3` of subtype `OpenType`: a TrueType or OpenType table directory.
    Sfnt,
    /// `/FontFile3` of subtype `Type1C` or `CIDFontType0C`: a bare CFF font.
    Cff,
}

/// The descriptor's font program, its format and data; `None` when there is none, its stream cannot be decoded, or a
/// `/FontFile3` has a subtype Table 126 does not list.
pub(crate) fn of(doc: &Document, descriptor: &Dict) -> Option<(Format, Vec<u8>)> {
    let stream = |key: &[u8]| match descriptor.get(key)?.value {
        Value::Reference(num, _) => Some(num),
        _ => None,
    };
    let (format, num) = if let Some(num) = stream(b"FontFile") {
        (Format::Type1, num)
    } else if let Some(num) = stream(b"FontFile2") {
        (Format::Sfnt, num)
    } else {
        let num = stream(b"FontFile3")?;
        let subtype = descriptor
            .get(b"FontFile3")
            .and_then(|f| doc.resolve(f))
            .and_then(Object::dict)
            .and_then(|d| d.get(b"Subtype"))
            .and_then(Object::name);
        let format = match subtype? {
            b"OpenType" => Format::Sfnt,
            b"Type1C" | b"CIDFontType0C" => Format::Cff,
            _ => return None,
        };
        (format, num)
    };
    Some((format, doc.stream_data(num)?))
}
