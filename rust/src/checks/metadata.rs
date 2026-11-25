//! PDF/X-4 (ISO 15930-7) metadata checks: trapping (6.9), the XMP packets and the document information dictionary
//! (6.10) and the version identification (6.11).
//!
//! The rows on the document's XMP read the catalog's metadata stream. When there is none, or it cannot be parsed, every
//! property they require is missing and each says so; `x4.xmp.present` and `x4.xmp.well-formed` give the cause.

use std::collections::BTreeSet;

use crate::common::xmp::{self, Kind, Packet, Value as Xmp};
use crate::common::{date, text};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Level, Location, X4_IDENTITY_NO_INFO_CONFORMANCE, X4_IDENTITY_PDFXID_ONLY_VERSION,
    X4_IDENTITY_VERSION_PRESENT, X4_IDENTITY_VERSION_VALUE, X4_INFO_AUTHOR_SINGLE_CREATOR, X4_INFO_PDFX_VERSION_PDFDOC,
    X4_INFO_XMP_ANALOGUE_EQUAL, X4_INFO_XMP_ANALOGUE_PRESENT, X4_TRAILER_ID, X4_TRAPPED_PRESENT,
    X4_TRAPPED_TRUE_OR_FALSE, X4_XMP_AGENT_NAMES, X4_XMP_CREATE_DATE, X4_XMP_DOCUMENT_ID, X4_XMP_DOCUMENT_ID_UUID,
    X4_XMP_METADATA_DATE, X4_XMP_MODIFY_DATE, X4_XMP_NAMESPACE_PREFIXES, X4_XMP_NO_BYTES_ENCODING_ATTRS,
    X4_XMP_PRESENT, X4_XMP_RENDITION_CLASS, X4_XMP_TITLE, X4_XMP_UNFILTERED, X4_XMP_VERSION_ID, X4_XMP_WELL_FORMED,
};

const DC: &str = "http://purl.org/dc/elements/1.1/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const MM: &str = "http://ns.adobe.com/xap/1.0/mm/";
const PDFXID: &str = "http://www.npes.org/pdfx/ns/id/";

/// Table 2 of 6.10.1: the prefix each namespace must be written with.
const PREFIXES: [(&str, &str); 5] = [
    (DC, "dc"),
    (PDF, "pdf"),
    (XMP, "xmp"),
    (MM, "xmpMM"),
    (PDFXID, "pdfxid"),
];

/// How an Info entry and its XMP analogue compare.
#[derive(Clone, Copy)]
enum Compare {
    Text,
    Date,
    Name,
}

/// Table 3 of 6.10.2: an Info key, its XMP property, which item of it, and how the values compare.
const CROSSWALK: [(&str, &str, &str, Item, Compare); 10] = [
    ("Title", DC, "title", Item::Default, Compare::Text),
    ("Author", DC, "creator", Item::First, Compare::Text),
    ("Subject", DC, "description", Item::Default, Compare::Text),
    ("Keywords", PDF, "Keywords", Item::Simple, Compare::Text),
    ("Creator", XMP, "CreatorTool", Item::Simple, Compare::Text),
    ("Producer", PDF, "Producer", Item::Simple, Compare::Text),
    ("CreationDate", XMP, "CreateDate", Item::Simple, Compare::Date),
    ("ModDate", XMP, "ModifyDate", Item::Simple, Compare::Date),
    ("Trapped", PDF, "Trapped", Item::Simple, Compare::Name),
    (
        "GTS_PDFXVersion",
        PDFXID,
        "GTS_PDFXVersion",
        Item::Simple,
        Compare::Text,
    ),
];

/// Which part of an XMP value Table 3 maps an Info entry to.
#[derive(Clone, Copy)]
enum Item {
    /// The whole value, simple text.
    Simple,
    /// A language alternative's `x-default` item.
    Default,
    /// An array's first item.
    First,
}

fn prefix(ns: &str) -> &'static str {
    PREFIXES.iter().find(|(n, _)| *n == ns).map_or("", |(_, p)| p)
}

fn at(num: u32) -> Vec<Location> {
    vec![Location::Object(num, 0)]
}

