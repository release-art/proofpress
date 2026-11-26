//! Cut a rectangle of a page out as a one-page PDF of its own: the page's content shown through a smaller window, so
//! text, vectors and images stay as they are. The page keeps its inherited resources and rotation; its annotations,
//! structure link and trim, bleed and art boxes are dropped; resources its content never names are left out, unless
//! its content cannot be read through, when every one is kept.

use std::collections::BTreeSet;

use super::{Writer, name};
use crate::common::rect::{Rect, rect};
use crate::content::{self, Item};
use crate::document::{Dict, Document, Object, Page, Value};
use crate::pipeline::Limits;

/// The resource categories whose entries content names (ISO 32000-2 7.8.3, Table 34).
const NAMED: [&[u8]; 7] = [
    b"ExtGState",
    b"ColorSpace",
    b"Pattern",
    b"Shading",
    b"XObject",
    b"Font",
    b"Properties",
];

/// The page keys a piece leaves out, or writes anew.
const DROPPED: [&[u8]; 11] = [
    b"Parent",
    b"Annots",
    b"StructParents",
    b"TrimBox",
    b"BleedBox",
    b"ArtBox",
    b"MediaBox",
    b"CropBox",
    b"Resources",
    b"Rotate",
    b"Contents",
];

/// Why a page could not be cut.
#[derive(Debug, PartialEq)]
pub enum CropError {
    /// The file could not be read: why.
    Unreadable(String),
    /// There is no such page: how many there are.
    NoPage(usize),
    /// The box misses the page: the page's media box, `(x0, y0, x1, y1)`.
    Outside([f64; 4]),
    /// The page needs objects the file holds damaged, so a piece would lack them: their numbers.
    Damaged(Vec<u32>),
}

/// The box `[x0, y0, x1, y1]` (either corner order, widened by `pad` on every side) of page `page`, counting from 1, of
/// the PDF `bytes`, as a one-page PDF; the box is kept within the page's media box.
///
/// # Errors
/// When the file cannot be read, the page does not exist, or the box misses it.
pub fn crop(bytes: &[u8], page: usize, bounds: [f64; 4], pad: f64) -> Result<Vec<u8>, CropError> {
    let doc = Document::open(bytes, &Limits::default()).map_err(|refusal| CropError::Unreadable(refusal.message))?;
    let pages = doc.pages();
    let source = page
        .checked_sub(1)
        .and_then(|i| pages.get(i))
        .ok_or(CropError::NoPage(pages.len()))?;
    let media = source.get(b"MediaBox").and_then(|m| rect(&doc, m)).unwrap_or(Rect {
        x0: 0.0,
        y0: 0.0,
        x1: 612.0,
        y1: 792.0,
    });
    let [a, b, c, d] = bounds;
    let window = [
        (a.min(c) - pad).max(media.x0),
        (b.min(d) - pad).max(media.y0),
        (a.max(c) + pad).min(media.x1),
        (b.max(d) + pad).min(media.y1),
    ];
    if window[2] <= window[0] || window[3] <= window[1] {
        return Err(CropError::Outside([media.x0, media.y0, media.x1, media.y1]));
    }
    let mut writer = Writer::new(&doc);
    let (root, tree, piece) = (writer.reserve(), writer.reserve(), writer.reserve());
    writer.set(root, format!("<</Type /Catalog /Pages {tree} 0 R>>").into_bytes());
    writer.set(
        tree,
        format!("<</Type /Pages /Kids [{piece} 0 R] /Count 1>>").into_bytes(),
    );
    let body = page_body(&mut writer, &doc, source, window, tree);
    writer.set(piece, body);
    writer.copy_all();
    if !writer.damaged.is_empty() {
        let mut damaged = std::mem::take(&mut writer.damaged);
        damaged.sort_unstable();
        return Err(CropError::Damaged(damaged));
    }
    Ok(writer.finish(root))
}

