//! PDF/X-4 (ISO 15930-7) output intent checks: the characterized printing condition (6.4.1) and the PDF/X output
//! intent that names it (6.4.2.1).
//!
//! Rows about the intent's contents judge the one `GTS_PDFX` intent; when there is none, or more than one, they find
//! nothing and `x4.output-intent.present` or `.unique` says why.

use super::Settings;
use crate::common::{icc, text};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Finding, X4_OUTPUT_INTENT_CHANNELS, X4_OUTPUT_INTENT_DEST_PROFILE_PRESENT, X4_OUTPUT_INTENT_IDENTIFIER_IN_REGISTRY,
    X4_OUTPUT_INTENT_IDENTIFIER_PRESENT, X4_OUTPUT_INTENT_IDENTIFIER_REGISTERED,
    X4_OUTPUT_INTENT_IDENTIFIER_TEXT_STRING, X4_OUTPUT_INTENT_NO_DEST_PROFILE_REF, X4_OUTPUT_INTENT_OUTPUT_CONDITION,
    X4_OUTPUT_INTENT_PRESENT, X4_OUTPUT_INTENT_PROFILE_CLASS_PRTR, X4_OUTPUT_INTENT_PROFILE_VERSION,
    X4_OUTPUT_INTENT_REGISTRY_NAME_COLOR_ORG, X4_OUTPUT_INTENT_REGISTRY_NAME_URL, X4_OUTPUT_INTENT_UNIQUE,
};

/// The ICC characterization registry's `RegistryName` (6.4.2.1).
const COLOR_ORG: &[u8] = b"http://www.color.org";

/// The reference names of the ICC Characterization Data Registry: the CMYK characterizations
/// (registry.color.org/cmyk-registry) and the three-component colour encodings (registry.color.org/rgb-registry), as
/// listed on 2026-11-25. The list only grows; nothing on it is marked withdrawn.
const ICC_REGISTRY: [&str; 73] = [
    "FOGRA53",
    "CGATS21-2-CRPC7",
    "APTEC_CTV_8",
    "APTEC_CTV_7",
    "APTEC_CTV_6",
    "FOGRA51",
    "JCS2011",
    "FOGRA50",
    "FOGRA49",
    "FOGRA43",
    "CGATS TR 006",
    "FOGRA39",
    "JC200103",
    "FOGRA27",
    "EUROSB104",
    "APTEC Coated CardBoard",
    "APTEC CCNB",
    "CGATS21-2-CRPC6",
    "CGATS21-2-CRPC5",
    "FOGRA45",
    "FOGRA46",
    "FOGRA41",
    "CGATS TR 001",
    "CGATS TR 003",
    "CGATS TR 005",
    "FOGRA28",
    "JCW2003",
    "EUROSB204",
    "APTEC_CTV_3",
    "FOGRA52",
    "CGATS21-2-CRPC3",
    "FOGRA47",
    "FOGRA44",
    "FOGRA29",
    "JC200104",
    "FOGRA54",
    "CGATS21-2-CRPC4",
    "FOGRA40",
    "FOGRA30",
    "FOGRA48",
    "CGATS21-2-CRPC2",
    "CGATS21-2-CRPC1",
    "FOGRA42",
    "IFRA26",
    "JCN2002",
    "CGATS TR 002",
    "APTEC_CTV_5",
    "APTEC_CTV_4",
    "FOGRA33",
    "FOGRA37",
    "FOGRA31",
    "FOGRA35",
    "FOGRA32",
    "FOGRA34",
    "FOGRA36",
    "FOGRA38",
    "sRGB",
    "Adobe RGB (1998)",
    "bg-sRGB",
    "sYCC",
    "scRGB",
    "scRGB-nl",
    "ROMM RGB",
    "RIMM RGB",
    "ERIMM RGB",
    "eciRGB",
    "opRGB",
    "BT.601",
    "BT.709",
    "BT.2020",
    "BT.2100",
    "DCI P3",
    "Display P3",
];

