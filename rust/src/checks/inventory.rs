//! What is in a print PDF, as plain data, for `inspect`: the facts a proofer or another tool reads, never judged. The
//! fonts (embedded or not, how many glyphs, any `.notdef`), the text colours and sizes, the spot inks, the images with
//! their effective resolution, the thinnest line, and, through a profile, the L* and CIEDE2000 difference of every
//! text and background pair. It reads the same walk the print checks do.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::contrast::{self, NO_CMYK, describe, measured, pairs};
use super::text::paints;
use crate::common::family::Family;
use crate::common::lab::{Model, ciede2000};
use crate::common::rect::{Rect, rect};
use crate::document::Document;
use crate::graphics::{self, Event, Ink, Kind};
use crate::pipeline::Limits;

const PT_PER_IN: f64 = 72.0;
const MM_PER_IN: f64 = 25.4;

/// Everything `inspect` reports about one file.
#[derive(Debug, PartialEq, Serialize)]
pub struct Inventory {
    pages: Vec<PageFact>,
    fonts: Vec<FontFact>,
    text_colours: Vec<ColourFact>,
    spot_inks: Vec<String>,
    images: Vec<ImageFact>,
    contrast: Vec<ContrastFact>,
    /// Why contrast could not be measured, such as CMYK text with no CMYK profile.
    contrast_error: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct PageFact {
    number: usize,
    media_mm: Option<(f64, f64)>,
    trim_mm: Option<(f64, f64)>,
    text_runs: usize,
    smallest_text_pt: Option<f64>,
    thinnest_stroke_pt: Option<f64>,
}

#[derive(Debug, PartialEq, Serialize)]
struct FontFact {
    name: String,
    subtype: String,
    embedded: bool,
    has_to_unicode: bool,
    pages: BTreeSet<u32>,
    glyphs: usize,
    notdef: usize,
}

#[derive(Debug, PartialEq, Serialize)]
struct ColourFact {
    colour: String,
    pages: BTreeSet<u32>,
    runs: usize,
    smallest_pt: f64,
}

#[derive(Debug, PartialEq, Serialize)]
struct ImageFact {
    page: u32,
    name: String,
    colour_space: &'static str,
    pixels: (i64, i64),
    /// Effective, from its placed width; `None` for a degenerate placement.
    dpi: Option<f64>,
}

#[derive(Debug, PartialEq, Serialize)]
struct ContrastFact {
    page: u32,
    text: String,
    /// A colour, `paper`, or `an image or shading`.
    background: String,
    runs: usize,
    smallest_pt: f64,
    delta_l: Option<f64>,
    delta_e: Option<f64>,
}

/// `x` rounded to `places` decimals as Python's `round()` does: on the exact binary value, ties to even.
fn round(x: f64, places: usize) -> f64 {
    format!("{x:.places$}").parse().unwrap_or(x)
}

fn size_mm(r: Option<Rect>) -> Option<(f64, f64)> {
    let mm = |pt: f64| round(pt / PT_PER_IN * MM_PER_IN, 2);
    r.map(|r| (mm(r.x1 - r.x0), mm(r.y1 - r.y0)))
}

/// Read `bytes` and report what is in them; `proof_profile` is the profile to measure contrast through instead of the
/// output intents' own, and `with_contrast` false skips measuring. An error for a file that cannot be read.
///
/// # Errors
///
/// The reason a file cannot be read: its structure is too damaged, or it is encrypted.
pub fn inspect(bytes: &[u8], proof_profile: Option<&[u8]>, with_contrast: bool) -> Result<Inventory, String> {
    let doc = Document::open(bytes, &Limits::default()).map_err(|refusal| refusal.message)?;
    Ok(facts(&doc, proof_profile, with_contrast))
}

fn facts(doc: &Document, proof_profile: Option<&[u8]>, with_contrast: bool) -> Inventory {
    let painted = paints(doc);
    let none = Vec::new();
    let mut pages = Vec::new();
    for (index, page) in doc.pages().iter().enumerate() {
        let number = index + 1;
        let on_page = u32::try_from(number)
            .ok()
            .and_then(|n| painted.get(&n))
            .unwrap_or(&none);
        let runs: Vec<f64> = on_page
            .iter()
            .filter(|(kind, _, ink)| matches!(kind, Kind::Text(_)) && ink.glyphs > 0)
            .map(|(_, _, ink)| ink.size)
            .collect();
        pages.push(PageFact {
            number,
            media_mm: size_mm(page.get(b"MediaBox").and_then(|b| rect(doc, b))),
            trim_mm: size_mm(page.get(b"TrimBox").and_then(|b| rect(doc, b))),
            text_runs: runs.len(),
            smallest_text_pt: runs.iter().copied().filter(|s| *s > 0.0).reduce(f64::min),
            thinnest_stroke_pt: on_page
                .iter()
                .filter_map(|(_, _, ink)| ink.stroke_width)
                .reduce(f64::min),
        });
    }
    let mut fonts: BTreeMap<(String, String), FontFact> = BTreeMap::new();
    let mut colours: BTreeMap<String, ColourFact> = BTreeMap::new();
    let mut images = Vec::new();
    for (&page, paints) in &painted {
        for (kind, bbox, ink) in paints {
            match kind {
                Kind::Text(_) => {
                    if let Some(font) = ink.font.as_ref().filter(|f| f.known) {
                        let fact = fonts
                            .entry((font.name.clone(), font.subtype.clone()))
                            .or_insert(FontFact {
                                name: font.name.clone(),
                                subtype: font.subtype.clone(),
                                embedded: font.embedded,
                                has_to_unicode: font.to_unicode,
                                pages: BTreeSet::new(),
                                glyphs: 0,
                                notdef: 0,
                            });
                        fact.pages.insert(page);
                        fact.glyphs += ink.glyphs;
                        fact.notdef += ink.notdef;
                    }
                    if ink.glyphs > 0 {
                        let colour = describe(Some(&ink.fill));
                        let fact = colours.entry(colour.clone()).or_insert(ColourFact {
                            colour,
                            pages: BTreeSet::new(),
                            runs: 0,
                            smallest_pt: f64::INFINITY,
                        });
                        fact.pages.insert(page);
                        fact.runs += 1;
                        fact.smallest_pt = fact.smallest_pt.min(ink.size);
                    }
                }
                Kind::Image | Kind::InlineImage => images.push(image(page, *kind, *bbox, ink)),
                _ => {}
            }
        }
    }
    let (contrast, contrast_error) = if with_contrast {
        measure(doc, proof_profile)
    } else {
        (Vec::new(), None)
    };
    Inventory {
        pages,
        fonts: fonts.into_values().collect(),
        text_colours: colours.into_values().collect(),
        spot_inks: spot_inks(doc),
        images,
        contrast,
        contrast_error,
    }
}

/// One image painted on `page`, with its effective resolution from its placed width.
fn image(page: u32, kind: Kind, bbox: Option<Rect>, ink: &Ink) -> ImageFact {
    let (width, height) = ink.pixels.unwrap_or_default();
    #[expect(clippy::cast_precision_loss, reason = "image sizes are far below 2^53")]
    let dpi = bbox
        .filter(|_| width > 0 && height > 0)
        .map(|b| (b.x1 - b.x0) / PT_PER_IN)
        .filter(|inches| *inches > 0.0)
        .map(|inches| round(width as f64 / inches, 1));
    let name = String::from_utf8_lossy(&ink.name);
    ImageFact {
        page,
        name: if kind == Kind::Image {
            format!("/{name}")
        } else {
            name.into_owned()
        },
        colour_space: match ink.space {
            Some(Family::Rgb) => "rgb",
            Some(Family::Cmyk) => "cmyk",
            Some(Family::Gray) => "gray",
            _ => "other",
        },
        pixels: (width, height),
        dpi,
    }
}

/// The spot inks the pages select, sorted.
fn spot_inks(doc: &Document) -> Vec<String> {
    let names: BTreeSet<String> = graphics::walk(doc)
        .into_iter()
        .filter_map(|event| match event {
            Event::Spots { names, .. } => Some(names),
            _ => None,
        })
        .flatten()
        .map(|n| String::from_utf8_lossy(&n).into_owned())
        .collect();
    names.into_iter().collect()
}

/// Every text and background pair, with its differences; or why none could be measured.
fn measure(doc: &Document, proof_profile: Option<&[u8]>) -> (Vec<ContrastFact>, Option<String>) {
    if proof_profile.is_some_and(|p| !Model::readable(p)) {
        return (
            Vec::new(),
            Some("the proof profile is not an ICC profile LittleCMS can read".to_owned()),
        );
    }
    let profiles = proof_profile.map_or_else(|| contrast::intent_profiles(doc), |p| vec![p.to_vec()]);
    let Some(model) = Model::from_profiles(&profiles) else {
        return (Vec::new(), Some("a profile does not convert to L*a*b*".to_owned()));
    };
    let paper = if model.has_cmyk() {
        contrast::fill(Family::Cmyk, &[0.0; 4])
    } else {
        contrast::fill(Family::Rgb, &[1.0; 3])
    };
    let mut out = Vec::new();
    for (page, (text, ground, on_paper, sizes)) in pairs(doc, &paper) {
        let differences = match &ground {
            None => None,
            Some(ground) => match (measured(&model, &text), measured(&model, ground)) {
                (Ok(one), Ok(two)) => one.zip(two),
                _ => return (Vec::new(), Some(NO_CMYK.to_owned())),
            },
        };
        out.push(ContrastFact {
            page,
            text: describe(Some(&text)),
            background: if on_paper {
                "paper".to_owned()
            } else {
                describe(ground.as_ref())
            },
            runs: sizes.len(),
            smallest_pt: round(sizes.iter().copied().fold(f64::INFINITY, f64::min), 2),
            delta_l: differences
                .as_ref()
                .map(|((a, _), (b, _))| round((a[0] - b[0]).abs(), 2)),
            delta_e: differences.map(|((a, _), (b, _))| round(ciede2000(a, b), 2)),
        });
    }
    (out, None)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::*;
    use crate::checks::test_doc::paged;

    const PROFILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/icc/little-cms-test1.icc"
    );

