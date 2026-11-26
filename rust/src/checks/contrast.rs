//! The contrast check (ported from the Python `contrast` module, under the parity gate): text whose lightness differs
//! from its background by less than the profile's minimum L* difference fails.
//!
//! This is deliberately not WCAG, a screen model: for ink on a substrate the figure that means anything is the L*
//! difference through the profile the job prints with (the proof profile given, else the output intents' own), with
//! the paper as the page's white. A text run's background is the last filled shape painted under the centre of its
//! box, else the paper. An image or shading under it has no known colour, nor does a spot or pattern colour, so that
//! text is reported as not measured, never as a pass or a fail. Clipping, transparency and blend modes are not
//! modelled: this estimates the design's intent. CMYK with no CMYK profile to measure it through is an error, since the
//! check was asked for.

use super::Settings;
use super::text::paints;
use crate::common::family::Family;
use crate::common::lab::{Lab, Model};
use crate::common::pyfmt::g;
use crate::common::rect::Rect;
use crate::document::{Document, Object, Value};
use crate::findings::{CONTRAST, Finding, Location, Severity};
use crate::graphics::{Fill, Kind};

const NO_CMYK: &str = "no CMYK profile: the PDF has no CMYK output intent and none was given";

/// The output intents' profiles, in order.
fn intent_profiles(doc: &Document) -> Vec<Vec<u8>> {
    let intents = doc
        .catalog()
        .and_then(|c| c.get(b"OutputIntents"))
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)
        .unwrap_or_default();
    intents
        .iter()
        .filter_map(
            |intent| match doc.resolve(intent)?.dict()?.get(b"DestOutputProfile")?.value {
                Value::Reference(num, _) => doc.stream_data(num),
                _ => None,
            },
        )
        .collect()
}

fn fill(family: Family, value: &[f64]) -> Fill {
    Fill {
        family,
        value: value.to_vec(),
        spot: Vec::new(),
        pattern: std::collections::BTreeSet::new(),
        profiled: false,
    }
}

/// `fill` as L*a*b* and in words: `None` for a colour no profile describes (a spot or pattern colour), an error for
/// CMYK with no CMYK profile. Gray is the K plate when there is a CMYK profile.
fn measured(model: &Model, fill: &Fill) -> Result<Option<(Lab, String)>, ()> {
    // Python's `round()` gives an integer: never "-0".
    let percent = |v: f64| (v * 100.0).round_ties_even() + 0.0;
    let cmyk = |c: [f64; 4]| model.cmyk(c).ok_or(());
    Ok(Some(match (fill.family, fill.value.as_slice()) {
        (Family::Cmyk, [c, m, y, k, ..]) => {
            let words = format!("C{} M{} Y{} K{}", percent(*c), percent(*m), percent(*y), percent(*k));
            (cmyk([*c, *m, *y, *k])?, words)
        }
        (Family::Rgb, [r, g, b, ..]) => {
            let byte = |v: f64| (v * 255.0).round_ties_even() + 0.0;
            (
                model.rgb([*r, *g, *b]),
                format!("RGB({},{},{})", byte(*r), byte(*g), byte(*b)),
            )
        }
        (Family::Gray, [gray, ..]) => {
            let lab = model
                .cmyk([0.0, 0.0, 0.0, 1.0 - gray])
                .unwrap_or_else(|| model.rgb([*gray; 3]));
            (lab, format!("gray {}%", percent(*gray)))
        }
        _ => return Ok(None),
    }))
}

/// One text colour on one background on a page: the background (`None` for an image or shading), whether that is the
/// paper, and the sizes of its runs.
type Pair = (Fill, Option<Fill>, bool, Vec<f64>);

/// The distinct (text colour, background) pairs of each page, in the order first met.
fn pairs(doc: &Document, paper: &Fill) -> Vec<(u32, Pair)> {
    let unbounded = Rect {
        x0: f64::NEG_INFINITY,
        y0: f64::NEG_INFINITY,
        x1: f64::INFINITY,
        y1: f64::INFINITY,
    };
    let mut out = Vec::new();
    for (page, paints) in paints(doc) {
        let mut grounds: Vec<(Rect, Option<&Fill>)> = Vec::new();
        let mut found: Vec<Pair> = Vec::new();
        for (kind, bbox, ink) in &paints {
            match (kind, bbox) {
                (Kind::Image | Kind::InlineImage | Kind::Shading, _) => grounds.push((bbox.unwrap_or(unbounded), None)),
                (Kind::Path { filled: true, .. }, Some(bbox)) => {
                    grounds.push((*bbox, (ink.fill.family != Family::Other).then_some(&ink.fill)));
                }
                (Kind::Text(_), Some(bbox)) if ink.glyphs > 0 => {
                    let (x, y) = (f64::midpoint(bbox.x0, bbox.x1), f64::midpoint(bbox.y0, bbox.y1));
                    let under = grounds
                        .iter()
                        .rev()
                        .find(|(r, _)| r.x0 <= x && x <= r.x1 && r.y0 <= y && y <= r.y1);
                    let (ground, on_paper) = under.map_or((Some(paper), true), |(_, f)| (*f, false));
                    let ground = ground.cloned();
                    match found
                        .iter_mut()
                        .find(|(t, gr, p, _)| *t == ink.fill && *gr == ground && *p == on_paper)
                    {
                        Some((_, _, _, sizes)) => sizes.push(ink.size),
                        None => found.push((ink.fill.clone(), ground, on_paper, vec![ink.size])),
                    }
                }
                _ => {}
            }
        }
        out.extend(found.into_iter().map(|pair| (page, pair)));
    }
    out
}