/// Every output intent dictionary in the catalog's `/OutputIntents` whose `/S` is `GTS_PDFX`.
fn pdfx_intents(doc: &Document) -> Vec<&Dict> {
    let intents = doc
        .catalog()
        .and_then(|c| c.get(b"OutputIntents"))
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)
        .unwrap_or_default();
    intents
        .iter()
        .filter_map(|i| doc.resolve(i)?.dict())
        .filter(|d| d.get(b"S").and_then(|s| doc.resolve(s)).and_then(Object::name) == Some(b"GTS_PDFX"))
        .collect()
}

/// The PDF/X output intent, when there is exactly one.
pub(super) fn the_intent(doc: &Document) -> Option<&Dict> {
    match pdfx_intents(doc).as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

/// The header of the PDF/X intent's `/DestOutputProfile`, when it is a stream with an ICC header.
pub(super) fn profile(doc: &Document) -> Option<icc::Header> {
    match the_intent(doc)?.get(b"DestOutputProfile")?.value {
        Value::Reference(num, _) => icc::header(&doc.stream_data(num)?),
        _ => None,
    }
}

/// The colour space of the printing condition: the embedded profile's, or under PDF/X-4p the external profile's, else
/// the one its reference declares (`/ProfileCS`).
pub(super) fn space(doc: &Document, settings: &Settings) -> Option<[u8; 4]> {
    profile(doc)
        .map(|h| h.space)
        .or_else(|| super::x4p::space(doc, settings))
}

/// The PDF/X intent's `/DestOutputProfile`: its object number and its data.
pub(super) fn profile_stream(doc: &Document) -> Option<(u32, Vec<u8>)> {
    match the_intent(doc)?.get(b"DestOutputProfile")?.value {
        Value::Reference(num, _) => Some((num, doc.stream_data(num)?)),
        _ => None,
    }
}

/// A finding with `message` from `check`, citing 6.4.2.1, when `failed`.
fn when(failed: bool, check: &'static crate::findings::Check, message: &str, clause: &str) -> Vec<Finding> {
    if failed {
        vec![Finding::new(check, message).citing(clause)]
    } else {
        Vec::new()
    }
}

/// 6.4.2.1: the catalog has a `GTS_PDFX` output intent.
pub(super) fn present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let none = pdfx_intents(doc).is_empty();
    when(
        none,
        X4_OUTPUT_INTENT_PRESENT,
        "no output intent has /S /GTS_PDFX",
        "6.4.2.1",
    )
}

/// 6.4.2.1: exactly one output intent is `GTS_PDFX`.
pub(super) fn unique(doc: &Document, _: &Settings) -> Vec<Finding> {
    let count = pdfx_intents(doc).len();
    when(
        count > 1,
        X4_OUTPUT_INTENT_UNIQUE,
        &format!("{count} output intents have /S /GTS_PDFX"),
        "6.4.2.1",
    )
}

pub(super) fn identifier_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let missing = the_intent(doc).is_some_and(|i| i.get(b"OutputConditionIdentifier").is_none());
    when(
        missing,
        X4_OUTPUT_INTENT_IDENTIFIER_PRESENT,
        "the PDF/X output intent has no /OutputConditionIdentifier",
        "6.4.2.1",
    )
}

/// 6.4.2.1: the identifier is a string that decodes as a text string.
pub(super) fn identifier_text_string(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(id) = the_intent(doc).and_then(|i| i.get(b"OutputConditionIdentifier")) else {
        return Vec::new();
    };
    let text = match &doc.resolve(id).map(|o| &o.value) {
        Some(Value::String(bytes, _)) => text::decode(bytes),
        _ => None,
    };
    when(
        text.is_none(),
        X4_OUTPUT_INTENT_IDENTIFIER_TEXT_STRING,
        "/OutputConditionIdentifier is not a valid text string",
        "6.4.2.1",
    )
}

