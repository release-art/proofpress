//! What `extract` recovers from a PDF, read by the core: the pages' boxes, the document information and XMP packet, the
//! embedded font programs with their descriptors, and the ICC profiles with the output intents that carry them.
//!
//! This reports raw facts and the bytes of each recovered file; naming, de-duplicating profiles by content and the
//! `document.json` layout are the Python `proof_pdf.extract`'s. Objects are visited in one fixed order, reachable from
//! the catalog depth-first with each dictionary's values in key order and the last pushed visited first, as
//! pikepdf walks them, so assets keep their order.

mod image;
mod interactive;
mod tagged;
mod vector;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::checks::Settings;
use crate::checks::contents;
use crate::checks::contrast::describe;
use crate::checks::painted::page_of;
use crate::common::family::{self, Family};
use crate::common::function;
use crate::common::icc;
use crate::common::rect::rect;
use crate::common::text;
use crate::document::{Dict, Document, Object, Value};
use crate::findings::Level;
use crate::findings::{OBJECT_DANGLING_REFERENCE, Severity};
use crate::graphics::{self, Event, Kind};
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
    /// Every image `XObject` reachable from the catalog, written out, when images were asked for.
    images: Vec<ImageFile>,
    /// Form fields, links, annotations, outline, page labels, layers and attachments.
    interactive: serde_json::Value,
    /// Every painted path, for vector artwork, when vector artwork was asked for.
    shapes: Vec<vector::ShapeFacts>,
    /// The structure tree, when the file is tagged.
    structure: Option<tagged::Structure>,
    /// What each page paints that `extract` reports, in the order painted: text runs and placed images.
    paints: Vec<Painted>,
    /// The spot inks each page selects, by page number from 1, sorted.
    spot_inks: BTreeMap<u32, BTreeSet<String>>,
    /// Every distinct `Separation` and `DeviceN` space reachable from the catalog, in the order met.
    spot_colours: Vec<SpotColour>,
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

/// A text run or a placed image, on page `page` (from 1), with its bounding box in page space.
#[derive(Debug, PartialEq, Serialize)]
struct Painted {
    page: u32,
    kind: &'static str,
    bbox: Option<[f64; 4]>,
    mcid: Option<i64>,
    artifact: Option<String>,
    /// An image's object number and placement matrix.
    image: Option<u32>,
    matrix: Option<[f64; 6]>,
    /// A text run's characters (`None` when its font does not tell them), and its codes when it does not.
    text: Option<String>,
    codes: Option<Vec<u32>>,
    glyphs: usize,
    font: Option<String>,
    size: f64,
    /// A text run's font size as `Tf` set it, before any matrix.
    tf_size: f64,
    /// Its fill colour in words, as the inventory writes colours.
    colour: String,
    rotation: f64,
    h_scale: f64,
    char_spacing: f64,
    render_mode: i64,
}

/// A spot colour space: its ink names, its alternate space and the alternate's components at full tint (`None` when
/// the tint transform is not understood), rounded to 4 places.
#[derive(Debug, PartialEq, Serialize)]
struct SpotColour {
    names: Vec<String>,
    alternate_space: Option<String>,
    alternate: Option<Vec<f64>>,
}

/// An image `XObject` written as a file, with what its dictionary says.
#[derive(Debug, PartialEq, Serialize)]
struct ImageFile {
    num: u32,
    /// The file's extension, without its dot, when it could be written.
    extension: Option<&'static str>,
    data: Blob,
    /// Its soft mask's object number, file extension and file, when it has one.
    mask: Option<(u32, Option<&'static str>, Blob)>,
    pixels: [i64; 2],
    colour_space: &'static str,
    bits: i64,
    filters: Vec<String>,
    stencil: bool,
    soft_mask: bool,
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
            Some(data) => self.store(data),
            None => Blob::Error(format!("{what} cannot be read")),
        }
    }

    fn store(&mut self, data: Vec<u8>) -> Blob {
        self.blobs.push(data);
        Blob::Data(self.blobs.len() - 1)
    }
}