/// The number of every metadata stream: what any `/Metadata` entry refers to, and every stream of `/Type /Metadata`.
fn metadata_streams(doc: &Document) -> BTreeSet<u32> {
    fn walk(object: &Object, out: &mut BTreeSet<u32>) {
        match &object.value {
            Value::Dict(dict) => {
                for (key, value) in &dict.entries {
                    if let (b"Metadata", Value::Reference(num, _)) = (key.as_slice(), &value.value) {
                        out.insert(*num);
                    }
                    walk(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut named = BTreeSet::new();
    let mut streams = BTreeSet::new();
    for (num, object, stream, _) in doc.objects() {
        walk(object, &mut named);
        if stream.is_some() {
            streams.insert(num);
            if object.dict().and_then(|d| d.get(b"Type")).and_then(Object::name) == Some(b"Metadata") {
                named.insert(num);
            }
        }
    }
    named.intersection(&streams).copied().collect()
}

/// The catalog's metadata stream: its number.
fn document_metadata(doc: &Document) -> Option<u32> {
    match doc.catalog()?.get(b"Metadata")?.value {
        Value::Reference(num, _) => metadata_streams(doc).contains(&num).then_some(num),
        _ => None,
    }
}

/// The document's XMP, parsed, when there is a catalog metadata stream that parses.
fn document_xmp(doc: &Document) -> Option<Packet> {
    xmp::parse(&doc.stream_data(document_metadata(doc)?)?).ok()
}

/// The document information dictionary.
fn info(doc: &Document) -> Option<&Dict> {
    doc.resolve(doc.trailer().get(b"Info")?)?.dict()
}

/// A string's bytes, resolving a reference.
fn string<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a [u8]> {
    match &doc.resolve(object)?.value {
        Value::String(bytes, _) => Some(bytes),
        _ => None,
    }
}

/// 6.10.1: the catalog has a metadata stream.
pub(super) fn present(doc: &Document, _: Level) -> Vec<Finding> {
    match document_metadata(doc) {
        Some(_) => Vec::new(),
        None => vec![Finding::new(X4_XMP_PRESENT, "the catalog has no metadata stream").citing("6.10.1")],
    }
}

/// 6.10.1 (X4-Q8: every metadata stream, not only the document's): no metadata stream has a `/Filter`.
pub(super) fn unfiltered(doc: &Document, _: Level) -> Vec<Finding> {
    let streams = metadata_streams(doc);
    doc.objects()
        .filter(|(num, ..)| streams.contains(num))
        .filter(|(_, object, ..)| object.dict().and_then(|d| d.get(b"Filter")).is_some())
        .map(|(num, ..)| {
            Finding::new(X4_XMP_UNFILTERED, format!("metadata stream {num} has a /Filter"))
                .citing("6.10.1")
                .at(at(num))
        })
        .collect()
}

/// 6.10.6: every metadata stream is well-formed XML and XMP's form of RDF.
pub(super) fn well_formed(doc: &Document, _: Level) -> Vec<Finding> {
    let mut out = Vec::new();
    for num in metadata_streams(doc) {
        let Some(data) = doc.stream_data(num) else { continue };
        let why = match xmp::parse(&data) {
            Ok(_) => continue,
            Err(xmp::Error::Encoding) => "is not UTF-8, UTF-16 or UTF-32 text".to_owned(),
            Err(xmp::Error::Xml(e)) => format!("is not well-formed XML: {e}"),
            Err(xmp::Error::Rdf(e)) => format!("is not XMP's RDF: {e}"),
        };
        out.push(
            Finding::new(X4_XMP_WELL_FORMED, format!("metadata stream {num} {why}"))
                .citing("6.10.6")
                .at(at(num)),
        );
    }
    out
}

/// 6.10.3: no packet header has a `bytes` or `encoding` attribute.
pub(super) fn header_attributes(doc: &Document, _: Level) -> Vec<Finding> {
    let mut out = Vec::new();
    for num in metadata_streams(doc) {
        let Some(data) = doc.stream_data(num) else { continue };
        for name in xmp::header_attributes(&data).into_iter().flatten() {
            if name == "bytes" || name == "encoding" {
                out.push(
                    Finding::new(
                        X4_XMP_NO_BYTES_ENCODING_ATTRS,
                        format!("the packet header in metadata stream {num} has a {name} attribute"),
                    )
                    .citing("6.10.3")
                    .at(at(num)),
                );
            }
        }
    }
    out
}

/// 6.10.1 and 6.11: the namespaces of Table 2 are written with its prefixes.
pub(super) fn prefixes(doc: &Document, _: Level) -> Vec<Finding> {
    let Some(packet) = document_xmp(doc) else {
        return Vec::new();
    };
    packet
        .prefixes
        .iter()
        .filter(|(p, ns)| PREFIXES.iter().any(|(n, want)| n == ns && p != want))
        .map(|(p, ns)| {
            Finding::new(
                X4_XMP_NAMESPACE_PREFIXES,
                format!("the namespace {ns} is written with the prefix {p}, not {}", prefix(ns)),
            )
            .citing("6.10.1")
        })
        .collect()
}

/// A finding from `check` unless the document's XMP has the property, with text that `valid` accepts.
fn require(
    doc: &Document,
    check: &'static Check,
    ns: &str,
    name: &str,
    clause: &str,
    valid: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let packet = document_xmp(doc);
    let value = packet.as_ref().and_then(|p| p.get(ns, name));
    let problem = match value.map(|v| v.text().map(valid)) {
        None => "has no",
        Some(Some(true)) => return Vec::new(),
        Some(_) => "has an invalid",
    };
    let property = format!("{}:{name}", prefix(ns));
    vec![Finding::new(check, format!("the document's XMP {problem} {property}")).citing(clause)]
}

fn any(_: &str) -> bool {
    true
}

fn non_empty(text: &str) -> bool {
    !text.trim().is_empty()
}

/// X4-Q7: a date must be there and parse.
fn valid_date(text: &str) -> bool {
    date::parse_xmp(text).is_some()
}

pub(super) fn document_id(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_DOCUMENT_ID, MM, "DocumentID", "6.10.4", &any)
}

pub(super) fn version_id(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_VERSION_ID, MM, "VersionID", "6.10.4", &any)
}

pub(super) fn rendition_class(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_RENDITION_CLASS, MM, "RenditionClass", "6.10.4", &any)
}

/// 6.10.4 recommends a `uuid:` URI of 128 bits for the document ID; a missing one is `x4.xmp.document-id`'s.
pub(super) fn document_id_uuid(doc: &Document, _: Level) -> Vec<Finding> {
    let Some(id) = document_xmp(doc).and_then(|p| p.get(MM, "DocumentID").and_then(Xmp::text).map(str::to_owned))
    else {
        return Vec::new();
    };
    let hex: String = id
        .strip_prefix("uuid:")
        .unwrap_or_default()
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if id.starts_with("uuid:") && hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Vec::new();
    }
    vec![
        Finding::new(
            X4_XMP_DOCUMENT_ID_UUID,
            format!("the document ID {id:?} is not a uuid: URI of 128 bits"),
        )
        .citing("6.10.4"),
    ]
}

pub(super) fn create_date(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_CREATE_DATE, XMP, "CreateDate", "6.10.4", &valid_date)
}