/// The piece's page dictionary: the page's own entries but those dropped, its contents, inherited rotation and pruned
/// resources, and the window as both media and crop box.
fn page_body(writer: &mut Writer<'_>, doc: &Document, page: &Page<'_>, window: [f64; 4], tree: u32) -> Vec<u8> {
    let mut out = format!("<</Type /Page /Parent {tree} 0 R").into_bytes();
    for (key, value) in page
        .dict
        .entries
        .iter()
        .filter(|(k, _)| !DROPPED.contains(&k.as_slice()) && k != b"Type")
    {
        out.push(b' ');
        out.extend(name(key));
        out.push(b' ');
        out.extend(writer.value(value));
    }
    for key in [b"Contents".as_slice(), b"Rotate"] {
        if let Some(value) = page.get(key) {
            out.push(b' ');
            out.extend(name(key));
            out.push(b' ');
            out.extend(writer.value(value));
        }
    }
    if let Some(resources) = page
        .get(b"Resources")
        .and_then(|r| doc.resolve(r))
        .and_then(Object::dict)
    {
        out.extend(b" /Resources ");
        let pruned = prune(doc, resources, used_names(doc, page).as_ref());
        out.extend(writer.dict(&pruned));
    }
    let window = window.map(super::real).join(" ");
    out.extend(format!(" /MediaBox [{window}] /CropBox [{window}]>>").as_bytes());
    out
}