/// Text differs enough in lightness from what it is printed on.
pub(super) fn contrast(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(minimum) = settings.params.min_delta_l else {
        return Vec::new();
    };
    let profiles = match &settings.params.proof_profile {
        Some(path) => match std::fs::read(path) {
            Ok(data) => vec![data],
            Err(e) => {
                return vec![Finding::new(
                    CONTRAST,
                    format!("cannot read the proof profile {path}: {e}"),
                )];
            }
        },
        None => intent_profiles(doc),
    };
    let Some(model) = Model::from_profiles(&profiles) else {
        return vec![Finding::new(
            CONTRAST,
            "cannot measure contrast: a profile does not convert to L*a*b*",
        )];
    };
    let paper = if model.has_cmyk() {
        fill(Family::Cmyk, &[0.0; 4])
    } else {
        fill(Family::Rgb, &[1.0; 3])
    };
    let mut out = Vec::new();
    let mut unmeasured = std::collections::BTreeSet::new();
    for (page, (text, ground, on_paper, sizes)) in pairs(doc, &paper) {
        let Some(ground_fill) = &ground else {
            unmeasured.insert(page);
            continue;
        };
        let (Ok(one), Ok(two)) = (measured(&model, &text), measured(&model, ground_fill)) else {
            return vec![Finding::new(CONTRAST, format!("cannot measure contrast: {NO_CMYK}"))];
        };
        let (Some((one, text)), Some((two, under))) = (one, two) else {
            unmeasured.insert(page);
            continue;
        };
        let delta = (one[0] - two[0]).abs();
        if delta < minimum {
            let ground = if on_paper { "the paper".to_owned() } else { under };
            let smallest = sizes.iter().copied().fold(f64::INFINITY, f64::min);
            let message = format!(
                "text in {text} on {ground} differs by only {delta:.1} L* (minimum {}; {} run(s), smallest {smallest:.1}pt)",
                g(minimum),
                sizes.len()
            );
            out.push(Finding::new(CONTRAST, message).at(vec![Location::Page(page)]));
        }
    }
    for page in unmeasured {
        out.push(Finding {
            severity: Severity::Warning,
            ..Finding::new(
                CONTRAST,
                "text over an image, a shading or a spot colour: contrast not measured",
            )
            .at(vec![Location::Page(page)])
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Params;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    const PROFILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/icc/little-cms-test1.icc"
    );

    /// A run of Helvetica at 12 points over the point (100, 100), its fill set by `colour`.
    fn text(colour: &str) -> String {
        format!("BT /H 12 Tf {colour} 95 96 Td (A) Tj ET")
    }

    fn found(pages: &[&str], catalog: &str, params: Params) -> Vec<(Severity, Option<u32>, String)> {
        let resources = "/Resources << /Font << /H << /Subtype /Type1 /BaseFont /Helvetica >> >> \
            /Pattern << /P << /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceGray >> >> >> \
            /ColorSpace << /S [/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C1 [0 0 1 0] /N 1 >>] >> \
            /Shading << /Sh << /ShadingType 2 /ColorSpace /DeviceGray >> >> >>";
        let pages: Vec<(&str, &str)> = pages.iter().map(|p| (resources, *p)).collect();
        let profile = std::fs::read(PROFILE).unwrap();
        let mut stream = format!("<< /N 4 /Length {} >>\nstream\n", profile.len()).into_bytes();
        stream.extend(profile);
        stream.extend(b"\nendstream");
        let doc = paged(&pages, catalog, &[(9, stream)]);
        let settings = Settings {
            level: Level::Print,
            params,
        };
        contrast(&doc, &settings)
            .into_iter()
            .map(|f| (f.severity, f.page, f.message))
            .collect()
    }

    fn at_least(minimum: f64) -> Params {
        Params {
            min_delta_l: Some(minimum),
            ..Params::default()
        }
    }

    const CMYK_INTENT: &str = "/OutputIntents [5 99 0 R << /S /GTS_PDFX >> << /DestOutputProfile << /N 4 >> >> << /DestOutputProfile 9 0 R >>]";

    #[test]
    fn text_on_its_background() {
        let page = [
            text("0 g"),
            format!("0.9 g 50 50 150 150 re f {}", text("0.8 g")),
            format!("1 0 0 rg 50 50 150 150 re f {}", text("0.9 0 0 rg")),
            format!(
                "1 0 0 rg 50 50 150 150 re f {} {}",
                text("0.9 0 0 rg"),
                text("0.9 0 0 rg")
            ),
        ]
        .join(" ");
        let error = |m: &str| (Severity::Error, Some(1), m.to_owned());
        assert_eq!(
            found(&[&page], "", at_least(30.0)),
            [
                error("text in gray 80% on gray 90% differs by only 9.4 L* (minimum 30; 1 run(s), smallest 12.0pt)"),
                error(
                    "text in RGB(230,0,0) on RGB(255,0,0) differs by only 5.1 L* (minimum 30; 3 run(s), smallest 12.0pt)"
                ),
            ]
        );
        assert_eq!(found(&[&page], "", Params::default()), [], "off without a minimum");
    }

    #[test]
    fn cmyk_through_a_profile() {
        let page = format!(
            "0 0 0 0.1 k 50 50 150 150 re f {} 0 0 0 1 k 50 50 150 150 re f {}",
            text("0 0 0 0.2 k"),
            text("-0.001 g")
        );
        let warned = |m: &str| (Severity::Error, Some(1), m.to_owned());
        let through = |catalog: &str, params: Params| found(&[&page], catalog, params);
        let want = [
            warned(
                "text in C0 M0 Y0 K20 on C0 M0 Y0 K10 differs by only 7.1 L* (minimum 30; 1 run(s), smallest 12.0pt)",
            ),
            warned("text in gray 0% on C0 M0 Y0 K100 differs by only 0.0 L* (minimum 30; 1 run(s), smallest 12.0pt)"),
        ];
        assert_eq!(through(CMYK_INTENT, at_least(30.0)), want);
        let proof = Params {
            proof_profile: Some(PROFILE.to_owned()),
            ..at_least(30.0)
        };
        assert_eq!(through("", proof), want);
        let none = [(
            Severity::Error,
            None,
            "cannot measure contrast: no CMYK profile: the PDF has no CMYK output intent and none was given".to_owned(),
        )];
        assert_eq!(through("", at_least(30.0)), none);
        assert_eq!(
            found(&[&text("0 0 0 1 k")], "", at_least(30.0)),
            none,
            "on the paper, which is RGB"
        );
        let missing = Params {
            proof_profile: Some("/nonexistent/profile.icc".to_owned()),
            ..at_least(30.0)
        };
        let got = through("", missing);
        assert!(
            matches!(got.as_slice(), [(Severity::Error, None, m)] if m.starts_with("cannot read the proof profile /nonexistent/profile.icc: ")),
            "{got:?}"
        );
    }

    #[test]
    fn unmeasured() {
        let pages = [
            format!("/Sh sh {}", text("0 g")),
            format!(
                "q 100 0 0 100 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /G ID \0 EI Q {}",
                text("0 g")
            ),
            format!("/Pattern cs /P scn 50 50 150 150 re f {}", text("0 g")),
            text("/S cs 1 scn"),
            format!("0 0 0 rg 0 0 m 50 50 150 150 re S {} 0 0 0 0 re f 1 1 l f", text("1 g")),
        ];
        let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
        let warning = |p: u32| {
            (
                Severity::Warning,
                Some(p),
                "text over an image, a shading or a spot colour: contrast not measured".to_owned(),
            )
        };
        assert_eq!(
            found(&pages, "", at_least(30.0)),
            [
                (
                    Severity::Error,
                    Some(5),
                    "text in gray 100% on the paper differs by only 0.0 L* (minimum 30; 1 run(s), smallest 12.0pt)"
                        .to_owned()
                ),
                warning(1),
                warning(2),
                warning(3),
                warning(4)
            ]
        );
    }

    #[test]
    fn a_profile_that_does_not_convert() {
        // An RGB display profile header with no tags: LittleCMS opens it, but it converts nothing.
        let mut header = vec![0u8; 132];
        header[0..4].copy_from_slice(&132u32.to_be_bytes());
        header[8] = 4;
        header[12..16].copy_from_slice(b"mntr");
        header[16..20].copy_from_slice(b"RGB ");
        header[20..24].copy_from_slice(b"XYZ ");
        header[36..40].copy_from_slice(b"acsp");
        let path = std::env::temp_dir().join(format!("proofpress-contrast-{}.icc", std::process::id()));
        std::fs::write(&path, header).unwrap();
        let params = Params {
            proof_profile: Some(path.to_string_lossy().into_owned()),
            ..at_least(30.0)
        };
        let got = found(&[&text("0 g")], "", params);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            got,
            [(
                Severity::Error,
                None,
                "cannot measure contrast: a profile does not convert to L*a*b*".to_owned()
            )]
        );
    }
}
