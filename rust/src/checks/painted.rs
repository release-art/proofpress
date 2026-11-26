//! Print checks on what each page paints and where (ported from the Python `structure` module, under the parity
//! gate): a `Do` that names no `XObject` leaves a hole where the artwork should be (ISO 32000-2 7.8.3), an object
//! painted wholly off the media box never prints but ships in the file, artwork that reaches the trim must run on to
//! the bleed edge, and nothing may be painted beyond the bleed.
//!
//! Where a paint lands is judged by what the clip leaves of it, a clip being its path's bounding box; transparency
//! counts as opaque, and a form's `/BBox` does not clip.

use super::Settings;
use std::collections::{BTreeMap, BTreeSet};

use super::page::{media_box, mm_to_pt, own_box, pt_to_mm, rotated};
use crate::common::rect::{Rect, rect};
use crate::document::Document;
use crate::findings::{ARTWORK_BLEED, Finding, Location, OUTSIDE_BLEED, OUTSIDE_MEDIA, Severity, XOBJECT_MISSING};
use crate::graphics::{self, Event, Kind};

/// The page an event is on, from its first frame.
pub(super) fn page_of(frames: &[Location]) -> u32 {
    match frames.first() {
        Some(Location::Page(n)) => *n,
        _ => 0,
    }
}

/// Each name a page paints with `Do` that gives no `XObject`, once per page.
pub(super) fn xobject_missing(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut missing: BTreeSet<(u32, String)> = BTreeSet::new();
    for event in graphics::walk(doc) {
        if let Event::Missing { name, frames } = event {
            missing.insert((page_of(&frames), format!("/{}", String::from_utf8_lossy(&name))));
        }
    }
    missing
        .into_iter()
        .map(|(page, name)| {
            Finding::new(
                XOBJECT_MISSING,
                format!("{name} is painted but names no XObject: nothing prints there"),
            )
            .at(vec![Location::Page(page)])
        })
        .collect()
}

/// A warning per page that paints something entirely beyond its media box; invisible text (render modes 3 and 7) is
/// not counted, and a page without a media box is not judged.
pub(super) fn outside_media(doc: &Document, _: &Settings) -> Vec<Finding> {
    let media: Vec<_> = doc
        .pages()
        .iter()
        .map(|p| p.get(b"MediaBox").and_then(|m| rect(doc, m)))
        .collect();
    let mut gone: BTreeMap<u32, usize> = BTreeMap::new();
    for event in graphics::walk(doc) {
        let Event::Paint {
            kind,
            bbox: Some(b),
            frames,
            ..
        } = event
        else {
            continue;
        };
        let page = page_of(&frames);
        let Some(Some(m)) = media.get(usize::try_from(page).unwrap_or(usize::MAX).wrapping_sub(1)) else {
            continue;
        };
        let off = b.x1 < m.x0 || b.x0 > m.x1 || b.y1 < m.y0 || b.y0 > m.y1;
        if off && !matches!(kind, Kind::Text(3 | 7)) {
            *gone.entry(page).or_default() += 1;
        }
    }
    gone.into_iter()
        .map(|(page, count)| {
            Finding::new(
                OUTSIDE_MEDIA,
                format!("{count} object(s) are painted entirely outside the MediaBox: hidden content"),
            )
            .at(vec![Location::Page(page)])
        })
        .collect()
}

/// Within this many points, an edge reaches another.
const SLACK: f64 = 0.5;
/// A path this thin, in points, outside the bleed is likely a printer's mark.
const MARK_THIN_PT: f64 = 1.0;
const SIDES: [&str; 4] = ["left", "bottom", "right", "top"];

/// A paint's kind, bounding box and visible part.
type Placed = (Kind, Option<Rect>, Option<Rect>);

/// Each page's paints, outside pattern cells, by page number.
fn placed(doc: &Document) -> BTreeMap<u32, Vec<Placed>> {
    let mut pages: BTreeMap<u32, Vec<Placed>> = BTreeMap::new();
    for event in graphics::walk(doc) {
        if let Event::Paint {
            kind,
            bbox,
            visible,
            ink,
            frames,
            ..
        } = event
            && !ink.cell
        {
            pages.entry(page_of(&frames)).or_default().push((kind, bbox, visible));
        }
    }
    pages
}