pub(super) fn modify_date(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_MODIFY_DATE, XMP, "ModifyDate", "6.10.4", &valid_date)
}

pub(super) fn metadata_date(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_METADATA_DATE, XMP, "MetadataDate", "6.10.4", &valid_date)
}

pub(super) fn title(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_XMP_TITLE, DC, "title", "6.10.4", &non_empty)
}

/// 6.10.4 recommends both `xmp:CreatorTool` and `pdf:Producer`.
pub(super) fn agent_names(doc: &Document, _: Level) -> Vec<Finding> {
    let mut out = require(doc, X4_XMP_AGENT_NAMES, XMP, "CreatorTool", "6.10.4", &any);
    out.extend(require(doc, X4_XMP_AGENT_NAMES, PDF, "Producer", "6.10.4", &any));
    out
}

pub(super) fn version_present(doc: &Document, _: Level) -> Vec<Finding> {
    require(
        doc,
        X4_IDENTITY_VERSION_PRESENT,
        PDFXID,
        "GTS_PDFXVersion",
        "6.11",
        &any,
    )
}

/// 6.11 and A.3 (X4-Q10): the version is exactly `PDF/X-4`, or `PDF/X-4p` for X-4p; a missing one is
/// `x4.identity.version-present`'s.
pub(super) fn version_value(doc: &Document, level: Level) -> Vec<Finding> {
    let want = if level == Level::X4p { "PDF/X-4p" } else { "PDF/X-4" };
    let packet = document_xmp(doc);
    let Some(value) = packet.as_ref().and_then(|p| p.get(PDFXID, "GTS_PDFXVersion")) else {
        return Vec::new();
    };
    match value.text() {
        Some(v) if v == want => Vec::new(),
        other => vec![
            Finding::new(
                X4_IDENTITY_VERSION_VALUE,
                format!(
                    "pdfxid:GTS_PDFXVersion is {:?}, not {want:?}",
                    other.unwrap_or("not text")
                ),
            )
            .citing("6.11"),
        ],
    }
}

