//! PDF/X-4 (ISO 15930-7) 6.25, the architectural limits of the PDF 1.6 Reference's Table C.1 (decision X4-Q14), for the
//! values a file is written in: integers within 32 bits, reals within 3.403e38 in magnitude (one nearer to 0 than
//! 1.175e-38 just reads as 0), names of at most 127 bytes and, inside content streams, strings of at most 32,767 bytes.
//! Names and strings are measured as decoded; dictionary keys are names too.
//!
//! And for what the file does (oracle, 2026-11-26): graphics states nested at most 28 deep, counting each `q` and each
//! form or tiling pattern being drawn (the PDF 1.6 Reference saves the state for those as if by `q`); at most 32
//! components in every `DeviceN` space in the file; and no CID above 65,535 in an embedded `CMap`, which is where every
//! CID a code can select comes from. Type 3 glyph procedures and annotation appearances are not walked, so their levels
//! are not counted.

use super::Settings;
use super::contents::contents;
use crate::common::pyfmt::g;
use crate::content::Item;
use crate::document::{Document, Object, Value};
use crate::findings::{Finding, Location, X4_LIMITS_GRAPHICS, X4_LIMITS_LEXICAL};
use crate::graphics::{self, Event};

const LARGEST_REAL: f64 = 3.403e38;
const LONGEST_NAME: usize = 127;
const LONGEST_CONTENT_STRING: usize = 32_767;
const DEEPEST_NESTING: usize = 28;
const MOST_COMPONENTS: usize = 32;
const LARGEST_CID: u64 = 65_535;

/// Each value in `object` beyond the limits, in words; strings count only `in_content`.
fn excesses(object: &Object, in_content: bool, out: &mut Vec<String>) {
    let name = |bytes: &[u8], out: &mut Vec<String>| {
        if bytes.len() > LONGEST_NAME {
            out.push(format!("a name of {} bytes, over 127", bytes.len()));
        }
    };
    match &object.value {
        Value::Integer(n) if i32::try_from(*n).is_err() => out.push(format!("the integer {n}, outside 32 bits")),
        Value::Real(r) if r.abs() > LARGEST_REAL => out.push(format!("the real {}, over 3.403e38", g(*r))),
        Value::Name(n) => name(n, out),
        Value::String(s, _) if in_content && s.len() > LONGEST_CONTENT_STRING => {
            out.push(format!(
                "a string of {} bytes in a content stream, over 32,767",
                s.len()
            ));
        }
        Value::Array(items) => items.iter().for_each(|i| excesses(i, in_content, out)),
        Value::Dict(dict) => {
            for (key, value) in dict.entries() {
                name(key, out);
                excesses(value, in_content, out);
            }
        }
        _ => {}
    }
}

fn finding(found: &[String], at: Vec<Location>) -> Option<Finding> {
    let first = found.first()?;
    let message = format!("{} value(s) beyond the PDF 1.6 limits, the first {first}", found.len());
    Some(Finding::new(X4_LIMITS_LEXICAL, message).citing("6.25").at(at))
}

