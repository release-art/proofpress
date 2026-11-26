//! What `extract` recovers from a PDF, read by the core: the pages' boxes, the document information and XMP packet, the
//! embedded font programs with their descriptors, and the ICC profiles with the output intents that carry them.
//!
//! This reports raw facts and the bytes of each recovered file; naming, de-duplicating profiles by content and the
//! `document.json` layout are the Python `proof_pdf.extract`'s. Objects are visited in one fixed order, reachable from
//! the catalog depth-first with each dictionary's values in key order and the last pushed visited first, as
//! pikepdf walks them, so assets keep their order.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::common::icc;
use crate::common::rect::rect;
use crate::common::text;
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{OBJECT_DANGLING_REFERENCE, Severity};
use crate::pipeline::Limits;

/// The font program keys of a font descriptor (ISO 32000-2 Table 120), in the order they are looked for.
const FONT_FILES: [&[u8]; 3] = [b"FontFile", b"FontFile2", b"FontFile3"];

/// Everything this module reads from one file. Byte payloads are in [`Recovered::blobs`], by index.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Facts {
    pages: Vec<PageFacts>,
    pdf_version: String,
    /// The document information dictionary, each value as text.
    info: BTreeMap<String, String>,
    /// The XMP packet's blob, or why it could not be read.
    xmp: Option<Blob>,
    fonts: Vec<FontProgram>,
    profiles: Vec<Profile>,
    output_intents: Vec<Intent>,
    /// What the reader found damaged or missing, each as `check: message`: the parts of the file nothing above
    /// could read.
    damage: Vec<String>,
}

/// A recovered file's bytes, by index into [`Recovered::blobs`], or why it could not be read.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Blob {
    Data(usize),
    Error(String),
}

#[derive(Debug, PartialEq, Serialize)]
struct PageFacts {
    number: usize,
    rotate: i64,
    media_pt: Option<[f64; 4]>,
    crop_pt: Option<[f64; 4]>,
    bleed_pt: Option<[f64; 4]>,
    trim_pt: Option<[f64; 4]>,
    art_pt: Option<[f64; 4]>,
}

#[derive(Debug, PartialEq, Serialize)]
struct FontProgram {
    /// The descriptor's `/FontName` as written, subset prefix and all.
    font_name: Option<String>,
    /// Which key held it: `FontFile`, `FontFile2` or `FontFile3`.
    key: String,
    /// The program stream's `/Subtype`, as a name without its slash.
    subtype: Option<String>,
    program: Blob,
    descriptor: Descriptor,
}