    const RESOURCES: &str = "/Resources << /Font << /H << /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 9 0 R >> \
        /N << /Subtype /TrueType /BaseFont /ABCDEF+Nunito /FontDescriptor << /FontFile2 9 0 R >> \
        /Encoding << /Differences [65 /.notdef] >> >> /X 7 >> \
        /XObject << /Im 10 0 R /Mask 11 0 R /G 12 0 R >> \
        /ColorSpace << /S [/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C1 [0 0 1 0] /N 1 >>] >> \
        /Shading << /Sh << /ShadingType 2 /ColorSpace /DeviceGray >> >> >>";

    fn of(pages: &[(&str, &str)], catalog: &str, profile: Option<&[u8]>, with_contrast: bool) -> Json {
        let stream = |dict: &str| format!("<< {dict} /Length 0 >>\nstream\n\nendstream").into_bytes();
        let more = [
            (9, stream("")),
            (
                10,
                stream("/Subtype /Image /Width 300 /Height 100 /BitsPerComponent 8 /ColorSpace /DeviceRGB"),
            ),
            (11, stream("/Subtype /Image /Width 10 /Height 10 /ImageMask true")),
            (
                12,
                stream("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray"),
            ),
        ];
        let doc = paged(pages, catalog, &more);
        serde_json::to_value(facts(&doc, profile, with_contrast)).unwrap()
    }

