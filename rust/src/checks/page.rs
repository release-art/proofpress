//! Print checks on the page boxes and the job's shape (ported from the Python `structure` module, under the parity
//! gate): the finished size, its bleed and centring against the profile's geometry; the page count and a uniform page
//! size; and the printing condition the output intent names.
//!
//! The trim is the `/TrimBox` (falling back to the `/ArtBox` for the finished size, and to the `/MediaBox` for the
//! uniform size), the bleed the `/BleedBox`; boxes are read with their corners in order, and a box that is not four
//! numbers is absent. Sizes are compared in millimetres, offsets in points.

use super::{Geometry, Settings};
use crate::common::icc;
use crate::common::pyfmt::{g, repr};
use crate::common::rect::{Rect, rect};
use crate::common::text;
use crate::document::{Document, Object, Page, Value};
use crate::findings::{
    BLEED, CENTRING, Check, Finding, GEOMETRY, Location, OUTPUT_CONDITION, PAGE_COUNT, PAGE_MEDIA_BOX, PAGE_ROTATE,
    PAGE_SIZE,
};

/// Two pages whose sizes differ by less than this, in millimetres, are the same size.
const UNIFORM_TOLERANCE_MM: f64 = 0.1;

pub(super) fn mm_to_pt(mm: f64) -> f64 {
    mm / 25.4 * 72.0
}

pub(super) fn pt_to_mm(pt: f64) -> f64 {
    pt / 72.0 * 25.4
}

/// A box the page gives itself (trim, bleed and art boxes are not inherited).
pub(super) fn own_box(doc: &Document, page: &Page<'_>, key: &[u8]) -> Option<Rect> {
    page.dict.get(key).and_then(|b| rect(doc, b))
}

/// The page's media box, its own or inherited.
pub(super) fn media_box(doc: &Document, page: &Page<'_>) -> Option<Rect> {
    page.get(b"MediaBox").and_then(|b| rect(doc, b))
}

/// ISO 32000-2 7.7.3.3: every page has a `/MediaBox` of four numbers, its own or inherited from the page tree.
pub(super) fn media_box_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    let pages = doc.pages();
    let missing = pages
        .iter()
        .enumerate()
        .filter(|(_, page)| media_box(doc, page).is_none());
    missing
        .map(|(i, page)| {
            let message = match page.get(b"MediaBox") {
                Some(_) => "the page's MediaBox is not four numbers",
                None => "the page has no MediaBox, its own or inherited",
            };
            on_page(PAGE_MEDIA_BOX, i + 1, message.to_owned()).citing("7.7.3.3")
        })
        .collect()
}

/// Whether `/Rotate` turns the page: its value truncated to a whole number is not a multiple of 360.
pub(super) fn rotated(page: &Page<'_>) -> bool {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a rotation is truncated to whole degrees"
    )]
    let degrees = match page.get(b"Rotate").map(|r| &r.value) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Real(r)) => *r as i64,
        _ => 0,
    };
    degrees.rem_euclid(360) != 0
}

fn size_mm(r: &Rect) -> (f64, f64) {
    (pt_to_mm(r.x1 - r.x0), pt_to_mm(r.y1 - r.y0))
}

fn on_page(check: &'static Check, page: usize, message: String) -> Finding {
    Finding::new(check, message).at(vec![Location::Page(u32::try_from(page).unwrap_or(u32::MAX))])
}

impl Geometry {
    /// The bleed each side of page `page` (counted from 1) should have, in millimetres: left, bottom, right, top. The
    /// binding edge, left on odd pages when binding on the left, takes the binding bleed when one is given.
    pub(super) fn bleeds_mm(&self, page: usize) -> [f64; 4] {
        let binding = self.binding_bleed_mm.unwrap_or(self.bleed_mm);
        let left_binds = (self.binding == "left") == (page % 2 == 1);
        let (left, right) = if left_binds {
            (binding, self.bleed_mm)
        } else {
            (self.bleed_mm, binding)
        };
        [left, self.bleed_mm, right, self.bleed_mm]
    }
}