/// Read `bytes` and recover the facts above, writing out every image when `images` is true and reading every painted
/// path when `vector` is.
///
/// # Errors
///
/// The reason a file cannot be read: its structure is too damaged, or it is encrypted.
pub fn recover(bytes: &[u8], images: bool, vector: bool) -> Result<Recovered, String> {
    let doc = Document::open(bytes, &Limits::default()).map_err(|refusal| refusal.message)?;
    let mut out = Recovered::default();
    out.facts.pdf_version = String::from_utf8_lossy(doc.version()).into_owned();
    out.facts.damage = doc
        .findings()
        .into_iter()
        // Errors lose data, and so does a dangling reference, which reads as null (a warning for verifying); the other
        // warnings are about how the file is written, not what can be read from it.
        .filter(|f| f.severity == Severity::Error || f.check == OBJECT_DANGLING_REFERENCE)
        .chain(contents::damaged(&doc, &Settings::from(Level::Print)))
        .map(|f| format!("{}: {}", f.check.id, f.message))
        .collect();
    out.facts.pages = pages(&doc);
    out.facts.info = info(&doc);
    if let Some(packet) = doc.catalog().and_then(|c| c.get(b"Metadata")) {
        let data = stream(&doc, packet);
        out.facts.xmp = Some(out.blob(data, "the XMP packet"));
    }
    intents(&doc, &mut out);
    walk(&doc, &mut out.facts);
    out.facts.structure = tagged::read(&doc);
    if vector {
        out.facts.shapes = vector::shapes(&doc);
    }
    out.facts.interactive = interactive::collect(&doc, &mut out);
    let mut fonts = BTreeSet::new();
    reachable(&doc, |num, object| {
        if images
            && let Some(num) = num
            && let Some(dict) = doc.stream_dict(num)
            && dict.get(b"Subtype").and_then(Object::name) == Some(b"Image")
        {
            let file = image_file(&doc, num, dict, &mut out);
            out.facts.images.push(file);
        }
        if let Some(spot) = spot_colour(&doc, object)
            && !out
                .facts
                .spot_colours
                .iter()
                .any(|s| s.names == spot.names && s.alternate_space == spot.alternate_space)
        {
            out.facts.spot_colours.push(spot);
        }
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

/// `object` read as a `Separation` or `DeviceN` space, or `None` when it is not one or is malformed.
fn spot_colour(doc: &Document, object: &Object) -> Option<SpotColour> {
    let [head, inks, alternate, transform, ..] = object.array()? else {
        return None;
    };
    let names = match head.name()? {
        b"Separation" => vec![name(doc, inks)?],
        b"DeviceN" => doc
            .resolve(inks)?
            .array()?
            .iter()
            .map(|n| name(doc, n))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let alternate_space = match family::classify(doc, Some(alternate), None) {
        Family::Gray => Some("DeviceGray".to_owned()),
        Family::Rgb => Some("DeviceRGB".to_owned()),
        Family::Cmyk => Some("DeviceCMYK".to_owned()),
        Family::Other => space_label(doc, alternate),
    };
    let alternate = function::evaluate(doc, transform, &vec![1.0; names.len()])
        .map(|values| values.into_iter().map(|v| vector::round(v, 4)).collect());
    Some(SpotColour {
        names,
        alternate_space,
        alternate,
    })
}

/// A colour space's name, its array's head, or `ICCBased(N)`.
fn space_label(doc: &Document, space: &Object) -> Option<String> {
    let space = doc.resolve(space)?;
    if let Some(name) = space.name() {
        return Some(String::from_utf8_lossy(name).into_owned());
    }
    let [head, rest @ ..] = space.array()? else { return None };
    let head = name(doc, head)?;
    if head != "ICCBased" {
        return Some(head);
    }
    let n = rest
        .first()
        .and_then(|s| doc.resolve(s))
        .and_then(Object::dict)
        .and_then(|d| d.get(b"N"))
        .and_then(|n| doc.resolve(n))
        .and_then(Object::integer)
        .unwrap_or(0);
    Some(format!("ICCBased({n})"))
}

/// Image `num` written out, with its soft mask and the facts its dictionary gives.
fn image_file(doc: &Document, num: u32, dict: &Dict, out: &mut Recovered) -> ImageFile {
    let (extension, data) = match image::write(doc, num) {
        Ok((extension, data)) => (Some(extension), out.store(data)),
        Err(why) => (None, Blob::Error(why)),
    };
    let smask = dict.get(b"SMask");
    let mask = smask.and_then(reference).map(|mask| match image::write(doc, mask) {
        Ok((extension, data)) => (mask, Some(extension), out.store(data)),
        Err(why) => (mask, None, Blob::Error(why)),
    });
    let int = |key: &[u8]| {
        dict.get(key)
            .and_then(|v| doc.resolve(v))
            .and_then(Object::integer)
            .unwrap_or(0)
    };
    let filters = match dict.get(b"Filter").and_then(|f| doc.resolve(f)).map(|f| &f.value) {
        Some(Value::Name(n)) => vec![String::from_utf8_lossy(n).into_owned()],
        Some(Value::Array(items)) => items.iter().filter_map(|i| name(doc, i)).collect(),
        _ => Vec::new(),
    };
    ImageFile {
        num,
        extension,
        data,
        mask,
        pixels: [int(b"Width"), int(b"Height")],
        colour_space: match family::classify(doc, dict.get(b"ColorSpace"), None) {
            Family::Gray => "gray",
            Family::Rgb => "rgb",
            Family::Cmyk => "cmyk",
            Family::Other => "other",
        },
        bits: int(b"BitsPerComponent"),
        filters,
        stencil: dict.get(b"ImageMask").and_then(|m| doc.resolve(m)).map(|m| &m.value) == Some(&Value::Bool(true)),
        soft_mask: smask.is_some(),
    }
}

/// The text runs and placed images of every page, and each page's spot inks, from the page walk.
fn walk(doc: &Document, facts: &mut Facts) {
    for event in graphics::walk(doc) {
        match event {
            Event::Spots { names, frames } => facts
                .spot_inks
                .entry(page_of(&frames))
                .or_default()
                .extend(names.iter().map(|n| String::from_utf8_lossy(n).into_owned())),
            Event::Paint {
                kind,
                bbox,
                ink,
                frames,
                ..
            } if !ink.cell => {
                let (kind, render_mode) = match kind {
                    Kind::Text(mode) => ("text", mode),
                    Kind::Image if ink.object.is_some() => ("image", 0),
                    _ => continue,
                };
                let text = ink.text.clone();
                facts.paints.push(Painted {
                    page: page_of(&frames),
                    kind,
                    bbox: bbox.map(|b| [b.x0, b.y0, b.x1, b.y1]),
                    mcid: ink.mcid,
                    artifact: ink.artifact.clone(),
                    image: ink.object,
                    matrix: ink.matrix,
                    codes: (kind == "text" && text.is_none()).then(|| ink.codes.clone()),
                    text,
                    glyphs: ink.glyphs,
                    font: ink.font.as_ref().filter(|_| kind == "text").map(|f| f.name.clone()),
                    size: ink.size,
                    tf_size: ink.show.as_ref().map_or(0.0, |s| s.size),
                    colour: describe(Some(&ink.fill)),
                    rotation: ink.rotation,
                    h_scale: ink.h_scale,
                    char_spacing: ink.char_spacing,
                    render_mode,
                });
            }
            _ => {}
        }
    }
}

/// Why pieces of vector artwork could not be written.
#[derive(Debug, PartialEq)]
pub enum PiecesError {
    /// The file cannot be read: why.
    Unreadable(String),
    /// The requests are not JSON of the right shape: why.
    Request(String),
}

/// The pieces of vector artwork `requests` (JSON, as the Python grouping writes them) asks for, each written out: its
/// content stream, the resources it names and its PDF, as JSON whose blobs index the bytes returned with it.
///
/// # Errors
///
/// When the file cannot be read, or the requests are not JSON of the right shape.
pub fn vector_pieces(bytes: &[u8], requests: &str) -> Result<(String, Vec<Vec<u8>>), PiecesError> {
    let requests: Vec<vector::Piece> =
        serde_json::from_str(requests).map_err(|e| PiecesError::Request(e.to_string()))?;
    let doc = Document::open(bytes, &Limits::default()).map_err(|refusal| PiecesError::Unreadable(refusal.message))?;
    let mut blobs = Vec::new();
    let pieces = vector::pieces(&doc, &requests, &mut blobs);
    Ok((serde_json::Value::from(pieces).to_string(), blobs))
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
    dict.map(|d| d.entries().iter())
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
/// UTF-16 behind either byte order mark, its pairs decoded as far as they go, a lone surrogate replaced.
fn loose_utf16(bytes: &[u8]) -> Option<String> {
    let (big, rest) = match bytes {
        [0xFE, 0xFF, rest @ ..] => (true, rest),
        [0xFF, 0xFE, rest @ ..] => (false, rest),
        _ => return None,
    };
    let units = rest.as_chunks::<2>().0.iter().map(|pair| {
        if big {
            u16::from_be_bytes(*pair)
        } else {
            u16::from_le_bytes(*pair)
        }
    });
    Some(
        char::decode_utf16(units)
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
    )
}

fn pdf_str(doc: &Document, object: &Object) -> Option<String> {
    Some(match &doc.resolve(object)?.value {
        // PDF 2.0 adds UTF-8 text strings behind a byte order mark (ISO 32000-2 7.9.2.2.1), as pikepdf reads them.
        Value::String(bytes, _) => match bytes.strip_prefix(b"\xEF\xBB\xBF") {
            Some(utf8) => String::from_utf8_lossy(utf8).into_owned(),
            // UTF-16 that is not valid (an odd last byte, a lone surrogate, little-endian behind its own mark) is read
            // pair by pair, as pikepdf reads it; and a byte PDFDocEncoding leaves undefined is a replacement character
            // if it is one of the three printable-range gaps, else (a NUL, say) its Latin-1 value, rather than costing
            // the whole string.
            None => text::decode(bytes).or_else(|| loose_utf16(bytes)).unwrap_or_else(|| {
                bytes
                    .iter()
                    .map(|&b| {
                        text::decode(&[b]).unwrap_or_else(|| match b {
                            0x7F | 0x9F | 0xAD => char::REPLACEMENT_CHARACTER.to_string(),
                            _ => char::from(b).to_string(),
                        })
                    })
                    .collect()
            }),
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
fn reachable<'a>(doc: &'a Document, mut visit: impl FnMut(Option<u32>, &'a Object)) {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&Object> = doc.trailer().get(b"Root").into_iter().collect();
    while let Some(next) = stack.pop() {
        if let Some(num) = reference(next)
            && !seen.insert(num)
        {
            continue;
        }
        let Some(object) = doc.resolve(next) else { continue };
        visit(reference(next), object);
        match &object.value {
            Value::Dict(dict) => {
                let mut entries: Vec<_> = dict.entries().iter().filter(|(k, _)| k != b"Parent").collect();
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
        recover(
            &classic(&refs, &format!("<< /Size SIZE /Root 1 0 R {trailer} >>")),
            true,
            true,
        )
        .unwrap()
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
                    b"<< /Title (Caf\xe9) /Subject <FEFF00E9> /Author <9F01> /Keywords <EFBBBFC3A9> /Trapped /False /Pages 7 /Scale 1.5 \
                      /Marked true /Off false /Gone null /Lost 99 0 R /LE <FFFE4100> /Odd <FEFF004100> /Lone <FEFFD800> >>"
                        .to_vec(),
                ),
                (3, stream("/Type /Metadata /Subtype /XML", b"<x:xmpmeta/>")),
            ],
            "/Info 2 0 R",
        );
        let got = json(&recovered);
        assert_eq!(
            got["info"],
            json!({"Author": "\u{FFFD}\u{1}", "Keywords": "é", "Marked": "True", "Off": "False", "Pages": "7", "Scale": "1.5",
                   "Subject": "é", "Title": "Café", "Trapped": "/False", "LE": "A", "Odd": "A", "Lone": "\u{FFFD}"})
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
    fn images_are_written_with_their_soft_masks_when_asked_for() {
        let objects = [
            (
                1,
                b"<< /Type /Catalog /Images [2 0 R 3 0 R 4 0 R 7 0 R 8 0 R] /Mask 5 0 R >>".to_vec(),
            ),
            (
                7,
                stream("/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB", &[1, 2, 3]),
            ),
            (
                8,
                stream(
                    "/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceCMYK /BitsPerComponent 3",
                    &[0],
                ),
            ),
            (
                2,
                stream(
                    "/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 5 0 R",
                    &[7],
                ),
            ),
            (
                3,
                stream(
                    "/Subtype /Image /Width 1 /Height 1 /ImageMask true /Filter [/DCTDecode] /SMask 6 0 R",
                    b"jpeg",
                ),
            ),
            (
                4,
                stream("/Subtype /Image /Filter /DCTDecode /ColorSpace /Lab", b"jpeg"),
            ),
            (
                5,
                stream(
                    "/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                    &[9],
                ),
            ),
            (6, b"(not a stream)".to_vec()),
        ];
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let bytes = classic(&refs, "<< /Size SIZE /Root 1 0 R >>");
        assert_eq!(json(&recover(&bytes, false, false).unwrap())["images"], json!([]));
        let recovered = recover(&bytes, true, false).unwrap();
        let images = json(&recovered)["images"].clone();
        let summary: Vec<Json> = images
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                json!([
                    i["num"],
                    i["extension"],
                    i["mask"],
                    i["colour_space"],
                    i["filters"],
                    i["stencil"]
                ])
            })
            .collect();
        assert_eq!(
            summary,
            [
                json!([5, "png", null, "gray", [], false]),
                json!([8, null, null, "cmyk", [], false]),
                json!([7, "png", null, "rgb", [], false]),
                json!([4, "jpg", null, "other", ["DCTDecode"], false]),
                json!([3, "jpg", [6, null, {"error": "it is not a stream that can be read"}], "other", ["DCTDecode"], true]),
                json!([2, "png", [5, "png", {"data": 5}], "gray", [], false]),
            ]
        );
        assert_eq!(
            (&images[5]["pixels"], &images[5]["bits"], &images[5]["soft_mask"]),
            (&json!([1, 1]), &json!(8), &json!(true))
        );
        assert_eq!(&recovered.blobs[2], b"jpeg");
        assert_eq!(
            images[1]["data"],
            json!({"error": "its /BitsPerComponent is not 1, 2, 4, 8 or 16"})
        );
    }

    #[test]
    fn spot_colours_once_each_with_their_alternates() {
        let spaces = "[/Separation /Gold /DeviceCMYK 3 0 R] [/Separation /Gold /DeviceCMYK 3 0 R] \
            [/DeviceN [/A /B] [/Lab << >>] << >>] [/Separation /P [/ICCBased 4 0 R] 3 0 R] \
            [/Separation /F /Foo 3 0 R] [/Separation /N 5 3 0 R] [/Separation /M [[1]] 3 0 R] \
            [/DeviceN /A /DeviceGray 3 0 R] [/Indexed /DeviceRGB 0 <00>] [/Separation] \
            [/Separation 5 /DeviceGray 3 0 R] [/DeviceN 99 0 R /DeviceGray 3 0 R] [/DeviceN [/A 5] /DeviceGray 3 0 R] \
            [/Separation /G /DeviceGray 3 0 R] [/Separation /R /DeviceRGB 3 0 R] [/Separation /D 98 0 R 3 0 R] \
            [/Separation /E [] 3 0 R] [/Separation /Q [/ICCBased 6 0 R] 3 0 R]";
        let recovered = file(
            &[
                (1, format!("<< /Type /Catalog /X [{spaces}] >>").into_bytes()),
                (
                    3,
                    b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.123456 1 0] /N 1 >>".to_vec(),
                ),
                (4, stream("", b"not a profile")),
                (6, stream("/N 2", b"not a profile")),
            ],
            "",
        );
        let got = json(&recovered)["spot_colours"].clone();
        let tint = json!([0.0, 0.1235, 1.0, 0.0]);
        assert_eq!(
            got,
            json!([
                {"names": ["Q"], "alternate_space": "ICCBased(2)", "alternate": tint},
                {"names": ["E"], "alternate_space": null, "alternate": tint},
                {"names": ["D"], "alternate_space": null, "alternate": tint},
                {"names": ["R"], "alternate_space": "DeviceRGB", "alternate": tint},
                {"names": ["G"], "alternate_space": "DeviceGray", "alternate": tint},
                {"names": ["M"], "alternate_space": null, "alternate": tint},
                {"names": ["N"], "alternate_space": null, "alternate": tint},
                {"names": ["F"], "alternate_space": "Foo", "alternate": tint},
                {"names": ["P"], "alternate_space": "ICCBased(0)", "alternate": tint},
                {"names": ["A", "B"], "alternate_space": "Lab", "alternate": null},
                {"names": ["Gold"], "alternate_space": "DeviceCMYK", "alternate": tint},
            ])
        );
    }

    #[test]
    fn the_page_walk_gives_text_runs_placed_images_and_spot_inks() {
        let content = "/S cs 1 scn /P << /MCID 2 >> BDC BT /F 10 Tf 1 0 0 1 72 72 Tm (Hi) Tj ET EMC BT /C 5 Tf <0001> Tj ET \
            q 20 0 0 10 0 0 cm /Im Do Q q 9 0 0 9 0 0 cm BI /W 1 /H 1 /CS /G /BPC 8 ID x EI Q 0 0 5 5 re f \
            /Pattern cs /T scn 0 0 5 5 re f";
        let recovered = file(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << \
                      /Font << /F << /Subtype /Type1 /BaseFont /Helvetica >> /C << /Subtype /Type0 /BaseFont /Cid \
                      /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 >>] >> >> /XObject << /Im 5 0 R >> \
                      /ColorSpace << /S [/Separation /Gold /DeviceCMYK 7 0 R] >> /Pattern << /T 6 0 R >> >> >>"
                        .to_vec(),
                ),
                (4, stream("", content.as_bytes())),
                (
                    5,
                    stream(
                        "/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                        &[0],
                    ),
                ),
                (
                    6,
                    stream(
                        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /Resources << >>",
                        b"BT ET 0 0 1 1 re f",
                    ),
                ),
                (7, b"<< /FunctionType 2 /Domain [0 1] /C1 [0 0 1 0] /N 1 >>".to_vec()),
            ],
            "",
        );
        let got = json(&recovered);
        assert_eq!(got["spot_inks"], json!({"1": ["Gold"]}));
        let paints = got["paints"].as_array().unwrap();
        assert_eq!(paints.len(), 3);
        let (text, image) = (&paints[0], &paints[2]);
        assert_eq!((&paints[1]["text"], &paints[1]["codes"]), (&json!(null), &json!([1])));
        assert_eq!(
            [
                &text["kind"],
                &text["text"],
                &text["codes"],
                &text["font"],
                &text["mcid"],
                &text["colour"]
            ],
            [
                &json!("text"),
                &json!("Hi"),
                &json!(null),
                &json!("Helvetica"),
                &json!(2),
                &json!("spot Gold 100%")
            ]
        );
        assert_eq!(
            (&text["size"], &text["render_mode"], &text["glyphs"]),
            (&json!(10.0), &json!(0), &json!(2))
        );
        assert_eq!(
            [
                &image["kind"],
                &image["image"],
                &image["matrix"],
                &image["bbox"],
                &image["font"]
            ],
            [
                &json!("image"),
                &json!(5),
                &json!([20.0, 0.0, 0.0, 10.0, 0.0, 0.0]),
                &json!([0.0, 0.0, 20.0, 10.0]),
                &json!(null)
            ]
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused() {
        assert!(recover(b"not a pdf", false, false).is_err());
    }
}