/// A font descriptor's values as written, numbers as numbers and names and strings as text.
#[derive(Debug, PartialEq, Serialize)]
struct Descriptor {
    flags: Option<f64>,
    weight: Option<f64>,
    italic_angle: Option<f64>,
    family: Option<String>,
    stretch: Option<String>,
    ascent: Option<f64>,
    descent: Option<f64>,
    cap_height: Option<f64>,
    x_height: Option<f64>,
    stem_v: Option<f64>,
    bbox: Option<Vec<f64>>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Profile {
    /// `output intent` or `colour space`.
    role: &'static str,
    /// The stream's `/N`.
    components: Option<f64>,
    data: Blob,
    /// The profile's own description (its `desc` tag).
    description: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Intent {
    subtype: Option<String>,
    output_condition_identifier: Option<String>,
    output_condition: Option<String>,
    registry: Option<String>,
    /// The index of its profile in `profiles`.
    profile: Option<usize>,
}

/// The facts and the recovered files' bytes.
#[derive(Debug, Default, PartialEq)]
pub struct Recovered {
    /// The facts, as `extract` reports them.
    pub facts: Facts,
    /// The bytes the facts' blobs index.
    pub blobs: Vec<Vec<u8>>,
}

impl Recovered {
    fn blob(&mut self, data: Option<Vec<u8>>, what: &str) -> Blob {
        match data {
            Some(data) => {
                self.blobs.push(data);
                Blob::Data(self.blobs.len() - 1)
            }
            None => Blob::Error(format!("{what} cannot be read")),
        }
    }
}

/// Read `bytes` and recover the facts above.
///
/// # Errors
///
/// The reason a file cannot be read: its structure is too damaged, or it is encrypted.
pub fn recover(bytes: &[u8]) -> Result<Recovered, String> {
    let doc = Document::open(bytes, &Limits::default()).map_err(|refusal| refusal.message)?;
    let mut out = Recovered::default();
    out.facts.pdf_version = String::from_utf8_lossy(doc.version()).into_owned();
    out.facts.damage = doc
        .findings()
        .into_iter()
        // Errors lose data, and so does a dangling reference, which reads as null (a warning for verifying); the other
        // warnings are about how the file is written, not what can be read from it.
        .filter(|f| f.severity == Severity::Error || f.check == OBJECT_DANGLING_REFERENCE)
        .map(|f| format!("{}: {}", f.check.id, f.message))
        .collect();
    out.facts.pages = pages(&doc);
    out.facts.info = info(&doc);
    if let Some(packet) = doc.catalog().and_then(|c| c.get(b"Metadata")) {
        let data = stream(&doc, packet);
        out.facts.xmp = Some(out.blob(data, "the XMP packet"));
    }
    intents(&doc, &mut out);
    let mut fonts = BTreeSet::new();
    reachable(&doc, |object| {
        let Some(dict) = object.dict() else {
            if let Some([space, profile]) = object.array()
                && space.name() == Some(b"ICCBased")
            {
                profile_of(&doc, profile, "colour space", &mut out);
            }
            return;
        };
        if dict.get(b"Type").and_then(Object::name) != Some(b"FontDescriptor") {
            return;
        }
        for key in FONT_FILES {
            let Some(program) = dict.get(key) else { continue };
            if let Value::Reference(num, _) = program.value
                && !fonts.insert(num)
            {
                continue;
            }
            let subtype = doc
                .resolve(program)
                .and_then(Object::dict)
                .and_then(|d| d.get(b"Subtype"))
                .and_then(|s| name(&doc, s));
            let font_name = dict.get(b"FontName").and_then(|n| pdf_str(&doc, n));
            let data = stream(&doc, program);
            let what = format!("the program of {}", font_name.as_deref().unwrap_or("a font"));
            let program = out.blob(data, &what);
            out.facts.fonts.push(FontProgram {
                font_name,
                key: String::from_utf8_lossy(key).into_owned(),
                subtype,
                program,
                descriptor: descriptor(&doc, dict),
            });
        }
    });
    Ok(out)
}

/// The number `object` refers to, when it is a reference.
fn reference(object: &Object) -> Option<u32> {
    match object.value {
        Value::Reference(num, _) => Some(num),
        _ => None,
    }
}

/// The decoded data of the stream `object` refers to.
fn stream(doc: &Document, object: &Object) -> Option<Vec<u8>> {
    reference(object).and_then(|num| doc.stream_data(num))
}

fn pages(doc: &Document) -> Vec<PageFacts> {
    let boxed = |page: &crate::document::Page<'_>, key: &[u8]| {
        page.get(key)
            .and_then(|b| rect(doc, b))
            .map(|r| [r.x0, r.y0, r.x1, r.y1])
    };
    doc.pages()
        .iter()
        .enumerate()
        .map(|(index, page)| PageFacts {
            number: index + 1,
            rotate: page
                .get(b"Rotate")
                .and_then(|r| doc.resolve(r))
                .and_then(Object::integer)
                .unwrap_or(0),
            media_pt: boxed(page, b"MediaBox"),
            crop_pt: boxed(page, b"CropBox"),
            bleed_pt: boxed(page, b"BleedBox"),
            trim_pt: boxed(page, b"TrimBox"),
            art_pt: boxed(page, b"ArtBox"),
        })
        .collect()
}

/// The trailer's `/Info` dictionary, each value as text.
fn info(doc: &Document) -> BTreeMap<String, String> {
    let dict = doc
        .trailer()
        .get(b"Info")
        .and_then(|i| doc.resolve(i))
        .and_then(Object::dict);
    dict.map(|d| d.entries.iter())
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((String::from_utf8_lossy(key).into_owned(), pdf_str(doc, value)?)))
        .collect()
}