/// 6.11: nothing but the version is in the `pdfxid` namespace.
pub(super) fn pdfxid_only_version(doc: &Document, _: Level) -> Vec<Finding> {
    let Some(packet) = document_xmp(doc) else {
        return Vec::new();
    };
    packet
        .properties
        .iter()
        .filter(|p| p.ns == PDFXID && p.name != "GTS_PDFXVersion")
        .map(|p| {
            Finding::new(
                X4_IDENTITY_PDFXID_ONLY_VERSION,
                format!("pdfxid:{} is not allowed", p.name),
            )
            .citing("6.11")
        })
        .collect()
}

/// 6.11: Info has no `/GTS_PDFXConformance`.
pub(super) fn no_info_conformance(doc: &Document, _: Level) -> Vec<Finding> {
    match info(doc).and_then(|i| i.get(b"GTS_PDFXConformance")) {
        Some(_) => vec![
            Finding::new(
                X4_IDENTITY_NO_INFO_CONFORMANCE,
                "the Info dictionary has /GTS_PDFXConformance",
            )
            .citing("6.11"),
        ],
        None => Vec::new(),
    }
}

/// 6.9 (X4-Q1): the trapped state is in XMP.
pub(super) fn trapped_present(doc: &Document, _: Level) -> Vec<Finding> {
    require(doc, X4_TRAPPED_PRESENT, PDF, "Trapped", "6.9", &any)
}

/// 6.9: the trapped state is `True` or `False`; a missing one is `x4.trapped.present`'s.
pub(super) fn trapped_true_or_false(doc: &Document, _: Level) -> Vec<Finding> {
    let packet = document_xmp(doc);
    let Some(value) = packet.as_ref().and_then(|p| p.get(PDF, "Trapped")) else {
        return Vec::new();
    };
    match value.text() {
        Some("True" | "False") => Vec::new(),
        other => vec![
            Finding::new(
                X4_TRAPPED_TRUE_OR_FALSE,
                format!("pdf:Trapped is {:?}, not True or False", other.unwrap_or("not text")),
            )
            .citing("6.9"),
        ],
    }
}

/// 6.10.4: the trailer has an `/ID` of two strings.
pub(super) fn trailer_id(doc: &Document, _: Level) -> Vec<Finding> {
    let id = doc
        .trailer()
        .get(b"ID")
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array);
    match id {
        Some([a, b]) if string(doc, a).is_some() && string(doc, b).is_some() => Vec::new(),
        _ => vec![Finding::new(X4_TRAILER_ID, "the trailer has no /ID of two strings").citing("6.10.4")],
    }
}

/// 6.10.2: Info `/GTS_PDFXVersion` is not UTF-16.
pub(super) fn info_version_pdfdoc(doc: &Document, _: Level) -> Vec<Finding> {
    match info(doc)
        .and_then(|i| i.get(b"GTS_PDFXVersion"))
        .and_then(|v| string(doc, v))
    {
        Some(bytes) if text::is_utf16(bytes) => vec![
            Finding::new(
                X4_INFO_PDFX_VERSION_PDFDOC,
                "Info /GTS_PDFXVersion is UTF-16, not PDFDocEncoding",
            )
            .citing("6.10.2"),
        ],
        _ => Vec::new(),
    }
}

/// The XMP item Table 3 maps to, as text.
fn item(value: &Xmp, which: Item) -> Option<&str> {
    match (which, value) {
        (Item::Simple, Xmp::Text(t)) => Some(t),
        (Item::Default, Xmp::Array(Kind::Alt, items)) => items
            .iter()
            .find(|(lang, _)| lang.as_deref() == Some("x-default"))
            .and_then(|(_, v)| v.text()),
        (Item::First, Xmp::Array(_, items)) => items.first().and_then(|(_, v)| v.text()),
        _ => None,
    }
}

