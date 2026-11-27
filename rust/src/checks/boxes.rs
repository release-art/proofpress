//! PDF/X-4 (ISO 15930-7) 6.12: every page has a trim or art box, not both, and the boxes nest: trim or art inside the
//! bleed box, everything inside the crop box, and everything inside the media box. Boxes are compared exactly, as the
//! clause states no tolerance. A box that is not four numbers is left to the PDF syntax rows.

use super::Settings;
use super::info::per_level;
use crate::common::rect::{Rect, rect};
use crate::document::{Document, Page};
use crate::findings::{
    Check, Finding, Location, X4_BOXES_NOT_BOTH, X4_BOXES_PREFER_TRIM, X4_BOXES_TRIM_OR_ART, X4_BOXES_WITHIN_BLEED,
    X4_BOXES_WITHIN_CROP, X4_BOXES_WITHIN_MEDIA, X13_BOXES_TRIM_WITHIN_CROP,
};

/// A finding on page `index` (counted from 0), located at the page and its object.
fn on_page(check: &'static Check, index: usize, page: &Page<'_>, message: &str) -> Finding {
    on_page_citing(check, index, page, message, "6.12")
}

/// [`on_page`], citing `clause`.
fn on_page_citing(check: &'static Check, index: usize, page: &Page<'_>, message: &str, clause: &str) -> Finding {
    let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
    let mut chain = vec![Location::Page(number)];
    if page.num != 0 {
        chain.push(Location::Object(page.num, 0));
    }
    Finding::new(check, format!("page {number}: {message}"))
        .citing(clause)
        .at(chain)
}

/// Each page with its index, run through `judge`.
fn per_page(doc: &Document, judge: &dyn Fn(usize, &Page<'_>) -> Vec<Finding>) -> Vec<Finding> {
    doc.pages()
        .iter()
        .enumerate()
        .flat_map(|(i, page)| judge(i, page))
        .collect()
}

/// The page's box `key` as a rectangle, inherited where PDF allows it.
fn page_box(doc: &Document, page: &Page<'_>, key: &[u8]) -> Option<Rect> {
    rect(doc, page.get(key)?)
}

/// The boxes `inner` that extend beyond `outer`, as findings from `check`, when the page has `outer`.
fn nested(doc: &Document, check: &'static Check, outer: &str, inner: &[&str]) -> Vec<Finding> {
    nested_citing(doc, check, outer, inner, "6.12")
}

/// [`nested`], citing `clause`.
fn nested_citing(doc: &Document, check: &'static Check, outer: &str, inner: &[&str], clause: &str) -> Vec<Finding> {
    per_page(doc, &|i, page| {
        let Some(bound) = page_box(doc, page, outer.as_bytes()) else {
            return Vec::new();
        };
        inner
            .iter()
            .filter(|name| page_box(doc, page, name.as_bytes()).is_some_and(|b| !b.within(&bound)))
            .map(|name| {
                on_page_citing(
                    check,
                    i,
                    page,
                    &format!("the {name} extends beyond the {outer}"),
                    clause,
                )
            })
            .collect()
    })
}

pub(super) fn trim_or_art(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, &|i, page| {
        if page.get(b"TrimBox").is_none() && page.get(b"ArtBox").is_none() {
            vec![on_page(
                X4_BOXES_TRIM_OR_ART,
                i,
                page,
                "neither a TrimBox nor an ArtBox",
            )]
        } else {
            Vec::new()
        }
    })
}

pub(super) fn not_both(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, &|i, page| {
        if page.get(b"TrimBox").is_some() && page.get(b"ArtBox").is_some() {
            vec![on_page(X4_BOXES_NOT_BOTH, i, page, "both a TrimBox and an ArtBox")]
        } else {
            Vec::new()
        }
    })
}

/// 6.12 recommends the trim box over the art box.
pub(super) fn prefer_trim(doc: &Document, _: &Settings) -> Vec<Finding> {
    per_page(doc, &|i, page| {
        if page.get(b"ArtBox").is_some() && page.get(b"TrimBox").is_none() {
            vec![on_page(
                X4_BOXES_PREFER_TRIM,
                i,
                page,
                "an ArtBox where a TrimBox is recommended",
            )]
        } else {
            Vec::new()
        }
    })
}

