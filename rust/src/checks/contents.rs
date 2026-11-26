//! Checks on content streams, read by tier 6: every content stream in the file can be read (ISO 32000-2, 7.8.2 and
//! 8.9.7) and uses only defined operators; PDF/X-4 6.19 (no operator the PDF 1.6 Reference lacks, even inside
//! `BX`/`EX`) and 6.23 (rendering intents), with 6.13's `/RI`.
//!
//! Content streams are each page's `/Contents`, read as one, and every form `XObject` (annotation appearances among
//! them), tiling pattern and Type 3 glyph, each read alone.

use super::Settings;
use super::walk::dicts;
use crate::content::{self, At, Content, Item, OPERATORS, Problem};
use crate::document::{Document, Object};
use crate::findings::{
    CONTENT_COMPATIBILITY, CONTENT_DAMAGED, CONTENT_OPERATOR, Check, Finding, Location, X4_CONTENT_KNOWN_OPERATORS,
    X4_CONTENT_NO_BX_EX, X4_INTENT_KNOWN_VALUES,
};
use crate::graphics;
use crate::pipeline::Limits;

/// The four rendering intents of the PDF 1.6 Reference, 4.5.4, which 6.23 allows.
const INTENTS: [&[u8]; 4] = [
    b"AbsoluteColorimetric",
    b"RelativeColorimetric",
    b"Saturation",
    b"Perceptual",
];

/// One content: where it lives, the object numbers of its streams, and what tier 6 read.
pub(super) struct Read {
    /// The page (counted from 1) or the object the content belongs to.
    owner: Location,
    streams: Vec<u32>,
    /// The streams whose filters could not be decoded.
    undecoded: Vec<u32>,
    pub(super) content: Content,
}

/// Every content in the file, read.
pub(super) fn contents(doc: &Document) -> Vec<Read> {
    let limits = Limits::default();
    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let streams = graphics::stream_numbers(doc, page.get(b"Contents"));
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        out.push(read(
            doc,
            Location::Page(number),
            streams,
            page.get(b"Resources"),
            &limits,
        ));
    }
    for node in dicts(doc) {
        let form = node.name(b"Subtype") == Some(b"Form");
        let pattern = node.dict.get(b"PatternType").and_then(Object::integer) == Some(1);
        if node.stream && (form || pattern || node.inside(b"CharProcs")) {
            out.push(read(
                doc,
                Location::Object(node.num, 0),
                vec![node.num],
                node.dict.get(b"Resources"),
                &limits,
            ));
        }
    }
    out
}

fn read(doc: &Document, owner: Location, streams: Vec<u32>, resources: Option<&Object>, limits: &Limits) -> Read {
    let data: Vec<(u32, Option<Vec<u8>>)> = streams.iter().map(|&n| (n, doc.stream_data(n))).collect();
    let undecoded = data.iter().filter(|(_, d)| d.is_none()).map(|(n, _)| *n).collect();
    let decoded: Vec<&[u8]> = data.iter().filter_map(|(_, d)| d.as_deref()).collect();
    let spaces = resources
        .and_then(|r| doc.resolve(r))
        .and_then(Object::dict)
        .and_then(|r| r.get(b"ColorSpace"))
        .and_then(|c| doc.resolve(c))
        .and_then(Object::dict);
    let named = |name: &[u8]| graphics::components(doc, spaces?.get(name)?);
    Read {
        owner,
        streams,
        undecoded,
        content: content::read(&decoded, limits, &named),
    }
}

impl Read {
    /// The location chain of a point in this content.
    pub(super) fn at(&self, at: At) -> Vec<Location> {
        let mut chain = vec![self.owner.clone()];
        if let Some(&num) = self.streams.get(at.stream)
            && self.owner != Location::Object(num, 0)
        {
            chain.push(Location::Object(num, 0));
        }
        chain.push(Location::Offset(at.offset));
        chain
    }

    fn finding(&self, check: &'static Check, at: At, message: String, clause: &str) -> Finding {
        Finding::new(check, message).citing(clause).at(self.at(at))
    }
}

