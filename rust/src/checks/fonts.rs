//! PDF/X-4 (ISO 15930-7) 6.5.4, the encodings of TrueType fonts. A non-symbolic font names `MacRomanEncoding` or
//! `WinAnsiEncoding`, directly or as the `/BaseEncoding` of an encoding dictionary without `/Differences`; a symbolic
//! one has no `/Encoding`, and its program's `cmap` lists exactly one encoding subtable.
//!
//! A font is symbolic when its descriptor's `/Flags` has the Symbolic bit (bit 3, ISO 32000-1 9.8.2); without a
//! descriptor or `/Flags` it is not. The fonts judged are those text runs use, `Tr 3` included. A
//! program is read from `/FontFile2`, or from a `/FontFile3` of subtype `OpenType`; a font without one is left to
//! 6.5.1 for its `cmap`.
// ponytail: a font dictionary written directly in a resource dictionary, not as an object of its own, is not judged;
// carry the dictionary on `Font` if one turns up. The walk's own limits apply (Type 3 glyph procedures, soft-mask
// content and annotation appearances are not walked).

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::Settings;
use super::info::per_level;
use super::painted::page_of;
use crate::common::font::Font;
use crate::common::glyphs::{self, Look};
use crate::common::pyfmt::g;
use crate::common::sfnt;
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Location, X4_FONT_EMBEDDED, X4_FONT_GLYPHS_PRESENT, X4_FONT_NO_NOTDEF,
    X4_FONT_TRUETYPE_NONSYMBOLIC_ENCODING, X4_FONT_TRUETYPE_NONSYMBOLIC_NO_DIFFERENCES,
    X4_FONT_TRUETYPE_SYMBOLIC_NO_ENCODING, X4_FONT_TRUETYPE_SYMBOLIC_ONE_CMAP, X4_FONT_WIDTHS_CONSISTENT,
    X13_FONT_EMBEDDING_PERMITTED,
};
use crate::graphics::{self, Event, Kind};

/// The Symbolic flag of a font descriptor's `/Flags` (ISO 32000-1 Table 123).
const SYMBOLIC: i64 = 4;

/// A TrueType font dictionary: its object number, the dictionary and its descriptor, if it has one.
struct TrueType<'a> {
    num: u32,
    dict: &'a Dict,
    descriptor: Option<&'a Dict>,
}

impl TrueType<'_> {
    fn symbolic(&self) -> bool {
        let flags = self
            .descriptor
            .and_then(|d| d.get(b"Flags"))
            .and_then(|f| match f.value {
                Value::Integer(n) => Some(n),
                _ => None,
            });
        flags.is_some_and(|f| f & SYMBOLIC != 0)
    }

    fn name(&self) -> String {
        let base = self.dict.get(b"BaseFont").and_then(Object::name).unwrap_or(b"?");
        format!("TrueType font {} (object {})", String::from_utf8_lossy(base), self.num)
    }

    fn finding(&self, check: &'static Check, problem: &str) -> Finding {
        Finding::new(check, format!("{} {problem}", self.name()))
            .citing("6.5.4")
            .at(vec![Location::Object(self.num, 0)])
    }
}

/// The TrueType fonts text runs use (6.1.3's print elements, decision X4-Q3), by the walk: page content, forms and
/// tiling patterns, optional content visible or not, every render mode.
fn truetype(doc: &Document) -> Vec<TrueType<'_>> {
    let used: BTreeSet<u32> = graphics::walk(doc)
        .into_iter()
        .filter_map(|event| match event {
            Event::Paint { ink, .. } => ink.font.and_then(|f| f.object),
            _ => None,
        })
        .collect();
    doc.objects()
        .filter(|(num, ..)| used.contains(num))
        .filter_map(|(num, object, ..)| {
            let truetype = |d: &&Dict| d.get(b"Subtype").and_then(Object::name) == Some(b"TrueType");
            object.dict().filter(truetype).map(|dict| (num, dict))
        })
        .map(|(num, dict)| TrueType {
            num,
            dict,
            descriptor: dict
                .get(b"FontDescriptor")
                .and_then(|d| doc.resolve(d))
                .and_then(Object::dict),
        })
        .collect()
}

