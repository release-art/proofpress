//! PDF/X-4 (ISO 15930-7) checks on what tiers 1 to 5 read: filters, names, permissions and object counts.

use crate::document::{Buffer, Document, Object, Value};
use crate::findings::{
    Finding, Location, X4_FILTER_NO_CRYPT, X4_FILTER_NO_LZW, X4_FILTER_STANDARD_ONLY, X4_LIMITS_OBJECTS,
    X4_NAMES_HEX_ESCAPED, X4_NAMES_UTF8, X4_PERMS_UR_ONLY,
};

/// The filters of PDF 1.6 Reference Table 3.5, the only ones 6.8 allows, by their full names.
const STANDARD_FILTERS: &[&[u8]] = &[
    b"ASCIIHexDecode",
    b"ASCII85Decode",
    b"LZWDecode",
    b"FlateDecode",
    b"RunLengthDecode",
    b"CCITTFaxDecode",
    b"JBIG2Decode",
    b"DCTDecode",
    b"JPXDecode",
    b"Crypt",
];

/// Table C.1 of the PDF 1.6 Reference: indirect objects in a file.
const MAX_OBJECTS: usize = 8_388_607;

/// Every stream's filter names, with the stream's object number.
fn stream_filters(doc: &Document) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    for (num, object, stream, _) in doc.objects() {
        let (Some(dict), Some(_)) = (object.dict(), stream) else {
            continue;
        };
        let names = match dict.get(b"Filter").and_then(|f| doc.resolve(f)) {
            Some(Object {
                value: Value::Array(items),
                ..
            }) => items.iter().filter_map(|i| doc.resolve(i)).collect(),
            Some(single) => vec![single],
            None => Vec::new(),
        };
        out.extend(names.into_iter().filter_map(Object::name).map(|n| (num, n.to_vec())));
    }
    out
}

fn at(num: u32) -> Vec<Location> {
    vec![Location::Object(num, 0)]
}

/// 6.8: no stream uses `LZWDecode`.
pub(super) fn no_lzw(doc: &Document) -> Vec<Finding> {
    stream_filters(doc)
        .into_iter()
        .filter(|(_, name)| name == b"LZWDecode" || name == b"LZW")
        .map(|(num, _)| {
            Finding::new(X4_FILTER_NO_LZW, format!("object {num} is LZW-encoded"))
                .citing("6.8")
                .at(at(num))
        })
        .collect()
}

/// 6.8: no stream uses the `Crypt` filter.
pub(super) fn no_crypt(doc: &Document) -> Vec<Finding> {
    stream_filters(doc)
        .into_iter()
        .filter(|(_, name)| name == b"Crypt")
        .map(|(num, _)| {
            Finding::new(X4_FILTER_NO_CRYPT, format!("object {num} uses the Crypt filter"))
                .citing("6.8")
                .at(at(num))
        })
        .collect()
}

/// 6.8: every filter is one of PDF 1.6's, by its full name (abbreviations belong to inline images only).
pub(super) fn standard_only(doc: &Document) -> Vec<Finding> {
    stream_filters(doc)
        .into_iter()
        .filter(|(_, name)| !STANDARD_FILTERS.contains(&name.as_slice()))
        .map(|(num, name)| {
            let name = String::from_utf8_lossy(&name);
            Finding::new(
                X4_FILTER_STANDARD_ONLY,
                format!("object {num} uses the filter /{name}, which PDF 1.6 does not define"),
            )
            .citing("6.8")
            .at(at(num))
        })
        .collect()
}

/// 6.15: the catalog's `/Perms` holds only usage-rights entries.
pub(super) fn ur_only(doc: &Document) -> Vec<Finding> {
    let perms = doc
        .catalog()
        .and_then(|c| c.get(b"Perms"))
        .and_then(|p| doc.resolve(p))
        .and_then(Object::dict);
    perms
        .into_iter()
        .flat_map(|p| p.entries().iter())
        .filter(|(key, _)| key != b"UR" && key != b"UR3")
        .map(|(key, _)| {
            let key = String::from_utf8_lossy(key);
            Finding::new(
                X4_PERMS_UR_ONLY,
                format!("/Perms holds /{key}; only /UR and /UR3 are allowed"),
            )
            .citing("6.15")
        })
        .collect()
}

/// 6.25: no more indirect objects than Table C.1 allows.
pub(super) fn object_count(doc: &Document) -> Vec<Finding> {
    let count = doc.listed_objects();
    (count > MAX_OBJECTS)
        .then(|| {
            Finding::new(
                X4_LIMITS_OBJECTS,
                format!("{count} indirect objects; PDF 1.6 allows {MAX_OBJECTS}"),
            )
            .citing("6.25")
        })
        .into_iter()
        .collect()
}

/// A font or separation name as it was written: its object, decoded bytes and raw spelling after the `/`.
struct Named<'a> {
    num: u32,
    role: &'static str,
    bytes: &'a [u8],
    raw: &'a [u8],
}

/// Every font and separation name in the file (6.6): `/BaseFont` and `/FontName`, the colorant of a `Separation`
/// space, and each colorant of a `DeviceN` space.
fn names(doc: &Document) -> Vec<Named<'_>> {
    let mut out = Vec::new();
    for (num, object, _, buffer) in doc.objects() {
        collect(doc, num, buffer, object, &mut out);
    }
    out
}

