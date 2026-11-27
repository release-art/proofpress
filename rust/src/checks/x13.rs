//! Rules of PDF/X-1a and PDF/X-3 (ISO 15930-1, -3, -4, -6) on what tiers 1 to 5 read: the file's structure, filters,
//! image sample depth and optional content. These parts build on PDF 1.3 and 1.4: a feature added later that changes
//! what prints is judged by the clause that would be broken (L-Q2, L-Q4), never by the version in the header (clause 5).

use std::collections::BTreeSet;

use super::Settings;
use super::info::per_level;
use super::x4::stream_filters;
use crate::document::{Document, Object, Value};
use crate::findings::{
    Finding, Location, X1A_FILTER_LISTED_2001, X13_FILTER_LISTED, X13_FILTER_NO_JBIG2, X13_IMAGE_BIT_DEPTH,
    X13_OC_HIDES_PRINT, X13_OC_PRESENT, X13_STRUCTURE_XREF_TABLE,
};

/// The data-structure clause of each part (15930-1 6.2, the others 6.1): features used as the part's PDF Reference
/// prescribes, in a file of header, body, cross-reference table and trailer.
const STRUCTURE: [&str; 4] = ["6.2", "6.1", "6.1", "6.1"];

/// The data-compression clause of each part after 15930-1 (15930-3 6.4, -4 and -6 6.5).
const COMPRESSION: [&str; 4] = ["", "6.4", "6.5", "6.5"];

/// The filters the PDF 1.4 Reference defines, by their full names (abbreviations belong to inline images): the
/// PDF 1.3 ones and `JBIG2Decode`. `JPXDecode` and `Crypt` came in PDF 1.5.
const PDF14_FILTERS: [&[u8]; 8] = [
    b"ASCIIHexDecode",
    b"ASCII85Decode",
    b"LZWDecode",
    b"FlateDecode",
    b"RunLengthDecode",
    b"CCITTFaxDecode",
    b"JBIG2Decode",
    b"DCTDecode",
];

fn at(num: u32) -> Vec<Location> {
    vec![Location::Object(num, 0)]
}

/// L-Q2: no cross-reference stream and no object stream, which a reader of PDF 1.3 or 1.4 cannot find objects
/// through. A hybrid file's `/XRefStm` alone is not judged: its table lists every object outside an object stream, and
/// one inside an object stream fails here anyway.
pub(super) fn xref_table(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let trailer = doc.trailer();
    let mut uses = Vec::new();
    if trailer.get(b"Type").and_then(Object::name) == Some(b"XRef") {
        uses.push("a cross-reference stream");
    }
    let object_streams = doc
        .objects()
        .filter(|(_, object, ..)| object.dict().and_then(|d| d.get(b"Type")).and_then(Object::name) == Some(b"ObjStm"))
        .count();
    if object_streams > 0 {
        uses.push("object streams");
    }
    uses.into_iter()
        .map(|what| {
            per_level(
                Finding::new(X13_STRUCTURE_XREF_TABLE, format!("the file uses {what}")),
                settings,
                STRUCTURE,
            )
        })
        .collect()
}

/// The object numbers of every image `XObject` and thumbnail image: streams with `/Subtype /Image`, and the `/Thumb` of
/// each page.
fn images(doc: &Document) -> BTreeSet<u32> {
    let mut out: BTreeSet<u32> = doc
        .objects()
        .filter(|(_, object, stream, _)| {
            stream.is_some() && object.dict().and_then(|d| d.get(b"Subtype")).and_then(Object::name) == Some(b"Image")
        })
        .map(|(num, ..)| num)
        .collect();
    for page in doc.pages() {
        if let Some(Object {
            value: Value::Reference(num, _),
            ..
        }) = page.dict.get(b"Thumb")
        {
            out.insert(*num);
        }
    }
    out
}