fn corners(r: &Rect) -> [f64; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// A finding on `page` (counted from 1) as a warning, whatever the check's own severity.
fn warning(finding: Finding) -> Finding {
    Finding {
        severity: Severity::Warning,
        ..finding
    }
}

/// The paints short of the bleed edge on `side`: each that reaches the trim edge but not the bleed edge, unless paints
/// that do reach the bleed edge cover its whole run along that edge; by index, with how far it reaches into the bleed.
fn short_on_side(fills: &[(usize, [f64; 4])], trim: [f64; 4], side: usize, zone: f64) -> BTreeMap<usize, f64> {
    let mut short = BTreeMap::new();
    if zone <= SLACK {
        return short;
    }
    let lo = usize::from(side.is_multiple_of(2));
    let at = |a: &[f64; 4], i: usize| a.get(i).copied().unwrap_or(0.0);
    let out = |v: &[f64; 4]| at(&[trim[0] - v[0], trim[1] - v[1], v[2] - trim[2], v[3] - trim[3]], side);
    let inward = |v: &[f64; 4]| at(&[v[2] - trim[0], v[3] - trim[1], trim[2] - v[0], trim[3] - v[1]], side);
    let mut covers: Vec<(f64, f64)> = fills
        .iter()
        .filter(|(_, v)| out(v) >= zone - SLACK && inward(v) >= -SLACK)
        .map(|(_, v)| (at(v, lo), at(v, lo + 2)))
        .collect();
    covers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (n, v) in fills {
        let touches = (0..2).all(|i| at(v, i + 2) > at(&trim, i) + SLACK && at(v, i) < at(&trim, i + 2) - SLACK);
        let reach = out(v);
        if !touches || !(-SLACK..zone - SLACK).contains(&reach) {
            continue;
        }
        let span = (at(v, lo).max(at(&trim, lo)), at(v, lo + 2).min(at(&trim, lo + 2)));
        if !spanned(span, &covers) {
            short.insert(*n, reach.max(0.0));
        }
    }
    short
}

/// Whether `covers`, sorted, leave no gap wider than the slack along `span`.
fn spanned(span: (f64, f64), covers: &[(f64, f64)]) -> bool {
    let mut reached = span.0;
    for (start, end) in covers {
        if *start > reached + SLACK {
            break;
        }
        reached = reached.max(*end);
    }
    reached >= span.1 - SLACK
}

/// Artwork that reaches the trim runs on to the bleed edge: fills, images and clipped shadings, side by side.
pub(super) fn artwork_bleed(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut paints = placed(doc);
    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let here = vec![Location::Page(number)];
        let Some(trim) = own_box(doc, page, b"TrimBox") else {
            continue;
        };
        let Some(b) = own_box(doc, page, b"BleedBox").or_else(|| media_box(doc, page)) else {
            continue;
        };
        let zones = [trim.x0 - b.x0, trim.y0 - b.y0, b.x1 - trim.x1, b.y1 - trim.y1].map(|z| z.max(0.0));
        if zones.iter().all(|z| *z <= SLACK) {
            continue;
        }
        if rotated(page) {
            let message = "bleed coverage not checked: the page is rotated with /Rotate";
            out.push(warning(Finding::new(ARTWORK_BLEED, message).at(here)));
            continue;
        }
        let fills: Vec<(usize, [f64; 4])> = paints
            .remove(&number)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter_map(|(n, (kind, _, visible))| {
                let fill = matches!(
                    kind,
                    Kind::Image | Kind::InlineImage | Kind::Shading | Kind::Path { filled: true, .. }
                );
                Some((n, corners(&visible.filter(|_| fill)?)))
            })
            .collect();
        let trim = corners(&trim);
        let short: Vec<BTreeMap<usize, f64>> = zones
            .iter()
            .enumerate()
            .map(|(side, zone)| short_on_side(&fills, trim, side, *zone))
            .collect();
        let parts: Vec<String> = short
            .iter()
            .zip(SIDES)
            .zip(zones)
            .filter(|((objects, _), _)| !objects.is_empty())
            .map(|((objects, side), zone)| {
                let least = objects.values().copied().fold(f64::INFINITY, f64::min);
                format!(
                    "{side}: {} object(s) reach the trim but extend only {:.1} mm of the {:.1} mm bleed",
                    objects.len(),
                    pt_to_mm(least),
                    pt_to_mm(zone)
                )
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        let total = short.iter().flat_map(BTreeMap::keys).collect::<BTreeSet<_>>().len();
        let message = format!(
            "{total} object(s) stop short of the bleed edge ({}) -- artwork must run to the bleed edge",
            parts.join("; ")
        );
        out.push(Finding::new(ARTWORK_BLEED, message).at(here));
    }
    out
}

/// How far the visible part of a paint reaches beyond `region`, in points; 0 for one unseen or invisible text.
fn overshoot(kind: Kind, visible: Option<Rect>, region: &Rect) -> f64 {
    let Some(v) = visible.filter(|_| !matches!(kind, Kind::Text(3 | 7))) else {
        return 0.0;
    };
    [region.x0 - v.x0, region.y0 - v.y0, v.x1 - region.x1, v.y1 - region.y1]
        .into_iter()
        .fold(0.0, f64::max)
}

/// Nothing is painted beyond the bleed box, or, without one, beyond the trim grown by the geometry's bleed.
pub(super) fn outside_bleed(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let geometry = settings.params.geometry.as_ref();
    let mut paints = placed(doc);
    let mut out = Vec::new();
    let mut note = None;
    for (i, page) in doc.pages().iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let here = vec![Location::Page(number)];
        let Some(trim) = own_box(doc, page, b"TrimBox") else {
            continue;
        };
        if rotated(page) {
            let message = "content outside the bleed not checked: the page is rotated";
            out.push(warning(Finding::new(OUTSIDE_BLEED, message).at(here)));
            continue;
        }
        let region = own_box(doc, page, b"BleedBox").unwrap_or_else(|| {
            let grow = geometry.map_or([0.0; 4], |g| g.bleeds_mm(i + 1).map(mm_to_pt));
            note = Some(if geometry.is_some() {
                "the TrimBox grown by the expected bleed"
            } else {
                "the TrimBox (no BleedBox, no bleed given)"
            });
            Rect {
                x0: trim.x0 - grow[0],
                y0: trim.y0 - grow[1],
                x1: trim.x1 + grow[2],
                y1: trim.y1 + grow[3],
            }
        });
        let outside: Vec<(Kind, Option<Rect>, f64)> = paints
            .remove(&number)
            .unwrap_or_default()
            .into_iter()
            .map(|(kind, bbox, visible)| (kind, bbox, overshoot(kind, visible, &region)))
            .filter(|(_, _, over)| *over > SLACK)
            .collect();
        if outside.is_empty() {
            continue;
        }
        let worst = outside.iter().map(|(_, _, over)| *over).fold(0.0, f64::max);
        let thin = outside.iter().any(|(kind, bbox, _)| {
            matches!(kind, Kind::Path { .. }) && bbox.is_some_and(|b| (b.x1 - b.x0).min(b.y1 - b.y0) <= MARK_THIN_PT)
        });
        let text = outside.iter().any(|(kind, _, _)| matches!(kind, Kind::Text(_)));
        let hints: Vec<&str> = [
            (thin, "thin shapes: likely printer's marks"),
            (text, "text: likely a slug"),
        ]
        .into_iter()
        .filter_map(|(on, hint)| on.then_some(hint))
        .collect();
        let hint = if hints.is_empty() {
            String::new()
        } else {
            format!(" ({})", hints.join("; "))
        };
        let message = format!(
            "{} object(s) lie outside the bleed region, by up to {:.1} mm{hint} -- remove printer's marks and slug text",
            outside.len(),
            pt_to_mm(worst)
        );
        out.push(Finding::new(OUTSIDE_BLEED, message).at(here));
    }
    if let Some(basis) = note {
        let message = format!("bleed region taken from {basis}: the page has no BleedBox");
        out.push(warning(Finding::new(OUTSIDE_BLEED, message)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    /// The ids and messages the two checks raise on a one-page file painting `data`, with `page` as the page's
    /// entries.
    fn found(page: &str, data: &str) -> Vec<(u32, String)> {
        let content = format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
        let page = format!("<< /Type /Page /Parent 2 0 R {page} /Contents 4 0 R >>");
        let objects: [(u32, &[u8]); 4] = [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, page.as_bytes()),
            (4, content.as_bytes()),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        [
            xobject_missing(&doc, &Level::Print.into()),
            outside_media(&doc, &Level::Print.into()),
        ]
        .concat()
        .into_iter()
        .map(|f| (f.page.unwrap(), f.message))
        .collect()
    }

    #[test]
    fn what_is_off_the_page_or_missing() {
        let media = "/MediaBox [0 0 100 100]";
        let off = "200 0 1 1 re f 0 200 1 1 re f -9 0 1 1 re f 0 -9 1 1 re f 99 99 9 9 re f";
        assert_eq!(
            found(media, off),
            [(
                1,
                "4 object(s) are painted entirely outside the MediaBox: hidden content".to_owned()
            )],
            "the fifth reaches onto the page"
        );
        assert_eq!(
            found(media, "BT 3 Tr 200 200 Td (x) Tj 7 Tr (x) Tj ET"),
            [],
            "invisible text"
        );
        assert_eq!(found("", off), [], "no media box to judge by");
        assert_eq!(
            found(media, "/A Do /A Do /B Do"),
            [
                (1, "/A is painted but names no XObject: nothing prints there".to_owned()),
                (1, "/B is painted but names no XObject: nothing prints there".to_owned()),
            ]
        );
        assert_eq!(page_of(&[]), 0);
    }

    /// The bleed checks' findings, as severity, page and message, on pages with `pages`' entries and content.
    fn bleeds(pages: &[(&str, &str)], params: crate::checks::Params, id: &str) -> Vec<(Severity, Option<u32>, String)> {
        let doc = crate::checks::test_doc::paged(pages, "", &[]);
        let settings = Settings {
            level: Level::Print,
            params,
            files: crate::checks::Files::Alone,
        };
        [artwork_bleed(&doc, &settings), outside_bleed(&doc, &settings)]
            .concat()
            .into_iter()
            .filter(|f| f.check.id == id)
            .map(|f| (f.severity, f.page, f.message))
            .collect()
    }

    #[test]
    fn artwork_runs_into_the_bleed() {
        let boxes = "/MediaBox [0 0 100 100] /TrimBox [10 10 90 90] /BleedBox [0 0 100 100]";
        let short = "10 10 80 20 re f";
        let covered = "10 10 80 20 re f 0 10 100 20 re f 10 0 80 10 re f";
        let gap = "10 10 80 20 re f 0 10 10 5 re f 0 20 10 10 re f";
        let fills = "0 0 100 100 re f BI /W 1 /H 1 /BPC 8 /CS /G ID a EI 0 0 1 1 re S BT (x) Tj ET";
        let got = bleeds(
            &[
                (boxes, short),
                (boxes, covered),
                (boxes, gap),
                (boxes, fills),
                (boxes, "10 40 5 20 re f 0 30 100 40 re f"),
                (&format!("{boxes} /Rotate 90"), short),
                (
                    "/MediaBox [0 0 100 100] /TrimBox [10 10 90 90] /BleedBox [10 10 90 90]",
                    short,
                ),
                ("/MediaBox [0 0 100 100]", short),
                ("/TrimBox [10 10 90 90]", short),
            ],
            crate::checks::Params::default(),
            "artwork-bleed",
        );
        assert_eq!(
            got,
            [
                (
                    Severity::Error,
                    Some(1),
                    "1 object(s) stop short of the bleed edge (left: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed; bottom: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed; right: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed) -- artwork must run to the bleed edge".to_owned()
                ),
                (
                    Severity::Error,
                    Some(3),
                    "1 object(s) stop short of the bleed edge (left: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed; bottom: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed; right: 1 object(s) reach the trim but extend only 0.0 mm of the 3.5 mm bleed) -- artwork must run to the bleed edge".to_owned()
                ),
                (
                    Severity::Warning,
                    Some(6),
                    "bleed coverage not checked: the page is rotated with /Rotate".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn nothing_beyond_the_bleed() {
        let trim = "/MediaBox [0 0 100 100] /TrimBox [10 10 90 90]";
        let marks =
            "0 0 0.5 50 re f BT 1 0 0 1 -50 50 Tm (x) Tj 3 Tr (y) Tj ET q 0 0 1 1 re W n /S sh Q 20 20 10 10 re f";
        let geometry = crate::checks::Params {
            geometry: Some(crate::checks::Geometry {
                trim_w_mm: 28.2,
                trim_h_mm: 28.2,
                bleed_mm: 3.175,
                tolerance_mm: 0.25,
                binding_bleed_mm: None,
                binding: "left".to_owned(),
            }),
            ..crate::checks::Params::default()
        };
        let pages = [
            (trim, marks),
            (&format!("{trim} /BleedBox [0 0 100 100]") as &str, "-20 0 5 5 re f"),
            (&format!("{trim} /Rotate 270") as &str, marks),
            ("/MediaBox [0 0 100 100]", marks),
        ];
        let outside = |params| bleeds(&pages, params, "outside-bleed");
        let report = |by: &str, hint: &str| {
            format!(
                "3 object(s) lie outside the bleed region, by up to {by} mm{hint} -- remove printer's marks and slug text"
            )
        };
        assert_eq!(
            outside(geometry),
            [
                (
                    Severity::Error,
                    Some(1),
                    report("18.0", " (thin shapes: likely printer's marks; text: likely a slug)")
                ),
                (
                    Severity::Error,
                    Some(2),
                    "1 object(s) lie outside the bleed region, by up to 7.1 mm -- remove printer's marks and slug text"
                        .to_owned()
                ),
                (
                    Severity::Warning,
                    Some(3),
                    "content outside the bleed not checked: the page is rotated".to_owned()
                ),
                (
                    Severity::Warning,
                    None,
                    "bleed region taken from the TrimBox grown by the expected bleed: the page has no BleedBox"
                        .to_owned()
                ),
            ]
        );
        let bled = format!("{trim} /BleedBox [0 0 100 100]");
        assert_eq!(
            bleeds(&[(&bled, "")], crate::checks::Params::default(), "outside-bleed"),
            []
        );
        let bare = outside(crate::checks::Params::default());
        assert_eq!(
            bare.last(),
            Some(&(
                Severity::Warning,
                None,
                "bleed region taken from the TrimBox (no BleedBox, no bleed given): the page has no BleedBox"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn spans_and_overshoots() {
        assert!(spanned((0.0, 10.0), &[(0.0, 4.0), (4.2, 10.0)]));
        assert!(!spanned((0.0, 10.0), &[(0.0, 4.0), (6.0, 10.0)]), "a gap");
        assert!(!spanned((0.0, 10.0), &[]));
        let fills = [(0, [10.0, 40.0, 15.0, 60.0]), (1, [0.0, 30.0, 100.0, 70.0])];
        assert!(
            short_on_side(&fills, [10.0, 10.0, 90.0, 90.0], 0, 10.0).is_empty(),
            "covered along its edge"
        );
        assert!(
            short_on_side(&fills, [10.0, 10.0, 90.0, 90.0], 0, 0.5).is_empty(),
            "no bleed on that side"
        );
        let region = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        let far = Rect {
            x0: 12.0,
            y0: 0.0,
            x1: 13.0,
            y1: 1.0,
        };
        assert_eq!(overshoot(Kind::Text(7), Some(far), &region), 0.0, "invisible text");
        assert_eq!(overshoot(Kind::Image, None, &region), 0.0, "nothing visible");
        assert_eq!(overshoot(Kind::Image, Some(far), &region), 3.0);
    }
}
