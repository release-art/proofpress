//! PDF/X-4 (ISO 15930-7) 6.3, complete exchange: every resource a print element uses is in the file (oracle,
//! 2026-11-26). Two ways one is not:
//!
//! - content names a resource its resources do not define (ISO 32000-1 7.8.3): an `XObject` (`Do`), a font (`Tf`), a
//!   colour space (`cs`, `CS`, an inline image's `/ColorSpace`; device names are not resources), a pattern (`scn`,
//!   `SCN` while a Pattern space is current), a graphics state (`gs`), a shading (`sh`), or the optional content of
//!   `BDC`/`DP` under `/OC`. An entry pointing at a missing object counts as undefined.
//! - a composite font that text uses names a `CMap` that is neither embedded nor one of ISO 32000-1 Table 118's
//!   predefined `CMap`s, or embeds one whose `/UseCMap` names such a `CMap`.
//!
//! Unembedded fonts are 6.5.1's (`x4.font.embedded`); external streams and reference `XObjects` are 6.7's. Contents
//! are each page's, and every form and tiling pattern with its own `/Resources`.
// ponytail: forms without `/Resources`, appearance streams and Type 3 glyph procedures are not judged here; a form's
// page is not known outside the walk, and appearances are print elements only for TrapNet and PrinterMark annotations.

use std::collections::BTreeSet;

use super::Settings;
use super::walk::dicts;
use crate::content::{self, At, Item};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{Finding, Location, X4_COMPLETE_RESOURCES_INSIDE};
use crate::graphics::{self, Event};
use crate::pipeline::Limits;

/// ISO 32000-1 Table 118's predefined `CMap`s.
const PREDEFINED: [&[u8]; 61] = [
    b"GB-EUC-H",
    b"GB-EUC-V",
    b"GBpc-EUC-H",
    b"GBpc-EUC-V",
    b"GBK-EUC-H",
    b"GBK-EUC-V",
    b"GBKp-EUC-H",
    b"GBKp-EUC-V",
    b"GBK2K-H",
    b"GBK2K-V",
    b"UniGB-UCS2-H",
    b"UniGB-UCS2-V",
    b"UniGB-UTF16-H",
    b"UniGB-UTF16-V",
    b"B5pc-H",
    b"B5pc-V",
    b"HKscs-B5-H",
    b"HKscs-B5-V",
    b"ETen-B5-H",
    b"ETen-B5-V",
    b"ETenms-B5-H",
    b"ETenms-B5-V",
    b"CNS-EUC-H",
    b"CNS-EUC-V",
    b"UniCNS-UCS2-H",
    b"UniCNS-UCS2-V",
    b"UniCNS-UTF16-H",
    b"UniCNS-UTF16-V",
    b"83pv-RKSJ-H",
    b"90ms-RKSJ-H",
    b"90ms-RKSJ-V",
    b"90msp-RKSJ-H",
    b"90msp-RKSJ-V",
    b"90pv-RKSJ-H",
    b"Add-RKSJ-H",
    b"Add-RKSJ-V",
    b"EUC-H",
    b"EUC-V",
    b"Ext-RKSJ-H",
    b"Ext-RKSJ-V",
    b"H",
    b"V",
    b"UniJIS-UCS2-H",
    b"UniJIS-UCS2-V",
    b"UniJIS-UCS2-HW-H",
    b"UniJIS-UCS2-HW-V",
    b"UniJIS-UTF16-H",
    b"UniJIS-UTF16-V",
    b"KSC-EUC-H",
    b"KSC-EUC-V",
    b"KSCms-UHC-H",
    b"KSCms-UHC-V",
    b"KSCms-UHC-HW-H",
    b"KSCms-UHC-HW-V",
    b"KSCpc-EUC-H",
    b"UniKS-UCS2-H",
    b"UniKS-UCS2-V",
    b"UniKS-UTF16-H",
    b"UniKS-UTF16-V",
    b"Identity-H",
    b"Identity-V",
];

/// Colour space names that are not resources (8.6.3, and 8.9.7's abbreviations for inline images).
const DEVICE: [&[u8]; 7] = [
    b"DeviceGray",
    b"DeviceRGB",
    b"DeviceCMYK",
    b"Pattern",
    b"G",
    b"RGB",
    b"CMYK",
];

/// Whether `resources` defines `name` under `category`, as an object that is there.
fn defined(doc: &Document, resources: &Dict, category: &[u8], name: &[u8]) -> bool {
    resources
        .get(category)
        .and_then(|c| doc.resolve(c))
        .and_then(Object::dict)
        .and_then(|c| c.get(name))
        .and_then(|o| doc.resolve(o))
        .is_some_and(|o| o.value != Value::Null)
}

