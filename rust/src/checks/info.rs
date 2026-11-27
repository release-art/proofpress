//! The document information dictionary at PDF/X-1a and PDF/X-3 (ISO 15930-1, -3, -4, -6): identification, dates,
//! title and the trapped state live in Info, not XMP, at these levels.

use super::Settings;
use super::metadata::{XMP, document_xmp};
use crate::common::{date, text};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Level, X1A_IDENTITY_CONFORMANCE, X13_IDENTITY_VERSION_PRESENT, X13_IDENTITY_VERSION_VALUE,
    X13_INFO_AGENT_NAMES, X13_INFO_CREATION_DATE, X13_INFO_MOD_DATE, X13_INFO_TITLE, X13_TRAPPED_PRESENT,
    X13_TRAPPED_TRUE_OR_FALSE, X13_XMP_MOD_DATE_AGREES,
};

/// `finding`, citing of the four older levels' clause numbers for its rule, in the order X-1a:2001, X-3:2002,
/// X-1a:2003, X-3:2003, the one of the level a file is verified at, in that level's standard.
pub(super) fn per_level(finding: Finding, settings: &Settings, numbers: [&'static str; 4]) -> Finding {
    let [x1a_2001, x3_2002, x1a_2003, x3_2003] = numbers;
    let number = match settings.level {
        Level::X32002 => x3_2002,
        Level::X1a2003 => x1a_2003,
        Level::X32003 => x3_2003,
        _ => x1a_2001,
    };
    finding.citing_in(settings.level.standard(), number)
}

/// The clause of each part on trapping (15930-1 6.9, -3 6.5, -4 and -6 6.6).
pub(super) const TRAPPING: [&str; 4] = ["6.9", "6.5", "6.6", "6.6"];

/// The clause of each part on identification and the Info entries (15930-1 6.13, -3 6.6, -4 and -6 6.7).
const IDENTIFICATION: [&str; 4] = ["6.13", "6.6", "6.7", "6.7"];

/// The document information dictionary.
fn info(doc: &Document) -> Option<&Dict> {
    doc.resolve(doc.trailer().get(b"Info")?)?.dict()
}

/// Info's entry `key`, resolved.
fn entry<'a>(doc: &'a Document, key: &[u8]) -> Option<&'a Object> {
    doc.resolve(info(doc)?.get(key)?)
}

/// An Info entry read as text.
enum Text {
    Missing,
    Other,
    Is(String),
}

impl Text {
    fn text(self) -> Option<String> {
        match self {
            Self::Is(text) => Some(text),
            _ => None,
        }
    }
}

/// Info's entry `key` as text.
fn text_entry(doc: &Document, key: &[u8]) -> Text {
    match entry(doc, key).map(|v| match &v.value {
        Value::String(bytes, _) => text::decode(bytes),
        _ => None,
    }) {
        None => Text::Missing,
        Some(None) => Text::Other,
        Some(Some(text)) => Text::Is(text),
    }
}

/// The trapped state is in Info.
pub(super) fn trapped_present(doc: &Document, settings: &Settings) -> Vec<Finding> {
    match entry(doc, b"Trapped") {
        Some(_) => Vec::new(),
        None => vec![per_level(
            Finding::new(X13_TRAPPED_PRESENT, "the Info dictionary has no /Trapped"),
            settings,
            TRAPPING,
        )],
    }
}

/// Info `/Trapped` is the name `True` or `False`; `Unknown`, another name or another type fails. A missing one is
/// `x13.trapped.present`'s.
pub(super) fn trapped_true_or_false(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(value) = entry(doc, b"Trapped") else {
        return Vec::new();
    };
    let what = match &value.value {
        Value::Name(name) if name == b"True" || name == b"False" => return Vec::new(),
        Value::Name(name) => format!("/{}", String::from_utf8_lossy(name)),
        Value::String(bytes, _) => format!("the string ({})", String::from_utf8_lossy(bytes)),
        _ => "not a name".to_owned(),
    };
    vec![per_level(
        Finding::new(
            X13_TRAPPED_TRUE_OR_FALSE,
            format!("Info /Trapped is {what}, not /True or /False"),
        ),
        settings,
        TRAPPING,
    )]
}

/// Whether Info `/Trapped` is the name `True`.
pub(super) fn trapped_true(doc: &Document) -> bool {
    entry(doc, b"Trapped").and_then(Object::name) == Some(b"True")
}