    #[test]
    fn a_page_of_everything() {
        let page = "BT /H 12 Tf 0.5 g 10 10 Td (Hi) Tj /N 6 Tf (AB) Tj /X 9 Tf (x) Tj 3 Tr (z) Tj ET \
            2 w 0 0 m 10 10 l S 0.5 w 0 0 m 5 5 l S \
            q 72 0 0 24 0 0 cm /Im Do Q q 0 0 0 0 0 0 cm /Im Do Q q 10 0 0 10 0 0 cm /Mask Do Q \
            q 36 0 0 36 0 0 cm BI /W 2 /H 2 /BPC 8 /CS /CMYK ID 0123456789abcdef EI Q \
            /S cs 0.25 scn 0 0 5 5 re f BT /H 0 Tf /S cs 0.5 scn (A) Tj ET";
        let got = of(
            &[
                (
                    &format!("/MediaBox [0 0 595.276 841.89] /TrimBox [10 10 307.638 430.945] {RESOURCES}"),
                    page,
                ),
                ("/MediaBox [0 0 100 100]", ""),
            ],
            "",
            None,
            false,
        );
        assert_eq!(
            got,
            json!({
                "pages": [
                    {"number": 1, "media_mm": [210.0, 297.0], "trim_mm": [105.0, 148.5], "text_runs": 5,
                     "smallest_text_pt": 6.0, "thinnest_stroke_pt": 0.5},
                    {"number": 2, "media_mm": [35.28, 35.28], "trim_mm": null, "text_runs": 0,
                     "smallest_text_pt": null, "thinnest_stroke_pt": null}
                ],
                "fonts": [
                    {"name": "Helvetica", "subtype": "Type1", "embedded": false, "has_to_unicode": true,
                     "pages": [1], "glyphs": 3, "notdef": 0},
                    {"name": "Nunito", "subtype": "TrueType", "embedded": true, "has_to_unicode": false,
                     "pages": [1], "glyphs": 2, "notdef": 1}
                ],
                "text_colours": [
                    {"colour": "gray 50%", "pages": [1], "runs": 4, "smallest_pt": 6.0},
                    {"colour": "spot Gold 50%", "pages": [1], "runs": 1, "smallest_pt": 0.0}
                ],
                "spot_inks": ["Gold"],
                "images": [
                    {"page": 1, "name": "/Im", "colour_space": "rgb", "pixels": [300, 100], "dpi": 300.0},
                    {"page": 1, "name": "/Im", "colour_space": "rgb", "pixels": [300, 100], "dpi": null},
                    {"page": 1, "name": "/Mask", "colour_space": "other", "pixels": [10, 10], "dpi": 72.0},
                    {"page": 1, "name": "(inline)", "colour_space": "cmyk", "pixels": [2, 2], "dpi": 4.0}
                ],
                "contrast": [],
                "contrast_error": null
            })
        );
    }