pub(super) fn lexical(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (num, object, ..) in doc.objects() {
        let mut found = Vec::new();
        excesses(object, false, &mut found);
        out.extend(finding(&found, vec![Location::Object(num, 0)]));
    }
    for read in contents(doc) {
        let mut found = Vec::new();
        let mut first = None;
        for item in &read.content.items {
            let before = found.len();
            let at = match item {
                Item::Op(op) => {
                    op.operands.iter().for_each(|o| excesses(o, true, &mut found));
                    op.at
                }
                Item::Image(image) => {
                    for (key, value) in image.dict.entries() {
                        let key = Object {
                            value: Value::Name(key.clone()),
                            start: 0,
                            end: 0,
                        };
                        excesses(&key, true, &mut found);
                        excesses(value, true, &mut found);
                    }
                    image.at
                }
            };
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

pub(super) fn graphics(doc: &Document, _: &Settings) -> Vec<Finding> {
    let found = |message: String, at: Vec<Location>| Finding::new(X4_LIMITS_GRAPHICS, message).citing("6.25").at(at);
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        if let Event::Nesting {
            explicit,
            implicit,
            frames,
        } = event
            && explicit + implicit > DEEPEST_NESTING
        {
            let message = format!(
                "graphics states nested {} deep ({explicit} by q, {implicit} by forms and patterns), over 28",
                explicit + implicit
            );
            out.push(found(message, frames));
        }
    }
    for (num, object, ..) in doc.objects() {
        if let Some(n) = widest_device_n(object).filter(|n| *n > MOST_COMPONENTS) {
            out.push(found(
                format!("a DeviceN space of {n} components, over 32"),
                vec![Location::Object(num, 0)],
            ));
        }
        let cmap = object
            .dict()
            .is_some_and(|d| d.get(b"Type").and_then(Object::name) == Some(b"CMap"));
        if let Some(cid) = cmap
            .then(|| doc.stream_data(num))
            .flatten()
            .and_then(|d| largest_cid(&d))
            && cid > LARGEST_CID
        {
            out.push(found(
                format!("a CMap that maps to CID {cid}, over 65,535"),
                vec![Location::Object(num, 0)],
            ));
        }
    }
    out
}

/// The most components of any `DeviceN` space in `object`.
fn widest_device_n(object: &Object) -> Option<usize> {
    match &object.value {
        Value::Array(items) => {
            let own = match (
                items.first().and_then(Object::name),
                items.get(1).and_then(Object::array),
            ) {
                (Some(b"DeviceN"), Some(names)) => Some(names.len()),
                _ => None,
            };
            items.iter().filter_map(widest_device_n).chain(own).max()
        }
        Value::Dict(dict) => dict.entries().iter().filter_map(|(_, v)| widest_device_n(v)).max(),
        _ => None,
    }
}

/// The largest CID a `CMap`'s `cidrange`, `cidchar` and `notdefrange` sections map to.
fn largest_cid(data: &[u8]) -> Option<u64> {
    let tokens: Vec<&[u8]> = data.split(u8::is_ascii_whitespace).filter(|t| !t.is_empty()).collect();
    let hex = |t: &[u8]| {
        let inner = t.strip_prefix(b"<").and_then(|t| t.strip_suffix(b">"));
        inner
            .and_then(|t| std::str::from_utf8(t).ok())
            .and_then(|t| u64::from_str_radix(t, 16).ok())
    };
    let number = |t: &[u8]| std::str::from_utf8(t).ok().and_then(|t| t.parse::<u64>().ok());
    let mut largest = None;
    let mut section: Option<usize> = None;
    let mut operands: Vec<&[u8]> = Vec::new();
    for token in tokens {
        match token {
            b"begincidrange" | b"beginnotdefrange" => section = Some(3),
            b"begincidchar" => section = Some(2),
            b"endcidrange" | b"endnotdefrange" | b"endcidchar" => section = None,
            _ => {
                let Some(size) = section else { continue };
                operands.push(token);
                if operands.len() == size {
                    let span = match operands.as_slice() {
                        [low, high, _] => hex(high).zip(hex(low)).map(|(h, l)| h.saturating_sub(l)),
                        _ => Some(0),
                    };
                    let found = operands.last().and_then(|c| number(c)).zip(span).map(|(c, s)| c + s);
                    largest = largest.max(found);
                    operands.clear();
                }
            }
        }
    }
    largest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    #[test]
    fn nesting_device_n_and_cids() {
        let deep = |n: usize| format!("{}{}", "q ".repeat(n), "Q ".repeat(n));
        let form = format!(
            "<< /Subtype /Form /Length {} >>\nstream\n{}\nendstream",
            deep(2).len(),
            deep(2)
        );
        let names = (0..33).map(|i| format!("/C{i}")).collect::<Vec<_>>().join(" ");
        let spaces = format!(
            "/Resources << /XObject << /F 20 0 R >> /ColorSpace << /N [/DeviceN [{names}] /DeviceCMYK 5] >> >>"
        );
        let cmap =
            |body: &str| format!("<< /Type /CMap /Length {} >>\nstream\n{body}\nendstream", body.len()).into_bytes();
        let more = [
            (20, form.into_bytes()),
            (
                21,
                cmap("1 begincidrange <0000> <00FF> 65400 endcidrange 1 begincidchar <01> 70000 endcidchar"),
            ),
            (
                22,
                cmap(
                    "1 begincidrange <00> <FF> 10 endcidrange 1 beginnotdefrange <00> <1F> 1 endnotdefrange x 7 <zz> <q",
                ),
            ),
            (23, b"<< /Type /CMap /Length 3 >>\nstream\nabc\nendstream".to_vec()),
        ];
        let doc = paged(
            &[
                (&spaces, &format!("{} /F Do {}", "q ".repeat(27), "Q ".repeat(27))),
                ("", &deep(28)),
                ("", "BT ET"),
            ],
            "/Refs [21 0 R 22 0 R 23 0 R]",
            &more,
        );
        let found: Vec<String> = graphics(&doc, &Level::X4.into())
            .into_iter()
            .map(|f| f.message)
            .collect();
        assert_eq!(
            found,
            [
                "graphics states nested 30 deep (29 by q, 1 by forms and patterns), over 28",
                "a CMap that maps to CID 70000, over 65,535",
                "a DeviceN space of 33 components, over 32",
            ]
        );
        assert_eq!(
            largest_cid(b"1 begincidrange <0000> <00FF> 65400 endcidrange"),
            Some(65_655)
        );
    }

    #[test]
    fn values_beyond_table_c1() {
        let long = "N".repeat(128);
        let string = "a".repeat(32_768);
        let (big, tiny) = (format!("4{}.0", "0".repeat(38)), format!("0.{}1", "0".repeat(39)));
        let page = format!("/Big 2147483648 /Small -2147483648 /R {big} /Fine {tiny} /{long} 1 /S ({string})");
        let content =
            format!("BT ({string}) Tj ET 5000000000 0 m BI /W 1 /H 1 /BPC 8 /CS /G /{long} 1 ID x EI 1 0 0 1 0 0 cm");
        let doc = paged(&[(&page, &content), ("", "q Q")], "", &[]);
        let found: Vec<String> = lexical(&doc, &Level::X4.into())
            .into_iter()
            .map(|f| f.message)
            .collect();
        assert_eq!(
            found,
            [
                "3 value(s) beyond the PDF 1.6 limits, the first the integer 2147483648, outside 32 bits",
                "3 value(s) beyond the PDF 1.6 limits, the first a string of 32768 bytes in a content stream, over 32,767",
            ]
        );
    }
}
