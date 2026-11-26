//! A PDF/X-5g or X-5pg file set (ISO 15930-8 clause 8): the files its reference `XObjects` name, each read once and
//! verified at the level it claims with the same engine (8.2, X5-Q13), and the rows that compare them with their
//! proxies (8.3, 8.4) and with the container (8.5, 8.7).
//!
//! A target is the file a `/Ref`'s `/F` names: embedded with `/EF` (X5-Q3), or a file on this computer beside the
//! container; a URL that only a network reaches is not fetched. A target that loops back to a file being verified, or
//! that nests deeper than `Limits::target_depth`, is not verified again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::metadata::{MM, PDFXID, document_xmp};
use super::{Files, Settings, intent, x5g};
use crate::common::url::{Place, place};
use crate::common::{icc, labels, text};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Level, Location, Report, Severity, Verdict, X5_G_NO_CIRCULAR_REFS, X5_G_SAME_PRINTING_CONDITION,
    X5_G_TARGET_CONFORMANCE_LEVEL, X5_G_TARGET_CONFORMS, X5_G_TARGET_IDENTITY, X5_G_TARGET_NO_OCG,
    X5_G_TARGET_PAGE_EXISTS, X5_G_TARGET_VERSION_POLICY, X5_G_TARGET_XMP_IDS,
};
use crate::pipeline::Limits;

/// The levels 8.2 allows a target to claim, as their identifiers read.
const ALLOWED: [&str; 8] = [
    "PDF/X-1a:2001",
    "PDF/X-1a:2003",
    "PDF/X-3:2002",
    "PDF/X-3:2003",
    "PDF/X-4",
    "PDF/X-4p",
    "PDF/X-5g",
    "PDF/X-5pg",
];

/// The file set: each proxy's target, in proxy order.
pub(super) struct Set {
    targets: Vec<Target>,
}

/// One proxy's target.
struct Target {
    /// The proxy's object number.
    proxy: u32,
    /// The target as the reference names it.
    name: String,
    /// The reference's `/Page`.
    page: Option<Page>,
    /// The reference's `/ID`.
    file_id: Option<[Vec<u8>; 2]>,
    /// The proxy's `xmpMM:RenditionOf` fields.
    rendition: Vec<(String, String)>,
    found: Found,
}

/// A reference's `/Page`: an index from 0, or a page label.
enum Page {
    Index(i64),
    Label(String),
}

/// What became of a target.
#[derive(Clone)]
enum Found {
    /// Not looked for, or not reachable without a network: why.
    NotRead(String),
    /// Nothing at the place it names, so the file set is incomplete.
    Missing(String),
    /// A file that is not a PDF the core can open: why.
    Unreadable(String),
    /// The file, read.
    Read(Box<Opened>),
}

/// What a target file holds.
#[derive(Clone)]
struct Opened {
    /// The file, as shown in findings.
    shown: String,
    /// The PDF/X level it claims, as its identifier reads (`PDF/X-1a:2001`, `PDF/X-4`).
    claim: Option<String>,
    /// Its trailer `/ID`.
    file_id: Option<[Vec<u8>; 2]>,
    /// Its XMP `xmpMM:DocumentID`, `VersionID` and `RenditionClass`.
    ids: [Option<String>; 3],
    /// Its page labels, one per page.
    labels: Vec<Option<String>>,
    /// Whether it has optional content: `/OCProperties`, or any `/Type /OCG` dictionary.
    optional_content: bool,
    /// Its PDF/X intent's `OutputConditionIdentifier` and `RegistryName`, and its profile's identity.
    condition: Condition,
    /// How verifying it at its level came out.
    verified: Verified,
}

/// A file's printing condition (X5-Q7): its PDF/X intent's identifier and registry, and its profile's identity.
#[derive(Clone, PartialEq, Eq)]
struct Condition {
    identifier: Option<String>,
    registry: Option<String>,
    profile: Option<[u8; 16]>,
}

/// The outcome of verifying a target at the level it claims.
#[derive(Clone)]
enum Verified {
    Report(Box<Report>),
    /// Its level is one this engine does not hold (X-1a, X-3; X5-Q9), or none 8.2 allows.
    Not(String),
    /// It loops back to a file already being verified.
    Cycle,
    /// It nests deeper than the limit.
    TooDeep(usize),
}

