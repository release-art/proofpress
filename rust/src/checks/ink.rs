//! Print checks on the inks a page puts down: overprint
//! used on anything but solid black text (advisory), white set to overprint (it vanishes on press), small text in a
//! built black (advisory: it mis-registers), and the opt-in rule that text uses the K plate only.
//!
//! Each reads the paints of the page, its forms included, but not the cells of tiling patterns, as the colours and
//! overprint the print checks have always read: the overprint flag is `OP` or `op`, whichever was set last.

use std::collections::BTreeMap;

use super::Settings;
use crate::common::family::Family;
use crate::document::Document;
use crate::findings::{Check, Finding, K_ONLY_TEXT, Location, OVERPRINT, RICH_BLACK, WHITE_OVERPRINT};
use crate::graphics::{self, Event, Fill, Ink, Kind, Use};

/// An ink amount this close to zero is none, and this close to one is solid.
const EPSILON: f64 = 0.01;
/// A built black: a heavy K plate with at least this much other ink on top.
const RICH_K_MIN: f64 = 0.5;
const RICH_OTHER_MIN: f64 = 0.3;

/// A paint: its kind, colour space uses and ink.
type Painted = (Kind, Vec<Use>, Ink);

/// Each page's paints, outside pattern cells, by page number.
fn paints(doc: &Document) -> BTreeMap<u32, Vec<Painted>> {
    let mut pages: BTreeMap<u32, Vec<_>> = BTreeMap::new();
    for event in graphics::walk(doc) {
        if let Event::Paint {
            kind,
            uses,
            ink,
            frames,
            ..
        } = event
            && !ink.cell
            && let Some(Location::Page(page)) = frames.first()
        {
            pages.entry(*page).or_default().push((kind, uses, *ink));
        }
    }
    pages
}

/// One finding per page whose paints `judge` has something to say about.
fn per_page(doc: &Document, check: &'static Check, judge: &dyn Fn(&[Painted]) -> Option<String>) -> Vec<Finding> {
    paints(doc)
        .into_iter()
        .filter_map(|(page, paints)| Some(Finding::new(check, judge(&paints)?).at(vec![Location::Page(page)])))
        .collect()
}

/// Components `from..to` of a colour that has them.
fn part(fill: &Fill, to: usize) -> &[f64] {
    fill.value.get(..to.min(fill.value.len())).unwrap_or_default()
}

fn is_white(fill: &Fill) -> bool {
    let (need, white): (usize, fn(&[f64]) -> bool) = match fill.family {
        Family::Gray => (1, |v| v.iter().all(|x| *x >= 1.0 - EPSILON)),
        Family::Rgb => (3, |v| v.iter().all(|x| *x >= 1.0 - EPSILON)),
        Family::Cmyk => (4, |v| v.iter().all(|x| *x <= EPSILON)),
        Family::Other => return false,
    };
    fill.value.len() >= need && white(part(fill, need))
}

/// Gray, or CMYK with no cyan, magenta or yellow: only the K plate, or no ink.
pub(super) fn is_k_only(fill: &Fill) -> bool {
    match fill.family {
        Family::Gray => true,
        Family::Cmyk => part(fill, 3).iter().all(|x| *x <= EPSILON),
        _ => false,
    }
}

/// Solid black on the K plate alone: gray 0, or CMYK 0/0/0/100.
fn is_full_k(fill: &Fill) -> bool {
    if fill.family == Family::Gray {
        return fill.value.first().is_some_and(|g| *g <= EPSILON);
    }
    is_k_only(fill) && fill.value.get(3).is_some_and(|k| *k >= 1.0 - EPSILON)
}

fn is_rich_black(fill: &Fill) -> bool {
    match (fill.family, fill.value.as_slice()) {
        (Family::Cmyk, [cyan, magenta, yellow, black, ..]) => {
            *black >= RICH_K_MIN && cyan + magenta + yellow >= RICH_OTHER_MIN
        }
        _ => false,
    }
}

