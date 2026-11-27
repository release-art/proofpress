//! PDF/X-4 (ISO 15930-7) 6.6's recommendation for every name that is not a font or separation name (those are
//! `x4.names.utf8` and `x4.names.hex-escaped`, in `x4.rs`): UTF-8 once decoded, every byte outside 33 to 126 written
//! as `#xx`. A warning, as the clause only recommends it.
//!
//! A name's raw spelling is kept for values in objects, so those are judged on both counts; dictionary keys and names
//! in content streams keep only their decoded bytes, so they are judged on UTF-8 alone.
// ponytail: keep the raw spelling of keys and content names if their escaping ever matters.

use std::collections::BTreeSet;

use super::Settings;
use super::contents::contents;
use crate::content::Item;
use crate::document::{Buffer, Document, Object, Value};
use crate::findings::{Finding, Location, Severity, X4_NAMES_OTHER_RECOMMENDED};

/// Why a name falls short, given its decoded bytes and, when kept, its raw spelling.
fn fault(bytes: &[u8], raw: Option<&[u8]>) -> Option<&'static str> {
    if std::str::from_utf8(bytes).is_err() {
        Some("is not UTF-8")
    } else if raw.is_some_and(|r| r.iter().any(|b| !(33..=126).contains(b))) {
        Some("has bytes not written as #xx")
    } else {
        None
    }
}

/// The font and separation names `x4.rs` judges, by where their spelling starts.
fn special(doc: &Document) -> BTreeSet<(u32, usize)> {
    let mut out = BTreeSet::new();
    for (num, object, ..) in doc.objects() {
        roles(num, object, &mut out);
    }
    out
}

fn roles(num: u32, object: &Object, out: &mut BTreeSet<(u32, usize)>) {
    match &object.value {
        Value::Dict(dict) => {
            let font = match dict.get(b"Type").and_then(Object::name) {
                Some(b"Font") => dict.get(b"BaseFont"),
                Some(b"FontDescriptor") => dict.get(b"FontName"),
                _ => None,
            };
            out.extend(font.map(|n| (num, n.start)));
            dict.entries().iter().for_each(|(_, v)| roles(num, v, out));
        }
        Value::Array(items) => {
            match (items.first().and_then(Object::name), items.get(1)) {
                (Some(b"Separation"), Some(name)) => {
                    out.insert((num, name.start));
                }
                (
                    Some(b"DeviceN"),
                    Some(Object {
                        value: Value::Array(colorants),
                        ..
                    }),
                ) => {
                    out.extend(colorants.iter().map(|n| (num, n.start)));
                }
                _ => {}
            }
            for item in items {
                roles(num, item, out);
            }
        }
        _ => {}
    }
}

fn push(out: &mut Vec<String>, bytes: &[u8], raw: Option<&[u8]>) {
    if let Some(what) = fault(bytes, raw) {
        out.push(format!("/{} {what}", String::from_utf8_lossy(raw.unwrap_or(bytes))));
    }
}

/// The first faulty name in `object`, as `/name what`, and how many there are.
fn faults(
    doc: &Document,
    spelled: Option<(u32, Buffer)>,
    object: &Object,
    special: &BTreeSet<(u32, usize)>,
    out: &mut Vec<String>,
) {
    match &object.value {
        Value::Name(bytes) => {
            let skip = spelled.is_some_and(|(num, _)| special.contains(&(num, object.start)));
            if !skip {
                let raw = spelled.map(|(_, buffer)| doc.raw(buffer, object.start + 1, object.end));
                push(out, bytes, raw);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| faults(doc, spelled, i, special, out)),
        Value::Dict(dict) => {
            for (key, value) in dict.entries() {
                push(out, key, None);
                faults(doc, spelled, value, special, out);
            }
        }
        _ => {}
    }
}

fn finding(found: &[String], at: Vec<Location>) -> Option<Finding> {
    let first = found.first()?;
    let message = format!(
        "{} name(s) are not UTF-8 with #xx escapes as recommended; the first {first}",
        found.len()
    );
    Some(Finding {
        severity: Severity::Warning,
        ..Finding::new(X4_NAMES_OTHER_RECOMMENDED, message).citing("6.6").at(at)
    })
}

pub(super) fn other_recommended(doc: &Document, _: &Settings) -> Vec<Finding> {
    let special = special(doc);
    let mut out = Vec::new();
    for (num, object, _, buffer) in doc.objects() {
        let mut found = Vec::new();
        faults(doc, Some((num, buffer)), object, &special, &mut found);
        out.extend(finding(&found, vec![Location::Object(num, 0)]));
    }
    for read in contents(doc) {
        let mut found = Vec::new();
        let mut first = None;
        for item in &read.content.items {
            let before = found.len();
            let (objects, at): (Vec<Object>, _) = match item {
                Item::Op(op) => (op.operands.clone(), op.at),
                Item::Image(image) => (
                    vec![Object {
                        value: Value::Dict(image.dict.clone()),
                        start: 0,
                        end: 0,
                    }],
                    image.at,
                ),
            };
            for object in &objects {
                faults(doc, None, object, &special, &mut found);
            }
            if first.is_none() && found.len() > before {
                first = Some(at);
            }
        }
        if let Some(at) = first {
            out.extend(finding(&found, read.at(at)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    #[test]
    fn other_names_are_utf8_and_escaped() {
        let fonts = "/Resources << /Font << /F << /Type /Font /BaseFont /Caf\u{e9} >> >> >> /Odd /Caf#e9 /Fine /Caf#C3#A9 \
            /Spaced /A#20B /R /Raw\u{e9} /Sep [/Separation /Sp\u{e9}cial /DeviceCMYK 5] /#ff 1";
        let doc = paged(
            &[
                (fonts, "/X#ff cs BI /W 1 /H 1 /BPC 8 /CS /G /Q#ff 1 ID x EI /Fine cs"),
                ("", "/Fine cs"),
            ],
            "",
            &[],
        );
        let found: Vec<(Severity, String)> = other_recommended(&doc, &Level::X4.into())
            .into_iter()
            .map(|f| (f.severity, f.message))
            .collect();
        let warn = |m: &str| (Severity::Warning, m.to_owned());
        assert_eq!(
            found,
            [
                warn("3 name(s) are not UTF-8 with #xx escapes as recommended; the first /Caf#e9 is not UTF-8"),
                warn("2 name(s) are not UTF-8 with #xx escapes as recommended; the first /X\u{fffd} is not UTF-8"),
            ]
        );
    }
}
