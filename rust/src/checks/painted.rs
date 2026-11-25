//! Print checks on what each page paints and where (ported from the Python `structure` module, under the parity
//! gate): a `Do` that names no `XObject` leaves a hole where the artwork should be (ISO 32000-2 7.8.3), and an object
//! painted wholly off the media box never prints but ships in the file.

use std::collections::{BTreeMap, BTreeSet};

use crate::common::rect::rect;
use crate::document::Document;
use crate::findings::{Finding, Level, Location, OUTSIDE_MEDIA, XOBJECT_MISSING};
use crate::graphics::{self, Event, Kind};

/// The page an event is on, from its first frame.
fn page_of(frames: &[Location]) -> u32 {
    match frames.first() {
        Some(Location::Page(n)) => *n,
        _ => 0,
    }
}

/// Each name a page paints with `Do` that gives no `XObject`, once per page.
pub(super) fn xobject_missing(doc: &Document, _: Level) -> Vec<Finding> {
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
pub(super) fn outside_media(doc: &Document, _: Level) -> Vec<Finding> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
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
        [xobject_missing(&doc, Level::Print), outside_media(&doc, Level::Print)]
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
}