/// The resource names one content uses that its resources do not define, as `/name (category)`, and where the first is.
fn undefined(doc: &Document, data: &[&[u8]], resources: &Dict) -> (Vec<String>, Option<At>) {
    let spaces = resources
        .get(b"ColorSpace")
        .and_then(|c| doc.resolve(c))
        .and_then(Object::dict);
    let named = |name: &[u8]| {
        spaces
            .and_then(|s| s.get(name))
            .and_then(|s| graphics::components(doc, s))
    };
    let content = content::read(data, &Limits::default(), &named);
    let is_pattern = |name: &[u8]| {
        name == b"Pattern"
            || spaces
                .and_then(|s| s.get(name))
                .and_then(|s| doc.resolve(s))
                .is_some_and(|s| {
                    s.name() == Some(b"Pattern")
                        || s.array().and_then(<[Object]>::first).and_then(Object::name) == Some(b"Pattern")
                })
    };
    let (mut fill_pattern, mut stroke_pattern) = (false, false);
    let mut missing = Vec::new();
    let mut first = None;
    for item in &content.items {
        let mut want = |category: &'static [u8], name: Option<&Object>, at: At| {
            if let Some(name) = name.and_then(Object::name)
                && !defined(doc, resources, category, name)
            {
                let category = String::from_utf8_lossy(category);
                missing.push(format!("/{} ({category})", String::from_utf8_lossy(name)));
                first = first.or(Some(at));
            }
        };
        match item {
            Item::Image(image) => {
                let space = image
                    .dict
                    .get(b"ColorSpace")
                    .filter(|s| !s.name().is_some_and(|n| DEVICE.contains(&n)));
                want(b"ColorSpace", space, image.at);
            }
            Item::Op(op) => {
                let first_operand = op.operands.first();
                let last = op.operands.last();
                match op.name.as_slice() {
                    b"Do" => want(b"XObject", first_operand, op.at),
                    b"Tf" => want(b"Font", first_operand, op.at),
                    b"gs" => want(b"ExtGState", first_operand, op.at),
                    b"sh" => want(b"Shading", first_operand, op.at),
                    b"cs" | b"CS" => {
                        let name = first_operand.and_then(Object::name).unwrap_or_default();
                        let pattern = is_pattern(name);
                        if op.name == b"cs" {
                            fill_pattern = pattern;
                        } else {
                            stroke_pattern = pattern;
                        }
                        want(b"ColorSpace", first_operand.filter(|_| !DEVICE.contains(&name)), op.at);
                    }
                    b"scn" if fill_pattern => want(b"Pattern", last, op.at),
                    b"SCN" if stroke_pattern => want(b"Pattern", last, op.at),
                    b"BDC" | b"DP" if first_operand.and_then(Object::name) == Some(b"OC") => {
                        want(b"Properties", op.operands.get(1), op.at);
                    }
                    _ => {}
                }
            }
        }
    }
    (missing, first)
}

pub(super) fn resources_inside(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut judge = |owner: Location, streams: Vec<u32>, resources: Option<&Dict>| {
        let data: Vec<Vec<u8>> = streams.iter().filter_map(|&n| doc.stream_data(n)).collect();
        let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let empty = Dict::default();
        let (missing, at) = undefined(doc, &slices, resources.unwrap_or(&empty));
        if let (Some(name), Some(at)) = (missing.first(), at) {
            let message = format!(
                "{} resource name(s) its resources do not define, the first {name}",
                missing.len()
            );
            let mut chain = vec![owner];
            chain.extend(streams.get(at.stream).map(|&n| Location::Object(n, 0)));
            chain.push(Location::Offset(at.offset));
            out.push(
                Finding::new(X4_COMPLETE_RESOURCES_INSIDE, message)
                    .citing("6.3")
                    .at(chain),
            );
        }
    };
    for (i, page) in doc.pages().iter().enumerate() {
        let resources = page
            .get(b"Resources")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict);
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        judge(
            Location::Page(number),
            graphics::stream_numbers(doc, page.get(b"Contents")),
            resources,
        );
    }
    for node in dicts(doc) {
        let form = node.name(b"Subtype") == Some(b"Form");
        let pattern = node.dict.get(b"PatternType").and_then(Object::integer) == Some(1);
        let resources = node
            .dict
            .get(b"Resources")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict);
        if node.stream && (form || pattern) && !node.inside(b"AP") && resources.is_some() {
            judge(Location::Object(node.num, 0), vec![node.num], resources);
        }
    }
    out.extend(cmaps(doc));
    out
}