/// An image's `/BitsPerComponent`, when it is a number.
fn bits(doc: &Document, num: u32) -> Option<i64> {
    let value = doc
        .stream_dict(num)?
        .get(b"BitsPerComponent")
        .and_then(|b| doc.resolve(b));
    match value.map(|v| &v.value) {
        Some(Value::Integer(n)) => Some(*n),
        _ => None,
    }
}

/// 15930-1 6.8 (L-Q18): a stream that is not an image is only Flate- or run-length-compressed; an image (an image `XObject`
/// or a thumbnail) may also be DCT-encoded, and CCITT-encoded at 1 bit per sample. The ASCII encodings compress nothing
/// and are allowed anywhere. Inline images are not judged here.
pub(super) fn listed_2001(doc: &Document, _: &Settings) -> Vec<Finding> {
    let images = images(doc);
    stream_filters(doc)
        .into_iter()
        .filter_map(|(num, name)| {
            let image = images.contains(&num);
            let anywhere = [
                b"FlateDecode".as_slice(),
                b"RunLengthDecode",
                b"ASCIIHexDecode",
                b"ASCII85Decode",
            ];
            let on_image = name == b"DCTDecode" || (name == b"CCITTFaxDecode" && bits(doc, num).is_none_or(|b| b == 1));
            let allowed = anywhere.contains(&name.as_slice()) || (image && on_image);
            (!allowed).then(|| {
                let what = if image { "image" } else { "stream" };
                let name = String::from_utf8_lossy(&name);
                Finding::new(
                    X1A_FILTER_LISTED_2001,
                    format!("object {num}, a{} {what}, uses /{name}", if image { "n" } else { "" }),
                )
                .citing("6.8")
                .at(at(num))
            })
        })
        .collect()
}

/// L-Q4: only filters the part's PDF Reference defines (`JBIG2Decode` is `x13.filter.no-jbig2`'s).
pub(super) fn listed(doc: &Document, settings: &Settings) -> Vec<Finding> {
    stream_filters(doc)
        .into_iter()
        .filter(|(_, name)| !PDF14_FILTERS.contains(&name.as_slice()))
        .map(|(num, name)| {
            let name = String::from_utf8_lossy(&name);
            let message =
                format!("object {num} uses the filter /{name}, which this level's PDF Reference does not define");
            per_level(
                Finding::new(X13_FILTER_LISTED, message).at(at(num)),
                settings,
                COMPRESSION,
            )
        })
        .collect()
}

/// 15930-4 and -6 6.5 ban `JBIG2Decode`; 15930-3 6.4 allows only PDF 1.3's filters, which it is not (L-Q4).
pub(super) fn no_jbig2(doc: &Document, settings: &Settings) -> Vec<Finding> {
    stream_filters(doc)
        .into_iter()
        .filter(|(_, name)| name == b"JBIG2Decode")
        .map(|(num, _)| {
            let finding = Finding::new(X13_FILTER_NO_JBIG2, format!("object {num} is JBIG2-encoded")).at(at(num));
            per_level(finding, settings, COMPRESSION)
        })
        .collect()
}

/// L-Q4: image samples of 1, 2, 4 or 8 bits, all PDF 1.3 and 1.4 define (16 came in PDF 1.5). Inline images are not
/// judged here.
pub(super) fn bit_depth(doc: &Document, settings: &Settings) -> Vec<Finding> {
    images(doc)
        .into_iter()
        .filter_map(|num| Some((num, bits(doc, num)?)))
        .filter(|(_, b)| !matches!(b, 1 | 2 | 4 | 8))
        .map(|(num, b)| {
            let finding = Finding::new(X13_IMAGE_BIT_DEPTH, format!("image {num} has {b} bits per sample")).at(at(num));
            per_level(finding, settings, STRUCTURE)
        })
        .collect()
}