/// 6.4.2.1 recommends that a `/RegistryName` other than the ICC's be a URL about the registry; an empty one names no
/// registry and is taken as absent.
pub(super) fn registry_name_url(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(name) = the_intent(doc).and_then(|i| i.get(b"RegistryName")) else {
        return Vec::new();
    };
    let url = match &doc.resolve(name).map(|o| &o.value) {
        // An empty name names no registry, so it reads as absent (decision log, 2026-11-25).
        Some(Value::String(bytes, _)) => {
            bytes.is_empty()
                || bytes.as_slice() == COLOR_ORG
                || bytes.starts_with(b"http://")
                || bytes.starts_with(b"https://")
        }
        _ => false,
    };
    when(
        !url,
        X4_OUTPUT_INTENT_REGISTRY_NAME_URL,
        "/RegistryName is not a URL",
        "6.4.2.1",
    )
}

/// 6.4.2.1: the PDF/X intent embeds its profile.
pub(super) fn dest_profile_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let missing = the_intent(doc).is_some_and(|i| i.get(b"DestOutputProfile").is_none());
    when(
        missing,
        X4_OUTPUT_INTENT_DEST_PROFILE_PRESENT,
        "the PDF/X output intent has no /DestOutputProfile",
        "6.4.2.1",
    )
}

/// 6.4.2.1: the PDF/X intent does not refer to an external profile (that is X-4p's, Annex A).
pub(super) fn no_dest_profile_ref(doc: &Document, _: &Settings) -> Vec<Finding> {
    let found = the_intent(doc).is_some_and(|i| i.get(b"DestOutputProfileRef").is_some());
    when(
        found,
        X4_OUTPUT_INTENT_NO_DEST_PROFILE_REF,
        "the PDF/X output intent has /DestOutputProfileRef",
        "6.4.2.1",
    )
}

/// 6.4.2.1 recommends `/OutputCondition`.
pub(super) fn output_condition(doc: &Document, _: &Settings) -> Vec<Finding> {
    let missing = the_intent(doc).is_some_and(|i| i.get(b"OutputCondition").is_none());
    when(
        missing,
        X4_OUTPUT_INTENT_OUTPUT_CONDITION,
        "the PDF/X output intent has no /OutputCondition",
        "6.4.2.1",
    )
}

fn signature(bytes: [u8; 4]) -> String {
    String::from_utf8_lossy(&bytes).trim_end().to_owned()
}

/// 6.4.1: the printing condition is gray, RGB or CMYK.
pub(super) fn channels(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(header) = profile(doc) else { return Vec::new() };
    let other = !matches!(&header.space, b"GRAY" | b"RGB " | b"CMYK");
    let message = format!(
        "the output intent's profile is for {}, not gray, RGB or CMYK",
        signature(header.space)
    );
    when(other, X4_OUTPUT_INTENT_CHANNELS, &message, "6.4.1")
}

/// 6.4.2.1: the profile is an output device profile.
pub(super) fn profile_class(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(header) = profile(doc) else { return Vec::new() };
    let message = format!(
        "the output intent's profile has class {}, not prtr",
        signature(header.class)
    );
    when(
        header.class != *b"prtr",
        X4_OUTPUT_INTENT_PROFILE_CLASS_PRTR,
        &message,
        "6.4.2.1",
    )
}

/// 6.4.2.1: the profile is ICC version 2 or 4 (decision log, 2026-11-25: any minor version).
pub(super) fn profile_version(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(header) = profile(doc) else { return Vec::new() };
    let (major, minor) = header.version;
    let message = format!("the output intent's profile is ICC version {major}.{minor}, not 2 or 4");
    when(
        !matches!(major, 2 | 4),
        X4_OUTPUT_INTENT_PROFILE_VERSION,
        &message,
        "6.4.2.1",
    )
}

/// The PDF/X intent's entry `key` as text.
fn text_entry(doc: &Document, key: &[u8]) -> Option<String> {
    match &doc.resolve(the_intent(doc)?.get(key)?)?.value {
        Value::String(bytes, _) => text::decode(bytes),
        _ => None,
    }
}

/// How a `/RegistryName` names the ICC registry: exactly, by a variant of its URL, or not at all.
#[derive(Debug, PartialEq, Eq)]
enum Registry {
    Exact,
    Variant,
    Other,
}

