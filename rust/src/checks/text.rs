//! Print checks on text and line work (ported from the Python `text` module, under the parity gate): characters shown
//! with the `.notdef` glyph, fonts that are not embedded (and Type 3 fonts, advisory), text below the profile's size
//! floor, strokes below its width floor, and text reaching outside its safe area.
//!
//! A text run counts when it shows at least one code, whatever its render mode and however it is clipped; its size is
//! the font size by the text rendering matrix's vertical scale, and its extent comes from its font's metrics (see
//! `common::font`). A stroke's width is the line width by the CTM's scale.

use std::collections::BTreeMap;

use super::Settings;
use super::page::{media_box, own_box};
use super::painted::page_of;
use crate::common::pyfmt::g;
use crate::common::rect::Rect;
use crate::document::Document;
use crate::findings::{
    Check, FONT_EMBEDDED, FONT_TYPE3, Finding, HAIRLINE, Location, MISSING_GLYPH, SAFE_AREA, Severity, TEXT_SIZE,
};
use crate::graphics::{self, Event, Ink, Kind};

/// A run within this many points of the safe area's edge is inside it.
const EDGE_TOLERANCE_PT: f64 = 0.05;

/// Each page's paints, outside pattern cells, in order.
pub(super) fn paints(doc: &Document) -> BTreeMap<u32, Vec<(Kind, Option<Rect>, Ink)>> {
    let mut out: BTreeMap<u32, Vec<(Kind, Option<Rect>, Ink)>> = BTreeMap::new();
    for event in graphics::walk(doc) {
        if let Event::Paint {
            kind,
            bbox,
            ink,
            frames,
            ..
        } = event
            && !ink.cell
        {
            out.entry(page_of(&frames)).or_default().push((kind, bbox, *ink));
        }
    }
    out
}

/// A page's text runs that show at least one code.
fn runs(paints: &[(Kind, Option<Rect>, Ink)]) -> impl Iterator<Item = &(Kind, Option<Rect>, Ink)> {
    paints
        .iter()
        .filter(|(kind, _, ink)| matches!(kind, Kind::Text(_)) && ink.glyphs > 0)
}

fn on_page(check: &'static Check, page: u32, message: String) -> Finding {
    Finding::new(check, message).at(vec![Location::Page(page)])
}

fn warning(finding: Finding) -> Finding {
    Finding {
        severity: Severity::Warning,
        ..finding
    }
}

/// Characters a font has no glyph for, shown with `.notdef`, by font name.
pub(super) fn missing_glyph(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (page, paints) in paints(doc) {
        let mut per_font: BTreeMap<String, usize> = BTreeMap::new();
        for (_, _, ink) in runs(&paints) {
            if let Some(font) = ink.font.as_ref().filter(|_| ink.notdef > 0) {
                *per_font.entry(font.name.clone()).or_default() += ink.notdef;
            }
        }
        if per_font.is_empty() {
            continue;
        }
        let total: usize = per_font.values().sum();
        let detail: Vec<String> = per_font
            .iter()
            .map(|(name, count)| format!("{count} in {name}"))
            .collect();
        let message = format!(
            "{total} character(s) are shown with the .notdef glyph ({}): the font has no glyph for them",
            detail.join(", ")
        );
        out.push(on_page(MISSING_GLYPH, page, message));
    }
    out
}

/// Runs by font name.
type Runs = BTreeMap<String, usize>;

/// Each page's runs by font name: those in fonts that are not embedded, and those in Type 3 fonts.
fn fonts(doc: &Document) -> Vec<(u32, Runs, Runs)> {
    paints(doc)
        .into_iter()
        .map(|(page, paints)| {
            let (mut unembedded, mut type3) = (BTreeMap::new(), BTreeMap::new());
            for font in runs(&paints)
                .filter_map(|(_, _, ink)| ink.font.as_ref())
                .filter(|f| f.known)
            {
                let counted = if font.subtype == "Type3" {
                    &mut type3
                } else if font.embedded {
                    continue;
                } else {
                    &mut unembedded
                };
                *counted.entry(font.name.clone()).or_insert(0usize) += 1;
            }
            (page, unembedded, type3)
        })
        .collect()
}

/// Fonts are embedded: a RIP would substitute one that is not.
pub(super) fn font_embedded(doc: &Document, _: &Settings) -> Vec<Finding> {
    fonts(doc)
        .into_iter()
        .flat_map(|(page, unembedded, _)| {
            unembedded.into_iter().map(move |(name, runs)| {
                let message =
                    format!("font {name} is not embedded ({runs} run(s)): the printer's RIP would substitute it");
                on_page(FONT_EMBEDDED, page, message)
            })
        })
        .collect()
}

