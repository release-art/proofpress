//! The structure tree of a tagged PDF (ISO 32000-2 14.7): each element's tag as written and the standard role its
//! `/RoleMap` makes it, the paragraph-like element it belongs to, its alternate and actual text, and the layout
//! attributes producers write, with the marked content (page and MCID) it holds directly.
//!
//! It reads the tree as the Python `proof_pdf._internal.tagged` did: depth-first, an indirect node once, at most
//! [`MAX_ELEMENTS`] elements [`MAX_DEPTH`] deep, and a node whose MCID is not a number ends the reading there, keeping
//! what was read before it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value as Json;

use super::{pdf_str, reference};
use crate::document::{Dict, Document, Object, Value};

/// Tags whose text is a unit of reading (a paragraph, a heading, a cell...): lines under one of them belong together.
const LEAF_BLOCKS: [&str; 22] = [
    "P",
    "H",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    "Title",
    "LBody",
    "Lbl",
    "TD",
    "TH",
    "Caption",
    "Code",
    "Formula",
    "Note",
    "BibEntry",
    "Reference",
    "Figure",
    "Quote",
    "BlockQuote",
];
/// The layout attributes reported (14.8.5).
const ATTRIBUTES: [&[u8]; 20] = [
    b"TextAlign",
    b"BlockAlign",
    b"InlineAlign",
    b"SpaceBefore",
    b"SpaceAfter",
    b"StartIndent",
    b"EndIndent",
    b"TextIndent",
    b"LineHeight",
    b"Placement",
    b"BBox",
    b"Width",
    b"Height",
    b"RowSpan",
    b"ColSpan",
    b"Scope",
    b"ListNumbering",
    b"WritingMode",
    b"BaselineShift",
    b"TextDecorationType",
];
const MAX_ELEMENTS: usize = 200_000;
const MAX_DEPTH: usize = 300;
const MAX_ROLE_HOPS: usize = 6;

/// The structure tree, as `extract` reports it.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Structure {
    /// Whether the catalog's `/MarkInfo` says the file is tagged.
    marked: bool,
    language: Option<String>,
    role_map: BTreeMap<String, String>,
    elements: Vec<Element>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Element {
    id: usize,
    parent: Option<usize>,
    tag: String,
    role: String,
    /// The paragraph-like element this one belongs to: itself, for one.
    block: usize,
    page: Option<u32>,
    /// The page and MCID of each piece of marked content it holds directly.
    marks: Vec<(u32, i64)>,
    alt: Option<String>,
    actual_text: Option<String>,
    title: Option<String>,
    lang: Option<String>,
    attrs: BTreeMap<String, Json>,
}

/// The structure tree of `doc`, or `None` when it has none.
pub(super) fn read(doc: &Document) -> Option<Structure> {
    let (catalog, root) = doc.catalog().and_then(|catalog| {
        let root = catalog
            .get(b"StructTreeRoot")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict);
        root.map(|root| (catalog, root))
    })?;
    let pages: BTreeMap<u32, u32> = doc
        .pages()
        .iter()
        .zip(1..)
        .filter(|(p, _)| p.num != 0)
        .map(|(p, n)| (p.num, n))
        .collect();
    let role_map = root
        .get(b"RoleMap")
        .and_then(|m| doc.resolve(m))
        .and_then(Object::dict)
        .map_or_else(BTreeMap::new, |m| {
            m.entries
                .iter()
                .filter_map(|(k, v)| {
                    let to = pdf_str(doc, v)?;
                    Some((
                        String::from_utf8_lossy(k).into_owned(),
                        to.trim_start_matches('/').to_owned(),
                    ))
                })
                .collect()
        });
    let mut reader = Reader {
        doc,
        pages,
        role_map,
        elements: Vec::new(),
        seen: BTreeSet::new(),
        broken: false,
    };
    reader.node(root.get(b"K"), None, None, 0);
    let marked = catalog
        .get(b"MarkInfo")
        .and_then(|m| doc.resolve(m))
        .and_then(Object::dict)
        .and_then(|m| m.get(b"Marked"))
        .and_then(|m| doc.resolve(m))
        .is_some_and(|m| m.value == Value::Bool(true));
    Some(Structure {
        marked,
        language: catalog.get(b"Lang").and_then(|l| pdf_str(doc, l)),
        role_map: reader.role_map,
        elements: reader.elements,
    })
}