/// Each page with a trim (or art) box, numbered from 1, with its trim and media boxes.
fn trimmed(doc: &Document) -> Vec<(usize, Page<'_>, Rect, Option<Rect>)> {
    doc.pages()
        .into_iter()
        .enumerate()
        .filter_map(|(i, page)| {
            let trim = own_box(doc, &page, b"TrimBox").or_else(|| own_box(doc, &page, b"ArtBox"))?;
            let media = media_box(doc, &page);
            Some((i + 1, page, trim, media))
        })
        .collect()
}

/// The finished size, either way round, within the tolerance.
pub(super) fn geometry(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(geometry) = &settings.params.geometry else {
        return Vec::new();
    };
    let mut expected = [geometry.trim_w_mm, geometry.trim_h_mm];
    expected.sort_by(f64::total_cmp);
    let mut out = Vec::new();
    for (i, _, trim, _) in trimmed(doc) {
        let (w, h) = size_mm(&trim);
        let mut got = [w, h];
        got.sort_by(f64::total_cmp);
        if got
            .iter()
            .zip(expected)
            .any(|(a, b)| (a - b).abs() > geometry.tolerance_mm)
        {
            let message = format!(
                "finished size is {w:.2} x {h:.2} mm, expected {} x {} mm",
                g(geometry.trim_w_mm),
                g(geometry.trim_h_mm)
            );
            out.push(on_page(GEOMETRY, i, message));
        }
    }
    out
}

/// The bleed box's distance outside the trim, each side, against the geometry's bleed.
pub(super) fn bleed(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(geometry) = &settings.params.geometry else {
        return Vec::new();
    };
    let tolerance = mm_to_pt(geometry.tolerance_mm);
    let mut out = Vec::new();
    for (i, page, trim, _) in trimmed(doc) {
        let Some(b) = own_box(doc, &page, b"BleedBox") else {
            continue;
        };
        let wants = geometry.bleeds_mm(i);
        let offsets = [trim.x0 - b.x0, trim.y0 - b.y0, b.x1 - trim.x1, b.y1 - trim.y1];
        if offsets
            .iter()
            .zip(wants)
            .all(|(off, want)| (off - mm_to_pt(want)).abs() <= tolerance)
        {
            continue;
        }
        let got: Vec<String> = offsets.iter().map(|o| format!("{:.2}", pt_to_mm(*o))).collect();
        let uniform = wants.iter().all(|w| w.to_bits() == wants[0].to_bits());
        let expected = if uniform {
            format!("{} mm on every side", g(geometry.bleed_mm))
        } else {
            format!("{} mm", wants.map(g).join(", "))
        };
        let message = format!(
            "bleed is {} mm (left, bottom, right, top), expected {expected}",
            got.join(", ")
        );
        out.push(on_page(BLEED, i, message));
    }
    out
}

/// The trim placed on the media box with the bleed the geometry expects on each side.
pub(super) fn centring(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(geometry) = &settings.params.geometry else {
        return Vec::new();
    };
    let tolerance = mm_to_pt(geometry.tolerance_mm);
    let mut out = Vec::new();
    for (i, _, trim, media) in trimmed(doc) {
        let Some(m) = media else { continue };
        let margins = [trim.x0 - m.x0, trim.y0 - m.y0, m.x1 - trim.x1, m.y1 - trim.y1];
        let [left, bottom, right, top] = margins;
        let [want_left, _, want_right, _] = geometry.bleeds_mm(i).map(mm_to_pt);
        if ((left - right) - (want_left - want_right)).abs() <= tolerance && (bottom - top).abs() <= tolerance {
            continue;
        }
        let shown: Vec<String> = margins.iter().map(|x| format!("{:.2}", pt_to_mm(*x))).collect();
        let skew = pt_to_mm(want_left - want_right);
        let expected = if skew.abs() > 1e-9 {
            format!("; expected left minus right {skew:+.2} mm")
        } else {
            String::new()
        };
        let message = format!(
            "the trim box is not centred on the MediaBox: margins are {} mm (left, bottom, right, top){expected}",
            shown.join(", ")
        );
        out.push(on_page(CENTRING, i, message));
    }
    out
}