/// Every Info entry of Table 3 with its XMP analogue's text, `None` when XMP lacks it.
fn crosswalk(doc: &Document) -> Vec<(&'static str, String, Compare, &Object, Option<String>)> {
    let Some(info) = info(doc) else { return Vec::new() };
    let packet = document_xmp(doc);
    CROSSWALK
        .iter()
        .filter_map(|&(key, ns, name, which, compare)| {
            let entry = info.get(key.as_bytes())?;
            let xmp = packet
                .as_ref()
                .and_then(|p| p.get(ns, name))
                .and_then(|v| item(v, which))
                .map(str::to_owned);
            Some((key, format!("{}:{name}", prefix(ns)), compare, entry, xmp))
        })
        .collect()
}

/// 6.10.2: every Info entry of Table 3 has its analogue in XMP.
pub(super) fn analogue_present(doc: &Document, _: Level) -> Vec<Finding> {
    crosswalk(doc)
        .into_iter()
        .filter(|(.., xmp)| xmp.is_none())
        .map(|(key, property, ..)| {
            Finding::new(
                X4_INFO_XMP_ANALOGUE_PRESENT,
                format!("Info /{key} has no {property} in XMP"),
            )
            .citing("6.10.2")
        })
        .collect()
}

/// 6.10.2: Info entries equal their analogues, text by code point, dates as instants in UTC, the trapped name by name.
pub(super) fn analogue_equal(doc: &Document, _: Level) -> Vec<Finding> {
    let mut out = Vec::new();
    for (key, property, compare, entry, xmp) in crosswalk(doc) {
        let Some(xmp) = xmp else { continue };
        let info_text = match compare {
            Compare::Name => doc
                .resolve(entry)
                .and_then(Object::name)
                .map(|n| String::from_utf8_lossy(n).into_owned()),
            Compare::Text | Compare::Date => string(doc, entry).and_then(text::decode),
        };
        let equal = info_text.as_deref().is_some_and(|info| match compare {
            Compare::Text | Compare::Name => info == xmp,
            Compare::Date => match (date::parse_pdf(info), date::parse_xmp(&xmp)) {
                (Some(a), Some(b)) => a.same_instant(&b),
                _ => false,
            },
        });
        if !equal {
            let shown = info_text.unwrap_or_else(|| "unreadable".to_owned());
            out.push(
                Finding::new(
                    X4_INFO_XMP_ANALOGUE_EQUAL,
                    format!("Info /{key} is {shown:?} but {property} is {xmp:?}"),
                )
                .citing("6.10.2"),
            );
        }
    }
    out
}