/// The output intents in order, each with its profile.
fn intents(doc: &Document, out: &mut Recovered) {
    let list = doc
        .catalog()
        .and_then(|c| c.get(b"OutputIntents"))
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)
        .unwrap_or_default();
    for intent in list.iter().filter_map(|i| doc.resolve(i)?.dict()) {
        let text = |key: &[u8]| intent.get(key).and_then(|v| pdf_str(doc, v));
        let profile = intent
            .get(b"DestOutputProfile")
            .map(|p| profile_of(doc, p, "output intent", out));
        out.facts.output_intents.push(Intent {
            subtype: intent.get(b"S").and_then(|s| name(doc, s)),
            output_condition_identifier: text(b"OutputConditionIdentifier"),
            output_condition: text(b"OutputCondition"),
            registry: text(b"RegistryName"),
            profile,
        });
    }
}

/// Record the profile stream `object` refers to in its `role`, returning its index.
fn profile_of(doc: &Document, object: &Object, role: &'static str, out: &mut Recovered) -> usize {
    let data = stream(doc, object);
    let components = reference(object)
        .and_then(|num| doc.stream_dict(num))
        .and_then(|d| d.get(b"N"))
        .and_then(|n| number(doc, n));
    let description = data.as_deref().and_then(icc::description);
    let data = out.blob(data, "an ICC profile");
    out.facts.profiles.push(Profile {
        role,
        components,
        data,
        description,
    });
    out.facts.profiles.len() - 1
}

fn descriptor(doc: &Document, dict: &Dict) -> Descriptor {
    let num = |key: &[u8]| dict.get(key).and_then(|v| number(doc, v));
    let bbox = dict
        .get(b"FontBBox")
        .and_then(|b| doc.resolve(b))
        .and_then(Object::array)
        .and_then(|values| values.iter().map(|v| number(doc, v)).collect());
    Descriptor {
        flags: num(b"Flags"),
        weight: num(b"FontWeight"),
        italic_angle: num(b"ItalicAngle"),
        family: dict.get(b"FontFamily").and_then(|v| pdf_str(doc, v)),
        stretch: dict.get(b"FontStretch").and_then(|v| pdf_str(doc, v)),
        ascent: num(b"Ascent"),
        descent: num(b"Descent"),
        cap_height: num(b"CapHeight"),
        x_height: num(b"XHeight"),
        stem_v: num(b"StemV"),
        bbox,
    }
}

/// `object` as a number.
#[expect(clippy::cast_precision_loss, reason = "as Python's float() of a PDF integer")]
fn number(doc: &Document, object: &Object) -> Option<f64> {
    match doc.resolve(object)?.value {
        Value::Integer(i) => Some(i as f64),
        Value::Real(r) => Some(r),
        _ => None,
    }
}

/// A name's text without its slash.
fn name(doc: &Document, object: &Object) -> Option<String> {
    doc.resolve(object)?
        .name()
        .map(|n| String::from_utf8_lossy(n).into_owned())
}

/// `object` as text, as pikepdf's `str()` gives it: a text string decoded, a name with its slash, a number or boolean
/// as Python writes it. Anything else, null included, is `None`.
fn pdf_str(doc: &Document, object: &Object) -> Option<String> {
    Some(match &doc.resolve(object)?.value {
        // PDF 2.0 adds UTF-8 text strings behind a byte order mark (ISO 32000-2 7.9.2.2.1), as pikepdf reads them.
        Value::String(bytes, _) => match bytes.strip_prefix(b"\xEF\xBB\xBF") {
            Some(utf8) => String::from_utf8_lossy(utf8).into_owned(),
            None => text::decode(bytes).unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
        },
        Value::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
        _ => return None,
    })
}