/// The font's `/Encoding`, resolved.
fn encoding<'a>(doc: &'a Document, font: &TrueType<'a>) -> Option<&'a Object> {
    font.dict.get(b"Encoding").and_then(|e| doc.resolve(e))
}

fn standard(name: Option<&[u8]>) -> bool {
    matches!(name, Some(b"MacRomanEncoding" | b"WinAnsiEncoding"))
}

pub(super) fn nonsymbolic_encoding(doc: &Document, _: &Settings) -> Vec<Finding> {
    truetype(doc)
        .iter()
        .filter(|font| !font.symbolic())
        .filter_map(|font| {
            let problem = match encoding(doc, font).map(|e| &e.value) {
                None => "is not symbolic and has no /Encoding".to_owned(),
                Some(Value::Name(n)) if standard(Some(n)) => return None,
                Some(Value::Dict(d)) if standard(d.get(b"BaseEncoding").and_then(Object::name)) => return None,
                Some(Value::Dict(_)) => {
                    "is not symbolic and its encoding has no /BaseEncoding MacRomanEncoding or WinAnsiEncoding"
                        .to_owned()
                }
                Some(_) => {
                    "is not symbolic and its /Encoding is neither MacRomanEncoding nor WinAnsiEncoding".to_owned()
                }
            };
            Some(font.finding(X4_FONT_TRUETYPE_NONSYMBOLIC_ENCODING, &problem))
        })
        .collect()
}

pub(super) fn nonsymbolic_no_differences(doc: &Document, _: &Settings) -> Vec<Finding> {
    truetype(doc)
        .iter()
        .filter(|font| !font.symbolic())
        .filter(|font| {
            encoding(doc, font)
                .and_then(Object::dict)
                .is_some_and(|d| d.get(b"Differences").is_some())
        })
        .map(|font| {
            font.finding(
                X4_FONT_TRUETYPE_NONSYMBOLIC_NO_DIFFERENCES,
                "is not symbolic and its encoding has /Differences",
            )
        })
        .collect()
}

pub(super) fn symbolic_no_encoding(doc: &Document, _: &Settings) -> Vec<Finding> {
    truetype(doc)
        .iter()
        .filter(|font| font.symbolic() && font.dict.get(b"Encoding").is_some())
        .map(|font| {
            font.finding(
                X4_FONT_TRUETYPE_SYMBOLIC_NO_ENCODING,
                "is symbolic and has an /Encoding",
            )
        })
        .collect()
}

pub(super) fn symbolic_one_cmap(doc: &Document, _: &Settings) -> Vec<Finding> {
    truetype(doc)
        .iter()
        .filter(|font| font.symbolic())
        .filter_map(|font| {
            let data = font.descriptor.and_then(|d| program(doc, d))?;
            let problem = match sfnt::cmap_subtables(&data) {
                Some(1) => return None,
                Some(n) => format!("is symbolic and its program's cmap has {n} encoding subtables, not one"),
                None => "is symbolic and its program has no cmap table that can be read".to_owned(),
            };
            Some(font.finding(X4_FONT_TRUETYPE_SYMBOLIC_ONE_CMAP, &problem))
        })
        .collect()
}

/// The data of the descriptor's TrueType or OpenType font program.
fn program(doc: &Document, descriptor: &Dict) -> Option<Vec<u8>> {
    let file = |key: &[u8]| descriptor.get(key).map(|o| (o, doc.resolve(o)));
    let opentype = |(_, o): &(&Object, Option<&Object>)| {
        o.and_then(Object::dict)
            .and_then(|d| d.get(b"Subtype"))
            .and_then(Object::name)
            == Some(b"OpenType")
    };
    let (reference, _) = file(b"FontFile2").or_else(|| file(b"FontFile3").filter(opentype))?;
    let Value::Reference(num, _) = reference.value else {
        return None;
    };
    doc.stream_data(num)
}

/// Every font descriptor in the file, with its object number.
fn descriptors(doc: &Document) -> Vec<(u32, &Dict)> {
    doc.objects()
        .filter_map(|(num, object, ..)| Some((num, object.dict()?)))
        .filter(|(_, d)| d.get(b"Type").and_then(Object::name) == Some(b"FontDescriptor"))
        .collect()
}