fn registry(name: &str) -> Registry {
    let lower = name.trim().to_ascii_lowercase();
    let host = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    match (
        name.as_bytes() == COLOR_ORG,
        host == "www.color.org" || host == "color.org",
    ) {
        (true, _) => Registry::Exact,
        (false, true) => Registry::Variant,
        (false, false) => Registry::Other,
    }
}

/// 6.4.2.1 recommends a registered identifier and no vague one such as "Custom": a warning for "Custom" (any case)
/// or an empty identifier.
pub(super) fn identifier_registered(doc: &Document, _: &Settings) -> Vec<Finding> {
    let vague = text_entry(doc, b"OutputConditionIdentifier").is_some_and(|id| {
        let id = id.trim();
        id.is_empty() || id.eq_ignore_ascii_case("custom")
    });
    when(
        vague,
        X4_OUTPUT_INTENT_IDENTIFIER_REGISTERED,
        "/OutputConditionIdentifier is \"Custom\" or empty",
        "6.4.2.1",
    )
}

/// 6.4.2.1: an ICC-registered condition names the ICC registry exactly. Checkable only where the file plainly meant
/// that registry: a misspelt color.org URL with an ICC reference name fails. Another registry's URL may name the
/// same condition (Fogra's for FOGRA39), and without `/RegistryName` a match is coincidental, so both pass (oracle).
pub(super) fn registry_name_color_org(doc: &Document, _: &Settings) -> Vec<Finding> {
    let (Some(name), Some(id)) = (
        text_entry(doc, b"RegistryName"),
        text_entry(doc, b"OutputConditionIdentifier"),
    ) else {
        return Vec::new();
    };
    let misspelt = registry(&name) == Registry::Variant && ICC_REGISTRY.contains(&id.as_str());
    let message = format!("/RegistryName is {name:?}, not exactly \"http://www.color.org\"");
    when(misspelt, X4_OUTPUT_INTENT_REGISTRY_NAME_COLOR_ORG, &message, "6.4.2.1")
}