pub(super) fn within_bleed(doc: &Document, _: &Settings) -> Vec<Finding> {
    nested(doc, X4_BOXES_WITHIN_BLEED, "BleedBox", &["TrimBox", "ArtBox"])
}

pub(super) fn within_crop(doc: &Document, _: &Settings) -> Vec<Finding> {
    nested(doc, X4_BOXES_WITHIN_CROP, "CropBox", &["ArtBox", "TrimBox", "BleedBox"])
}

/// 15930-1 6.14, -3 6.7: only the trim or art box must lie inside the crop box at the 2001 and 2002 levels.
pub(super) fn trim_within_crop(doc: &Document, settings: &Settings) -> Vec<Finding> {
    nested_citing(
        doc,
        X13_BOXES_TRIM_WITHIN_CROP,
        "CropBox",
        &["TrimBox", "ArtBox"],
        "6.14",
    )
    .into_iter()
    .map(|f| per_level(f, settings, ["6.14", "6.7", "", ""]))
    .collect()
}

pub(super) fn within_media(doc: &Document, _: &Settings) -> Vec<Finding> {
    nested(
        doc,
        X4_BOXES_WITHIN_MEDIA,
        "MediaBox",
        &["ArtBox", "TrimBox", "CropBox", "BleedBox"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 6] = [
        trim_or_art,
        not_both,
        prefer_trim,
        within_bleed,
        within_crop,
        within_media,
    ];

    fn found(objects: &[(u32, &[u8])]) -> Vec<(&'static str, Option<u32>)> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| (f.check.id, f.page))
            .collect()
    }

    #[test]
    fn boxes_are_inherited_and_pages_walked_in_order() {
        let tree: [(u32, &[u8]); 5] = [
            (1, b"<< /Pages 2 0 R >>"),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /CropBox [0 0 50 50] >>",
            ),
            (3, b"<< /Type /Page /TrimBox [0 0 60 60] >>"),
            (4, b"<< /Kids [5 0 R << /ArtBox [0 0 10 10] /MediaBox [0 0 5 5] >>] >>"),
            (
                5,
                b"<< /Type /Page /TrimBox [0 0 10 10] /ArtBox [0 0 10 10] /BleedBox [1 1 5 5] >>",
            ),
        ];
        assert_eq!(
            found(&tree),
            [
                ("x4.boxes.not-both", Some(2)),
                ("x4.boxes.prefer-trim", Some(3)),
                ("x4.boxes.within-bleed", Some(2)),
                ("x4.boxes.within-bleed", Some(2)),
                ("x4.boxes.within-crop", Some(1)),
                ("x4.boxes.within-media", Some(3)),
                ("x4.boxes.within-media", Some(3)),
            ]
        );
    }

    #[test]
    fn odd_trees_end_without_a_page_or_a_loop() {
        let looped: [(u32, &[u8]); 3] = [
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [2 0 R 3 0 R 9 0 R] >>"),
            (3, b"<< /Type /Page /MediaBox [0 0 1 1] >>"),
        ];
        assert_eq!(found(&looped), [("x4.boxes.trim-or-art", Some(1))]);
        assert_eq!(found(&[(1, b"<< >>")]), []);
    }

    #[test]
    fn the_trim_box_inside_the_crop_box_at_the_older_levels() {
        let doc = crate::checks::test_doc::paged(
            &[(
                "/MediaBox [0 0 9 9] /CropBox [1 1 9 9] /TrimBox [0 0 5 5] /BleedBox [0 0 9 9]",
                "",
            )],
            "",
            &[],
        );
        let cited: Vec<_> = trim_within_crop(&doc, &Level::X32002.into())
            .into_iter()
            .map(|f| (f.message, f.clause.map(|c| c.standard)))
            .collect();
        assert_eq!(
            cited,
            [(
                "page 1: the TrimBox extends beyond the CropBox".to_owned(),
                Some("iso-15930-3")
            )]
        );
    }
}
