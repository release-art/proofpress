//! Print checks on images' effective resolution:
//! below the profile's minimum fails, below its advisory level warns, above its ceiling warns (it only grows the file),
//! and an image scaled unevenly past the profile's limit warns.
//!
//! An image's resolution on each axis is its samples over its placed length (the CTM's scale on that axis) in inches.
//! One finding per image name per page and rule reports the worst placement of that image, the first on a tie: a job
//! repeats the same logo on every card. Images placed smaller than a point either way are not judged.

use std::collections::BTreeMap;

use super::Settings;
use super::page::{media_box, pt_to_mm};
use super::painted::page_of;
use crate::common::pyfmt::g;
use crate::document::Document;
use crate::findings::{Check, Finding, IMAGE_DPI, IMAGE_DPI_MAX, IMAGE_STRETCH, Location, Severity};
use crate::graphics::{self, Event, Kind};

/// An image placed smaller than this, in points, on either axis is not judged.
const MIN_PLACED_PT: f64 = 1.0;
/// Resolutions within this fraction of a limit meet it, so 299.9 dpi of a 300 dpi rule passes.
const ROUNDING: f64 = 1e-3;

/// One placement of an image: its resolution on each axis, and where its lower-left corner is, in millimetres from
/// the media box's.
#[derive(Clone, Copy)]
struct Placement {
    dpi_x: f64,
    dpi_y: f64,
    at: (f64, f64),
}

impl Placement {
    fn low(&self) -> f64 {
        self.dpi_x.min(self.dpi_y)
    }

    fn high(&self) -> f64 {
        self.dpi_x.max(self.dpi_y)
    }

    fn stretch_pct(&self) -> f64 {
        (self.high() / self.low() - 1.0) * 100.0
    }

    fn describe(&self) -> String {
        let (x, y) = (format!("{:.0}", self.dpi_x), format!("{:.0}", self.dpi_y));
        let both = if x == y {
            String::new()
        } else {
            format!(" ({x} x {y} dpi)")
        };
        format!(
            "{:.0} dpi{both} at {:.1}, {:.1} mm from the page's bottom-left",
            self.low(),
            self.at.0,
            self.at.1
        )
    }
}

/// Each page's image placements by name (`/Im0`, or `(inline)` for every inline image).
fn placements(doc: &Document) -> BTreeMap<(u32, String), (Placement, Vec<Placement>)> {
    let origins: Vec<(f64, f64)> = doc
        .pages()
        .iter()
        .map(|p| media_box(doc, p).map_or((0.0, 0.0), |m| (m.x0, m.y0)))
        .collect();
    let mut out: BTreeMap<(u32, String), (Placement, Vec<Placement>)> = BTreeMap::new();
    for event in graphics::walk(doc) {
        let Event::Paint { kind, ink, frames, .. } = event else {
            continue;
        };
        let (Some([sx, ry, rx, sy, tx, ty]), Some((samples_x, samples_y)), false) = (ink.matrix, ink.pixels, ink.cell)
        else {
            continue;
        };
        let (width, height) = (sx.hypot(ry), rx.hypot(sy));
        if width < MIN_PLACED_PT || height < MIN_PLACED_PT {
            continue;
        }
        let page = page_of(&frames);
        let origin = origins
            .get(usize::try_from(page).unwrap_or(usize::MAX).wrapping_sub(1))
            .copied()
            .unwrap_or((0.0, 0.0));
        let xs = [tx, sx + tx, rx + tx, sx + rx + tx];
        let ys = [ty, ry + ty, sy + ty, ry + sy + ty];
        let lowest = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
        let name = String::from_utf8_lossy(&ink.name).into_owned();
        let name = if kind == Kind::InlineImage {
            name
        } else {
            format!("/{name}")
        };
        #[expect(clippy::cast_precision_loss, reason = "sample counts far below 2^53")]
        let placement = Placement {
            dpi_x: samples_x as f64 / (width / 72.0),
            dpi_y: samples_y as f64 / (height / 72.0),
            at: (pt_to_mm(lowest(xs) - origin.0), pt_to_mm(lowest(ys) - origin.1)),
        };
        out.entry((page, name))
            .and_modify(|(_, rest)| rest.push(placement))
            .or_insert((placement, Vec::new()));
    }
    out
}

/// The first placement whose `measure` is least (or most, with `most`).
fn worst((first, rest): &(Placement, Vec<Placement>), measure: fn(&Placement) -> f64, most: bool) -> Placement {
    rest.iter().fold(*first, |best, p| {
        let better = if most {
            measure(p) > measure(&best)
        } else {
            measure(p) < measure(&best)
        };
        if better { *p } else { best }
    })
}

fn on_page(check: &'static Check, page: u32, message: String) -> Finding {
    Finding::new(check, message).at(vec![Location::Page(page)])
}

/// Images meet the minimum resolution, or (as a warning) the advisory level.
pub(super) fn image_dpi(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let params = &settings.params;
    let mut out = Vec::new();
    for ((page, name), found) in placements(doc) {
        let soft = worst(&found, Placement::low, false);
        if soft.low() < params.min_dpi * (1.0 - ROUNDING) {
            let message = format!(
                "image {name} is placed at {}, below the {:.0} dpi minimum",
                soft.describe(),
                params.min_dpi
            );
            out.push(on_page(IMAGE_DPI, page, message));
        } else if let Some(warn) = params.warn_dpi.filter(|w| soft.low() < w * (1.0 - ROUNDING)) {
            let message = format!(
                "image {name} is placed at {}, below the {warn:.0} dpi advisory level",
                soft.describe()
            );
            out.push(Finding {
                severity: Severity::Warning,
                ..on_page(IMAGE_DPI, page, message)
            });
        }
    }
    out
}