/// The `GTS_PDFXVersion` each older level's files carry.
fn version_of(level: Level) -> &'static str {
    match level {
        Level::X32002 => "PDF/X-3:2002",
        Level::X1a2003 => "PDF/X-1a:2003",
        Level::X32003 => "PDF/X-3:2003",
        _ => "PDF/X-1:2001",
    }
}

/// Info has `/GTS_PDFXVersion`, a string.
pub(super) fn version_present(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let problem = match text_entry(doc, b"GTS_PDFXVersion") {
        Text::Is(_) => return Vec::new(),
        Text::Other => "a /GTS_PDFXVersion that is not a text string",
        Text::Missing => "no /GTS_PDFXVersion",
    };
    vec![per_level(
        Finding::new(
            X13_IDENTITY_VERSION_PRESENT,
            format!("the Info dictionary has {problem}"),
        ),
        settings,
        IDENTIFICATION,
    )]
}

/// Info `/GTS_PDFXVersion` names the level exactly; a missing one is `x13.identity.version-present`'s.
pub(super) fn version_value(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let want = version_of(settings.level);
    match text_entry(doc, b"GTS_PDFXVersion") {
        Text::Is(value) if value != want => vec![per_level(
            Finding::new(
                X13_IDENTITY_VERSION_VALUE,
                format!("Info /GTS_PDFXVersion is {value:?}, not {want:?}"),
            ),
            settings,
            IDENTIFICATION,
        )],
        _ => Vec::new(),
    }
}

/// 15930-1 6.13 (L-Q13): Info `/GTS_PDFXConformance` is `PDF/X-1a:2001`. A file claiming plain PDF/X-1 (version
/// `PDF/X-1:2001` with no conformance, or `PDF/X-1:2001` as its conformance) is not evaluated (L-Q1).
pub(super) fn conformance(doc: &Document, _: &Settings) -> Result<Vec<Finding>, String> {
    let version = text_entry(doc, b"GTS_PDFXVersion").text();
    let value = text_entry(doc, b"GTS_PDFXConformance");
    let plain = match &value {
        Text::Missing => true,
        Text::Is(v) => v == "PDF/X-1:2001",
        Text::Other => false,
    };
    if version.as_deref() == Some("PDF/X-1:2001") && plain {
        return Err("the file claims plain PDF/X-1, not PDF/X-1a".into());
    }
    let what = match value {
        Text::Is(v) if v == "PDF/X-1a:2001" => return Ok(Vec::new()),
        Text::Is(v) => format!("Info /GTS_PDFXConformance is {v:?}, not \"PDF/X-1a:2001\""),
        Text::Other => "Info /GTS_PDFXConformance is not a text string".to_owned(),
        Text::Missing => "the Info dictionary has no /GTS_PDFXConformance".to_owned(),
    };
    Ok(vec![Finding::new(X1A_IDENTITY_CONFORMANCE, what).citing("6.13")])
}

/// A finding from `check` unless Info's `key` is a text string that `valid` accepts.
fn require(
    doc: &Document,
    settings: &Settings,
    check: &'static Check,
    key: &str,
    valid: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let problem = match text_entry(doc, key.as_bytes()) {
        Text::Is(v) if valid(&v) => return Vec::new(),
        Text::Is(v) if v.trim().is_empty() => format!("an empty /{key}"),
        Text::Is(v) => format!("a /{key} of {v:?}, which is not a date"),
        Text::Other => format!("a /{key} that is not a text string"),
        Text::Missing => format!("no /{key}"),
    };
    vec![per_level(
        Finding::new(check, format!("the Info dictionary has {problem}")),
        settings,
        IDENTIFICATION,
    )]
}

/// X4-Q7's reading: a date there is not empty and parses.
fn valid_date(text: &str) -> bool {
    date::parse_pdf(text).is_some()
}

fn non_empty(text: &str) -> bool {
    !text.trim().is_empty()
}

/// Info `/CreationDate` is a date.
pub(super) fn creation_date(doc: &Document, settings: &Settings) -> Vec<Finding> {
    require(doc, settings, X13_INFO_CREATION_DATE, "CreationDate", &valid_date)
}

/// Info `/ModDate` is a date.
pub(super) fn mod_date(doc: &Document, settings: &Settings) -> Vec<Finding> {
    require(doc, settings, X13_INFO_MOD_DATE, "ModDate", &valid_date)
}

/// Info `/Title` is not empty.
pub(super) fn title(doc: &Document, settings: &Settings) -> Vec<Finding> {
    require(doc, settings, X13_INFO_TITLE, "Title", &non_empty)
}