/// The page count is a multiple of the profile's number.
pub(super) fn page_count(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let multiple = settings.params.pages_multiple_of.unwrap_or(0);
    let count = i64::try_from(doc.pages().len()).unwrap_or(i64::MAX);
    if multiple == 0 || count % multiple == 0 {
        return Vec::new();
    }
    vec![Finding::new(
        PAGE_COUNT,
        format!("{count} pages is not a multiple of {multiple}"),
    )]
}

/// Each page's size (its trim box, else its media box) against the first page's, and no `/Rotate`, when the profile
/// asks for uniform pages.
fn uniform(doc: &Document, settings: &Settings) -> (Vec<Finding>, Vec<Finding>) {
    let (mut sizes, mut rotations) = (Vec::new(), Vec::new());
    if !settings.params.uniform_pages {
        return (sizes, rotations);
    }
    let mut reference: Option<(f64, f64)> = None;
    for (i, page) in doc.pages().iter().enumerate() {
        let Some(r) = own_box(doc, page, b"TrimBox").or_else(|| media_box(doc, page)) else {
            continue;
        };
        let size = size_mm(&r);
        let first = *reference.get_or_insert(size);
        if (size.0 - first.0).abs() > UNIFORM_TOLERANCE_MM || (size.1 - first.1).abs() > UNIFORM_TOLERANCE_MM {
            let message = format!(
                "the page is {} x {} mm but the first page is {} x {} mm",
                g(size.0),
                g(size.1),
                g(first.0),
                g(first.1)
            );
            sizes.push(on_page(PAGE_SIZE, i + 1, message));
        }
        if rotated(page) {
            rotations.push(on_page(
                PAGE_ROTATE,
                i + 1,
                "the page is rotated with /Rotate".to_owned(),
            ));
        }
    }
    (sizes, rotations)
}

pub(super) fn page_size(doc: &Document, settings: &Settings) -> Vec<Finding> {
    uniform(doc, settings).0
}

pub(super) fn page_rotate(doc: &Document, settings: &Settings) -> Vec<Finding> {
    uniform(doc, settings).1
}