/// Images do not exceed the resolution ceiling (a warning: it only grows the file).
pub(super) fn image_dpi_max(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(max) = settings.params.max_dpi else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ((page, name), found) in placements(doc) {
        let sharp = worst(&found, Placement::high, true);
        if sharp.high() <= max * (1.0 + ROUNDING) {
            continue;
        }
        let message = format!(
            "image {name} is placed at {:.0} dpi at {:.1}, {:.1} mm from the page's bottom-left, above the {max:.0} dpi limit: it only increases the file size",
            sharp.high(),
            sharp.at.0,
            sharp.at.1
        );
        out.push(on_page(IMAGE_DPI_MAX, page, message));
    }
    out
}

/// Images are not scaled more on one axis than the other past the limit (a warning).
pub(super) fn image_stretch(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(limit) = settings.params.max_stretch_pct else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ((page, name), found) in placements(doc) {
        let stretched = worst(&found, Placement::stretch_pct, true);
        if stretched.stretch_pct() <= limit {
            continue;
        }
        let message = format!(
            "image {name} is stretched {:.0}%, past the {}% limit: {}",
            stretched.stretch_pct(),
            g(limit),
            stretched.describe()
        );
        out.push(on_page(IMAGE_STRETCH, page, message));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Params;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    fn found(page: &str, data: &str, params: Params) -> Vec<(Severity, String)> {
        let more = [
            (5, b"<< /Subtype /Image /Width 300 /Height 300 /Length 0 >>\nstream\n\nendstream".to_vec()),
            (6, b"<< /Subtype /Image /Width 0 /Height 300 /Length 0 >>\nstream\n\nendstream".to_vec()),
            (7, b"<< /Subtype /Image /Width 300.7 /Height /X /Length 0 >>\nstream\n\nendstream".to_vec()),
            (9, b"<< /Subtype /Image /Width 99 0 R /Height 300 /Length 0 >>\nstream\n\nendstream".to_vec()),
            (8, b"<< /PatternType 1 /PaintType 1 /Resources << /XObject << /I 5 0 R >> >> /Length 6 >>\nstream\n/I Do\nendstream".to_vec()),
        ];
        let entries = format!(
            "{page} /Resources << /XObject << /I 5 0 R /Z 6 0 R /X 7 0 R /W 9 0 R >> /Pattern << /P 8 0 R >> >>"
        );
        let doc = paged(&[(&entries, data)], "", &more);
        let settings = Settings {
            level: Level::Print,
            params,
            files: crate::checks::Files::Alone,
        };
        [
            image_dpi(&doc, &settings),
            image_dpi_max(&doc, &settings),
            image_stretch(&doc, &settings),
        ]
        .concat()
        .into_iter()
        .map(|f| (f.severity, f.message))
        .collect()
    }

    #[test]
    fn effective_resolution() {
        let data = "q 72 0 0 72 10 20 cm /I Do Q q 144 0 0 36 0 0 cm /I Do Q q 0.5 0 0 0.5 0 0 cm /I Do Q /Nope Do \
            q 72 0 0 72 10 20 cm /I Do /W Do Q \
            q 72 0 0 72 0 0 cm /Z Do /X Do BI /W 10 /H 10 /BPC 8 /CS /G ID 0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789 EI Q \
            /Pattern cs /P scn 0 0 1 1 re f";
        let all = Params {
            min_dpi: 100.0,
            warn_dpi: Some(400.0),
            max_dpi: Some(250.0),
            max_stretch_pct: Some(50.0),
            ..Params::default()
        };
        let msg = |s: Severity, m: &str| (s, m.to_owned());
        assert_eq!(
            found("/MediaBox [10 10 100 100]", data, all),
            [
                msg(
                    Severity::Error,
                    "image (inline) is placed at 10 dpi at -3.5, -3.5 mm from the page's bottom-left, below the 100 dpi minimum"
                ),
                msg(
                    Severity::Warning,
                    "image /I is placed at 150 dpi (150 x 600 dpi) at -3.5, -3.5 mm from the page's bottom-left, below the 400 dpi advisory level"
                ),
                msg(
                    Severity::Warning,
                    "image /I is placed at 600 dpi at -3.5, -3.5 mm from the page's bottom-left, above the 250 dpi limit: it only increases the file size"
                ),
                msg(
                    Severity::Warning,
                    "image /I is stretched 300%, past the 50% limit: 150 dpi (150 x 600 dpi) at -3.5, -3.5 mm from the page's bottom-left"
                ),
            ]
        );
        let plain = found("", "q 72 0 0 72 10 20 cm /I Do Q", Params::default());
        assert_eq!(plain, [], "300 dpi, limits off, no media box");
        let warned = Params {
            warn_dpi: Some(301.0),
            ..Params::default()
        };
        assert_eq!(found("", "q 72 0 0 72 10 20 cm /I Do Q", warned).len(), 1);
    }
}