/// 6.4.2.1: with the ICC registry named, the identifier is one of its reference names, exactly. A warning, not the
/// clause's error, by the owner's decision (2026-11-25): Adobe writes ECI profile names under color.org, and 14 Ghent
/// positives that pdfToolbox passes do. Other registries' lists are not held, so they are not judged.
pub(super) fn identifier_in_registry(doc: &Document, _: &Settings) -> Vec<Finding> {
    let (Some(name), Some(id)) = (
        text_entry(doc, b"RegistryName"),
        text_entry(doc, b"OutputConditionIdentifier"),
    ) else {
        return Vec::new();
    };
    let unlisted = registry(&name) != Registry::Other && !ICC_REGISTRY.contains(&id.as_str());
    let message = format!("{id:?} is not a reference name in the ICC registry that /RegistryName names");
    when(unlisted, X4_OUTPUT_INTENT_IDENTIFIER_IN_REGISTRY, &message, "6.4.2.1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 14] = [
        identifier_registered,
        registry_name_color_org,
        identifier_in_registry,
        present,
        unique,
        identifier_present,
        identifier_text_string,
        registry_name_url,
        dest_profile_present,
        no_dest_profile_ref,
        output_condition,
        channels,
        profile_class,
        profile_version,
    ];

    fn ids(objects: &[(u32, &[u8])]) -> Vec<&'static str> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn registries() {
        let ids = |registry: &str, identifier: &str| -> Vec<&'static str> {
            let catalog = format!(
                "<< /OutputIntents [<< /S /GTS_PDFX /OutputCondition (c) /DestOutputProfile 2 0 R {registry} /OutputConditionIdentifier {identifier} >>] >>"
            );
            let doc = Document::open(
                &classic(
                    &[(1, catalog.as_bytes()), (2, b"<< >>")],
                    "<< /Size SIZE /Root 1 0 R >>",
                ),
                &Limits::default(),
            )
            .unwrap();
            [identifier_registered, registry_name_color_org, identifier_in_registry]
                .iter()
                .flat_map(|run| run(&doc, &Level::X4.into()))
                .map(|f| f.check.id)
                .collect()
        };
        assert_eq!(
            ids("/RegistryName (http://www.color.org)", "(FOGRA39)"),
            Vec::<&str>::new()
        );
        assert_eq!(
            ids("", "(FOGRA39)"),
            Vec::<&str>::new(),
            "no registry named: a match is coincidental"
        );
        assert_eq!(
            ids("/RegistryName (http://www.fogra.org)", "(FOGRA39)"),
            Vec::<&str>::new()
        );
        assert_eq!(
            ids("/RegistryName (https://www.color.org/)", "(FOGRA39)"),
            ["x4.output-intent.registry-name-color-org"]
        );
        assert_eq!(
            ids("/RegistryName (http://www.color.org)", "(ISO Coated v2)"),
            ["x4.output-intent.identifier-in-registry"]
        );
        assert_eq!(
            ids("/RegistryName (color.org)", "( custom )"),
            [
                "x4.output-intent.identifier-registered",
                "x4.output-intent.identifier-in-registry"
            ]
        );
        assert_eq!(ids("/RegistryName 5", "()"), ["x4.output-intent.identifier-registered"]);
        assert_eq!(ids("/RegistryName (http://www.color.org)", "/N"), Vec::<&str>::new());
        assert_eq!(
            ids("/RegistryName 9 0 R", "(FOGRA39)"),
            Vec::<&str>::new(),
            "a name that refers to nothing"
        );
        assert_eq!(registry("http://www.color.org"), Registry::Exact);
        let direct = Document::open(
            &classic(
                &[(1, b"<< /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile << >> >>] >>")],
                "<< /Size SIZE /Root 1 0 R >>",
            ),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            profile_stream(&direct),
            None,
            "a profile written directly is not a stream"
        );
        let none = Document::open(
            &classic(&[(1, b"<< >>")], "<< /Size SIZE /Root 1 0 R >>"),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(profile_stream(&none), None, "no intent");
        let bare = Document::open(
            &classic(
                &[(1, b"<< /OutputIntents [<< /S /GTS_PDFX >>] >>")],
                "<< /Size SIZE /Root 1 0 R >>",
            ),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(profile_stream(&bare), None, "no profile");
    }

    #[test]
    fn intents_that_are_odd_rather_than_wrong() {
        let intent =
            |entries: &str| format!("<< /OutputIntents [<< /S /GTS_PDFX /OutputCondition (c) {entries} >>] >>");
        let fine =
            intent("/OutputConditionIdentifier 2 0 R /RegistryName (http://www.color.org) /DestOutputProfile 3 0 R");
        let objects: [(u32, &[u8]); 3] = [
            (1, fine.as_bytes()),
            (2, b"(FOGRA39)"),
            (3, b"<< /Length 4 >>\nstream\nnone\nendstream"),
        ];
        assert_eq!(
            ids(&objects),
            Vec::<&str>::new(),
            "a profile without an ICC header is left to the colour checks"
        );
        let direct = intent("/OutputConditionIdentifier (x) /RegistryName 5 /DestOutputProfile << >>");
        assert_eq!(ids(&[(1, direct.as_bytes())]), ["x4.output-intent.registry-name-url"]);
        let bare = intent("/OutputConditionIdentifier /N /RegistryName (https://r.example) /DestOutputProfile 2 0 R");
        let found = ids(&[(1, bare.as_bytes()), (2, b"<< >>")]);
        assert_eq!(
            found,
            ["x4.output-intent.identifier-text-string"],
            "a profile that is not a stream is unread"
        );
        let named = intent("/OutputConditionIdentifier (x) /RegistryName ()");
        assert_eq!(ids(&[(1, named.as_bytes())]), ["x4.output-intent.dest-profile-present"]);
        let unreadable = b"<< /OutputIntents [7 0 R 8 0 R] >>";
        assert_eq!(ids(&[(1, unreadable), (8, b"5")]), ["x4.output-intent.present"]);
    }
}