/// A non-K text colour in words.
pub(super) fn describe(fill: &Fill) -> String {
    if !fill.pattern.is_empty() {
        let mut names: Vec<&str> = fill
            .pattern
            .iter()
            .map(|f| match f {
                Family::Gray => "GRAY",
                Family::Rgb => "RGB",
                Family::Cmyk => "CMYK",
                Family::Other => "OTHER",
            })
            .collect();
        names.sort_unstable();
        return format!("a pattern using {}", names.join("/"));
    }
    match (fill.family, fill.value.as_slice()) {
        (Family::Cmyk, [cyan, magenta, yellow, black, ..]) => {
            #[expect(clippy::cast_possible_truncation, reason = "a percentage, rounded")]
            let p = |v: &f64| (v * 100.0).round_ties_even() as i64;
            format!("C{} M{} Y{} K{}", p(cyan), p(magenta), p(yellow), p(black))
        }
        (Family::Rgb, _) => "RGB".to_owned(),
        _ => "a spot, pattern or other non-K colour".to_owned(),
    }
}

/// A page that paints anything but solid black text with overprint on (advisory).
pub(super) fn overprint(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, OVERPRINT, &|paints| {
        paints
            .iter()
            .any(|(kind, _, ink)| ink.overprint && !(matches!(kind, Kind::Text(_)) && is_full_k(&ink.fill)))
            .then(|| "overprint is set on something other than black text; check it is intended".to_owned())
    })
}

/// White text or filled shapes set to overprint: they add no ink, so they vanish on press.
pub(super) fn white_overprint(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, WHITE_OVERPRINT, &|paints| {
        let whites = paints
            .iter()
            .filter(|(kind, _, ink)| {
                let shape = matches!(kind, Kind::Text(_) | Kind::Path { filled: true, .. });
                ink.overprint && is_white(&ink.fill) && shape
            })
            .count();
        (whites > 0).then(|| {
            format!("{whites} white object(s) set to overprint will disappear on press; set them to knock out")
        })
    })
}

/// Text no larger than `small_text_pt` set in a built black (advisory).
pub(super) fn rich_black(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let small = settings.params.small_text_pt;
    per_page(doc, RICH_BLACK, &|paints| {
        let smallest = paints
            .iter()
            .filter(|(kind, uses, ink)| {
                matches!(kind, Kind::Text(_))
                    && graphics::colours(*kind, uses, ink).into_iter().any(is_rich_black)
                    && ink.size <= small
            })
            .map(|(_, _, ink)| ink.size)
            .reduce(f64::min)?;
        Some(format!(
            "text as small as {smallest:.1}pt is set in rich black and will blur if the plates mis-register; use K only"
        ))
    })
}