struct Reader<'a> {
    doc: &'a Document,
    /// Page object numbers to page numbers from 1.
    pages: BTreeMap<u32, u32>,
    role_map: BTreeMap<String, String>,
    elements: Vec<Element>,
    seen: BTreeSet<u32>,
    /// Set by a node that cannot be read: nothing after it is.
    broken: bool,
}

impl Reader<'_> {
    fn node(&mut self, kids: Option<&Object>, parent: Option<usize>, page: Option<u32>, depth: usize) {
        let Some(kids) = kids else { return };
        if self.broken || depth > MAX_DEPTH || self.elements.len() > MAX_ELEMENTS {
            return;
        }
        let number = reference(kids);
        let Some(resolved) = self.doc.resolve(kids) else { return };
        match &resolved.value {
            Value::Array(items) => {
                for kid in items {
                    self.node(Some(kid), parent, page, depth);
                }
            }
            Value::Integer(mcid) => {
                if let (Some(parent), Some(page)) = (parent, page) {
                    self.mark(parent, page, *mcid);
                }
            }
            Value::Dict(kid) => {
                if let Some(num) = number
                    && !self.seen.insert(num)
                {
                    return;
                }
                self.dictionary(kid, parent, page, depth);
            }
            _ => {}
        }
    }

    fn dictionary(&mut self, node: &Dict, parent: Option<usize>, page: Option<u32>, depth: usize) {
        let own = match node.get(b"Pg") {
            Some(pg) => reference(pg).and_then(|n| self.pages.get(&n).copied()).or(page),
            None => page,
        };
        let kind = node
            .get(b"Type")
            .and_then(|t| self.doc.resolve(t))
            .and_then(Object::name);
        if kind == Some(b"MCR")
            && let Some(parent) = parent
            && let Some(mcid) = node.get(b"MCID")
            && node.get(b"Stm").is_none()
        {
            // Python's int() truncates a real.
            #[expect(clippy::cast_possible_truncation, reason = "as Python's int() of a PDF real")]
            let number = self.doc.resolve(mcid).and_then(|m| match m.value {
                Value::Integer(i) => Some(i),
                Value::Real(r) => Some(r as i64),
                _ => None,
            });
            match number {
                Some(mcid) => {
                    if let Some(own) = own {
                        self.mark(parent, own, mcid);
                    }
                }
                None => self.broken = true,
            }
        } else if let Some(tag) = node.get(b"S") {
            self.element(node, tag, parent, own, depth);
        }
    }

    fn mark(&mut self, element: usize, page: u32, mcid: i64) {
        self.elements
            .get_mut(element)
            .into_iter()
            .for_each(|e| e.marks.push((page, mcid)));
    }

    fn element(&mut self, node: &Dict, tag: &Object, parent: Option<usize>, page: Option<u32>, depth: usize) {
        let tag = pdf_str(self.doc, tag)
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned();
        let role = self.role(&tag);
        let index = self.elements.len();
        let above = parent.and_then(|p| self.elements.get(p));
        let in_leaf = above
            .and_then(|a| self.elements.get(a.block))
            .is_some_and(|b| LEAF_BLOCKS.contains(&b.role.as_str()));
        let block = match above {
            Some(above) if in_leaf && !LEAF_BLOCKS.contains(&role.as_str()) => above.block,
            _ => index,
        };
        let text = |key: &[u8]| {
            node.get(key)
                .and_then(|v| pdf_str(self.doc, v))
                .map(|t| t.replace('\0', "").trim().to_owned())
                .filter(|t| !t.is_empty())
        };
        self.elements.push(Element {
            id: index,
            parent,
            tag,
            role,
            block,
            page,
            marks: Vec::new(),
            alt: text(b"Alt"),
            actual_text: text(b"ActualText"),
            title: text(b"T"),
            lang: text(b"Lang"),
            attrs: self.attributes(node.get(b"A")),
        });
        self.node(node.get(b"K"), Some(index), page, depth + 1);
    }

    fn role(&self, tag: &str) -> String {
        let mut tag = tag.to_owned();
        for _ in 0..MAX_ROLE_HOPS {
            match self.role_map.get(&tag) {
                Some(next) if *next != tag => tag.clone_from(next),
                _ => break,
            }
        }
        tag
    }

    /// The known layout attributes of an attribute object or an array of them, as plain values.
    fn attributes(&self, value: Option<&Object>) -> BTreeMap<String, Json> {
        let value = value.and_then(|v| self.doc.resolve(v));
        let groups: Vec<&Object> = match value.map(|v| &v.value) {
            Some(Value::Array(items)) => items.iter().filter_map(|i| self.doc.resolve(i)).collect(),
            _ => value.into_iter().collect(),
        };
        let mut found = BTreeMap::new();
        for group in groups.iter().filter_map(|g| g.dict()) {
            for (key, item) in &group.entries {
                if ATTRIBUTES.contains(&key.as_slice())
                    && let Some(plain) = self.plain(item)
                {
                    found.insert(String::from_utf8_lossy(key).into_owned(), plain);
                }
            }
        }
        found
    }

    /// A number, boolean or name as JSON (a name without its slash), an array of them as an array, a string that
    /// reads as a number as that number; anything else is `None`.
    fn plain(&self, value: &Object) -> Option<Json> {
        match &self.doc.resolve(value)?.value {
            Value::Bool(b) => Some(Json::Bool(*b)),
            Value::Integer(i) => Some(Json::from(*i)),
            Value::Real(r) => Some(Json::from(*r)),
            Value::Name(n) => Some(Json::from(String::from_utf8_lossy(n).into_owned())),
            Value::Array(items) => items
                .iter()
                .map(|i| self.plain(i))
                .collect::<Option<Vec<_>>>()
                .map(Json::from),
            Value::String(..) => pdf_str(self.doc, value)
                .and_then(|t| t.trim().parse::<f64>().ok())
                .filter(|f| f.is_finite())
                .map(Json::from),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    /// The structure of a file of two pages (objects 3 and 4) whose catalog has `catalog`, with objects `more`.
    fn structure(catalog: &str, more: &[(u32, String)]) -> Json {
        let mut objects = vec![
            (1, format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>")),
            (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned()),
            (3, "<< /Type /Page /Parent 2 0 R >>".to_owned()),
            (4, "<< /Type /Page /Parent 2 0 R >>".to_owned()),
        ];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_bytes())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        serde_json::to_value(read(&doc)).unwrap()
    }

    #[test]
    fn elements_roles_blocks_and_marks() {
        let tree = "<< /Type /StructTreeRoot /RoleMap << /Nil null /Heading /H1 /Body /Para /Para /P /Loop /Loop \
            /A1 /A2 /A2 /A3 /A3 /A4 /A4 /A5 /A5 /A6 /A6 /A7 /A7 /A8 /Odd 5 >> /K [10 0 R 11 0 R 10 0 R 99 (text) 12 0 R << /S /Sect /K << /Type /MCR /MCID 8 >> >>] >>";
        let more = [
            (9, tree.to_owned()),
            (10, "<< /S /Heading /Pg 3 0 R /K 0 /Lang (da) /T (Top) >>".to_owned()),
            (12, "[1".to_owned()),
            (
                11,
                "<< /S /Body /Pg 3 0 R /K [1 << /Type /MCR /Pg 4 0 R /MCID 2 >> << /Type /MCR /MCID 3 /Stm 5 0 R >> \
                 << /Type /OBJR /Obj 3 0 R >> << /S /Span /K 4 >> << /S /Figure /K 5 /Alt (Op\\351ra\\000) \
                 /ActualText ( ) /A [<< /TextAlign /Center /BBox [0 1.5 2 3] /Width 7.25 /SpaceBefore (12) \
                 /Placement null /Ignored 1 /Height (tall) /RowSpan [1 (x)] /ColSpan 12 0 R >> 3] >> << /S /Loop >> << /S /A1 >> \
                 << /S /Odd >> << /S /Para /Pg 9 0 R /K [<< /Type /MCR /MCID 6 >>] >>] >>"
                    .to_owned(),
            ),
        ];
        let got = structure("/StructTreeRoot 9 0 R /MarkInfo << /Marked true >> /Lang (en)", &more);
        assert_eq!((&got["marked"], &got["language"]), (&json!(true), &json!("en")));
        assert_eq!(got["role_map"]["Heading"], json!("H1"));
        let summary: Vec<Json> = got["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| json!([e["tag"], e["role"], e["parent"], e["block"], e["page"], e["marks"]]))
            .collect();
        assert_eq!(
            summary,
            [
                json!(["Heading", "H1", null, 0, 1, [[1, 0]]]),
                json!(["Body", "P", null, 1, 1, [[1, 1], [2, 2]]]),
                json!(["Span", "Span", 1, 1, 1, [[1, 4]]]),
                json!(["Figure", "Figure", 1, 3, 1, [[1, 5]]]),
                json!(["Loop", "Loop", 1, 1, 1, []]),
                json!(["A1", "A7", 1, 1, 1, []]),
                json!(["Odd", "5", 1, 1, 1, []]),
                json!(["Para", "P", 1, 7, 1, [[1, 6]]]),
                json!(["Sect", "Sect", null, 8, null, []]),
            ]
        );
        let (heading, figure) = (&got["elements"][0], &got["elements"][3]);
        assert_eq!(
            (&heading["lang"], &heading["title"], &heading["alt"]),
            (&json!("da"), &json!("Top"), &json!(null))
        );
        assert_eq!(
            (&figure["alt"], &figure["actual_text"]),
            (&json!("Opéra"), &json!(null))
        );
        assert_eq!(
            figure["attrs"],
            json!({"TextAlign": "Center", "BBox": [0, 1.5, 2, 3], "Width": 7.25, "SpaceBefore": 12.0})
        );
        // A plain attribute dictionary is read as a one-element list.
        let single = [(
            9,
            "<< /K << /S /P /A << /Scope /Row /RowSpan 2 /ColSpan true >> >> >>".to_owned(),
        )];
        let got = structure("/StructTreeRoot 9 0 R", &single);
        assert_eq!(
            got["elements"][0]["attrs"],
            json!({"Scope": "Row", "RowSpan": 2, "ColSpan": true})
        );
        assert_eq!((&got["marked"], &got["language"]), (&json!(false), &json!(null)));
    }

    #[test]
    fn reading_stops_at_a_bad_mcid_and_at_the_depth_limit() {
        let tree =
            "<< /K [<< /S /P /K [<< /Type /MCR /Pg 3 0 R /MCID 1.9 >> << /Type /MCR /MCID (x) >>] >> << /S /H >>] >>";
        let got = structure("/StructTreeRoot 9 0 R", &[(9, tree.to_owned())]);
        // The real MCID is truncated as Python's int() does (with no page, nothing is marked); the string stops the
        // reading, so /H is never reached.
        assert_eq!(got["elements"].as_array().unwrap().len(), 1);
        // A chain of elements deeper than the limit: the deeper ones are not read.
        let chain: Vec<(u32, String)> = (0..u32::try_from(MAX_DEPTH).unwrap() + 5)
            .map(|i| (20 + i, format!("<< /S /Div /K {} 0 R >>", 21 + i)))
            .collect();
        let got = structure(
            "/StructTreeRoot 9 0 R",
            &[vec![(9, "<< /K 20 0 R >>".to_owned())], chain].concat(),
        );
        assert_eq!(got["elements"].as_array().unwrap().len(), MAX_DEPTH + 1);
        assert_eq!(structure("", &[]), Json::Null);
        assert_eq!(structure("/StructTreeRoot 7", &[]), Json::Null);
    }
}