/// Composite fonts that text uses whose `CMap` is outside the file.
fn cmaps(doc: &Document) -> Vec<Finding> {
    let used: BTreeSet<u32> = graphics::walk(doc)
        .into_iter()
        .filter_map(|event| match event {
            Event::Paint { ink, .. } => ink.font.and_then(|f| f.object),
            _ => None,
        })
        .collect();
    let outside = |name: &[u8]| !PREDEFINED.contains(&name);
    let mut out = Vec::new();
    for (num, object, ..) in doc.objects().filter(|(n, ..)| used.contains(n)) {
        let Some(font) = object
            .dict()
            .filter(|d| d.get(b"Subtype").and_then(Object::name) == Some(b"Type0"))
        else {
            continue;
        };
        let encoding = font.get(b"Encoding").and_then(|e| doc.resolve(e));
        let problem = match encoding.map(|e| &e.value) {
            Some(Value::Name(name)) if outside(name) => Some(format!(
                "names the CMap /{}, which is neither embedded nor predefined",
                String::from_utf8_lossy(name)
            )),
            Some(Value::Dict(cmap)) => cmap
                .get(b"UseCMap")
                .and_then(Object::name)
                .filter(|n| outside(n))
                .map(|n| {
                    format!(
                        "embeds a CMap that uses /{}, which is neither embedded nor predefined",
                        String::from_utf8_lossy(n)
                    )
                }),
            _ => None,
        };
        if let Some(problem) = problem {
            let base = font.get(b"BaseFont").and_then(Object::name).unwrap_or(b"?");
            let message = format!(
                "the composite font {} (object {num}) {problem}",
                String::from_utf8_lossy(base)
            );
            out.push(
                Finding::new(X4_COMPLETE_RESOURCES_INSIDE, message)
                    .citing("6.3")
                    .at(vec![Location::Object(num, 0)]),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    fn messages(doc: &Document) -> Vec<String> {
        resources_inside(doc, &Level::X4.into())
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn resources_content_names() {
        let resources = "/Resources << /Font << /F << /Subtype /Type1 >> >> /ColorSpace << /P /Pattern /Q [/Pattern] \
            /S 9 0 R >> /XObject << /Gone 99 0 R /Nil null >> >>";
        let used = "/F 9 Tf /DeviceRGB cs /P cs /Pat scn /Q CS /Pat2 SCN /S cs 1 scn /X Do /Gone Do /Nil Do /G gs /Sh sh \
            /OC /L BDC EMC /Span << >> BDC EMC /Tag /Prop DP BI /W 1 /H 1 /BPC 8 /CS /Mine /L 1 ID x EI \
            BI /W 1 /H 1 /BPC 8 /CS /G ID x EI BI /W 1 /H 1 /BPC 8 /CS /S ID x EI";
        let form = |extra: &str, data: &str| {
            format!(
                "<< /Subtype /Form {extra} /Length {} >>\nstream\n{data}\nendstream",
                data.len()
            )
            .into_bytes()
        };
        let more = [
            (9, b"[/Separation /S /DeviceGray 5]".to_vec()),
            (20, form("/Resources << >>", "/Y Do")),
            (21, form("", "/Z Do")),
            (22, form("/Resources << >>", "q Q")),
        ];
        let doc = paged(&[(resources, used), ("", "")], "/Forms [20 0 R 21 0 R 22 0 R]", &more);
        assert_eq!(
            messages(&doc),
            [
                "9 resource name(s) its resources do not define, the first /Pat (Pattern)",
                "1 resource name(s) its resources do not define, the first /Y (XObject)",
            ]
        );
    }

    #[test]
    fn composite_fonts_cmaps() {
        let type0 = |encoding: &str| format!("<< /Type /Font /Subtype /Type0 /BaseFont /CJK /Encoding {encoding} >>");
        let cmap = |use_cmap: &str| format!("<< /Type /CMap {use_cmap} /Length 0 >>\nstream\n\nendstream");
        let more = [
            (10, type0("/UniJIS-UCS2-H").into_bytes()),
            (11, type0("/UniJIS2004-UTF16-H").into_bytes()),
            (12, type0("20 0 R").into_bytes()),
            (13, type0("21 0 R").into_bytes()),
            (14, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
            (15, type0("5").into_bytes()),
            (20, cmap("/UseCMap /Custom-H").into_bytes()),
            (21, cmap("/UseCMap /H").into_bytes()),
        ];
        let fonts = (10..=15)
            .map(|n| format!("/F{n} {n} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        let shown = (10..=15)
            .map(|n| format!("/F{n} 9 Tf <0041> Tj"))
            .collect::<Vec<_>>()
            .join(" ");
        let doc = paged(
            &[(
                &format!("/Resources << /Font << {fonts} >> >>"),
                &format!("BT {shown} ET"),
            )],
            "",
            &more,
        );
        assert_eq!(
            messages(&doc),
            [
                "the composite font CJK (object 11) names the CMap /UniJIS2004-UTF16-H, which is neither embedded nor \
                 predefined",
                "the composite font CJK (object 12) embeds a CMap that uses /Custom-H, which is neither embedded nor \
                 predefined",
            ]
        );
    }
}