/// How a target compares with the reference that names it (8.4).
#[derive(Debug, PartialEq, Eq)]
enum Identity {
    Exact,
    OtherVersion,
    Other(&'static str),
}

/// The file set of `doc`, its targets read and verified. `chain` is the files being verified, outermost first, the
/// container last.
pub(super) fn resolve(doc: &Document, settings: &Settings, limits: &Limits, chain: &[PathBuf]) -> Set {
    let mut read: BTreeMap<PathBuf, Found> = BTreeMap::new();
    let mut targets = Vec::new();
    for proxy in x5g::proxies(doc) {
        let spec = proxy.reference.get(b"F").and_then(|f| doc.resolve(f));
        let name = spec.and_then(|s| file_name(doc, s)).unwrap_or_default();
        let found = match (&settings.files, spec) {
            (Files::Alone, _) => Found::NotRead("the file is verified alone, without its targets".into()),
            (_, None) => Found::Missing("the reference has no /F".into()),
            (_, Some(_)) if name.is_empty() => Found::Missing("the reference's /F names no file".into()),
            (Files::Set(folder), Some(spec)) => match source(doc, spec, &name, folder) {
                Source::Network => Found::NotRead(format!("{name} needs a network, which is not used")),
                Source::Missing(why) => Found::Missing(why),
                Source::File(path, data) => {
                    let key = std::fs::canonicalize(&path).unwrap_or(path);
                    let open = || open(&key, &data, settings, limits, chain);
                    read.entry(key.clone()).or_insert_with(open).clone()
                }
            },
        };
        targets.push(Target {
            proxy: proxy.num,
            name,
            page: page(doc, proxy.reference),
            file_id: x5g::file_id(doc, proxy.reference).map(|[a, b]| [a.to_vec(), b.to_vec()]),
            rendition: x5g::rendition_of(doc, proxy.form).unwrap_or_default(),
            found,
        });
    }
    Set { targets }
}

/// What a file specification names: its `/UF` or `/F`, or the string it is.
fn file_name(doc: &Document, spec: &Object) -> Option<String> {
    let bytes = match &spec.value {
        Value::String(bytes, _) => bytes,
        Value::Dict(dict) => [b"UF".as_slice(), b"F"]
            .iter()
            .filter_map(|k| dict.get(k).and_then(|v| doc.resolve(v)))
            .find_map(|v| match &v.value {
                Value::String(bytes, _) => Some(bytes),
                _ => None,
            })?,
        _ => return None,
    };
    text::decode(bytes)
}

/// Where a target's bytes come from.
enum Source {
    File(PathBuf, Vec<u8>),
    Network,
    Missing(String),
}

/// A target's bytes: the embedded file, a URL on this computer, or a file name relative to `folder`.
fn source(doc: &Document, spec: &Object, name: &str, folder: &Path) -> Source {
    let dict = spec.dict();
    let entry = |key: &[u8]| dict.and_then(|d| d.get(key)).and_then(|v| doc.resolve(v));
    if let Some(Value::Reference(num, _)) = entry(b"EF")
        .and_then(Object::dict)
        .and_then(|ef| ef.get(b"F"))
        .map(|f| &f.value)
    {
        return match doc.stream_data(*num) {
            Some(data) => Source::File(folder.join(format!("{name} (embedded)")), data),
            None => Source::Missing(format!("the embedded {name} cannot be read")),
        };
    }
    let path = if entry(b"FS").and_then(Object::name) == Some(b"URL") {
        match place(name, folder) {
            Place::Network => return Source::Network,
            Place::Local(path) => path,
        }
    } else {
        folder.join(name)
    };
    match std::fs::read(&path) {
        Ok(data) => Source::File(path, data),
        Err(_) => Source::Missing(format!("no file at {name}")),
    }
}

/// Read the target at `path` and verify it at the level it claims, unless that loops or nests too deep.
fn open(path: &Path, data: &[u8], settings: &Settings, limits: &Limits, chain: &[PathBuf]) -> Found {
    let doc = match Document::open(data, limits) {
        Ok(doc) => doc,
        Err(refusal) => return Found::Unreadable(format!("{} cannot be read: {}", path.display(), refusal.message)),
    };
    let claim = claim(&doc);
    let verified = if chain.iter().any(|c| c == path) {
        Verified::Cycle
    } else if chain.len() > limits.target_depth {
        Verified::TooDeep(limits.target_depth)
    } else {
        match claim.as_deref().and_then(level) {
            Some(level) => {
                let folder = path.parent().map_or_default(Path::to_owned);
                let target = Settings {
                    level,
                    params: settings.params.clone(),
                    files: Files::Set(folder),
                };
                let mut deeper = chain.to_vec();
                deeper.push(path.to_owned());
                let file = path.display().to_string();
                Verified::Report(Box::new(super::verify_chain(&file, data, &target, limits, &deeper)))
            }
            None => Verified::Not(match &claim {
                Some(c) if ALLOWED.contains(&c.as_str()) => {
                    format!("{c} is not checked here, since its standard is not held")
                }
                Some(c) => format!("{c} is not a level a target may have"),
                None => "it claims no PDF/X level".into(),
            }),
        }
    };
    let xmp = document_xmp(&doc);
    let mm = |name: &str| {
        xmp.as_ref()
            .and_then(|p| p.get(MM, name))
            .and_then(|v| v.text())
            .map(str::to_owned)
    };
    Found::Read(Box::new(Opened {
        shown: path.display().to_string(),
        claim,
        file_id: trailer_id(&doc),
        ids: [mm("DocumentID"), mm("VersionID"), mm("RenditionClass")],
        labels: labels::labels(&doc, doc.pages().len()),
        optional_content: optional_content(&doc),
        condition: condition(&doc),
        verified,
    }))
}

/// The level a claim is verified at, when this engine holds it.
fn level(claim: &str) -> Option<Level> {
    match claim {
        "PDF/X-4" => Some(Level::X4),
        "PDF/X-4p" => Some(Level::X4p),
        "PDF/X-5g" => Some(Level::X5g),
        "PDF/X-5pg" => Some(Level::X5pg),
        _ => None,
    }
}

/// The PDF/X level a file claims: its XMP `pdfxid:GTS_PDFXVersion`, else its Info `GTS_PDFXVersion`, with
/// `GTS_PDFXConformance` naming the X-1a level when the version is `PDF/X-1:2001`.
fn claim(doc: &Document) -> Option<String> {
    let xmp = document_xmp(doc).and_then(|p| {
        p.get(PDFXID, "GTS_PDFXVersion")
            .and_then(|v| v.text().map(str::to_owned))
    });
    xmp.or_else(|| {
        let info = doc
            .trailer()
            .get(b"Info")
            .and_then(|i| doc.resolve(i))
            .and_then(Object::dict)?;
        let entry = |key: &[u8]| match info.get(key).and_then(|v| doc.resolve(v)).map(|v| &v.value) {
            Some(Value::String(bytes, _)) => text::decode(bytes),
            _ => None,
        };
        let version = entry(b"GTS_PDFXVersion")?;
        Some(if version == "PDF/X-1:2001" {
            entry(b"GTS_PDFXConformance").unwrap_or(version)
        } else {
            version
        })
    })
}

/// A file's trailer `/ID`, when it is two strings.
fn trailer_id(doc: &Document) -> Option<[Vec<u8>; 2]> {
    let items = doc
        .trailer()
        .get(b"ID")
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)?;
    let string = |o: &Object| match doc.resolve(o).map(|o| &o.value) {
        Some(Value::String(bytes, _)) => Some(bytes.clone()),
        _ => None,
    };
    match items {
        [a, b] => Some([string(a)?, string(b)?]),
        _ => None,
    }
}