/// Whether optional content can hide something from print: the default configuration turns a group off (`/BaseState
/// /OFF`, or groups in `/OFF`), sets groups by the print event (`/AS`), or a group's usage says not to print it.
fn hides_print(doc: &Document, properties: &crate::document::Dict) -> bool {
    let get = |dict: &crate::document::Dict, key: &[u8]| dict.get(key).and_then(|v| doc.resolve(v)).cloned();
    let default = get(properties, b"D");
    let default = default.as_ref().and_then(Object::dict);
    let off_base = default
        .and_then(|d| get(d, b"BaseState"))
        .is_some_and(|b| b.name() == Some(b"OFF"));
    let off_list = default
        .and_then(|d| get(d, b"OFF"))
        .is_some_and(|o| o.array().is_some_and(|a| !a.is_empty()));
    let automatic = default
        .and_then(|d| get(d, b"AS"))
        .is_some_and(|a| a.array().is_some_and(|a| !a.is_empty()));
    let not_printed = get(properties, b"OCGs").is_some_and(|groups| {
        groups.array().unwrap_or_default().iter().any(|g| {
            let group = doc.resolve(g).and_then(Object::dict);
            let print = group
                .and_then(|g| get(g, b"Usage"))
                .and_then(|u| u.dict().and_then(|u| get(u, b"Print")))
                .and_then(|p| p.dict().and_then(|p| get(p, b"PrintState")));
            print.is_some_and(|s| s.name() == Some(b"OFF"))
        })
    });
    off_base || off_list || automatic || not_printed
}

/// L-Q4: optional content (PDF 1.5) that can hide something from print fails, as a reader of
/// the part's PDF version prints everything; present but always on, it is a warning.
pub(super) fn optional_content(doc: &Document, settings: &Settings) -> (Vec<Finding>, Vec<Finding>) {
    let Some(properties) = doc
        .catalog()
        .and_then(|c| c.get(b"OCProperties"))
        .and_then(|p| doc.resolve(p))
        .and_then(Object::dict)
    else {
        return (Vec::new(), Vec::new());
    };
    if hides_print(doc, properties) {
        let message = "optional content can hide part of the page from print";
        (
            vec![per_level(
                Finding::new(X13_OC_HIDES_PRINT, message),
                settings,
                STRUCTURE,
            )],
            Vec::new(),
        )
    } else {
        let message = "the file has optional content, always on";
        (
            Vec::new(),
            vec![per_level(Finding::new(X13_OC_PRESENT, message), settings, STRUCTURE)],
        )
    }
}

/// `x13.oc.hides-print`: see [`optional_content`].
pub(super) fn oc_hides_print(doc: &Document, settings: &Settings) -> Vec<Finding> {
    optional_content(doc, settings).0
}