/// Info names its `/Creator` and `/Producer` (a recommendation).
pub(super) fn agent_names(doc: &Document, settings: &Settings) -> Vec<Finding> {
    ["Creator", "Producer"]
        .into_iter()
        .flat_map(|key| require(doc, settings, X13_INFO_AGENT_NAMES, key, &non_empty))
        .collect()
}

/// 15930-4 and -6 6.7 (L-Q5): when the document's XMP has a modification date, it is Info `/ModDate`'s instant (a
/// warning: the XMP "should" be updated). Without either date, or one that does not parse, nothing is judged here.
pub(super) fn mod_date_agrees(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let info = text_entry(doc, b"ModDate").text();
    let packet = document_xmp(doc);
    let xmp = packet
        .as_ref()
        .and_then(|p| p.get(XMP, "ModifyDate"))
        .and_then(|v| v.text());
    let (Some(info), Some(xmp)) = (info, xmp) else {
        return Vec::new();
    };
    match (date::parse_pdf(&info), date::parse_xmp(xmp)) {
        (Some(a), Some(b)) if !a.same_instant(&b) => vec![per_level(
            Finding::new(
                X13_XMP_MOD_DATE_AGREES,
                format!("XMP xmp:ModifyDate {xmp:?} does not match Info /ModDate {info:?}"),
            ),
            settings,
            IDENTIFICATION,
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// A file whose Info dictionary is `info`, with `xmp` as its document metadata when given.
    fn open(info: &str, xmp: Option<&str>) -> Document {
        let catalog = if xmp.is_some() {
            "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>"
        } else {
            "<< /Type /Catalog /Pages 2 0 R >>"
        };
        let packet = format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="{XMP}">{}</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#,
            xmp.unwrap_or_default()
        );
        let metadata = format!(
            "<< /Type /Metadata /Length {} >>\nstream\n{packet}\nendstream",
            packet.len()
        );
        let objects: [(u32, &[u8]); 4] = [
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, info.as_bytes()),
            (4, metadata.as_bytes()),
        ];
        Document::open(
            &classic(&objects, "<< /Size SIZE /Root 1 0 R /Info 3 0 R >>"),
            &Limits::default(),
        )
        .unwrap()
    }

    fn at(level: Level) -> Settings {
        level.into()
    }

    /// Each finding's message and cited clause.
    fn said(findings: Vec<Finding>) -> Vec<(String, String)> {
        findings
            .into_iter()
            .map(|f| {
                let clause = f.clause.map_or_default(|c| format!("{}:{}", c.standard, c.number));
                (f.message, clause)
            })
            .collect()
    }

    #[test]
    fn the_trapped_state_in_info() {
        let x3 = at(Level::X32002);
        assert_eq!(trapped_present(&open("<< /Trapped /False >>", None), &x3), []);
        assert_eq!(trapped_true_or_false(&open("<< >>", None), &x3), []);
        let string = trapped_true_or_false(&open("<< /Trapped (True) >>", None), &x3);
        assert_eq!(
            said(string),
            [(
                "Info /Trapped is the string (True), not /True or /False".to_owned(),
                "iso-15930-3:6.5".to_owned()
            )]
        );
        let number = trapped_true_or_false(&open("<< /Trapped 1 >>", None), &at(Level::X32003));
        assert_eq!(said(number)[0].0, "Info /Trapped is not a name, not /True or /False");
        let unknown = trapped_true_or_false(&open("<< /Trapped /Unknown >>", None), &x3);
        assert_eq!(said(unknown)[0].0, "Info /Trapped is /Unknown, not /True or /False");
        assert_eq!(
            said(trapped_present(&open("<< >>", None), &x3))[0].0,
            "the Info dictionary has no /Trapped"
        );
        assert!(trapped_true(&open("<< /Trapped /True >>", None)));
        assert!(!trapped_true(&open("<< >>", None)));
    }

    #[test]
    fn the_version_and_conformance() {
        let x1a = at(Level::X1a2001);
        let not_text = version_present(&open("<< /GTS_PDFXVersion 3 >>", None), &x1a);
        assert_eq!(
            said(not_text),
            [(
                "the Info dictionary has a /GTS_PDFXVersion that is not a text string".to_owned(),
                "iso-15930-1:6.13".to_owned()
            )]
        );
        let missing = version_present(&open("<< >>", None), &x1a);
        assert_eq!(said(missing)[0].0, "the Info dictionary has no /GTS_PDFXVersion");
        let x3 = open("<< /GTS_PDFXVersion (PDF/X-3:2002) >>", None);
        assert_eq!(version_present(&x3, &x1a), []);
        assert_eq!(version_value(&x3, &at(Level::X32002)), []);
        assert_eq!(version_value(&open("<< >>", None), &x1a), []);
        assert_eq!(said(version_value(&x3, &x1a))[0].1, "iso-15930-1:6.13");
        // Plain PDF/X-1, by an absent or plain conformance key, is not evaluated (L-Q1).
        let plain = open(
            "<< /GTS_PDFXVersion (PDF/X-1:2001) /GTS_PDFXConformance (PDF/X-1:2001) >>",
            None,
        );
        assert!(conformance(&plain, &x1a).is_err());
        let messages = |info: &str| said(conformance(&open(info, None), &x1a).unwrap());
        assert_eq!(
            messages("<< /GTS_PDFXVersion (PDF/X-1:2001) /GTS_PDFXConformance (PDF/X-1a:2001) >>"),
            []
        );
        assert_eq!(
            messages("<< /GTS_PDFXConformance (PDF/X-a1:2001) >>")[0].0,
            "Info /GTS_PDFXConformance is \"PDF/X-a1:2001\", not \"PDF/X-1a:2001\""
        );
        assert_eq!(
            messages("<< /GTS_PDFXConformance 1 >>")[0].0,
            "Info /GTS_PDFXConformance is not a text string"
        );
        // Another version without the key is not plain X-1: the key is missing.
        assert_eq!(
            messages("<< /GTS_PDFXVersion (PDF/X-3:2002) >>")[0].0,
            "the Info dictionary has no /GTS_PDFXConformance"
        );
    }

    #[test]
    fn dates_title_and_names() {
        let x1a = at(Level::X1a2003);
        let messages = |info: &str, run: fn(&Document, &Settings) -> Vec<Finding>| {
            said(run(&open(info, None), &x1a))
                .into_iter()
                .map(|(m, _)| m)
                .collect::<Vec<_>>()
        };
        assert_eq!(messages("<< /CreationDate (D:20261127) >>", creation_date), [""; 0]);
        assert_eq!(
            messages("<< /CreationDate () >>", creation_date),
            ["the Info dictionary has an empty /CreationDate"]
        );
        assert_eq!(
            messages("<< /ModDate /D >>", mod_date),
            ["the Info dictionary has a /ModDate that is not a text string"]
        );
        assert_eq!(
            messages("<< /ModDate (yesterday) >>", mod_date),
            ["the Info dictionary has a /ModDate of \"yesterday\", which is not a date"]
        );
        assert_eq!(messages("<< /Title (T) >>", title), [""; 0]);
        assert_eq!(
            messages("<< /Producer (p) >>", agent_names),
            ["the Info dictionary has no /Creator"]
        );
    }

    #[test]
    fn without_an_info_dictionary_every_entry_is_missing() {
        let x3 = at(Level::X32002);
        let pages: (u32, &[u8]) = (2, b"<< /Type /Pages /Kids [] /Count 0 >>");
        let catalog: (u32, &[u8]) = (1, b"<< /Type /Catalog /Pages 2 0 R >>");
        let none = classic(&[catalog, pages], "<< /Size SIZE /Root 1 0 R >>");
        let number = classic(&[catalog, pages, (3, b"7")], "<< /Size SIZE /Root 1 0 R /Info 3 0 R >>");
        let dangling = classic(&[catalog, pages], "<< /Size SIZE /Root 1 0 R /Info 9 0 R >>");
        for file in [none, number, dangling] {
            let doc = Document::open(&file, &Limits::default()).unwrap();
            assert_eq!(
                said(trapped_present(&doc, &x3))[0].0,
                "the Info dictionary has no /Trapped"
            );
        }
    }

    #[test]
    fn the_xmp_modification_date() {
        let x3 = at(Level::X32003);
        let run = |info: &str, xmp: &str| said(mod_date_agrees(&open(info, Some(xmp)), &x3));
        let date = "<xmp:ModifyDate>2026-11-27T12:00:00Z</xmp:ModifyDate>";
        assert_eq!(run("<< /ModDate (D:20261127120000Z) >>", date), []);
        assert_eq!(run("<< /ModDate (D:20261127130000+01'00') >>", date), []);
        assert_eq!(run("<< /ModDate (soon) >>", date), []);
        assert_eq!(run("<< >>", date), []);
        assert_eq!(run("<< /ModDate (D:20261127120000Z) >>", ""), []);
        assert_eq!(run("<< /ModDate (D:20261128120000Z) >>", date)[0].1, "iso-15930-6:6.7");
    }
}