fn name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every content stream can be read to its end: its filters decode, and tier 6 meets no damage.
pub(super) fn damaged(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for &num in &read.undecoded {
            let message = format!("content stream {num} cannot be decoded");
            out.push(
                Finding::new(CONTENT_DAMAGED, message)
                    .citing("7.8.2")
                    .at(vec![read.owner.clone(), Location::Object(num, 0)]),
            );
        }
        for (at, problem) in &read.content.problems {
            let (what, clause) = match problem {
                Problem::Syntax(_) => ("content that cannot be read on".to_owned(), "7.8.2"),
                Problem::InlineEnd => ("an inline image whose end cannot be determined".to_owned(), "8.9.7"),
                Problem::InlineLength { given, measured } => (
                    format!("an inline image whose /L is {given} but whose data is {measured} bytes"),
                    "8.9.7",
                ),
                Problem::InlineDict => ("an inline image dictionary that is malformed".to_owned(), "8.9.7"),
                Problem::NoEi => ("an inline image not followed by EI".to_owned(), "8.9.7"),
                Problem::UnknownOperator(_) | Problem::Compatibility => continue,
            };
            out.push(read.finding(CONTENT_DAMAGED, *at, format!("{what}; the rest is not read"), clause));
        }
    }
    out
}

/// An operator ISO 32000 does not define is used outside `BX`/`EX`.
pub(super) fn operator(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for (at, problem) in &read.content.problems {
            if let Problem::UnknownOperator(word) = problem {
                let message = format!("the operator {} is not defined and is not inside BX/EX", name(word));
                out.push(read.finding(CONTENT_OPERATOR, *at, message, "7.8.2"));
            }
        }
    }
    out
}

/// `BX` and `EX` pair up.
pub(super) fn compatibility(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for (at, problem) in &read.content.problems {
            if *problem == Problem::Compatibility {
                out.push(read.finding(
                    CONTENT_COMPATIBILITY,
                    *at,
                    "BX and EX do not pair up".to_owned(),
                    "7.8.2",
                ));
            }
        }
    }
    out
}

/// 6.19: no operator outside the PDF 1.6 Reference, inside `BX`/`EX` or not.
pub(super) fn known_operators(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for item in &read.content.items {
            if let Item::Op(op) = item
                && !OPERATORS.contains(&op.name.as_slice())
            {
                let message = format!("the operator {} is not in the PDF 1.6 Reference", name(&op.name));
                out.push(read.finding(X4_CONTENT_KNOWN_OPERATORS, op.at, message, "6.19"));
            }
        }
    }
    out
}

/// 6.19 recommends against `BX`/`EX`.
pub(super) fn no_bx_ex(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for item in &read.content.items {
            if let Item::Op(op) = item
                && op.name == b"BX"
            {
                out.push(read.finding(X4_CONTENT_NO_BX_EX, op.at, "a BX/EX section".to_owned(), "6.19"));
            }
        }
    }
    out
}

/// A finding unless `value` is one of the four rendering intents.
fn intent(value: Option<&Object>, place: &str, located: Vec<Location>) -> Option<Finding> {
    let value = value?;
    let known = value.name().is_some_and(|n| INTENTS.contains(&n));
    let shown = value.name().map_or_else(|| "not a name".to_owned(), name);
    (!known).then(|| {
        Finding::new(
            X4_INTENT_KNOWN_VALUES,
            format!("{place} names the rendering intent {shown}"),
        )
        .citing("6.23")
        .at(located)
    })
}