/// `x13.oc.present`: see [`optional_content`].
pub(super) fn oc_present(doc: &Document, settings: &Settings) -> Vec<Finding> {
    optional_content(doc, settings).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::{classic, object_stream, with_xref_stream};
    use crate::findings::Level;
    use crate::pipeline::Limits;

    fn messages(findings: Vec<Finding>) -> Vec<String> {
        findings.into_iter().map(|f| f.message).collect()
    }

    fn stream(dict: &str) -> Vec<u8> {
        format!("<< {dict} /Length 1 >>\nstream\nx\nendstream").into_bytes()
    }

    /// A file of a catalog with `catalog`'s entries, one page with `page`'s, and `more` objects.
    fn open(catalog: &str, page: &str, more: &[(u32, Vec<u8>)], trailer: &str) -> Document {
        let catalog = format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>");
        let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] {page} >>");
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, catalog.into_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (3, page.into_bytes()),
        ];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let trailer = format!("<< /Size SIZE /Root 1 0 R {trailer} >>");
        Document::open(&classic(&refs, &trailer), &Limits::default()).unwrap()
    }

    #[test]
    fn the_cross_reference_structure() {
        let x3 = Level::X32002.into();
        assert_eq!(messages(xref_table(&open("", "", &[], ""), &x3)), [""; 0]);
        let stm = object_stream(&[(2, "<< /A 1 >>")]);
        let packed = with_xref_stream(&[(1, b"<< /P 2 0 R >>"), (5, &stm)], &[(2, 5, 0)], 6, "/Root 1 0 R");
        let doc = Document::open(&packed, &Limits::default()).unwrap();
        assert_eq!(
            messages(xref_table(&doc, &x3)),
            ["the file uses a cross-reference stream", "the file uses object streams"]
        );
    }

    #[test]
    fn filters_at_x1a_2001() {
        let x1a = Level::X1a2001.into();
        let more = [
            (
                10,
                stream("/Subtype /Image /BitsPerComponent 8 /Filter /CCITTFaxDecode"),
            ),
            (
                11,
                stream("/Subtype /Image /BitsPerComponent 1 /Filter [/ASCII85Decode /CCITTFaxDecode]"),
            ),
            (12, stream("/Filter /DCTDecode")),
            (13, stream("/Filter /DCTDecode")),
            (
                14,
                stream("/Subtype /Image /BitsPerComponent /Eight /Filter /CCITTFaxDecode"),
            ),
        ];
        let xobjects =
            "/Thumb 13 0 R /Resources << /XObject << /A 10 0 R /B 11 0 R /C 12 0 R /D 14 0 R /E 15 0 R /F 16 0 R >> >>";
        let doc = open("", xobjects, &more, "");
        assert_eq!(
            messages(listed_2001(&doc, &x1a)),
            [
                "object 10, an image, uses /CCITTFaxDecode",
                "object 12, a stream, uses /DCTDecode"
            ]
        );
        // A thumbnail that names no object has no depth to judge.
        let refs = "/Thumb 99 0 R /Resources << /XObject << /A 10 0 R /D 14 0 R >> >>";
        let depth = open(
            "",
            refs,
            &[(10, stream("/Subtype /Image /BitsPerComponent 16")), more[4].clone()],
            "",
        );
        assert_eq!(messages(bit_depth(&depth, &x1a)), ["image 10 has 16 bits per sample"]);
        let refs = "/Resources << /XObject << /A 10 0 R /B 11 0 R >> >>";
        let listed_ok = open(
            "",
            refs,
            &[(10, stream("/Filter /JBIG2Decode")), (11, stream("/Filter /Fl"))],
            "",
        );
        assert_eq!(
            messages(no_jbig2(&listed_ok, &Level::X32002.into())),
            ["object 10 is JBIG2-encoded"]
        );
        assert_eq!(
            messages(listed(&listed_ok, &Level::X32003.into())),
            ["object 11 uses the filter /Fl, which this level's PDF Reference does not define"]
        );
    }

    #[test]
    fn optional_content_that_can_hide_print() {
        let x3 = Level::X32003.into();
        let hides = |props: &str, more: &[(u32, Vec<u8>)]| {
            let doc = open(&format!("/OCProperties {props}"), "", more, "");
            let (error, warning) = optional_content(&doc, &x3);
            (error.len(), warning.len())
        };
        assert_eq!(hides("<< /OCGs [] /D << /OFF [9 0 R] >> >>", &[]), (1, 0));
        assert_eq!(hides("<< /OCGs [] /D << /BaseState /OFF >> >>", &[]), (1, 0));
        assert_eq!(hides("<< /OCGs [] /D << /AS [<< /Event /Print >>] >> >>", &[]), (1, 0));
        let unprinted = (
            9,
            b"<< /Type /OCG /Usage << /Print << /PrintState /OFF >> >> >>".to_vec(),
        );
        assert_eq!(hides("<< /OCGs [9 0 R] /D << >> >>", &[unprinted]), (1, 0));
        let printed = (
            9,
            b"<< /Type /OCG /Usage << /Print << /PrintState /ON >> >> >>".to_vec(),
        );
        assert_eq!(hides("<< /OCGs [9 0 R] /D << /OFF [] /AS [] >> >>", &[printed]), (0, 1));
        assert_eq!(hides("<< /D 5 >>", &[]), (0, 1));
        let doc = open("", "", &[], "");
        assert_eq!((oc_hides_print(&doc, &x3), oc_present(&doc, &x3)), (vec![], vec![]));
    }
}