fn collect<'a>(doc: &'a Document, num: u32, buffer: Buffer, object: &'a Object, out: &mut Vec<Named<'a>>) {
    let named = |role, name: &'a Object| {
        let raw = doc.raw(buffer, name.start + 1, name.end);
        name.name().map(|bytes| Named { num, role, bytes, raw })
    };
    match &object.value {
        Value::Dict(dict) => {
            let font = match dict.get(b"Type").and_then(Object::name) {
                Some(b"Font") => dict.get(b"BaseFont"),
                Some(b"FontDescriptor") => dict.get(b"FontName"),
                _ => None,
            };
            out.extend(font.and_then(|n| named("font", n)));
            for (_, value) in dict.entries() {
                collect(doc, num, buffer, value, out);
            }
        }
        Value::Array(items) => {
            match (items.first().and_then(Object::name), items.get(1)) {
                (Some(b"Separation"), Some(name)) => out.extend(named("separation", name)),
                (
                    Some(b"DeviceN"),
                    Some(Object {
                        value: Value::Array(colorants),
                        ..
                    }),
                ) => {
                    out.extend(colorants.iter().filter_map(|n| named("colorant", n)));
                }
                _ => {}
            }
            for item in items {
                collect(doc, num, buffer, item, out);
            }
        }
        _ => {}
    }
}

/// 6.6: font and separation names are UTF-8 once their escapes are decoded.
pub(super) fn names_utf8(doc: &Document) -> Vec<Finding> {
    names(doc)
        .into_iter()
        .filter(|n| std::str::from_utf8(n.bytes).is_err())
        .map(|n| {
            let raw = String::from_utf8_lossy(n.raw);
            Finding::new(
                X4_NAMES_UTF8,
                format!("the {} name /{raw} in object {} is not UTF-8", n.role, n.num),
            )
            .citing("6.6")
            .at(at(n.num))
        })
        .collect()
}

/// 6.6: in font and separation names, every byte outside 33 to 126 is written as `#xx`.
pub(super) fn names_escaped(doc: &Document) -> Vec<Finding> {
    names(doc)
        .into_iter()
        .filter(|n| n.raw.iter().any(|b| !(33..=126).contains(b)))
        .map(|n| {
            let raw = String::from_utf8_lossy(n.raw);
            Finding::new(
                X4_NAMES_HEX_ESCAPED,
                format!(
                    "the {} name /{raw} in object {} has bytes not written as #xx",
                    n.role, n.num
                ),
            )
            .citing("6.6")
            .at(at(n.num))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::{classic, object_stream, with_xref_stream};
    use crate::pipeline::Limits;

    fn open(objects: &[(u32, &[u8])]) -> Document {
        Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    fn ids(findings: &[Finding]) -> Vec<&'static str> {
        findings.iter().map(|f| f.check.id).collect()
    }

    #[test]
    fn filters_given_as_arrays_and_by_reference() {
        let doc = open(&[
            (1, b"<< /S 2 0 R >>"),
            (2, b"<< /Filter [/LZWDecode 3 0 R] /Length 1 >>\nstream\nx\nendstream"),
            (3, b"/Crypt"),
        ]);
        assert_eq!(ids(&no_lzw(&doc)), ["x4.filter.no-lzw"]);
        assert_eq!(ids(&no_crypt(&doc)), ["x4.filter.no-crypt"]);
        assert_eq!(ids(&standard_only(&doc)), Vec::<&str>::new());
    }

    #[test]
    fn separation_and_devicen_names() {
        let doc = open(&[(1, b"<< /A [/Separation /Gold#FF /DeviceCMYK 2 0 R] /B [/DeviceN [/Cyan /Silv\xc3\xa9r] /DeviceCMYK 2 0 R] >>"), (2, b"<< >>")]);
        assert_eq!(ids(&names_utf8(&doc)), ["x4.names.utf8"]);
        assert_eq!(ids(&names_escaped(&doc)), ["x4.names.hex-escaped"]);
    }

    #[test]
    fn names_inside_an_object_stream_are_read_from_its_data() {
        let stm = object_stream(&[(2, "<< /Type /Font /BaseFont /Caf\u{e9} >>")]);
        let buf = with_xref_stream(&[(1, b"<< /F 2 0 R >>"), (5, &stm)], &[(2, 5, 0)], 6, "/Root 1 0 R");
        let doc = Document::open(&buf, &Limits::default()).unwrap();
        assert_eq!(ids(&names_escaped(&doc)), ["x4.names.hex-escaped"]);
        assert_eq!(doc.raw(Buffer::ObjectStream(9), 0, 1), b"");
    }

    #[test]
    fn permissions_that_cannot_be_read_hold_nothing() {
        assert_eq!(ids(&ur_only(&open(&[(1, b"<< /Perms 9 0 R >>")]))), Vec::<&str>::new());
        assert_eq!(
            ids(&ur_only(&open(&[(1, b"<< /Perms << /UR3 1 /DocMDP 2 >> >>")]))),
            ["x4.perms.ur-only"]
        );
    }

    #[test]
    fn a_damaged_document_lists_only_what_it_read() {
        let doc = open(&[(1, b"<< /A 2 0 R >>"), (2, b"[1")]);
        assert_eq!(doc.objects().count(), 1);
    }
}