/// Type 3 fonts, which are bitmap or hand-built faces (a warning).
pub(super) fn font_type3(doc: &Document, _: &Settings) -> Vec<Finding> {
    fonts(doc)
        .into_iter()
        .flat_map(|(page, _, type3)| {
            type3.into_iter().map(move |(name, runs)| {
                let message =
                    format!("font {name} is a Type 3 font ({runs} run(s)): bitmap or hand-built faces print poorly");
                on_page(FONT_TYPE3, page, message)
            })
        })
        .collect()
}

/// Text is no smaller than the profile's floor.
pub(super) fn text_size(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(floor) = settings.params.min_text_pt else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (page, paints) in paints(doc) {
        let sizes: Vec<f64> = runs(&paints)
            .map(|(_, _, ink)| ink.size)
            .filter(|size| *size > 0.0 && *size < floor)
            .collect();
        if let Some(smallest) = sizes.iter().copied().reduce(f64::min) {
            let message = format!(
                "text as small as {smallest:.1}pt is below the {}pt minimum ({} run(s))",
                g(floor),
                sizes.len()
            );
            out.push(on_page(TEXT_SIZE, page, message));
        }
    }
    out
}

/// Strokes are no thinner than the profile's floor.
pub(super) fn hairline(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(floor) = settings.params.min_stroke_pt else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (page, paints) in paints(doc) {
        let widths: Vec<f64> = paints
            .iter()
            .filter_map(|(_, _, ink)| ink.stroke_width)
            .filter(|width| *width < floor)
            .collect();
        if let Some(thinnest) = widths.iter().copied().reduce(f64::min) {
            let label = if thinnest <= 0.0 {
                "a hairline (zero width)".to_owned()
            } else {
                format!("{thinnest:.2}pt")
            };
            let message = format!(
                "lines as thin as {label} are below the {}pt minimum ({} stroke(s))",
                g(floor),
                widths.len()
            );
            out.push(on_page(HAIRLINE, page, message));
        }
    }
    out
}