/// The older levels (15930-1 6.4, -3, -4 and -6 6.3): only fonts whose licence lets them be embedded are used. A
/// TrueType or OpenType program whose `OS/2` `fsType` asks for restricted-licence embedding (bit 1 alone among the
/// permission bits) warns: an agreement with the font's owner may still allow it, and the file cannot show one.
pub(super) fn embedding_permitted(doc: &Document, settings: &Settings) -> Vec<Finding> {
    descriptors(doc)
        .into_iter()
        .filter_map(|(num, descriptor)| {
            let data = program(doc, descriptor)?;
            let os2 = sfnt::table(&data, *b"OS/2")?;
            let fs_type = os2
                .get(8..)
                .and_then(<[u8]>::first_chunk::<2>)
                .map(|b| u16::from_be_bytes(*b))?;
            (fs_type & 0x000F == 0x0002).then(|| {
                let finding = Finding::new(
                    X13_FONT_EMBEDDING_PERMITTED,
                    format!("object {num}: the font program's licence restricts embedding (OS/2 fsType {fs_type})"),
                )
                .at(vec![Location::Object(num, 0)]);
                per_level(finding, settings, ["6.4", "6.3", "6.3", "6.3"])
            })
        })
        .collect()
}

/// 6.5.1: a font that text uses is embedded, unless every run in it is in render mode 3 (invisible). A Type 3 font's
/// glyphs are in the file. Text in a font the resources do not give is 6.3's matter, not this row's.
pub(super) fn embedded(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut runs: BTreeMap<(u32, String), usize> = BTreeMap::new();
    for event in graphics::walk(doc) {
        if let Event::Paint {
            kind: Kind::Text(mode),
            ink,
            frames,
            ..
        } = event
            && mode != 3
            && ink.glyphs > 0
            && let Some(font) = ink.font.filter(|f| f.known && !f.embedded)
        {
            *runs.entry((page_of(&frames), font.name.clone())).or_default() += 1;
        }
    }
    runs.into_iter()
        .map(|((page, name), count)| {
            let message = format!("font {name} is not embedded, and {count} run(s) show text in it visibly");
            Finding::new(X4_FONT_EMBEDDED, message)
                .citing("6.5.1")
                .at(vec![Location::Page(page)])
        })
        .collect()
}

/// A `/Widths` or `/W` entry within this of the program's advance agrees with it (X4-Q16): each is rounded to a
/// thousandth of an em at most, so honest values differ by up to half a unit each.
const WIDTH_TOLERANCE: f64 = 1.0;

/// The codes shown in each font object, with the walk's metrics for the font and, per code, whether any appearance of
/// it is outside render mode 3.
type Shown = BTreeMap<u32, (Rc<Font>, BTreeMap<u32, bool>)>;

fn shown(doc: &Document) -> Shown {
    let mut out: Shown = BTreeMap::new();
    for event in graphics::walk(doc) {
        if let Event::Paint {
            kind: Kind::Text(mode),
            ink,
            ..
        } = event
            && let Some(font) = ink.font.filter(|f| f.known)
            && let Some(num) = font.object
        {
            let codes = &mut out.entry(num).or_insert_with(|| (Rc::clone(&font), BTreeMap::new())).1;
            for code in ink.codes {
                *codes.entry(code).or_default() |= mode != 3;
            }
        }
    }
    out
}

/// Each shown font with its codes judged against its program.
/// A shown font: its object number, the walk's metrics, its codes as `Shown` has them, and each code judged.
type Judged = (u32, Rc<Font>, BTreeMap<u32, bool>, BTreeMap<u32, (Look, Option<u32>)>);

fn judged(doc: &Document) -> Vec<Judged> {
    let mut shown = shown(doc);
    doc.objects()
        .filter_map(|(num, object, ..)| {
            let (font, codes) = shown.remove(&num)?;
            object.dict().map(|dict| {
                let looks = glyphs::judge(doc, dict, &codes.keys().copied().collect());
                (num, font, codes, looks)
            })
        })
        .collect()
}