/// Whether a file has optional content (X5-Q8): `/OCProperties` in its catalog, or any `/Type /OCG` dictionary.
fn optional_content(doc: &Document) -> bool {
    doc.catalog().is_some_and(|c| c.get(b"OCProperties").is_some())
        || doc
            .objects()
            .filter_map(|(_, object, _, _)| object.dict())
            .any(|d| d.get(b"Type").and_then(|t| doc.resolve(t)).and_then(Object::name) == Some(b"OCG"))
}

fn condition(doc: &Document) -> Condition {
    Condition {
        identifier: intent::text_entry(doc, b"OutputConditionIdentifier"),
        registry: intent::text_entry(doc, b"RegistryName"),
        profile: intent::profile_stream(doc).and_then(|(_, data)| icc::identity(&data)),
    }
}

/// A reference's `/Page`.
fn page(doc: &Document, reference: &Dict) -> Option<Page> {
    match reference.get(b"Page").and_then(|p| doc.resolve(p)).map(|p| &p.value) {
        Some(Value::Integer(n)) => Some(Page::Index(*n)),
        Some(Value::String(bytes, _)) => text::decode(bytes).map(Page::Label),
        _ => None,
    }
}

/// How `opened` compares with the reference that names it (8.4): different when the first `/ID` or the document ID
/// differs; another version when only the second `/ID`, the version ID or the rendition class does. What either side
/// lacks is not compared.
fn identity(target: &Target, opened: &Opened) -> Identity {
    let ids = |i: usize| {
        (
            target.file_id.as_ref().and_then(|f| f.get(i)),
            opened.file_id.as_ref().and_then(|f| f.get(i)),
        )
    };
    let field = |name: &str, i: usize| {
        (
            x5g::field(&target.rendition, name),
            opened.ids.get(i).and_then(Option::as_deref),
        )
    };
    let differ = |pair: (Option<&_>, Option<&_>)| matches!(pair, (Some(a), Some(b)) if a != b);
    let differ_text = |pair: (Option<&str>, Option<&str>)| matches!(pair, (Some(a), Some(b)) if a != b);
    if differ(ids(0)) {
        Identity::Other("its first /ID differs from the reference's")
    } else if differ_text(field("documentID", 0)) {
        Identity::Other("its xmpMM:DocumentID differs from the proxy's stRef:documentID")
    } else if differ(ids(1)) || differ_text(field("versionID", 1)) || differ_text(field("renditionClass", 2)) {
        Identity::OtherVersion
    } else {
        Identity::Exact
    }
}

