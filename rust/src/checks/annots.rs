//! PDF/X-4 (ISO 15930-7) annotation rows: where annotations may sit (6.17) and what a `TrapNet` annotation requires
//! (6.9).
//!
//! 6.17: an annotation's `/Rect` is completely outside a box when the two rectangles share no area, so touching
//! edges are outside and a rectangle that encloses the box is not.

use super::intent;
use super::metadata;
use crate::common::rect::{Rect, rect};
use crate::document::{Dict, Document, Object, Page};
use crate::findings::{
    Check, Finding, Level, Location, X4_ANNOT_OUTSIDE_BLEED, X4_ANNOT_PRINTERMARK_OUTSIDE_TRIM,
    X4_TRAPNET_FONT_FAUXING_EMPTY, X4_TRAPNET_PCM_MATCHES_INTENT, X4_TRAPPED_TRAPNET_IMPLIES_TRUE,
};

/// Every annotation dictionary with its page's index and the page.
fn annotations<'a>(doc: &'a Document, pages: &'a [Page<'a>]) -> Vec<(usize, &'a Page<'a>, &'a Dict)> {
    let mut out = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let annots = page
            .dict
            .get(b"Annots")
            .and_then(|a| doc.resolve(a))
            .and_then(Object::array)
            .unwrap_or_default();
        out.extend(
            annots
                .iter()
                .filter_map(|a| doc.resolve(a)?.dict())
                .map(|a| (i, page, a)),
        );
    }
    out
}

fn subtype(annot: &Dict) -> &[u8] {
    annot.get(b"Subtype").and_then(Object::name).unwrap_or_default()
}

/// Whether two rectangles share no area.
fn apart(a: &Rect, b: &Rect) -> bool {
    a.x1 <= b.x0 || b.x1 <= a.x0 || a.y1 <= b.y0 || b.y1 <= a.y0
}

fn on_page(check: &'static Check, index: usize, message: &str, clause: &str) -> Finding {
    let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
    Finding::new(check, format!("page {number}: {message}"))
        .citing(clause)
        .at(vec![Location::Page(number)])
}

/// The first of `keys` the page has as a rectangle, with its name.
fn first_box<'a>(doc: &Document, page: &Page<'_>, keys: &[&'a str]) -> Option<(&'a str, Rect)> {
    keys.iter()
        .find_map(|k| Some((*k, rect(doc, page.get(k.as_bytes())?)?)))
}

/// Annotations of `which` kind whose `/Rect` overlaps the first of `boxes` the page has.
fn overlapping(doc: &Document, check: &'static Check, which: &dyn Fn(&[u8]) -> bool, boxes: &[&str]) -> Vec<Finding> {
    let pages = doc.pages();
    annotations(doc, &pages)
        .into_iter()
        .filter(|(_, _, a)| which(subtype(a)))
        .filter_map(|(i, page, annot)| {
            let (name, bound) = first_box(doc, page, boxes)?;
            let area = rect(doc, annot.get(b"Rect")?)?;
            let kind = String::from_utf8_lossy(subtype(annot)).into_owned();
            (!apart(&area, &bound))
                .then(|| on_page(check, i, &format!("a {kind} annotation overlaps the {name}"), "6.17"))
        })
        .collect()
}

pub(super) fn outside_bleed(doc: &Document, _: Level) -> Vec<Finding> {
    let other = |s: &[u8]| s != b"TrapNet" && s != b"PrinterMark";
    overlapping(doc, X4_ANNOT_OUTSIDE_BLEED, &other, &["BleedBox", "TrimBox", "ArtBox"])
}

pub(super) fn printermark_outside_trim(doc: &Document, _: Level) -> Vec<Finding> {
    overlapping(
        doc,
        X4_ANNOT_PRINTERMARK_OUTSIDE_TRIM,
        &|s| s == b"PrinterMark",
        &["TrimBox", "ArtBox"],
    )
}

/// The `TrapNet` annotations with their page indices.
fn trapnets<'a>(doc: &'a Document, pages: &'a [Page<'a>]) -> Vec<(usize, &'a Dict)> {
    annotations(doc, pages)
        .into_iter()
        .filter(|(_, _, a)| subtype(a) == b"TrapNet")
        .map(|(i, _, a)| (i, a))
        .collect()
}