/// 6.23 and 6.13: every rendering intent is one of the four: the `ri` operator, an inline image's `/Intent`, an
/// `ExtGState`'s `/RI` and an image `XObject`'s `/Intent`.
pub(super) fn known_intents(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for read in contents(doc) {
        for item in &read.content.items {
            let found = match item {
                Item::Op(op) if op.name == b"ri" => intent(op.operands.last(), "ri", read.at(op.at)),
                Item::Image(image) => intent(image.dict.get(b"Intent"), "an inline image", read.at(image.at)),
                Item::Op(_) => None,
            };
            out.extend(found);
        }
    }
    for node in dicts(doc) {
        let at = vec![Location::Object(node.num, 0)];
        if node.inside(b"ExtGState") || node.name(b"Type") == Some(b"ExtGState") {
            out.extend(intent(
                node.dict.get(b"RI").and_then(|v| doc.resolve(v)),
                "an ExtGState's /RI",
                at.clone(),
            ));
        }
        if node.stream && node.name(b"Subtype") == Some(b"Image") {
            out.extend(intent(
                node.dict.get(b"Intent").and_then(|v| doc.resolve(v)),
                "an image's /Intent",
                at,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 6] = [
        damaged,
        operator,
        compatibility,
        known_operators,
        no_bx_ex,
        known_intents,
    ];

    fn stream(data: &str) -> Vec<u8> {
        format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes()
    }

    fn found(objects: &[(u32, &[u8])]) -> Vec<(&'static str, String)> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| (f.check.id, f.message))
            .collect()
    }

    #[test]
    fn pages_name_their_content_in_every_way() {
        let (a, b) = (stream("q"), stream("Q /Vivid ri"));
        let objects: [(u32, &[u8]); 8] = [
            (1, b"<< /Pages 2 0 R >>"),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R << /Type /Page /Contents 9 >>] >>",
            ),
            (3, b"<< /Type /Page /Contents [6 0 R 7 0 R] >>"),
            (4, b"<< /Type /Page /Contents 8 0 R >>"),
            (5, b"<< /Type /Page >>"),
            (6, &a),
            (7, &b),
            (8, b"[6 0 R 7 0 R]"),
        ];
        let intents: Vec<String> = found(&objects).into_iter().map(|(_, m)| m).collect();
        assert_eq!(
            intents,
            [
                "ri names the rendering intent Vivid",
                "ri names the rendering intent Vivid"
            ]
        );
    }

    /// A stream object with `entries` in its dictionary.
    fn with(entries: &str, data: &str) -> Vec<u8> {
        format!("<< {entries} /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes()
    }

    #[test]
    fn forms_patterns_glyphs_and_their_resources() {
        let spaces = "/Resources << /ColorSpace << /A /DeviceRGB /B [/ICCBased 9 0 R] /C [/Indexed /DeviceRGB 1 <00>] \
            /D [/DeviceN [/X /Y] /DeviceGray 10 0 R] /E [/Separation /S /DeviceGray 10 0 R] >> >>";
        let form = with(
            &format!("/Subtype /Form {spaces}"),
            "BI /W 1 /H 1 /BPC 8 /CS /A ID abc EI BI /W 1 /H 1 /BPC 8 /CS /B ID abcd EI BX frob",
        );
        let pattern = with(
            &format!("/PatternType 1 {spaces}"),
            "BI /W 2 /H 1 /BPC 8 /CS /C ID ab EI BI /W 1 /H 1 /BPC 8 /CS /D ID ab EI frob EX EX",
        );
        let glyph = with(
            spaces,
            "BI /W 1 /H 1 /BPC 8 /CS /E /Intent /Vivid ID a EI BI /W 1 /H 1 /BPC 8 /CS /Lab ID abc EI q (x",
        );
        let objects: [(u32, &[u8]); 8] = [
            (
                1,
                b"<< /A 3 0 R /B 4 0 R /C << /CharProcs << /g 5 0 R >> >> /D 6 0 R /E 7 0 R >>",
            ),
            (3, &form),
            (4, &pattern),
            (5, &glyph),
            (6, b"<< /Subtype /Form /Filter /Nope /Length 1 >>\nstream\nx\nendstream"),
            (7, b"<< /Subtype /Image /Intent 5 /Length 1 >>\nstream\nx\nendstream"),
            (9, b"<< /N 4 /Length 0 >>\nstream\n\nendstream"),
            (10, b"<< /FunctionType 2 >>"),
        ];
        let got = found(&objects);
        let ids: Vec<&str> = got.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            [
                "content.damaged",
                "content.damaged",
                "content.operator",
                "content.compatibility",
                "content.compatibility",
                "content.compatibility",
                "x4.content.known-operators",
                "x4.content.known-operators",
                "x4.content.no-bx-ex",
                "x4.intent.known-values",
                "x4.intent.known-values",
            ],
            "{got:#?}"
        );
    }

    #[test]
    fn the_extgstate_and_odd_colour_spaces() {
        let page = stream("BI /W 1 /H 1 /BPC 8 /CS /X ID a EI");
        let objects: [(u32, &[u8]); 4] = [
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, b"<< /Type /Page /Contents 4 0 R /Resources << /ColorSpace << /X 5 >> /ExtGState << /G << /RI /Saturation >> /H << /RI (x) >> >> >> >>"),
            (4, &page),
        ];
        let got = found(&objects);
        assert_eq!(
            got.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(),
            [
                "an inline image whose end cannot be determined; the rest is not read",
                "an ExtGState's /RI names the rendering intent not a name"
            ]
        );
        let doc = Document::open(
            &classic(&[(1, b"<< >>")], "<< /Size SIZE /Root 1 0 R >>"),
            &Limits::default(),
        )
        .unwrap();
        let obj = |text: &[u8]| {
            let pdf = classic(
                &[(1, &[b"<< /X ".as_slice(), text, b" >>"].concat())],
                "<< /Size SIZE /Root 1 0 R >>",
            );
            let d = Document::open(&pdf, &Limits::default()).unwrap();
            graphics::components(&d, d.catalog().unwrap().get(b"X").unwrap())
        };
        assert_eq!(
            (
                obj(b"/CalRGB"),
                obj(b"/DeviceCMYK"),
                obj(b"/Pattern"),
                obj(b"[]"),
                obj(b"[/ICCBased 9 0 R]")
            ),
            (Some(3), Some(4), None, None, None)
        );
        assert_eq!(obj(b"[/DeviceN 5]"), None);
        assert_eq!(
            (obj(b"[/ICCBased 5]"), obj(b"[/ICCBased << >>]")),
            (None, None),
            "a profile that is not one, or has no /N"
        );
        assert!(doc.catalog().is_some());
    }

    #[test]
    fn inline_image_problems_and_lookups_that_find_nothing() {
        let pages = |contents: &[(&str, &str)]| -> Vec<String> {
            let mut objects: Vec<(u32, Vec<u8>)> = vec![(1, b"<< /Pages 2 0 R >>".to_vec())];
            let kids: Vec<String> = (0..contents.len()).map(|i| format!("{} 0 R", 10 + 2 * i)).collect();
            objects.push((2, format!("<< /Type /Pages /Kids [{}] >>", kids.join(" ")).into_bytes()));
            for (i, (resources, data)) in contents.iter().enumerate() {
                let n = u32::try_from(10 + 2 * i).unwrap();
                objects.push((
                    n,
                    format!("<< /Type /Page /Contents [{} 0 R 5] /Resources {resources} >>", n + 1).into_bytes(),
                ));
                objects.push((n + 1, stream(data)));
            }
            objects.push((9, b"<< /N 3 >>".to_vec()));
            let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
            found(&refs).into_iter().map(|(_, m)| m).collect()
        };
        let image =
            |space: &str, extra: &str, data: &str| format!("BI /W 1 /H 1 /BPC 8 /CS {space} {extra} ID {data} EI");
        let messages = pages(&[
            ("<< >>", &image("/A", "", "a")),
            ("<< /ColorSpace << >> >>", &image("/A", "", "a")),
            ("<< /ColorSpace << /A 8 0 R >> >>", &image("/A", "", "a")),
            ("<< /ColorSpace << /A [5] >> >>", &image("/A", "", "a")),
            ("<< /ColorSpace << /A [/ICCBased 9 0 R] >> >>", &image("/A", "", "abc")),
            ("<< /ColorSpace << /A [/ICCBased 8 0 R] >> >>", &image("/A", "", "a")),
            ("<< /ColorSpace << /A [/DeviceN] >> >>", &image("/A", "", "a")),
            ("<< >>", &image("/G", "/L 3", "a")),
            ("<< >>", "BI 5 ID a EI"),
            ("<< >>", &image("/G", "", "ab")),
        ]);
        let unknown = "an inline image whose end cannot be determined; the rest is not read";
        assert_eq!(
            messages,
            [
                unknown,
                unknown,
                unknown,
                // The ICCBased profile's /N 3 makes "abc" exactly the image.
                unknown,
                unknown,
                unknown,
                "an inline image whose /L is 3 but whose data is 1 bytes; the rest is not read",
                "an inline image dictionary that is malformed; the rest is not read",
                "an inline image not followed by EI; the rest is not read",
            ]
        );
    }
}