fn found(
    check: &'static Check,
    target: &Target,
    shown: &str,
    message: impl std::fmt::Display,
    clause: &str,
) -> Finding {
    Finding::new(check, format!("object {}: {message}", target.proxy))
        .citing(clause)
        .at(vec![
            Location::Object(target.proxy, 0),
            Location::File(shown.to_owned()),
        ])
}

/// A row over the targets that were read: `judge` gives each one's findings, or why it cannot be judged. A target not
/// read leaves the row unjudged unless another target fails it.
/// Findings, or why they could not be told.
type Judged = Result<Vec<Finding>, String>;

fn each(set: &Set, judge: &dyn Fn(&Target, &Opened) -> Judged) -> Judged {
    let mut out = Vec::new();
    let mut unjudged = None;
    for target in &set.targets {
        let why = match &target.found {
            Found::Read(opened) => match judge(target, opened) {
                Ok(findings) => {
                    out.extend(findings);
                    continue;
                }
                Err(why) => why,
            },
            Found::NotRead(why) | Found::Missing(why) | Found::Unreadable(why) => why.clone(),
        };
        unjudged.get_or_insert(why);
    }
    match unjudged {
        Some(why) if out.is_empty() => Err(why),
        _ => Ok(out),
    }
}

/// 8.2: a target claims X-1a, X-3, X-4, X-4p, X-5g or X-5pg.
pub(super) fn conformance_level(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        let allowed = opened.claim.as_deref().is_some_and(|c| ALLOWED.contains(&c));
        let message = match &opened.claim {
            Some(c) => format!("{} claims {c}, which a target may not", target.name),
            None => format!("{} claims no PDF/X level", target.name),
        };
        Ok((!allowed)
            .then(|| found(X5_G_TARGET_CONFORMANCE_LEVEL, target, &opened.shown, message, "8.2"))
            .into_iter()
            .collect())
    })
}

/// 8.2: a target conforms to the level it claims, verified with the same engine.
pub(super) fn conforms(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| match &opened.verified {
        Verified::Report(report) if report.verdict == Verdict::Pass => Ok(Vec::new()),
        Verified::Report(report) => {
            // What fails it: an error, or a check left unjudged; warnings do not.
            let error = |id: &str| {
                report
                    .findings
                    .iter()
                    .any(|f| f.check.id == id && f.severity == Severity::Error)
            };
            let failed: Vec<&str> = report
                .checks
                .iter()
                .filter(|c| matches!(c.outcome, crate::findings::Outcome::NotEvaluated(_)) || error(c.id.id))
                .map(|c| c.id.id)
                .collect();
            let claim = opened.claim.as_deref().unwrap_or_default();
            let message = format!("{} does not conform to {claim}: {}", target.name, failed.join(", "));
            Ok(vec![found(X5_G_TARGET_CONFORMS, target, &opened.shown, message, "8.2")])
        }
        Verified::Not(why) => Err(format!("{}: {why}", target.name)),
        Verified::Cycle => Err(format!("{} refers back to a file being verified", target.name)),
        Verified::TooDeep(depth) => Err(format!("{} is nested more than {depth} references deep", target.name)),
    })
}

/// 8.2: following references never leads back to a file already being verified.
pub(super) fn no_circular(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        let message = format!("{} refers back to a file being verified", target.name);
        Ok(matches!(opened.verified, Verified::Cycle)
            .then(|| found(X5_G_NO_CIRCULAR_REFS, target, &opened.shown, message, "8.2"))
            .into_iter()
            .collect())
    })
}