/// Visit every object reachable from the catalog, depth-first: an indirect object once, a direct one each time it is
/// met; a dictionary's values in key order (but its `/Parent`), and an array's items, pushed in order and visited last
/// first.
fn reachable<'a>(doc: &'a Document, mut visit: impl FnMut(&'a Object)) {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&Object> = doc.trailer().get(b"Root").into_iter().collect();
    while let Some(next) = stack.pop() {
        if let Some(num) = reference(next)
            && !seen.insert(num)
        {
            continue;
        }
        let Some(object) = doc.resolve(next) else { continue };
        visit(object);
        match &object.value {
            Value::Dict(dict) => {
                let mut entries: Vec<_> = dict.entries.iter().filter(|(k, _)| k != b"Parent").collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                stack.extend(entries.into_iter().map(|(_, v)| v));
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::*;
    use crate::document::test_pdf::classic;

    const PROFILE: &[u8] = include_bytes!("../../tests/fixtures/icc/little-cms-test1.icc");

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// A file of `objects` (numbered as given; 1 is the catalog) with `trailer` besides its size and root.
    fn file(objects: &[(u32, Vec<u8>)], trailer: &str) -> Recovered {
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        recover(&classic(&refs, &format!("<< /Size SIZE /Root 1 0 R {trailer} >>"))).unwrap()
    }

    fn json(recovered: &Recovered) -> Json {
        serde_json::to_value(&recovered.facts).unwrap()
    }

    #[test]
    fn pages_with_their_boxes_and_rotation() {
        let got = json(&file(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (
                    2,
                    b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /Rotate 90 /CropBox [0 0 50 50] >>".to_vec(),
                ),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /TrimBox [10 10 90 190] >>".to_vec(),
                ),
                (
                    4,
                    b"<< /Type /Page /Parent 2 0 R /Rotate 0 /BleedBox [5 5 0 0] /ArtBox [1 1 2 2] >>".to_vec(),
                ),
            ],
            "",
        ));
        assert_eq!(
            got["pages"],
            json!([
                {"number": 1, "rotate": 90, "media_pt": [0.0, 0.0, 100.0, 200.0], "crop_pt": [0.0, 0.0, 50.0, 50.0],
                 "bleed_pt": null, "trim_pt": [10.0, 10.0, 90.0, 190.0], "art_pt": null},
                {"number": 2, "rotate": 0, "media_pt": null, "crop_pt": [0.0, 0.0, 50.0, 50.0],
                 "bleed_pt": [0.0, 0.0, 5.0, 5.0], "trim_pt": null, "art_pt": [1.0, 1.0, 2.0, 2.0]}
            ])
        );
        assert_eq!(got["pdf_version"], "1.7");
    }

    #[test]
    fn info_values_as_text_and_the_xmp_packet() {
        let recovered = file(
            &[
                (1, b"<< /Type /Catalog /Metadata 3 0 R >>".to_vec()),
                (
                    2,
                    b"<< /Title (Caf\xe9) /Subject <FEFF00E9> /Author <9F> /Keywords <EFBBBFC3A9> /Trapped /False /Pages 7 /Scale 1.5 \
                      /Marked true /Off false /Gone null /Lost 99 0 R >>"
                        .to_vec(),
                ),
                (3, stream("/Type /Metadata /Subtype /XML", b"<x:xmpmeta/>")),
            ],
            "/Info 2 0 R",
        );
        let got = json(&recovered);
        assert_eq!(
            got["info"],
            json!({"Author": "\u{FFFD}", "Keywords": "é", "Marked": "True", "Off": "False", "Pages": "7", "Scale": "1.5",
                   "Subject": "é", "Title": "Café", "Trapped": "/False"})
        );
        assert_eq!(got["xmp"], json!({"data": 0}));
        assert_eq!(recovered.blobs, [b"<x:xmpmeta/>".to_vec()]);
        assert_eq!(
            got["damage"],
            json!(["object.dangling-reference: a reference to 99 0 R reads as null: it is beyond the trailer's /Size"])
        );
        // A packet that is not a stream cannot be read, and a file with no information has none.
        let broken = json(&file(&[(1, b"<< /Type /Catalog /Metadata << >> >>".to_vec())], ""));
        assert_eq!(broken["xmp"], json!({"error": "the XMP packet cannot be read"}));
        assert_eq!((&broken["info"], &broken["pages"]), (&json!({}), &json!([])));
    }

    #[test]
    fn font_programs_once_each_with_their_descriptors() {
        let descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme-Bold /Flags 262178 /FontWeight 700 \
            /ItalicAngle -12.5 /FontFamily (Acme) /FontStretch /Condensed /Ascent 750 /Descent -250 /CapHeight 700 \
            /XHeight 500 /StemV 80 /FontBBox [-10 -250 1000 900]";
        // Visited last pushed first: 6 (not a descriptor), 5, 4, then 3, whose program 4 already gave.
        let recovered = file(
            &[
                (1, b"<< /Type /Catalog /Fonts [3 0 R 4 0 R 5 0 R 6 0 R] >>".to_vec()),
                (3, b"<< /Type /FontDescriptor /FontFile2 7 0 R >>".to_vec()),
                (
                    4,
                    format!("<< {descriptor} /FontFile2 7 0 R /FontFile3 8 0 R >>").into_bytes(),
                ),
                // A program that is not a stream cannot be read, and a bounding box that is not numbers is none.
                (
                    5,
                    b"<< /Type /FontDescriptor /FontName (Odd) /FontFile << >> /FontBBox [0 (x)] /Flags 9 0 R >>"
                        .to_vec(),
                ),
                (6, b"<< /Type /Font /FontFile 7 0 R >>".to_vec()),
                (7, stream("", b"true")),
                (8, stream("/Subtype /OpenType", b"OTTO")),
                // A damaged object, which reads as nothing.
                (9, b"[1 2".to_vec()),
            ],
            "",
        );
        let got = json(&recovered);
        let none = json!({"flags": null, "weight": null, "italic_angle": null, "family": null, "stretch": null,
                          "ascent": null, "descent": null, "cap_height": null, "x_height": null, "stem_v": null,
                          "bbox": null});
        let acme = json!({"flags": 262_178.0, "weight": 700.0, "italic_angle": -12.5, "family": "Acme",
                          "stretch": "/Condensed", "ascent": 750.0, "descent": -250.0, "cap_height": 700.0,
                          "x_height": 500.0, "stem_v": 80.0, "bbox": [-10.0, -250.0, 1000.0, 900.0]});
        assert_eq!(
            got["fonts"],
            json!([
                {"font_name": "Odd", "key": "FontFile", "subtype": null,
                 "program": {"error": "the program of Odd cannot be read"}, "descriptor": none},
                {"font_name": "/ABCDEF+Acme-Bold", "key": "FontFile2", "subtype": null, "program": {"data": 0},
                 "descriptor": acme},
                {"font_name": "/ABCDEF+Acme-Bold", "key": "FontFile3", "subtype": "OpenType", "program": {"data": 1},
                 "descriptor": acme}
            ])
        );
        assert_eq!(recovered.blobs, [b"true".to_vec(), b"OTTO".to_vec()]);
        assert_eq!(
            got["damage"],
            json!(["object.damaged: object 9 cannot be read (MisplacedKeyword)"])
        );
    }

    #[test]
    fn profiles_of_output_intents_and_colour_spaces() {
        let recovered = file(
            &[
                (
                    1,
                    b"<< /Type /Catalog /OutputIntents [3 0 R 4 0 R 9 0 R null] /Spaces [5 0 R [/ICCBased 6 0 R] \
                      [/ICCBased] [/Separation /ICCBased]] /Up 1 0 R /Parent 2 0 R >>"
                        .to_vec(),
                ),
                (2, b"<< /Type /FontDescriptor /FontFile 7 0 R >>".to_vec()),
                (
                    3,
                    b"<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) /OutputCondition \
                      (Coated) /RegistryName (http://www.color.org) /DestOutputProfile 6 0 R >>"
                        .to_vec(),
                ),
                (4, b"<< /S 99 0 R /Info (none) >>".to_vec()),
                (5, b"[/ICCBased 8 0 R]".to_vec()),
                (6, stream("/N 3", PROFILE)),
                (7, stream("", b"font")),
                (8, b"(not a stream)".to_vec()),
            ],
            "",
        );
        let got = json(&recovered);
        assert_eq!(
            got["output_intents"],
            json!([
                {"subtype": "GTS_PDFX", "output_condition_identifier": "FOGRA39", "output_condition": "Coated",
                 "registry": "http://www.color.org", "profile": 0},
                {"subtype": null, "output_condition_identifier": null, "output_condition": null, "registry": null,
                 "profile": null}
            ])
        );
        let description = icc::description(PROFILE);
        assert!(description.is_some());
        assert_eq!(
            got["profiles"],
            json!([
                {"role": "output intent", "components": 3.0, "data": {"data": 0}, "description": description},
                {"role": "colour space", "components": 3.0, "data": {"data": 1}, "description": description},
                {"role": "colour space", "components": null, "data": {"error": "an ICC profile cannot be read"},
                 "description": null}
            ])
        );
        // The descriptor under /Parent is never reached, so its font is not recovered.
        assert_eq!(got["fonts"], json!([]));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused() {
        assert!(recover(b"not a pdf").is_err());
    }
}