/// 6.10.2 (X4-Q6): with Info `/Author`, `dc:creator` is an ordered array of one item.
pub(super) fn author_single_creator(doc: &Document, _: Level) -> Vec<Finding> {
    let packet = document_xmp(doc);
    let creator = packet.as_ref().and_then(|p| p.get(DC, "creator"));
    match (info(doc).and_then(|i| i.get(b"Author")), creator) {
        (Some(_), Some(Xmp::Array(Kind::Seq, items))) if items.len() == 1 => Vec::new(),
        (Some(_), Some(_)) => vec![
            Finding::new(
                X4_INFO_AUTHOR_SINGLE_CREATOR,
                "Info has /Author but dc:creator is not a one-item rdf:Seq",
            )
            .citing("6.10.2"),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, Level) -> Vec<Finding>; 25] = [
        present,
        unfiltered,
        well_formed,
        header_attributes,
        prefixes,
        document_id,
        version_id,
        rendition_class,
        document_id_uuid,
        create_date,
        modify_date,
        metadata_date,
        title,
        agent_names,
        version_present,
        version_value,
        pdfxid_only_version,
        no_info_conformance,
        trapped_present,
        trapped_true_or_false,
        trailer_id,
        info_version_pdfdoc,
        analogue_present,
        analogue_equal,
        author_single_creator,
    ];

    /// A file whose catalog `/Metadata` is `metadata` (object 2 holds `xmp` as a stream), with Info `info`.
    fn open(metadata: &str, xmp: &[u8], info: &str, id: &str) -> Document {
        let mut stream = format!("<< /Type /Metadata /Length {} >>\nstream\n", xmp.len()).into_bytes();
        stream.extend(xmp);
        stream.extend(b"\nendstream");
        let catalog = format!("<< /Metadata {metadata} >>");
        let trailer = format!("<< /Size SIZE /Root 1 0 R /Info 3 0 R /ID {id} >>");
        let pdf = classic(&[(1, catalog.as_bytes()), (2, &stream), (3, info.as_bytes())], &trailer);
        Document::open(&pdf, &Limits::default()).unwrap()
    }

    /// A packet declaring Table 2's namespaces, holding `body`.
    fn packet(body: &str) -> Vec<u8> {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="{DC}" xmlns:pdf="{PDF}" xmlns:xmp="{XMP}" xmlns:xmpMM="{MM}" xmlns:pdfxid="{PDFXID}">{body}</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
        )
        .into_bytes()
    }

    /// Every check's findings, by id, at PDF/X-4.
    fn ids(doc: &Document) -> Vec<&'static str> {
        RUNS.iter()
            .flat_map(|run| run(doc, Level::X4))
            .map(|f| f.check.id)
            .collect()
    }

    const FULL: &str = "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">T</rdf:li></rdf:Alt></dc:title>\
        <dc:creator><rdf:Seq><rdf:li>A</rdf:li></rdf:Seq></dc:creator>\
        <xmp:CreateDate>2026-11-25T12:00Z</xmp:CreateDate><xmp:ModifyDate>2026-11-25T12:00Z</xmp:ModifyDate>\
        <xmp:MetadataDate>2026-11-25</xmp:MetadataDate><xmp:CreatorTool>c</xmp:CreatorTool><pdf:Producer>p</pdf:Producer>\
        <pdf:Trapped>True</pdf:Trapped><xmpMM:DocumentID>uuid:5f0b8a8e6f0e4a579d552c1f3f1d0a40</xmpMM:DocumentID>\
        <xmpMM:VersionID>1</xmpMM:VersionID><xmpMM:RenditionClass>default</xmpMM:RenditionClass>\
        <pdfxid:GTS_PDFXVersion>PDF/X-4p</pdfxid:GTS_PDFXVersion>";

    const INFO: &str = "<< /Title (T) /Author (A) /CreationDate (D:20261125130000+01'00') /Trapped /True >>";

    #[test]
    fn a_complete_packet_passes_and_x4p_is_its_own_version() {
        let doc = open("2 0 R", &packet(FULL), INFO, "[(a) (b)]");
        assert_eq!(ids(&doc), ["x4.identity.version-value"]);
        assert_eq!(version_value(&doc, Level::X4p), []);
    }

    #[test]
    fn without_a_readable_packet() {
        for (metadata, xmp, why) in [
            ("5", b"".as_slice(), "not a reference"),
            ("2 0 R", b"\xC3(", "not text"),
            ("2 0 R", b"<a/>", "not RDF"),
        ] {
            let doc = open(metadata, xmp, "<< >>", "[(a) (b)]");
            let found = ids(&doc);
            assert!(found.contains(&"x4.xmp.document-id"), "{why}: {found:?}");
            assert!(!found.contains(&"x4.xmp.namespace-prefixes") && !found.contains(&"x4.identity.version-value"));
        }
        let messages = |xmp: &[u8]| -> Vec<String> {
            well_formed(&open("2 0 R", xmp, "<< >>", "[(a) (b)]"), Level::X4)
                .into_iter()
                .map(|f| f.message)
                .collect()
        };
        assert_eq!(
            messages(b"\xC3("),
            ["metadata stream 2 is not UTF-8, UTF-16 or UTF-32 text"]
        );
        assert_eq!(
            messages(b"<a/>"),
            ["metadata stream 2 is not XMP's RDF: no rdf:RDF element"]
        );
        assert_eq!(messages(b"<a>").len(), 1, "not well-formed XML");
        let header = packet("");
        let header = String::from_utf8(header)
            .unwrap()
            .replace("id=", "encoding=\"UTF-8\" id=");
        let doc = open("2 0 R", header.as_bytes(), "<< >>", "[(a) (b)]");
        assert_eq!(header_attributes(&doc, Level::X4).len(), 1);
        assert_eq!(present(&open("5", b"", "<< >>", "[]"), Level::X4).len(), 1);
    }

    #[test]
    fn what_cannot_be_read_is_missing() {
        let pdf = classic(
            &[
                (1, b"<< /Metadata 2 0 R >>"),
                (
                    2,
                    b"<< /Type /Metadata /Filter 4 0 R /Length 2 >>\nstream\nzz\nendstream",
                ),
                (3, b"<< /Title 9 0 R >>"),
                (4, b"/ASCIIHexDecode"),
            ],
            "<< /Size SIZE /Root 1 0 R /Info 3 0 R >>",
        );
        let doc = Document::open(&pdf, &Limits::default()).unwrap();
        assert!(well_formed(&doc, Level::X4).is_empty() && header_attributes(&doc, Level::X4).is_empty());
        assert_eq!(
            analogue_present(&doc, Level::X4).len(),
            1,
            "the dangling /Title is still an entry"
        );
        assert_eq!(present(&doc, Level::X4), []);
        assert!(ids(&doc).contains(&"x4.xmp.document-id"));
        let odd = classic(&[(1, b"5")], "<< /Size SIZE /Root 1 0 R /Info 9 0 R >>");
        let doc = Document::open(&odd, &Limits::default()).unwrap();
        assert_eq!(present(&doc, Level::X4).len(), 1, "a catalog that is not a dictionary");
        assert_eq!(analogue_present(&doc, Level::X4), [], "Info that cannot be read");
        let bare = classic(&[(1, b"<< >>")], "<< /Size SIZE /Root 1 0 R /ID [9 0 R 8 0 R] >>");
        let doc = Document::open(&bare, &Limits::default()).unwrap();
        assert_eq!(analogue_present(&doc, Level::X4), [], "no Info");
        assert_eq!(present(&doc, Level::X4).len(), 1, "a catalog without /Metadata");
        assert_eq!(trailer_id(&doc, Level::X4).len(), 1, "an /ID of references to nothing");
    }

    #[test]
    fn values_of_the_wrong_kind() {
        let body = "<dc:title>T</dc:title><dc:creator><rdf:Bag><rdf:li>A</rdf:li></rdf:Bag></dc:creator>\
            <dc:description><rdf:Alt><rdf:li xml:lang=\"en\">S</rdf:li></rdf:Alt></dc:description>\
            <pdf:Trapped>Maybe</pdf:Trapped>\
            <pdfxid:GTS_PDFXVersion rdf:parseType=\"Resource\"><pdfxid:x>1</pdfxid:x></pdfxid:GTS_PDFXVersion>\
            <xmpMM:DocumentID>uuid:xyz</xmpMM:DocumentID><xmp:CreateDate>2026-11-25T12:00Z</xmp:CreateDate>";
        let info = "<< /Title (T) /Author (A) /Subject (S) /Trapped (Maybe) /CreationDate (D:2026x) \
            /Keywords <FEFF00> /GTS_PDFXVersion (PDF/X-4) >>";
        let doc = open("2 0 R", &packet(body), info, "[1 2]");
        let found = ids(&doc);
        for id in [
            "x4.info.author-single-creator",
            "x4.trapped.true-or-false",
            "x4.identity.version-value",
            "x4.identity.version-present",
            "x4.xmp.document-id-uuid",
            "x4.trailer.id",
        ] {
            assert!(found.contains(&id), "{id}: {found:?}");
        }
        let present: Vec<String> = analogue_present(&doc, Level::X4)
            .into_iter()
            .map(|f| f.message)
            .collect();
        assert_eq!(
            present,
            [
                "Info /Title has no dc:title in XMP",
                "Info /Subject has no dc:description in XMP",
                "Info /Keywords has no pdf:Keywords in XMP",
                "Info /GTS_PDFXVersion has no pdfxid:GTS_PDFXVersion in XMP",
            ]
        );
        let equal: Vec<String> = analogue_equal(&doc, Level::X4).into_iter().map(|f| f.message).collect();
        assert_eq!(
            equal,
            [
                r#"Info /CreationDate is "D:2026x" but xmp:CreateDate is "2026-11-25T12:00Z""#,
                r#"Info /Trapped is "unreadable" but pdf:Trapped is "Maybe""#,
            ]
        );
    }
}