fn font_finding(check: &'static Check, clause: &str, num: u32, font: &Font, problem: &str) -> Finding {
    Finding::new(check, format!("font {} (object {num}) {problem}", font.name))
        .citing(clause)
        .at(vec![Location::Object(num, 0)])
}

/// At most ten codes, in words.
fn listed(codes: &[u32]) -> String {
    let shown: Vec<String> = codes.iter().take(10).map(u32::to_string).collect();
    let more = if codes.len() > 10 { ", ..." } else { "" };
    format!("{}{more}", shown.join(", "))
}

/// 6.5.1: every code a font shows visibly has a glyph in its embedded program.
pub(super) fn glyphs_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (num, font, codes, looks) in judged(doc) {
        let judged = |want: &Look| -> Vec<u32> {
            looks
                .iter()
                .filter(|(c, (l, _))| l == want && codes.get(c) == Some(&true))
                .map(|(c, _)| *c)
                .collect()
        };
        if !judged(&Look::Unreadable).is_empty() {
            out.push(font_finding(
                X4_FONT_GLYPHS_PRESENT,
                "6.5.1",
                num,
                &font,
                "has a program that cannot be read",
            ));
        }
        let missing = judged(&Look::Missing);
        if !missing.is_empty() {
            let problem = format!(
                "has no glyph in its program for {} code(s) shown: {}",
                missing.len(),
                listed(&missing)
            );
            out.push(font_finding(X4_FONT_GLYPHS_PRESENT, "6.5.1", num, &font, &problem));
        }
    }
    out
}

/// 6.5.2: no code shown, in any render mode, selects `.notdef`.
pub(super) fn no_notdef(doc: &Document, _: &Settings) -> Vec<Finding> {
    judged(doc)
        .into_iter()
        .filter_map(|(num, font, _, looks)| {
            let notdef: Vec<u32> = looks
                .iter()
                .filter(|(_, (l, _))| *l == Look::Notdef)
                .map(|(c, _)| *c)
                .collect();
            (!notdef.is_empty()).then(|| {
                let problem = format!("shows .notdef for {} code(s): {}", notdef.len(), listed(&notdef));
                font_finding(X4_FONT_NO_NOTDEF, "6.5.2", num, &font, &problem)
            })
        })
        .collect()
}