/// 6.9: with a `TrapNet` annotation, `pdf:Trapped` is `True`.
pub(super) fn trapnet_implies_true(doc: &Document, _: Level) -> Vec<Finding> {
    let pages = doc.pages();
    let Some((i, _)) = trapnets(doc, &pages).into_iter().next() else {
        return Vec::new();
    };
    if metadata::trapped(doc).as_deref() == Some("True") {
        return Vec::new();
    }
    vec![on_page(
        X4_TRAPPED_TRAPNET_IMPLIES_TRUE,
        i,
        "a TrapNet annotation, but pdf:Trapped is not True",
        "6.9",
    )]
}

/// 6.9: a `TrapNet` annotation's `/FontFauxing` is absent or an empty array.
pub(super) fn font_fauxing(doc: &Document, _: Level) -> Vec<Finding> {
    let pages = doc.pages();
    trapnets(doc, &pages)
        .into_iter()
        .filter(|(_, a)| {
            a.get(b"FontFauxing")
                .is_some_and(|f| doc.resolve(f).and_then(Object::array).is_none_or(|f| !f.is_empty()))
        })
        .map(|(i, _)| {
            on_page(
                X4_TRAPNET_FONT_FAUXING_EMPTY,
                i,
                "a TrapNet annotation with /FontFauxing",
                "6.9",
            )
        })
        .collect()
}

/// The device colour space a `/PCM` name must be for the intent's data colour space.
fn pcm_for(space: [u8; 4]) -> &'static [u8] {
    match &space {
        b"GRAY" => b"DeviceGray",
        b"RGB " => b"DeviceRGB",
        b"CMYK" => b"DeviceCMYK",
        _ => b"",
    }
}