/// 8.3: a target's XMP has `xmpMM:DocumentID`, `VersionID` and `RenditionClass`.
pub(super) fn xmp_ids(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        Ok(["DocumentID", "VersionID", "RenditionClass"]
            .iter()
            .zip(&opened.ids)
            .filter(|(_, id)| id.is_none())
            .map(|(name, _)| {
                let message = format!("{}'s XMP has no xmpMM:{name}", target.name);
                found(X5_G_TARGET_XMP_IDS, target, &opened.shown, message, "8.3")
            })
            .collect())
    })
}

/// 8.4: the target is the file the reference names: its `/ID` and XMP IDs are the reference's and the proxy's. A target
/// that is not there fails: the file set is incomplete.
pub(super) fn target_identity(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    let mut out = Vec::new();
    for target in &set.targets {
        if let Found::Missing(why) = &target.found {
            let message = if target.name.is_empty() {
                why.clone()
            } else {
                format!("the file set has no {}: {why}", target.name)
            };
            out.push(found(X5_G_TARGET_IDENTITY, target, &target.name, message, "8.4"));
        }
    }
    let read = each(set, &|target, opened| match identity(target, opened) {
        Identity::Other(why) => {
            let message = format!("{} is not the file the reference names: {why}", target.name);
            Ok(vec![found(X5_G_TARGET_IDENTITY, target, &opened.shown, message, "8.4")])
        }
        Identity::Exact | Identity::OtherVersion => Ok(Vec::new()),
    });
    match read {
        Ok(findings) => Ok(out.into_iter().chain(findings).collect()),
        Err(_) if !out.is_empty() => Ok(out),
        Err(why) => Err(why),
    }
}

/// 8.4 NOTE 2 (X5-Q5): a target that is another version of the document is accepted or not as the job says; when it
/// does not say, that target is not evaluated.
pub(super) fn version_policy(_: &Document, settings: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        if identity(target, opened) != Identity::OtherVersion {
            return Ok(Vec::new());
        }
        match settings.params.accept_target_versions {
            Some(true) => Ok(Vec::new()),
            Some(false) => {
                let message = format!("{} is another version of the document the reference names", target.name);
                Ok(vec![found(
                    X5_G_TARGET_VERSION_POLICY,
                    target,
                    &opened.shown,
                    message,
                    "8.4",
                )])
            }
            None => Err(format!(
                "{} is another version of the document the reference names, and no choice was made to accept or \
                 reject that",
                target.name
            )),
        }
    })
}

/// ISO 32000-1 8.10.4: the reference's `/Page`, an index from 0 or a page label, is a page of the target.
pub(super) fn page_exists(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        let message = match &target.page {
            Some(Page::Index(i)) if usize::try_from(*i).is_ok_and(|i| i < opened.labels.len()) => return Ok(Vec::new()),
            Some(Page::Label(l)) if opened.labels.iter().any(|o| o.as_deref() == Some(l.as_str())) => {
                return Ok(Vec::new());
            }
            Some(Page::Index(i)) => format!(
                "{} has {} pages, so no page index {i}",
                target.name,
                opened.labels.len()
            ),
            Some(Page::Label(l)) => format!("{} has no page labelled {l:?}", target.name),
            None => "the reference has no /Page that is a number or a text string".to_owned(),
        };
        Ok(vec![found(
            X5_G_TARGET_PAGE_EXISTS,
            target,
            &opened.shown,
            message,
            "8.10.4",
        )])
    })
}

/// 8.5 (X5-Q7): a target is prepared for the container's printing condition: the same PDF/X intent identifier and
/// registry. A different output profile for that condition is a warning.
pub(super) fn same_printing_condition(doc: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    let mine = condition(doc);
    each(set, &|target, opened| {
        let theirs = &opened.condition;
        let (message, warning) = if (&theirs.identifier, &theirs.registry) != (&mine.identifier, &mine.registry) {
            let shown = |c: &Condition| {
                format!(
                    "{:?} ({:?})",
                    c.identifier.as_deref().unwrap_or("none"),
                    c.registry.as_deref().unwrap_or("no registry")
                )
            };
            (
                format!(
                    "{} is for the printing condition {}, not {}",
                    target.name,
                    shown(theirs),
                    shown(&mine)
                ),
                false,
            )
        } else if theirs.profile.is_some() && mine.profile.is_some() && theirs.profile != mine.profile {
            (
                format!(
                    "{} names the same printing condition with another output profile",
                    target.name
                ),
                true,
            )
        } else {
            return Ok(Vec::new());
        };
        let mut finding = found(X5_G_SAME_PRINTING_CONDITION, target, &opened.shown, message, "8.5");
        if warning {
            finding.severity = Severity::Warning;
        }
        Ok(vec![finding])
    })
}