/// Every name the page's content holds, as operand or in an inline image; `None` when a content stream cannot be
/// decoded or read through, so that nothing it might name is lost.
fn used_names(doc: &Document, page: &Page<'_>) -> Option<BTreeSet<Vec<u8>>> {
    let contents = page.get(b"Contents").and_then(|c| doc.resolve(c));
    let refs: Vec<&Object> = match contents.map(|c| &c.value) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(_) => page.get(b"Contents").into_iter().collect(),
        None => Vec::new(),
    };
    let data: Vec<Vec<u8>> = refs
        .iter()
        .map(|r| match r.value {
            Value::Reference(num, _) => doc.stream_data(num),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let read = content::read(&slices, &Limits::default(), &|_| None);
    if !read.problems.is_empty() {
        return None;
    }
    let mut names = BTreeSet::new();
    let mut collect = |object: &Object| {
        if let Value::Name(n) = &object.value {
            names.insert(n.clone());
        }
    };
    for item in &read.items {
        match item {
            Item::Op(op) => op.operands.iter().for_each(&mut collect),
            Item::Image(image) => image.dict.entries.iter().for_each(|(_, v)| collect(v)),
        }
    }
    Some(names)
}

/// `resources` with each named category holding only the entries `used` names; all of them when `used` is `None`.
fn prune(doc: &Document, resources: &Dict, used: Option<&BTreeSet<Vec<u8>>>) -> Dict {
    let mut out = Dict::default();
    for (key, value) in &resources.entries {
        let category = doc
            .resolve(value)
            .and_then(Object::dict)
            .filter(|_| NAMED.contains(&key.as_slice()));
        let kept = match (category, used) {
            (Some(category), Some(used)) => {
                let entries = category
                    .entries
                    .iter()
                    .filter(|(k, _)| used.contains(k))
                    .cloned()
                    .collect();
                Object {
                    value: Value::Dict(Dict {
                        entries,
                        overridden: Vec::new(),
                    }),
                    start: 0,
                    end: 0,
                }
            }
            _ => value.clone(),
        };
        out.entries.push((key.clone(), kept));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;

    fn stream(data: &str) -> Vec<u8> {
        format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes()
    }

    /// A two-page file whose pages inherit their media box, resources and rotation, with `content` on page 2.
    fn source(content: &str) -> Vec<u8> {
        let contents = stream(content);
        let objects: [(u32, &[u8]); 7] = [
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 100] /Rotate 90 \
                  /Resources << /XObject << /Used 6 0 R /Unused 6 0 R >> /ProcSet [/PDF] /Font 7 0 R >> >>",
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>"),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Annots [] /TrimBox [0 0 1 1] /UserUnit 1 >>",
            ),
            (5, &contents),
            (6, &stream("0 0 1 1 re f")),
            (7, b"<< /F1 1 0 R >>"),
        ];
        classic(&objects, "<< /Size SIZE /Root 1 0 R >>")
    }

    fn piece(bytes: &[u8]) -> Document {
        Document::open(bytes, &Limits::default()).unwrap()
    }

    #[test]
    fn a_piece_is_its_page_through_a_window() {
        let cut = crop(&source("q /Used Do Q"), 2, [150.0, 90.0, 20.0, 10.0], 5.0).unwrap();
        let doc = piece(&cut);
        let pages = doc.pages();
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        let boxes = |key: &[u8]| page.get(key).and_then(|b| rect(&doc, b));
        let window = Rect {
            x0: 15.0,
            y0: 5.0,
            x1: 155.0,
            y1: 95.0,
        };
        assert_eq!((boxes(b"MediaBox"), boxes(b"CropBox")), (Some(window), Some(window)));
        assert!(page.get(b"TrimBox").is_none() && page.get(b"Annots").is_none());
        assert_eq!(page.get(b"Rotate").map(|r| r.value.clone()), Some(Value::Integer(90)));
        assert_eq!(page.get(b"UserUnit").map(|r| r.value.clone()), Some(Value::Integer(1)));
        let resources = page.get(b"Resources").and_then(Object::dict).unwrap();
        let xobjects = resources.get(b"XObject").and_then(Object::dict).unwrap();
        assert!(xobjects.get(b"Used").is_some() && xobjects.get(b"Unused").is_none());
        assert_eq!(
            resources.get(b"Font").and_then(Object::dict).map(|f| f.entries.len()),
            Some(0)
        );
        assert!(resources.get(b"ProcSet").is_some());
        assert!(doc.findings().is_empty(), "{:?}", doc.findings());
    }

    #[test]
    fn unreadable_content_keeps_every_resource() {
        let cut = crop(&source("q /Used Do Q ("), 2, [0.0, 0.0, 200.0, 100.0], 0.0).unwrap();
        let doc = piece(&cut);
        let pages = doc.pages();
        let resources = pages[0].get(b"Resources").and_then(Object::dict).unwrap();
        let xobjects = resources.get(b"XObject").and_then(Object::dict).unwrap();
        assert_eq!(xobjects.entries.len(), 2);
    }

    #[test]
    fn a_page_without_contents_or_boxes() {
        let cut = crop(&source(""), 1, [0.0, 0.0, 10.0, 10.0], 0.0).unwrap();
        assert_eq!(piece(&cut).pages().len(), 1);
    }

    #[test]
    fn what_cannot_be_cut_says_why() {
        assert_eq!(crop(&source(""), 3, [0.0; 4], 0.0), Err(CropError::NoPage(2)));
        assert_eq!(crop(&source(""), 0, [0.0; 4], 0.0), Err(CropError::NoPage(2)));
        let outside = crop(&source(""), 1, [300.0, 300.0, 400.0, 400.0], 0.0);
        assert_eq!(outside, Err(CropError::Outside([0.0, 0.0, 200.0, 100.0])));
        assert!(matches!(
            crop(b"not a PDF", 1, [0.0; 4], 0.0),
            Err(CropError::Unreadable(_))
        ));
        // The form the content draws ends without endstream: a damaged object, so the piece would lack it.
        let damaged = String::from_utf8_lossy(&source("q /Used Do Q"))
            .replace("0 0 1 1 re f\nendstream", "0 0 1 1 re f\nendstreaX");
        assert_eq!(
            crop(damaged.as_bytes(), 2, [0.0, 0.0, 200.0, 100.0], 0.0),
            Err(CropError::Damaged(vec![6]))
        );
    }

    /// A one-page file whose page has `entries`, its media box and the objects `more`.
    fn one_page(entries: &str, more: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let page = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] {entries} >>");
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, page.as_bytes()),
        ];
        objects.extend(more.iter().map(|(n, b)| (*n, b.as_slice())));
        classic(&objects, "<< /Size SIZE /Root 1 0 R >>")
    }

    fn xobjects(cut: &[u8]) -> usize {
        let doc = piece(cut);
        let pages = doc.pages();
        let resources = pages[0].get(b"Resources").and_then(Object::dict).unwrap();
        resources
            .get(b"XObject")
            .and_then(Object::dict)
            .map_or(0, |x| x.entries.len())
    }

    #[test]
    fn contents_in_parts_inline_images_and_no_resources() {
        let resources = "/Resources << /XObject << /A 6 0 R /B 6 0 R /C 6 0 R >> >>";
        let form = (6, stream("0 0 1 1 re f"));
        let parts = (5, stream("BI /W 1 /H 1 /CS /G /BPC 8 /Intent /C ID x EI"));
        let split = one_page(
            &format!("{resources} /Contents [4 0 R 5 0 R]"),
            &[(4, stream("1 0 0 1 0 0 cm /B Do")), parts, form.clone()],
        );
        assert_eq!(xobjects(&crop(&split, 1, [0.0, 0.0, 10.0, 10.0], 0.0).unwrap()), 2);
        // A part that is not a stream cannot be read, so every resource is kept.
        let odd = one_page(
            &format!("{resources} /Contents [4 0 R 5]"),
            &[(4, stream("/B Do")), form],
        );
        assert_eq!(xobjects(&crop(&odd, 1, [0.0, 0.0, 10.0, 10.0], 0.0).unwrap()), 3);
        let bare = one_page("", &[]);
        let doc = piece(&crop(&bare, 1, [0.0, 0.0, 5.0, 5.0], 0.0).unwrap());
        assert!(doc.pages()[0].get(b"Resources").is_none());
    }
}