/// 6.9: the `/PCM` of a `TrapNet` annotation's appearances names the intent's colour space. Without a readable intent
/// profile there is nothing to match, and `x4.output-intent.*` reports why.
pub(super) fn pcm_matches_intent(doc: &Document, _: Level) -> Vec<Finding> {
    let Some(header) = intent::profile(doc) else {
        return Vec::new();
    };
    let want = pcm_for(header.space);
    let pages = doc.pages();
    let mut out = Vec::new();
    for (i, annot) in trapnets(doc, &pages) {
        let normal = annot
            .get(b"AP")
            .and_then(|ap| doc.resolve(ap))
            .and_then(Object::dict)
            .and_then(|ap| ap.get(b"N"))
            .and_then(|n| doc.resolve(n));
        let Some(normal) = normal.and_then(Object::dict) else {
            continue;
        };
        // A normal appearance is a stream, or a dictionary of streams by state.
        let streams: Vec<&Dict> = if normal.get(b"PCM").is_some() {
            vec![normal]
        } else {
            normal
                .entries
                .iter()
                .filter_map(|(_, s)| doc.resolve(s)?.dict())
                .collect()
        };
        for stream in streams {
            let pcm = stream.get(b"PCM").and_then(|p| doc.resolve(p)).and_then(Object::name);
            if pcm.is_some_and(|p| p != want) {
                let message = format!(
                    "a TrapNet appearance with /PCM /{}, not /{}",
                    String::from_utf8_lossy(pcm.unwrap_or_default()),
                    String::from_utf8_lossy(want)
                );
                out.push(on_page(X4_TRAPNET_PCM_MATCHES_INTENT, i, &message, "6.9"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, Level) -> Vec<Finding>; 5] = [
        outside_bleed,
        printermark_outside_trim,
        trapnet_implies_true,
        font_fauxing,
        pcm_matches_intent,
    ];

    fn ids(objects: &[(u32, &[u8])]) -> Vec<(&'static str, Option<u32>)> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, Level::X4))
            .map(|f| (f.check.id, f.page))
            .collect()
    }

    #[test]
    fn annotations_and_their_boxes() {
        let objects: [(u32, &[u8]); 4] = [
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] >>"),
            (3, b"<< /Type /Page /BleedBox [0 0 100 100] /TrimBox [10 10 90 90] /Annots [\
                << /Subtype /Link /Rect [100 0 120 20] >> << /Subtype /Link /Rect [-10 -10 110 110] >> \
                << /Subtype /PrinterMark /Rect [0 0 10 10] >> << /Subtype /PrinterMark /Rect [5 5 20 20] >> \
                << /Subtype /Text >> 9 0 R] >>"),
            (4, b"<< /Type /Page /ArtBox [0 0 10 10] /Annots [<< /Subtype /Widget /Rect [5 5 6 6] >> << /Subtype /TrapNet /Rect [0 0 1 1] >>] >>"),
        ];
        assert_eq!(
            ids(&objects),
            [
                ("x4.annot.outside-bleed", Some(1)),
                ("x4.annot.outside-bleed", Some(2)),
                ("x4.annot.printermark-outside-trim", Some(1)),
                ("x4.trapped.trapnet-implies-true", Some(2)),
            ]
        );
    }

    #[test]
    fn trap_networks() {
        let intent = "<< /S /GTS_PDFX /DestOutputProfile 6 0 R >>";
        let mut icc = vec![0u8; 128];
        icc.get_mut(16..20).unwrap().copy_from_slice(b"CMYK");
        icc.get_mut(36..40).unwrap().copy_from_slice(b"acsp");
        let mut profile = format!("<< /Length {} >>\nstream\n", icc.len()).into_bytes();
        profile.extend(&icc);
        profile.extend(b"\nendstream");
        let catalog = format!("<< /Pages 2 0 R /OutputIntents [{intent}] >>");
        let objects: [(u32, &[u8]); 6] = [
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /BleedBox [0 0 1 1] >>"),
            (3, b"<< /Type /Page /Annots [<< /Subtype /TrapNet /FontFauxing [1 0 R] /AP << /N 4 0 R >> >> \
                << /Subtype /TrapNet /FontFauxing [] /AP << /N << /On 5 0 R /Off 5 0 R >> >> >> << /Subtype /TrapNet /AP << /D 4 0 R >> >> << /Subtype /TrapNet /AP << /N << /On 9 0 R >> >> >>] >>"),
            (4, b"<< /PCM /DeviceCMYK /Length 0 >>\nstream\n\nendstream"),
            (5, b"<< /PCM /DeviceRGB /Length 0 >>\nstream\n\nendstream"),
            (6, &profile),
        ];
        assert_eq!(
            ids(&objects),
            [
                ("x4.trapped.trapnet-implies-true", Some(1)),
                ("x4.trapnet.font-fauxing-empty", Some(1)),
                ("x4.trapnet.pcm-matches-intent", Some(1)),
                ("x4.trapnet.pcm-matches-intent", Some(1)),
            ]
        );
        let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:pdf="http://ns.adobe.com/pdf/1.3/" pdf:Trapped="True"/></rdf:RDF></x:xmpmeta>"#;
        let mut metadata = format!("<< /Type /Metadata /Length {} >>\nstream\n", xmp.len()).into_bytes();
        metadata.extend(xmp.as_slice());
        metadata.extend(b"\nendstream");
        let trapped: [(u32, &[u8]); 4] = [
            (1, b"<< /Pages 2 0 R /Metadata 4 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (
                3,
                b"<< /Type /Page /Annots [<< /Subtype /TrapNet /AP << /N << /On 9 0 R >> >> >>] >>",
            ),
            (4, &metadata),
        ];
        assert_eq!(
            ids(&trapped),
            [],
            "trapped, and an appearance state that refers to nothing"
        );
        assert_eq!(pcm_for(*b"GRAY"), b"DeviceGray");
        assert_eq!(pcm_for(*b"RGB "), b"DeviceRGB");
        assert_eq!(pcm_for(*b"Lab "), b"");
        let bare: [(u32, &[u8]); 3] = [
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] >>"),
            (3, b"<< /Type /Page /Annots [<< /Subtype /Link /Rect [0 0 1 1] >>] >>"),
        ];
        assert_eq!(ids(&bare), [], "a page with no box to compare with");
        let odd: [(u32, &[u8]); 3] = [
            (1, b"<< /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, b"<< /Type /Page /BleedBox [1] /TrimBox [0 0 9 9] /Annots [<< /Subtype /Link >> << /Subtype /Link /Rect [1 2] >>] >>"),
        ];
        assert_eq!(ids(&odd), [], "boxes and rectangles that are not four numbers");
    }
}