/// An output intent entry as text, as Python's `str()` gives it: a text string decoded, a name with its slash.
fn as_text(doc: &Document, object: Option<&Object>) -> String {
    match object.and_then(|o| doc.resolve(o)).map(|o| &o.value) {
        Some(Value::String(bytes, _)) => {
            text::decode(bytes).unwrap_or_else(|| bytes.iter().map(|b| char::from(*b)).collect())
        }
        Some(Value::Name(name)) => format!("/{}", String::from_utf8_lossy(name)),
        Some(Value::Integer(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Some output intent names the expected printing condition: in its identifier, condition, info or profile description.
pub(super) fn output_condition(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(expected) = &settings.params.output_condition else {
        return Vec::new();
    };
    let intents: Vec<_> = doc
        .catalog()
        .and_then(|c| c.get(b"OutputIntents"))
        .and_then(|i| doc.resolve(i))
        .and_then(Object::array)
        .unwrap_or_default()
        .iter()
        .filter_map(|i| doc.resolve(i)?.dict())
        .collect();
    if intents.is_empty() {
        let message = format!("no output intent to compare with the expected {}", repr(expected));
        return vec![Finding::new(OUTPUT_CONDITION, message)];
    }
    let wanted = expected.to_lowercase();
    let mut named = Vec::new();
    for intent in intents {
        let mut texts: Vec<String> = [b"OutputConditionIdentifier".as_slice(), b"OutputCondition", b"Info"]
            .iter()
            .map(|key| as_text(doc, intent.get(key)))
            .collect();
        let profile = match intent.get(b"DestOutputProfile").map(|p| &p.value) {
            Some(Value::Reference(num, _)) => doc.stream_data(*num),
            _ => None,
        };
        texts.push(
            profile
                .as_deref()
                .and_then(icc::description)
                .map_or_else(String::new, |d| d.trim().to_owned()),
        );
        if texts
            .iter()
            .any(|t| !t.is_empty() && t.to_lowercase().contains(&wanted))
        {
            return Vec::new();
        }
        named.push(
            texts
                .into_iter()
                .find(|t| !t.is_empty())
                .unwrap_or_else(|| "unnamed".to_owned()),
        );
    }
    let named: Vec<String> = named.iter().map(|n| repr(n)).collect();
    let message = format!("the output intent is {}, expected {}", named.join(", "), repr(expected));
    vec![Finding::new(OUTPUT_CONDITION, message)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::test_doc::paged;
    use crate::checks::{Params, Settings};
    use crate::findings::Level;

    type Run = fn(&Document, &Settings) -> Vec<Finding>;

    /// Each finding of `runs` as its page and message.
    fn found(doc: &Document, params: Params, runs: &[Run]) -> Vec<(Option<u32>, String)> {
        let settings = Settings {
            level: Level::Print,
            params,
        };
        runs.iter()
            .flat_map(|run| run(doc, &settings))
            .map(|f| (f.page, f.message))
            .collect()
    }

    #[test]
    fn a_page_needs_a_media_box_of_four_numbers() {
        let doc = paged(
            &[("/MediaBox [0 0 10 10]", ""), ("", ""), ("/MediaBox [0 0 10]", "")],
            "",
            &[],
        );
        assert_eq!(
            found(&doc, Params::default(), &[media_box_present]),
            [
                (Some(2), "the page has no MediaBox, its own or inherited".to_owned()),
                (Some(3), "the page's MediaBox is not four numbers".to_owned()),
            ]
        );
    }

    /// A 1 by 2 inch trim with 1/8 inch bleed: 72 by 144 points, 9 points of bleed.
    fn card(binding: Option<f64>, side: &str) -> Params {
        Params {
            geometry: Some(Geometry {
                trim_w_mm: 25.4,
                trim_h_mm: 50.8,
                bleed_mm: 3.175,
                tolerance_mm: 0.25,
                binding_bleed_mm: binding,
                binding: side.to_owned(),
            }),
            ..Params::default()
        }
    }

    #[test]
    fn finished_size_bleed_and_centring() {
        let good = "/MediaBox [0 0 90 162] /TrimBox [9 9 81 153] /BleedBox [0 0 90 162]";
        let doc = paged(
            &[
                (good, ""),
                (
                    "/MediaBox [0 0 100 100] /ArtBox [10 10 90 90] /BleedBox [5 5 95 95]",
                    "",
                ),
                ("/MediaBox [0 0 90 162]", ""),
                ("/MediaBox [0 0 99 162] /TrimBox [9 9 81 153]", ""),
                ("/TrimBox [9 9 81 153]", ""),
            ],
            "",
            &[],
        );
        let runs: [Run; 3] = [geometry, bleed, centring];
        let msg = |page: u32, m: &str| (Some(page), m.to_owned());
        assert_eq!(
            found(&doc, card(None, "left"), &runs),
            [
                msg(2, "finished size is 28.22 x 28.22 mm, expected 25.4 x 50.8 mm"),
                msg(
                    2,
                    "bleed is 1.76, 1.76, 1.76, 1.76 mm (left, bottom, right, top), expected 3.175 mm on every side"
                ),
                msg(
                    4,
                    "the trim box is not centred on the MediaBox: margins are 3.17, 3.17, 6.35, 3.17 mm (left, bottom, right, top)"
                ),
            ]
        );
        let bound = paged(&[(good, "")], "", &[]);
        assert_eq!(
            found(&bound, card(Some(0.0), "right"), &runs),
            [
                msg(
                    1,
                    "bleed is 3.17, 3.17, 3.17, 3.17 mm (left, bottom, right, top), expected 3.175, 3.175, 0, 3.175 mm"
                ),
                msg(
                    1,
                    "the trim box is not centred on the MediaBox: margins are 3.17, 3.17, 3.17, 3.17 mm (left, bottom, right, top); expected left minus right +3.17 mm"
                ),
            ]
        );
        assert_eq!(
            found(&bound, Params::default(), &runs),
            [],
            "no geometry, nothing to judge"
        );
    }

    #[test]
    fn page_count_size_and_rotation() {
        let doc = paged(
            &[
                ("/MediaBox [0 0 72 144] /Rotate 90.5", ""),
                ("/MediaBox [0 0 72 144] /TrimBox [0 0 144 72] /Rotate -360", ""),
                ("/MediaBox [0 0 72 144] /Rotate -90", ""),
                ("/Rotate 180", ""),
            ],
            "",
            &[],
        );
        let params = Params {
            uniform_pages: true,
            pages_multiple_of: Some(3),
            ..Params::default()
        };
        let runs: [Run; 3] = [page_count, page_size, page_rotate];
        assert_eq!(
            found(&doc, params, &runs),
            [
                (None, "4 pages is not a multiple of 3".to_owned()),
                (
                    Some(2),
                    "the page is 50.8 x 25.4 mm but the first page is 25.4 x 50.8 mm".to_owned()
                ),
                (Some(1), "the page is rotated with /Rotate".to_owned()),
                (Some(3), "the page is rotated with /Rotate".to_owned()),
            ]
        );
        let fours = Params {
            pages_multiple_of: Some(4),
            ..Params::default()
        };
        assert_eq!(
            found(&doc, fours, &runs),
            [],
            "a multiple, and pages not held to one size"
        );
    }

    /// A profile stream whose description is `name`, as a v2 text description.
    fn profile(name: &str) -> Vec<u8> {
        let mut data = vec![b' '; 128];
        data.splice(36..40, *b"acsp");
        data.extend(1u32.to_be_bytes());
        data.extend(*b"desc");
        data.extend(144u32.to_be_bytes());
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend(u32::try_from(name.len() + 1).unwrap().to_be_bytes());
        tag.extend(name.as_bytes());
        tag.push(0);
        data.extend(u32::try_from(tag.len()).unwrap().to_be_bytes());
        data.extend(tag);
        let mut stream = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        stream.extend(data);
        stream.extend(b"\nendstream");
        stream
    }

    #[test]
    fn output_conditions() {
        let condition = |intents: &str, expected: &str| {
            let doc = paged(&[("", "")], intents, &[(5, profile("Coated FOGRA39"))]);
            let params = Params {
                output_condition: Some(expected.to_owned()),
                ..Params::default()
            };
            found(&doc, params, &[output_condition])
                .into_iter()
                .map(|(_, m)| m)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            condition("", "FOGRA39"),
            ["no output intent to compare with the expected 'FOGRA39'"]
        );
        assert_eq!(
            condition("/OutputIntents [<< /DestOutputProfile 5 0 R >>]", "fogra39"),
            Vec::<String>::new()
        );
        assert_eq!(
            condition("/OutputIntents [<< /OutputCondition (Fogra 39 coated) >>]", "39 COATED"),
            Vec::<String>::new()
        );
        assert_eq!(
            condition(
                "/OutputIntents [7 99 0 R << /OutputConditionIdentifier /SWOP /Info 5 >> << /Info (\\237) >> << /DestOutputProfile << >> >>]",
                "FOGRA39"
            ),
            ["the output intent is '/SWOP', '\\x9f', 'unnamed', expected 'FOGRA39'"]
        );
        assert_eq!(
            found(&paged(&[("", "")], "", &[]), Params::default(), &[output_condition]),
            []
        );
    }
}