    #[test]
    fn colours_and_images_in_words() {
        let resources = RESOURCES.replace(
            "/ColorSpace <<",
            "/ColorSpace << /DN [/DeviceN [/Gold /Silver] /DeviceCMYK << /FunctionType 2 /Domain [0 1] /N 1 >>] \
             /P [/Pattern /DeviceRGB]",
        );
        let page = "BT /H 12 Tf /DN cs 0.1 0.2 scn (A) Tj /P cs 1 0 0 /Pat scn (B) Tj () Tj ET \
            q 72 0 0 72 0 0 cm /G Do Q";
        let got = of(&[(&resources, page)], "", None, false);
        assert_eq!(
            got["text_colours"],
            json!([
                {"colour": "a spot or pattern colour", "pages": [1], "runs": 1, "smallest_pt": 12.0},
                {"colour": "spot Gold+Silver", "pages": [1], "runs": 1, "smallest_pt": 12.0}
            ])
        );
        assert_eq!(got["fonts"][0]["glyphs"], json!(2), "the empty run adds no glyph");
        assert_eq!(
            got["images"],
            json!([{"page": 1, "name": "/G", "colour_space": "gray", "pixels": [1, 1], "dpi": 1.0}])
        );
    }

    #[test]
    fn contrast_through_a_profile() {
        let page = "BT /H 12 Tf 0.2 g 100 100 Td (A) Tj ET 0.9 g 50 50 150 150 re f \
            BT /H 12 Tf 0 0 0 0.8 k 100 100 Td (A) Tj ET \
            /Sh sh BT /H 12 Tf 0 g 100 100 Td (A) Tj ET \
            0 0 0 rg 50 50 150 150 re f BT /H 10 Tf /S cs 1 scn 100 100 Td (A) Tj ET";
        let pages = [(RESOURCES, page)];
        let profile = std::fs::read(PROFILE).unwrap();
        let got = of(&pages, "", Some(&profile), true);
        let pair = |text: &str, background: &str, delta_l: Json, delta_e: Json| {
            json!({"page": 1, "text": text, "background": background, "runs": 1,
                   "smallest_pt": if text.starts_with("spot") { 10.0 } else { 12.0 }, "delta_l": delta_l, "delta_e": delta_e})
        };
        assert_eq!(
            got["contrast"],
            json!([
                pair("gray 20%", "paper", json!(58.82), json!(45.2)),
                pair("C0 M0 Y0 K80", "gray 90%", json!(50.98), json!(41.06)),
                pair("gray 0%", "an image or shading", Json::Null, Json::Null),
                pair("spot Gold 100%", "RGB(0,0,0)", Json::Null, Json::Null),
            ])
        );
        assert_eq!(got["contrast_error"], Json::Null);
        let none = of(&pages, "", None, true);
        assert_eq!(
            (&none["contrast"], &none["contrast_error"]),
            (&json!([]), &json!(NO_CMYK))
        );
        let off = of(&pages, "", None, false);
        assert_eq!((&off["contrast"], &off["contrast_error"]), (&json!([]), &Json::Null));
        // A CMYK printer profile header with no tags: LittleCMS reads it, but it converts nothing.
        let mut empty = vec![0u8; 132];
        empty[0..4].copy_from_slice(&132u32.to_be_bytes());
        empty[8] = 4;
        empty[12..16].copy_from_slice(b"prtr");
        empty[16..20].copy_from_slice(b"CMYK");
        empty[20..24].copy_from_slice(b"Lab ");
        empty[36..40].copy_from_slice(b"acsp");
        let inert = of(&pages, "", Some(&empty), true);
        assert_eq!(inert["contrast_error"], json!("a profile does not convert to L*a*b*"));
        let broken = of(&pages, "", Some(b"not a profile"), true);
        assert_eq!(
            broken["contrast_error"],
            json!("the proof profile is not an ICC profile LittleCMS can read")
        );
    }

    #[test]
    fn a_file_is_read_or_refused() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs/good.pdf")).unwrap();
        assert_eq!(inspect(&bytes, None, true).unwrap().pages.len(), 1);
        assert!(inspect(b"not a pdf", None, true).is_err());
    }
}