/// 6.5.3: the width the font dictionary gives each glyph shown agrees with the program's advance (X4-Q16). A simple font
/// without `/Widths` is not judged; a composite font's `/DW` stands for the CIDs `/W` does not list.
pub(super) fn widths_consistent(doc: &Document, _: &Settings) -> Vec<Finding> {
    judged(doc)
        .into_iter()
        .filter(|(_, font, ..)| font.code_bytes == 2 || font.measured())
        .filter_map(|(num, font, _, looks)| {
            let differ: Vec<(u32, f64, f64)> = looks
                .iter()
                .filter_map(|(code, (look, key))| {
                    let (Look::Glyph(Some(advance)), Some(key)) = (look, key) else {
                        return None;
                    };
                    let declared = font.width(*key);
                    ((declared - advance).abs() > WIDTH_TOLERANCE).then_some((*code, declared, *advance))
                })
                .collect();
            let (code, declared, advance) = *differ.first()?;
            let problem = format!(
                "declares widths that differ from its program's for {} code(s) shown, code {code} among them: {} in \
                 the PDF, {} in the program",
                differ.len(),
                g(declared),
                g(advance)
            );
            Some(font_finding(X4_FONT_WIDTHS_CONSISTENT, "6.5.3", num, &font, &problem))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::common::sfnt::tests::program;
    use crate::findings::Level;

    fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    #[test]
    fn fonts_text_shows_visibly_are_embedded() {
        let fonts = "/Resources << /Font << /H << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> \
            /U << /Type /Font /Subtype /Type1 /BaseFont /Unseen >> /T << /Type /Font /Subtype /Type3 >> \
            /E << /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Embedded /FontDescriptor << /FontFile2 9 0 R >> >> >> >>";
        let text = "BT /H 9 Tf (a) Tj (b) Tj () Tj 7 Tr (c) Tj 3 Tr /U 9 Tf (d) Tj /H 9 Tf (e) Tj 0 Tr /T 9 Tf (f) Tj \
            /E 9 Tf (g) Tj /Q 9 Tf (h) Tj ET";
        let program = (9, b"<< /Length 0 >>\nstream\n\nendstream".to_vec());
        let doc = paged(&[(fonts, text), (fonts, "BT /H 9 Tf (a) Tj ET")], "", &[program]);
        let found: Vec<(Option<u32>, String)> = embedded(&doc, &Level::X4.into())
            .into_iter()
            .map(|f| (f.page, f.message))
            .collect();
        let msg = |page, runs| {
            (
                Some(page),
                format!("font Helvetica is not embedded, and {runs} run(s) show text in it visibly"),
            )
        };
        assert_eq!(found, [msg(1, 3), msg(2, 1)]);
    }

    #[test]
    fn glyphs_notdef_and_widths_from_the_program() {
        const TTF: &[u8] = include_bytes!("../../../tests/fixtures/fonts/proof.ttf");
        let mut widths = vec!["600"; 95];
        widths[usize::from(b'B' - 32)] = "650";
        let tt = |file: &str, widths: &str| {
            format!(
                "<< /Type /Font /Subtype /TrueType /BaseFont /ProofBox /FirstChar 32 {widths} /Encoding /WinAnsiEncoding \
                 /FontDescriptor << /Flags 32 /FontFile2 {file} >> >>"
            )
            .into_bytes()
        };
        let objects = vec![
            (10, tt("20 0 R", &format!("/Widths [{}]", widths.join(" ")))),
            (11, tt("21 0 R", "/Widths [600]")),
            (12, tt("20 0 R", "")),
            (20, stream("", TTF)),
            (21, stream("", b"junk")),
        ];
        let fonts =
            "/Resources << /Font << /F 10 0 R /U 11 0 R /N 12 0 R /D << /Subtype /Type1 /BaseFont /Direct >> >> >>";
        // 0xE9 and 0xEA have no glyph: the first only ever in render mode 3, the second also once visibly. 0x10 has no
        // name in WinAnsiEncoding.
        let text = "BT /F 9 Tf (AB) Tj 3 Tr (\\351\\352\\020) Tj 0 Tr (\\352) Tj /U 9 Tf (A) Tj /N 9 Tf (B) Tj /D 9 Tf (A) Tj ET";
        let doc = paged(&[(fonts, text)], "", &objects);
        let settings = Level::X4.into();
        let found: Vec<String> = [glyphs_present, no_notdef, widths_consistent]
            .iter()
            .flat_map(|run| run(&doc, &settings))
            .map(|f| f.message)
            .collect();
        assert_eq!(
            found,
            [
                "font ProofBox (object 10) has no glyph in its program for 1 code(s) shown: 234",
                "font ProofBox (object 11) has a program that cannot be read",
                "font ProofBox (object 10) shows .notdef for 1 code(s): 16",
                "font ProofBox (object 10) declares widths that differ from its program's for 1 code(s) shown, code 66 \
                 among them: 650 in the PDF, 600 in the program",
            ]
        );
        assert_eq!(
            listed(&(0..12).collect::<Vec<u32>>()),
            "0, 1, 2, 3, 4, 5, 6, 7, 8, 9, ..."
        );
    }

    #[test]
    fn truetype_encodings_and_cmaps() {
        let tt = |rest: &str| format!("<< /Type /Font /Subtype /TrueType /BaseFont /F {rest} >>").into_bytes();
        let symbolic = |file: &str| tt(&format!("/FontDescriptor << /Flags 4 {file} >>"));
        let fonts: Vec<(u32, Vec<u8>)> = vec![
            (10, tt("/Encoding /WinAnsiEncoding /FontDescriptor << /Flags 32 >>")),
            (11, tt("")),
            (12, b"<< /Type /Font /Subtype /TrueType >>".to_vec()),
            (
                13,
                tt("/Encoding << /BaseEncoding /MacRomanEncoding /Differences [] >> /FontDescriptor << /Flags /X >>"),
            ),
            (14, tt("/Encoding << /Differences [] >>")),
            (15, tt("/Encoding /StandardEncoding")),
            (
                16,
                tt("/Encoding /WinAnsiEncoding /FontDescriptor << /Flags 4 /FontFile2 30 0 R >>"),
            ),
            (17, symbolic("/FontFile2 31 0 R")),
            (18, symbolic("/FontFile3 32 0 R")),
            (19, symbolic("/FontFile3 33 0 R")),
            (20, symbolic("")),
            (21, symbolic("/FontFile2 5")),
            (22, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
            (
                23,
                b"<< /Type /Font /Subtype /TrueType /Encoding /WinAnsiEncoding /FontDescriptor 5 >>".to_vec(),
            ),
            (30, stream("", &program(&[(b"cmap", &[0, 0, 0, 1])]))),
            (31, stream("", &program(&[(b"cmap", &[0, 0, 0, 2])]))),
            (32, stream("/Subtype /OpenType", &program(&[(b"head", &[0])]))),
            (33, stream("/Subtype /Type1C", b"x")),
        ];
        let names: Vec<String> = (10..=23).map(|n| format!("/F{n} {n} 0 R")).collect();
        let shown: Vec<String> = (10..=23)
            .filter(|n| *n != 11)
            .map(|n| format!("/F{n} 9 Tf (a) Tj"))
            .collect();
        let resources = format!("/Resources << /Font << {} >> >>", names.join(" "));
        let doc = paged(&[(&resources, &format!("BT {} ET", shown.join(" ")))], "", &fonts);
        let found: Vec<String> = [
            nonsymbolic_encoding,
            nonsymbolic_no_differences,
            symbolic_no_encoding,
            symbolic_one_cmap,
        ]
        .iter()
        .flat_map(|run| run(&doc, &Level::X4.into()))
        .map(|f| f.message)
        .collect();
        assert_eq!(
            found,
            [
                "TrueType font ? (object 12) is not symbolic and has no /Encoding",
                "TrueType font F (object 14) is not symbolic and its encoding has no /BaseEncoding MacRomanEncoding or \
                 WinAnsiEncoding",
                "TrueType font F (object 15) is not symbolic and its /Encoding is neither MacRomanEncoding nor \
                 WinAnsiEncoding",
                "TrueType font F (object 13) is not symbolic and its encoding has /Differences",
                "TrueType font F (object 14) is not symbolic and its encoding has /Differences",
                "TrueType font F (object 16) is symbolic and has an /Encoding",
                "TrueType font F (object 17) is symbolic and its program's cmap has 2 encoding subtables, not one",
                "TrueType font F (object 18) is symbolic and its program has no cmap table that can be read",
            ]
        );
    }

    #[test]
    fn a_program_whose_licence_restricts_embedding() {
        let os2 = |fs_type: u16| {
            let mut table = vec![0u8; 8];
            table.extend(fs_type.to_be_bytes());
            program(&[(b"OS/2", &table)])
        };
        let font = |n: u32, file: u32| {
            (
                n,
                format!("<< /Type /FontDescriptor /FontName /F{n} /FontFile2 {file} 0 R >>").into_bytes(),
            )
        };
        let objects = vec![
            font(10, 20),
            font(11, 21),
            font(12, 22),
            (20, stream("", &os2(2))),
            (21, stream("", &os2(8))),
            (22, stream("", b"junk")),
            (13, b"<< /Type /FontDescriptor /FontName /F13 >>".to_vec()),
            font(14, 24),
            (24, stream("", &program(&[(b"OS/2", &[0u8; 9])]))),
            (15, b"7".to_vec()),
        ];
        let page = "/Resources << /X << /A 10 0 R /B 11 0 R /C 12 0 R /D 13 0 R /E 14 0 R /F 15 0 R >> >>";
        let doc = paged(&[(page, "")], "", &objects);
        let found: Vec<_> = embedding_permitted(&doc, &Level::X1a2003.into())
            .into_iter()
            .map(|f| (f.message, f.clause.map(|c| c.standard)))
            .collect();
        assert_eq!(
            found,
            [(
                "object 10: the font program's licence restricts embedding (OS/2 fsType 2)".to_owned(),
                Some("iso-15930-4")
            )]
        );
    }
}