/// Text stays inside the trim (or media box) by the profile's safe margin; an error when the worst run's font
/// declares its widths, a warning when its extent is only estimated.
pub(super) fn safe_area(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(margin) = settings.params.safe_mm else {
        return Vec::new();
    };
    let inset = margin / 25.4 * 72.0;
    let mut found = paints(doc);
    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let Some(trim) = own_box(doc, page, b"TrimBox").or_else(|| media_box(doc, page)) else {
            continue;
        };
        let safe = [trim.x0 + inset, trim.y0 + inset, trim.x1 - inset, trim.y1 - inset];
        let paints = found.remove(&number).unwrap_or_default();
        let outside: Vec<(f64, &Ink)> = runs(&paints)
            .filter_map(|(_, bbox, ink)| {
                let over = bbox.map_or(0.0, |b| {
                    [safe[0] - b.x0, safe[1] - b.y0, b.x1 - safe[2], b.y1 - safe[3]]
                        .into_iter()
                        .fold(0.0, f64::max)
                });
                (over > EDGE_TOLERANCE_PT).then_some((over, ink))
            })
            .collect();
        let Some((worst, ink)) = outside.iter().copied().reduce(|a, b| if b.0 > a.0 { b } else { a }) else {
            continue;
        };
        let measured = ink.font.as_ref().is_some_and(|f| f.measured());
        let estimated = if measured {
            ""
        } else {
            " (estimated: the font declares no glyph widths)"
        };
        let message = format!(
            "{} text run(s) reach outside the {}mm safe area, by up to {:.1}mm{estimated}",
            outside.len(),
            g(margin),
            worst / 72.0 * 25.4
        );
        let finding = on_page(SAFE_AREA, number, message);
        out.push(if measured { finding } else { warning(finding) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Params;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    fn found(pages: &[(&str, &str)], params: Params) -> Vec<(Severity, Option<u32>, String)> {
        let fonts = "/Resources << /Font << /H << /Subtype /Type1 /BaseFont /Helvetica >> /E << /Subtype /TrueType /BaseFont /ABCDEF+Nunito \
            /FirstChar 32 /Widths [500 500] /FontDescriptor << /FontFile2 9 0 R >> /Encoding << /Differences [32 /.notdef] >> >> \
            /T << /Subtype /Type3 /FontMatrix [0.001 0 0 0.001 0 0] >> /Z << /Subtype /Type0 /BaseFont /Zed /Encoding /Identity-H \
            /DescendantFonts [<< /FontDescriptor << >> >>] >> /B 5 >> /ExtGState << /L << /LW 0.1 >> >> >>";
        let pages: Vec<(String, &str)> = pages.iter().map(|(e, d)| (format!("{fonts} {e}"), *d)).collect();
        let refs: Vec<(&str, &str)> = pages.iter().map(|(e, d)| (e.as_str(), *d)).collect();
        let doc = paged(&refs, "", &[(9, b"<< /Length 0 >>\nstream\n\nendstream".to_vec())]);
        let settings = Settings {
            level: Level::Print,
            params,
            files: crate::checks::Files::Alone,
        };
        [missing_glyph, font_embedded, font_type3, text_size, hairline, safe_area]
            .iter()
            .flat_map(|run| run(&doc, &settings))
            .map(|f| (f.severity, f.page, f.message))
            .collect()
    }

    #[test]
    fn text_and_strokes() {
        let box_ = "/MediaBox [0 0 100 100]";
        let text = "BT /H 4 Tf 50 50 Td (a) Tj /E 10 Tf ( !) Tj /Z 8 Tf <0000> Tj /T 12 Tf (x) Tj /B 3 Tf (y) Tj /H 0 Tf (z) Tj [] TJ ET \
            BT /E 10 Tf 1 1 Td ( ) Tj /H 5 Tf 95 95 Td (q) Tj ET";
        let lines = "0 w 0 0 m 10 10 l S /L gs 0 0 m 1 1 l S 1 w 0 0 m 5 5 l S 0 0 1 1 re f";
        let params = Params {
            min_text_pt: Some(6.0),
            min_stroke_pt: Some(0.25),
            safe_mm: Some(3.0),
            ..Params::default()
        };
        let msg = |s: Severity, p: u32, m: &str| (s, Some(p), m.to_owned());
        assert_eq!(
            found(&[(box_, text), (box_, lines)], params),
            [
                msg(
                    Severity::Error,
                    1,
                    "3 character(s) are shown with the .notdef glyph (2 in Nunito, 1 in Zed): the font has no glyph for them"
                ),
                msg(
                    Severity::Error,
                    1,
                    "font Helvetica is not embedded (3 run(s)): the printer's RIP would substitute it"
                ),
                msg(
                    Severity::Error,
                    1,
                    "font Zed is not embedded (1 run(s)): the printer's RIP would substitute it"
                ),
                msg(
                    Severity::Warning,
                    1,
                    "font ? is a Type 3 font (1 run(s)): bitmap or hand-built faces print poorly"
                ),
                msg(
                    Severity::Error,
                    1,
                    "text as small as 3.0pt is below the 6pt minimum (3 run(s))"
                ),
                msg(
                    Severity::Error,
                    2,
                    "lines as thin as a hairline (zero width) are below the 0.25pt minimum (2 stroke(s))"
                ),
                msg(
                    Severity::Error,
                    1,
                    "2 text run(s) reach outside the 3mm safe area, by up to 3.4mm"
                ),
            ]
        );
        let estimated = found(
            &[(
                box_,
                "BT /H 10 Tf 0 0 Td (a) Tj 2 2 Td (b) Tj 1 0 0 1 -1 -1 Tm (c) Tj ET",
            )],
            Params {
                safe_mm: Some(3.0),
                ..Params::default()
            },
        );
        assert_eq!(
            estimated.last(),
            Some(&msg(
                Severity::Warning,
                1,
                "3 text run(s) reach outside the 3mm safe area, by up to 4.1mm (estimated: the font declares no glyph widths)"
            ))
        );
        assert_eq!(
            found(
                &[("", "BT /H 10 Tf (a) Tj ET 0.1 w 0 0 m 1 1 l S")],
                Params {
                    min_stroke_pt: Some(0.25),
                    safe_mm: Some(3.0),
                    ..Params::default()
                }
            )
            .last(),
            Some(&msg(
                Severity::Error,
                1,
                "lines as thin as 0.10pt are below the 0.25pt minimum (1 stroke(s))"
            )),
            "no box for the safe area"
        );
        assert_eq!(found(&[(box_, "")], Params::default()), [], "limits off");
    }
}