/// 8.7 (X5-Q8): an X-4, X-4p, X-5g or X-5pg target has no optional content.
pub(super) fn no_ocg(_: &Document, _: &Settings, set: &Set) -> Result<Vec<Finding>, String> {
    each(set, &|target, opened| {
        let x4_or_x5 = opened.claim.as_deref().and_then(level).is_some();
        let message = format!("{} has optional content", target.name);
        Ok((x4_or_x5 && opened.optional_content)
            .then(|| found(X5_G_TARGET_NO_OCG, target, &opened.shown, message, "8.7"))
            .into_iter()
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// XMP with `body` in its description.
    fn xmp(body: &str) -> Vec<u8> {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmpMM="{MM}" xmlns:pdfxid="{PDFXID}" xmlns:stRef="{}">{body}</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#,
            x5g::ST_REF
        )
        .into_bytes()
    }

    /// A one-page target: its catalog's extra entries, its Info and trailer /ID, and XMP with `body`.
    fn target(catalog: &str, info: &str, id: &str, body: &str) -> Vec<u8> {
        let catalog = format!("<< /Pages 2 0 R /Metadata 5 0 R {catalog} >>");
        let metadata = stream("/Type /Metadata /Subtype /XML", &xmp(body));
        let objects: [(u32, &[u8]); 5] = [
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>"),
            (4, info.as_bytes()),
            (5, &metadata),
        ];
        classic(&objects, &format!("<< /Size SIZE /Root 1 0 R /Info 4 0 R {id} >>"))
    }

    /// A container whose page's proxies have the `/Ref`s `refs`, with `RenditionOf` `rendition` and `more` objects.
    fn container(catalog: &str, refs: &[&str], rendition: &str, more: &[(u32, Vec<u8>)]) -> Document {
        let metadata = stream("/Type /Metadata /Subtype /XML", &xmp(rendition));
        let proxies: Vec<Vec<u8>> = refs
            .iter()
            .map(|r| {
                stream(
                    &format!("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Metadata 9 0 R /Ref {r}"),
                    b"",
                )
            })
            .collect();
        let catalog = format!("<< /Pages 2 0 R {catalog} >>");
        let names: Vec<String> = (20..20 + refs.len()).map(|n| format!("/P{n} {n} 0 R")).collect();
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << {} >> >> >>",
            names.join(" ")
        );
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, page.as_bytes()),
            (9, &metadata),
        ];
        objects.extend(proxies.iter().zip(20..).map(|(p, n)| (n, p.as_slice())));
        objects.extend(more.iter().map(|(n, b)| (*n, b.as_slice())));
        Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    const DOC_ID: &str = "<xmpMM:DocumentID>uuid:1</xmpMM:DocumentID><xmpMM:VersionID>1</xmpMM:VersionID><xmpMM:RenditionClass>default</xmpMM:RenditionClass>";
    const RENDITION: &str = r#"<xmpMM:RenditionOf rdf:parseType="Resource"><stRef:documentID>uuid:1</stRef:documentID></xmpMM:RenditionOf>"#;

    fn messages(result: Judged) -> Result<Vec<String>, String> {
        result.map(|f| f.into_iter().map(|f| f.message).collect())
    }

    #[test]
    fn targets_are_read_from_every_kind_of_file_specification() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-targets-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let version = |v: &str| format!("<pdfxid:GTS_PDFXVersion>{v}</pdfxid:GTS_PDFXVersion>{DOC_ID}");
        let id = "/ID [<01> <02>]";
        std::fs::write(folder.join("x4p.pdf"), target("", "<< >>", id, &version("PDF/X-4p"))).unwrap();
        std::fs::write(folder.join("x5pg.pdf"), target("", "<< >>", id, &version("PDF/X-5pg"))).unwrap();
        std::fs::write(
            folder.join("none.pdf"),
            target("", "<< /GTS_PDFXVersion 5 >>", "/ID 7", DOC_ID),
        )
        .unwrap();
        let labelled = target("/PageLabels << /Nums [0 << /S /r >>] >>", "<< >>", "/ID [<01>]", "");
        std::fs::write(folder.join("labelled.pdf"), labelled).unwrap();
        std::fs::write(folder.join("odd.pdf"), target("", "<< >>", "/ID [<01> 2]", "")).unwrap();
        std::fs::write(folder.join("broken.pdf"), b"not a PDF").unwrap();
        std::fs::write(
            folder.join("x3.pdf"),
            target("", "<< /GTS_PDFXVersion (PDF/X-3:2002) >>", "/ID [1 <02>]", ""),
        )
        .unwrap();
        std::fs::write(folder.join("noinfo.pdf"), target("", "5", "", "")).unwrap();
        let embedded = target("", "<< >>", id, &version("PDF/X-4"));
        let more = [
            (40, stream("/Type /EmbeddedFile", &embedded)),
            (41, stream("/Type /EmbeddedFile /Filter /FlateDecode", b"not flate")),
        ];
        let refs = [
            "<< /Page 0 >>",
            "<< /F (x4p.pdf) /Page 0 /ID [<01> <02>] >>",
            "<< /F << /UF (x5pg.pdf) >> /Page 0 >>",
            "<< /F << /FS /URL /F (none.pdf) >> /Page /First >>",
            "<< /F (labelled.pdf) /Page (i) >>",
            "<< /F (odd.pdf) /Page (i) >>",
            "<< /F (broken.pdf) /Page 0 >>",
            "<< /F << /F (e.pdf) /EF << /F 40 0 R >> >> /Page 0 >>",
            "<< /F << /F (bad.pdf) /EF << /F 41 0 R >> >> /Page 0 >>",
            "<< /F 5 /Page 0 >>",
            "<< /F (x3.pdf) /Page 0 >>",
            "<< /F (noinfo.pdf) >>",
            "<< /F << /F 5 >> /Page 0 >>",
        ];
        let doc = container("", &refs, RENDITION, &more);
        let settings = Settings {
            level: Level::X5g,
            params: super::super::Params::default(),
            files: Files::Set(folder.clone()),
        };
        let set = resolve(&doc, &settings, &Limits::default(), &[folder.join("container.pdf")]);
        let levels = messages(conformance_level(&doc, &settings, &set)).unwrap();
        assert_eq!(
            levels,
            [
                "object 23: none.pdf claims no PDF/X level",
                "object 24: labelled.pdf claims no PDF/X level",
                "object 25: odd.pdf claims no PDF/X level",
                "object 31: noinfo.pdf claims no PDF/X level"
            ]
        );
        let pages = messages(page_exists(&doc, &settings, &set)).unwrap();
        let no_page = "object 23: the reference has no /Page that is a number or a text string";
        let missing_page = "object 31: the reference has no /Page that is a number or a text string";
        assert_eq!(
            pages,
            [no_page, "object 25: odd.pdf has no page labelled \"i\"", missing_page]
        );
        let identity = messages(target_identity(&doc, &settings, &set)).unwrap();
        assert_eq!(identity[0], "object 20: the reference has no /F");
        assert!(
            identity
                .iter()
                .any(|m| m.ends_with("the embedded bad.pdf cannot be read")),
            "{identity:?}"
        );
        assert_eq!(identity[2], "object 29: the reference's /F names no file");
        assert_eq!(identity[3], "object 32: the reference's /F names no file");
        let conforms = messages(conforms(&doc, &settings, &set)).unwrap();
        assert_eq!(conforms.len(), 3, "{conforms:?}");
        let levels = messages(conformance_level(&doc, &settings, &set)).unwrap();
        assert_eq!(
            levels.last().map(String::as_str),
            Some("object 31: noinfo.pdf claims no PDF/X level")
        );

        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn targets_alone_remote_missing_looping_or_of_other_levels() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-loop-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let version = |v: &str| format!("<pdfxid:GTS_PDFXVersion>{v}</pdfxid:GTS_PDFXVersion>{DOC_ID}");
        std::fs::write(folder.join("me.pdf"), target("", "<< >>", "", &version("PDF/X-4"))).unwrap();
        std::fs::write(folder.join("x2.pdf"), target("", "<< >>", "", &version("PDF/X-2"))).unwrap();
        std::fs::write(folder.join("x5g.pdf"), target("", "<< >>", "", &version("PDF/X-5g"))).unwrap();
        let info = "<< /GTS_PDFXVersion (PDF/X-1:2001) /GTS_PDFXConformance (PDF/X-1a:2001) >>";
        std::fs::write(folder.join("x1a.pdf"), target("", info, "", DOC_ID)).unwrap();
        let refs = [
            "<< /F (me.pdf) /Page 0 >>",
            "<< /F (x2.pdf) /Page 0 >>",
            "<< /F (x5g.pdf) /Page 0 >>",
            "<< /F (x1a.pdf) /Page 0 >>",
            "<< /F << /FS /URL /F (https://example.com/t.pdf) >> /Page 0 >>",
            "<< /F (gone.pdf) /Page 0 >>",
        ];
        let doc = container("", &refs, RENDITION, &[]);
        let me = std::fs::canonicalize(folder.join("me.pdf")).unwrap();
        let settings = Settings {
            level: Level::X5g,
            params: super::super::Params::default(),
            files: Files::Set(folder.clone()),
        };
        let set = resolve(&doc, &settings, &Limits::default(), &[me]);
        let cycles = messages(no_circular(&doc, &settings, &set)).unwrap();
        assert_eq!(cycles, ["object 20: me.pdf refers back to a file being verified"]);
        let levels = messages(conformance_level(&doc, &settings, &set)).unwrap();
        assert_eq!(levels, ["object 21: x2.pdf claims PDF/X-2, which a target may not"]);
        let identity = messages(target_identity(&doc, &settings, &set)).unwrap();
        assert_eq!(
            identity,
            ["object 25: the file set has no gone.pdf: no file at gone.pdf"]
        );
        // A target that fails outweighs one that cannot be judged; warnings are not failures.
        let failing = messages(conforms(&doc, &settings, &set)).unwrap();
        let why = "x4.trapped.present, x4.xmp.create-date";
        assert!(
            failing[0].starts_with(&format!("object 22: x5g.pdf does not conform to PDF/X-5g: {why}")),
            "{failing:?}"
        );
        assert!(
            !failing[0].contains("uuid") && !failing[0].contains("ref-present"),
            "{failing:?}"
        );
        let alone = Settings::from(Level::X5g);
        let set = resolve(&doc, &alone, &Limits::default(), &[]);
        assert_eq!(
            target_identity(&doc, &alone, &set),
            Err("the file is verified alone, without its targets".into())
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_target_too_deep_or_of_another_document_is_said_so() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-deep-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let body = format!("<pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>{DOC_ID}");
        std::fs::write(folder.join("t.pdf"), target("", "<< >>", "/ID [<01> <02>]", &body)).unwrap();
        let other = RENDITION.replace("uuid:1", "uuid:2");
        let doc = container("", &["<< /F (t.pdf) /Page 0 >>"], &other, &[]);
        let settings = Settings {
            level: Level::X5g,
            params: super::super::Params::default(),
            files: Files::Set(folder.clone()),
        };
        let shallow = Limits {
            target_depth: 0,
            ..Limits::default()
        };
        let set = resolve(&doc, &settings, &shallow, &[folder.join("c.pdf")]);
        assert_eq!(
            conforms(&doc, &settings, &set),
            Err("t.pdf is nested more than 0 references deep".into())
        );
        let identity = messages(target_identity(&doc, &settings, &set)).unwrap();
        let why = "its xmpMM:DocumentID differs from the proxy's stRef:documentID";
        assert_eq!(
            identity,
            [format!("object 20: t.pdf is not the file the reference names: {why}")]
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_same_condition_with_another_profile_is_a_warning() {
        let profile = |byte: u8| {
            let mut icc = vec![0u8; 128];
            icc[36..40].copy_from_slice(b"acsp");
            icc[100] = byte;
            stream("/N 4", &icc)
        };
        let intent = "/OutputIntents [<< /S /GTS_PDFX /OutputConditionIdentifier (C) /DestOutputProfile 30 0 R >>]";
        let mine = container(intent, &[], "", &[(30, profile(1))]);
        let theirs = container(intent, &[], "", &[(30, profile(2))]);
        let opened = Opened {
            shown: "t.pdf".into(),
            claim: None,
            file_id: None,
            ids: [None, None, None],
            labels: Vec::new(),
            optional_content: false,
            condition: condition(&theirs),
            verified: Verified::Cycle,
        };
        let set = Set {
            targets: vec![Target {
                proxy: 20,
                name: "t.pdf".into(),
                page: None,
                file_id: None,
                rendition: Vec::new(),
                found: Found::Read(Box::new(opened)),
            }],
        };
        let found = same_printing_condition(&mine, &Level::X5g.into(), &set).unwrap();
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(
            found[0].message,
            "object 20: t.pdf names the same printing condition with another output profile"
        );
    }
}