/// Text painted with any ink but K (opt-in): a four-colour build, RGB, a spot or a pattern.
pub(super) fn k_only_text(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, K_ONLY_TEXT, &|paints| {
        let bad: Vec<&Fill> = paints
            .iter()
            .filter(|(kind, _, _)| matches!(kind, Kind::Text(_)))
            .flat_map(|(kind, uses, ink)| graphics::colours(*kind, uses, ink))
            .filter(|fill| !is_k_only(fill))
            .collect();
        let first = bad.first()?;
        let more = if bad.len() > 1 {
            format!(
                " (and {} more {})",
                bad.len() - 1,
                if bad.len() == 2 { "run" } else { "runs" }
            )
        } else {
            String::new()
        };
        Some(format!("text is painted in {}{more}, not K only", describe(first)))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    /// The four checks' page and message on a file whose pages paint `contents`.
    fn found(contents: &[&str]) -> Vec<(u32, String)> {
        let resources = "<< /ExtGState << /O << /OP true >> >> >>";
        let kids: Vec<String> = (0..contents.len()).map(|i| format!("{} 0 R", 10 + 2 * i)).collect();
        let pages = format!(
            "<< /Type /Pages /Kids [{}] /Count {} /Resources {resources} >>",
            kids.join(" "),
            kids.len()
        );
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, pages.into_bytes()),
        ];
        for (i, data) in contents.iter().enumerate() {
            let n = u32::try_from(10 + 2 * i).unwrap();
            objects.push((
                n,
                format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>", n + 1).into_bytes(),
            ));
            objects.push((
                n + 1,
                format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes(),
            ));
        }
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let settings = Settings::from(Level::Print);
        [overprint, white_overprint, rich_black, k_only_text]
            .iter()
            .flat_map(|run| run(&doc, &settings))
            .map(|f| (f.page.unwrap(), f.message))
            .collect()
    }

    #[test]
    fn what_each_page_puts_down() {
        let black = "/O gs 0 g BT /F 10 Tf (a) Tj ET 0 0 0 1 k BT (b) Tj ET";
        let mixed = "/O gs 1 g 0 0 1 1 re f 0 0 1 1 re S BT 1 1 1 rg (w) Tj \
            0.6 0.5 0.4 0.9 k 0 0 1 0 K 1 Tr /F 5 Tf (x) Tj ET";
        assert_eq!(
            found(&[black, mixed, "BT 1 0 0 0 k (c) Tj ET"]),
            [
                (
                    2,
                    "overprint is set on something other than black text; check it is intended".to_owned()
                ),
                (
                    2,
                    "2 white object(s) set to overprint will disappear on press; set them to knock out".to_owned()
                ),
                (
                    2,
                    "text as small as 5.0pt is set in rich black and will blur if the plates mis-register; use K only"
                        .to_owned()
                ),
                (2, "text is painted in RGB (and 2 more runs), not K only".to_owned()),
                (3, "text is painted in C100 M0 Y0 K0, not K only".to_owned()),
            ]
        );
    }

    fn fill(family: Family, value: &[f64]) -> Fill {
        Fill {
            family,
            value: value.to_vec(),
            spot: Vec::new(),
            pattern: BTreeSet::new(),
            profiled: false,
        }
    }

    #[test]
    fn one_more_run_is_singular() {
        assert_eq!(
            found(&["BT /F 10 Tf 1 0 0 0 k (c) Tj (d) Tj ET"]),
            [(
                1,
                "text is painted in C100 M0 Y0 K0 (and 1 more run), not K only".to_owned()
            )]
        );
    }

    #[test]
    fn colours_judged() {
        use Family::{Cmyk, Gray, Other, Rgb};
        assert!(is_white(&fill(Gray, &[1.0])) && !is_white(&fill(Gray, &[])) && !is_white(&fill(Other, &[1.0])));
        assert!(is_white(&fill(Rgb, &[1.0; 3])) && !is_white(&fill(Rgb, &[1.0, 1.0])));
        assert!(is_white(&fill(Cmyk, &[0.0; 4])) && !is_white(&fill(Cmyk, &[0.0, 0.5, 0.0, 0.0])));
        assert!(is_k_only(&fill(Cmyk, &[])) && !is_k_only(&fill(Rgb, &[0.0; 3])));
        assert!(is_full_k(&fill(Gray, &[0.0])) && !is_full_k(&fill(Gray, &[])));
        assert!(is_full_k(&fill(Cmyk, &[0.0, 0.0, 0.0, 1.0])) && !is_full_k(&fill(Cmyk, &[0.0; 3])));
        assert!(!is_rich_black(&fill(Gray, &[0.0])));
        let pattern = Fill {
            pattern: BTreeSet::from([Rgb, Cmyk, Gray, Other]),
            ..fill(Other, &[])
        };
        assert_eq!(describe(&pattern), "a pattern using CMYK/GRAY/OTHER/RGB");
        assert_eq!(describe(&fill(Cmyk, &[0.125, 0.135, 0.0, 1.0])), "C12 M14 Y0 K100");
        assert_eq!(describe(&fill(Cmyk, &[0.5])), "a spot, pattern or other non-K colour");
    }
}
