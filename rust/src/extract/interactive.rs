//! What a PDF holds beyond its pages: form fields, links, other annotations, the outline, page labels, optional
//! content groups (layers) and embedded files, read as the Python `proof_pdf._internal.interactive` read them. A
//! malformed entry is skipped; it never costs the rest.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as Json, json};

use super::{Recovered, number, pdf_str, reference};
use crate::document::{Dict, Document, Object, Value};

const BUTTON_RADIO: i64 = 1 << 15;
const BUTTON_PUSH: i64 = 1 << 16;
const CHOICE_COMBO: i64 = 1 << 17;
/// An outline holds at most this many entries; a hostile or broken one can loop or be enormous.
const MAX_OUTLINE: usize = 5000;
/// The deepest name or number tree, or field hierarchy, followed.
const DEPTH: usize = 32;
/// The field attributes a kid inherits from its parent (12.7.4.1, Table 226).
const INHERITED: [&[u8]; 8] = [b"FT", b"V", b"DV", b"Ff", b"DA", b"Opt", b"MaxLen", b"Q"];

/// Everything above, as `document.json`'s `interactive` holds it; an attachment's bytes are a blob.
pub(super) fn collect(doc: &Document, out: &mut Recovered) -> Json {
    let reader = Reader::new(doc);
    let mut attachments = Vec::new();
    for (name, spec) in reader.tree(doc.catalog().and_then(|c| c.get(b"Names")), b"EmbeddedFiles", b"Names") {
        let Some(spec) = doc.resolve(spec).and_then(Object::dict) else {
            continue;
        };
        let text = |key: &[u8]| spec.get(key).and_then(|v| pdf_str(doc, v)).filter(|t| !t.is_empty());
        let files = spec.get(b"EF").and_then(|e| doc.resolve(e)).and_then(Object::dict);
        let Some(stream) = files.and_then(|f| f.get(b"F").or_else(|| f.get(b"UF"))) else {
            continue;
        };
        let data = reference(stream).and_then(|n| doc.stream_data(n));
        let what = format!("attachment {name}");
        let blob = out.blob(data, &what);
        attachments.push(json!({
            "name": text(b"UF").or_else(|| text(b"F")).unwrap_or(name),
            "data": blob,
            "description": text(b"Desc"),
        }));
    }
    json!({
        "fields": reader.fields(),
        "links": reader.links(),
        "annotations": reader.annotations(),
        "outline": reader.outline(),
        "page_labels": reader.page_labels(),
        "layers": reader.layers(),
        "attachments": attachments,
    })
}

struct Reader<'a> {
    doc: &'a Document,
    /// Page object numbers to page numbers from 1.
    pages: BTreeMap<u32, usize>,
    /// Named destinations, from the catalog's `/Dests` and the `/Dests` name tree.
    names: BTreeMap<String, &'a Object>,
    /// Each page's annotations, in page order.
    annotations: Vec<(usize, Option<u32>, &'a Dict)>,
}

impl<'a> Reader<'a> {
    fn new(doc: &'a Document) -> Self {
        let pages: Vec<_> = doc.pages();
        let mut annotations = Vec::new();
        for (page, number) in pages.iter().zip(1..) {
            let list = page.get(b"Annots").and_then(|a| doc.resolve(a)).and_then(Object::array);
            for annotation in list.unwrap_or_default() {
                if let Some(dict) = doc.resolve(annotation).and_then(Object::dict) {
                    annotations.push((number, reference(annotation), dict));
                }
            }
        }
        let mut reader = Self {
            doc,
            pages: pages
                .iter()
                .zip(1..)
                .filter(|(p, _)| p.num != 0)
                .map(|(p, n)| (p.num, n))
                .collect(),
            names: BTreeMap::new(),
            annotations,
        };
        let catalog = doc.catalog();
        if let Some(dests) = catalog
            .and_then(|c| c.get(b"Dests"))
            .and_then(|d| doc.resolve(d))
            .and_then(Object::dict)
        {
            for (key, value) in &dests.entries {
                reader.names.insert(String::from_utf8_lossy(key).into_owned(), value);
            }
        }
        for (key, value) in reader.tree(catalog.and_then(|c| c.get(b"Names")), b"Dests", b"Names") {
            reader.names.insert(key, value);
        }
        reader
    }

    /// The leaves of the tree under `key` of the dictionary `holder`, sorted by key: a name tree (`Names`, keys as
    /// text) or a number tree (`Nums`, keys as numbers written out).
    fn tree(&self, holder: Option<&'a Object>, key: &[u8], leaves: &[u8]) -> Vec<(String, &'a Object)> {
        let doc = self.doc;
        let root = holder
            .and_then(|h| doc.resolve(h))
            .and_then(Object::dict)
            .and_then(|h| h.get(key));
        let mut out = Vec::new();
        let mut stack: Vec<(&Object, usize)> = root.map(|r| (r, 0)).into_iter().collect();
        let mut seen = BTreeSet::new();
        while let Some((node, depth)) = stack.pop() {
            if depth > DEPTH || reference(node).is_some_and(|n| !seen.insert(n)) {
                continue;
            }
            let Some(dict) = doc.resolve(node).and_then(Object::dict) else {
                continue;
            };
            let list = |k: &[u8]| {
                dict.get(k)
                    .and_then(|a| doc.resolve(a))
                    .and_then(Object::array)
                    .unwrap_or_default()
            };
            out.extend(
                list(leaves)
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter_map(|[k, v]| Some((pdf_str(doc, k)?, v))),
            );
            stack.extend(list(b"Kids").iter().map(|kid| (kid, depth + 1)));
        }
        if leaves == b"Nums" {
            out.sort_by_key(|(k, _)| k.parse::<i64>().unwrap_or(i64::MAX));
        } else {
            out.sort_by(|a, b| a.0.cmp(&b.0));
        }
        // A key written twice is one entry, the last read, as a dictionary of them keeps it.
        out.reverse();
        out.dedup_by(|a, b| a.0 == b.0);
        out.reverse();
        out
    }

    fn text(&self, dict: &Dict, key: &[u8]) -> String {
        dict.get(key).and_then(|v| pdf_str(self.doc, v)).unwrap_or_default()
    }

    /// Where a destination points: explicit, named, or a dictionary holding `/D`.
    fn target(&self, dest: Option<&Object>) -> Json {
        let doc = self.doc;
        let mut dest = dest.and_then(|d| doc.resolve(d));
        if let Some(Value::Name(_) | Value::String(..)) = dest.map(|d| &d.value) {
            let key = dest.and_then(|d| pdf_str(doc, d)).unwrap_or_default();
            dest = self.names.get(key.trim_start_matches('/')).and_then(|d| doc.resolve(d));
        }
        if let Some(inner) = dest.and_then(Object::dict) {
            dest = inner.get(b"D").and_then(|d| doc.resolve(d));
        }
        let Some([first, rest @ ..]) = dest.and_then(Object::array) else {
            return Json::Null;
        };
        let page = reference(first).and_then(|n| self.pages.get(&n));
        let view = rest
            .first()
            .and_then(|v| pdf_str(doc, v))
            .map(|v| v.trim_start_matches('/').to_owned());
        let at: Vec<f64> = rest.iter().skip(1).filter_map(|v| number(doc, v)).collect();
        json!({"page": page, "view": view, "at": if at.is_empty() { None } else { Some(at) }})
    }

    /// What clicking `annotation` (or an outline entry) does: its action, or its destination.
    fn action(&self, annotation: &Dict) -> BTreeMap<&'static str, Json> {
        let doc = self.doc;
        let Some(act) = annotation.get(b"A").and_then(|a| doc.resolve(a)).and_then(Object::dict) else {
            return match annotation.get(b"Dest") {
                Some(dest) => BTreeMap::from([("to", self.target(Some(dest)))]),
                None => BTreeMap::new(),
            };
        };
        let kind = self.text(act, b"S").trim_start_matches('/').to_owned();
        let mut out = BTreeMap::from([("action", json!(kind))]);
        match kind.as_str() {
            "URI" => {
                out.insert("uri", json!(self.text(act, b"URI")));
            }
            "GoTo" => {
                out.insert("to", self.target(act.get(b"D")));
            }
            "GoToR" | "Launch" => {
                let spec = act.get(b"F").and_then(|f| doc.resolve(f));
                let file = match spec.and_then(Object::dict) {
                    Some(spec) => [b"UF".as_slice(), b"F"]
                        .iter()
                        .find_map(|k| spec.get(k).and_then(|v| pdf_str(doc, v))),
                    None => spec.and_then(|s| pdf_str(doc, s)),
                };
                out.insert("file", json!(file.unwrap_or_default()));
            }
            _ => {}
        }
        out
    }

    fn subtype(annotation: &Dict) -> Option<&[u8]> {
        annotation.get(b"Subtype").and_then(Object::name)
    }

    fn links(&self) -> Vec<Json> {
        self.annotations
            .iter()
            .filter(|(_, _, a)| Self::subtype(a) == Some(b"Link"))
            .map(|(page, _, a)| {
                let mut link =
                    serde_json::Map::from_iter([("page".to_owned(), json!(page)), ("rect".to_owned(), self.rect(a))]);
                link.extend(self.action(a).into_iter().map(|(k, v)| (k.to_owned(), v)));
                Json::Object(link)
            })
            .collect()
    }

    /// Notes, highlights, stamps and the like: links and form widgets have their own lists.
    fn annotations(&self) -> Vec<Json> {
        let optional = |t: String| if t.is_empty() { None } else { Some(t) };
        self.annotations
            .iter()
            .filter(|(_, _, a)| !matches!(Self::subtype(a), Some(b"Link" | b"Widget" | b"Popup")))
            .map(|(page, _, a)| {
                json!({
                    "page": page,
                    "type": self.text(a, b"Subtype").trim_start_matches('/'),
                    "rect": self.rect(a),
                    "contents": optional(self.text(a, b"Contents")),
                    "author": optional(self.text(a, b"T")),
                })
            })
            .collect()
    }

    fn rect(&self, annotation: &Dict) -> Json {
        let corners: Option<Vec<f64>> = annotation
            .get(b"Rect")
            .and_then(|r| self.doc.resolve(r))
            .and_then(Object::array)
            .and_then(|r| r.iter().map(|v| number(self.doc, v)).collect());
        match corners.as_deref() {
            Some(&[x0, y0, x1, y1]) => json!([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]),
            _ => Json::Null,
        }
    }

    fn fields(&self) -> Vec<Json> {
        let doc = self.doc;
        let Some(form) = doc
            .catalog()
            .and_then(|c| c.get(b"AcroForm"))
            .and_then(|f| doc.resolve(f))
            .and_then(Object::dict)
        else {
            return Vec::new();
        };
        let widget_pages: BTreeMap<u32, usize> = self
            .annotations
            .iter()
            .filter(|(_, _, a)| Self::subtype(a) == Some(b"Widget"))
            .filter_map(|(page, num, _)| Some(((*num)?, *page)))
            .collect();
        let mut found = Vec::new();
        let tops = form
            .get(b"Fields")
            .and_then(|f| doc.resolve(f))
            .and_then(Object::array)
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        for top in tops {
            self.field(
                top,
                "",
                &BTreeMap::new(),
                &widget_pages,
                form.get(b"DA"),
                &mut seen,
                0,
                &mut found,
            );
        }
        found
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the field walk's context, as the Python passed it down"
    )]
    fn field(
        &self,
        node: &'a Object,
        prefix: &str,
        inherited: &BTreeMap<&'static [u8], &'a Object>,
        widget_pages: &BTreeMap<u32, usize>,
        default_da: Option<&'a Object>,
        seen: &mut BTreeSet<u32>,
        depth: usize,
        found: &mut Vec<Json>,
    ) {
        let doc = self.doc;
        let Some(dict) = doc.resolve(node).and_then(Object::dict) else {
            return;
        };
        if depth > DEPTH || reference(node).is_some_and(|n| !seen.insert(n)) {
            return;
        }
        let mut inherited = inherited.clone();
        for key in INHERITED {
            if let Some(value) = dict.get(key) {
                inherited.insert(key, value);
            }
        }
        let name = match dict.get(b"T").and_then(|t| pdf_str(doc, t)) {
            Some(t) => format!("{prefix}.{t}").trim_start_matches('.').to_owned(),
            None => prefix.to_owned(),
        };
        let kids = dict
            .get(b"Kids")
            .and_then(|k| doc.resolve(k))
            .and_then(Object::array)
            .unwrap_or_default();
        let widgets: Vec<&Object> = kids
            .iter()
            .filter(|k| {
                doc.resolve(k)
                    .and_then(Object::dict)
                    .is_some_and(|d| d.get(b"T").is_none() && Self::subtype(d) == Some(b"Widget"))
            })
            .collect();
        if !kids.is_empty() && widgets.is_empty() {
            for kid in kids {
                self.field(kid, &name, &inherited, widget_pages, default_da, seen, depth + 1, found);
            }
            return;
        }
        let shown: Vec<&Object> = if widgets.is_empty() { vec![node] } else { widgets };
        for widget in shown {
            let page = reference(widget).and_then(|n| widget_pages.get(&n));
            found.push(self.describe(&name, &inherited, widget, page.copied(), dict, default_da));
        }
    }

    fn describe(
        &self,
        name: &str,
        inherited: &BTreeMap<&'static [u8], &'a Object>,
        widget: &Object,
        page: Option<usize>,
        node: &Dict,
        default_da: Option<&Object>,
    ) -> Json {
        let doc = self.doc;
        let get = |key: &[u8]| inherited.get(key).copied();
        let integer = |key: &[u8]| get(key).and_then(|v| int(doc, v));
        let kind = get(b"FT")
            .and_then(|v| pdf_str(doc, v))
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned();
        let flags = integer(b"Ff").unwrap_or(0);
        let shown = match kind.as_str() {
            "Tx" => "text".to_owned(),
            "Sig" => "signature".to_owned(),
            "Ch" if flags & CHOICE_COMBO != 0 => "dropdown".to_owned(),
            "Ch" => "list".to_owned(),
            "Btn" if flags & BUTTON_RADIO != 0 => "radio".to_owned(),
            "Btn" if flags & BUTTON_PUSH != 0 => "button".to_owned(),
            "Btn" => "checkbox".to_owned(),
            "" => "unknown".to_owned(),
            other => other.to_owned(),
        };
        let appearance = get(b"DA")
            .or(default_da)
            .and_then(|d| pdf_str(doc, d))
            .and_then(|da| font_of(&da));
        let options: Vec<Json> = get(b"Opt")
            .and_then(|o| doc.resolve(o))
            .and_then(Object::array)
            .unwrap_or_default()
            .iter()
            .map(|o| match doc.resolve(o).and_then(Object::array) {
                Some(pair) => self.value(pair.get(1)),
                None => self.value(Some(o)),
            })
            .collect();
        let tooltip = node.get(b"TU").and_then(|t| pdf_str(doc, t)).filter(|t| !t.is_empty());
        json!({
            "name": name,
            "type": shown,
            "page": page,
            "rect": doc.resolve(widget).and_then(Object::dict).map_or(Json::Null, |w| self.rect(w)),
            "value": if kind == "Sig" { self.signature(get(b"V")) } else { self.value(get(b"V")) },
            "default": self.value(get(b"DV")),
            "options": if options.is_empty() { None } else { Some(options) },
            "max_length": integer(b"MaxLen"),
            "alignment": match integer(b"Q").unwrap_or(0) { 0 => Some("left"), 1 => Some("center"), 2 => Some("right"), _ => None },
            "font": appearance.as_ref().map(|(font, _)| font),
            "font_size_pt": appearance.map(|(_, size)| size),
            "tooltip": tooltip,
            "required": flags & 2 != 0,
            "read_only": flags & 1 != 0,
        })
    }

    /// A field's value as text: a name without its slash; a dictionary, stream or array is left out.
    fn value(&self, value: Option<&Object>) -> Json {
        match value.and_then(|v| self.doc.resolve(v)).map(|v| &v.value) {
            None | Some(Value::Null | Value::Dict(_) | Value::Array(_)) => Json::Null,
            Some(Value::Name(n)) => json!(String::from_utf8_lossy(n)),
            Some(_) => json!(value.and_then(|v| pdf_str(self.doc, v))),
        }
    }

    /// The facts of a digital signature (who, when, how); its contents are not copied.
    fn signature(&self, value: Option<&Object>) -> Json {
        let Some(sig) = value.and_then(|v| self.doc.resolve(v)).and_then(Object::dict) else {
            return Json::Null;
        };
        let ranges: Option<Vec<i64>> = sig
            .get(b"ByteRange")
            .and_then(|r| self.doc.resolve(r))
            .and_then(Object::array)
            .map(|r| r.iter().filter_map(|v| int(self.doc, v)).collect());
        json!({
            "signer": self.value(sig.get(b"Name")),
            "date": self.value(sig.get(b"M")),
            "reason": self.value(sig.get(b"Reason")),
            "location": self.value(sig.get(b"Location")),
            "filter": self.value(sig.get(b"Filter")),
            "sub_filter": self.value(sig.get(b"SubFilter")),
            "byte_range": ranges,
        })
    }

    fn outline(&self) -> Vec<Json> {
        let doc = self.doc;
        let mut entries = Vec::new();
        let root = doc
            .catalog()
            .and_then(|c| c.get(b"Outlines"))
            .and_then(|o| doc.resolve(o))
            .and_then(Object::dict);
        let Some(root) = root else { return entries };
        let mut budget = MAX_OUTLINE;
        // Depth-first, each entry before its children and its children before its next sibling.
        let mut stack: Vec<(&Object, usize)> = root.get(b"First").map(|f| (f, 1)).into_iter().collect();
        while let Some((item, level)) = stack.pop() {
            let Some(dict) = doc.resolve(item).and_then(Object::dict) else {
                continue;
            };
            if budget == 0 {
                break;
            }
            budget -= 1;
            let mut entry = serde_json::Map::from_iter([
                ("level".to_owned(), json!(level)),
                ("title".to_owned(), json!(self.text(dict, b"Title"))),
            ]);
            if dict.get(b"A").is_some() {
                entry.extend(self.action(dict).into_iter().map(|(k, v)| (k.to_owned(), v)));
            } else {
                entry.insert("to".to_owned(), self.target(dict.get(b"Dest")));
            }
            entries.push(Json::Object(entry));
            if let Some(next) = dict.get(b"Next") {
                stack.push((next, level));
            }
            if let Some(first) = dict.get(b"First") {
                stack.push((first, level + 1));
            }
        }
        entries
    }

    fn page_labels(&self) -> Vec<Json> {
        let doc = self.doc;
        let catalog = doc.trailer().get(b"Root");
        self.tree(catalog, b"PageLabels", b"Nums")
            .into_iter()
            .filter_map(|(index, label)| {
                let label = doc.resolve(label).and_then(Object::dict)?;
                let style = self.text(label, b"S").trim_start_matches('/').to_owned();
                let start = label.get(b"St").and_then(|s| int(doc, s)).unwrap_or(1);
                Some(json!({
                    "from_page": index.parse::<i64>().ok()? + 1,
                    "style": if style.is_empty() { None } else { Some(style) },
                    "prefix": self.text(label, b"P"),
                    "start": start,
                }))
            })
            .collect()
    }

    fn layers(&self) -> Vec<Json> {
        let doc = self.doc;
        let properties = doc
            .catalog()
            .and_then(|c| c.get(b"OCProperties"))
            .and_then(|p| doc.resolve(p))
            .and_then(Object::dict);
        let Some(properties) = properties else {
            return Vec::new();
        };
        let list = |dict: Option<&'a Dict>, key: &[u8]| {
            dict.and_then(|d| d.get(key))
                .and_then(|a| doc.resolve(a))
                .and_then(Object::array)
                .unwrap_or_default()
        };
        let config = properties.get(b"D").and_then(|d| doc.resolve(d)).and_then(Object::dict);
        let hidden: BTreeSet<u32> = list(config, b"OFF").iter().filter_map(reference).collect();
        list(Some(properties), b"OCGs")
            .iter()
            .filter_map(|group| {
                let dict = doc.resolve(group).and_then(Object::dict)?;
                Some(json!({
                    "name": self.text(dict, b"Name"),
                    "visible": reference(group).is_none_or(|n| !hidden.contains(&n)),
                }))
            })
            .collect()
    }
}

/// `object` as Python's `int()` reads a PDF number: an integer as it is, a real truncated.
#[expect(clippy::cast_possible_truncation, reason = "as Python's int() of a PDF real")]
fn int(doc: &Document, object: &Object) -> Option<i64> {
    match doc.resolve(object)?.value {
        Value::Integer(i) => Some(i),
        Value::Real(r) => Some(r as i64),
        _ => None,
    }
}

/// The font name and size a default appearance string sets: its first `/Name size Tf`.
fn font_of(da: &str) -> Option<(String, f64)> {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    tokens.windows(3).find_map(|w| match w {
        [name, size, "Tf"] if name.len() > 1 && size.chars().all(|c| c.is_ascii_digit() || c == '.') => {
            Some((name.strip_prefix('/')?.to_owned(), size.parse().ok()?))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn the_font_a_default_appearance_sets() {
        // As the Python read it: the first slash starts the name, so a name written with two keeps one.
        assert_eq!(font_of("xy 3 Tf //Helv 9 Tf"), Some(("/Helv".to_owned(), 9.0)));
    }

    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    fn read(catalog: &str, page_one: &str, more: &[(u32, &str)]) -> (Json, Recovered) {
        let one = format!("<< /Type /Page /Parent 2 0 R {page_one} >>");
        let mut objects: Vec<(u32, String)> = vec![
            (1, format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>")),
            (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned()),
            (3, one),
            (4, "<< /Type /Page /Parent 2 0 R >>".to_owned()),
        ];
        objects.extend(more.iter().map(|(n, s)| (*n, (*s).to_owned())));
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_bytes())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let mut out = Recovered::default();
        let got = collect(&doc, &mut out);
        (got, out)
    }

    #[test]
    fn links_annotations_and_destinations() {
        let annots = "/Annots [10 0 R 11 0 R 12 0 R 13 0 R 14 0 R 15 0 R 16 0 R 17 0 R null 18 0 R 19 0 R 20 0 R]";
        let catalog = "/Dests << /Intro [4 0 R /Fit] >> /Names << /Dests << /Kids [30 0 R 30 0 R] >> >>";
        let more = [
            (
                10,
                "<< /Subtype /Link /Rect [10 20 0 5] /A << /S /URI /URI (https://example.com) >> >>",
            ),
            (11, "<< /Subtype /Link /Dest /Intro >>"),
            (12, "<< /Subtype /Link /Dest (Chapter) >>"),
            (
                13,
                "<< /Subtype /Link /A << /S /GoTo /D [3 0 R /XYZ 54.1 784.5 null] >> >>",
            ),
            (14, "<< /Subtype /Link /A << /S /GoToR /F << /F (other.pdf) >> >> >>"),
            (15, "<< /Subtype /Link /A << /S /Launch /F (run.sh) >> /Rect [1 2 3] >>"),
            (16, "<< /Subtype /Link /A << /S /JavaScript >> >>"),
            (17, "<< /Subtype /Text /Rect [0 0 1 1] /Contents (Note) /T (Ana) >>"),
            (20, "<< /Subtype /Square >>"),
            (18, "<< /Subtype /Popup >>"),
            (
                19,
                "<< /Subtype /Link /Dest << /D [0 /Fit] >> /A << /S /GoToR /F 5 >> >>",
            ),
            (30, "<< /Names [(Chapter) << /D [4 0 R /FitH 700] >> (Gone) 9] >>"),
        ];
        let (got, _) = read(catalog, annots, &more);
        assert_eq!(
            got["links"],
            json!([
                {"page": 1, "rect": [0.0, 5.0, 10.0, 20.0], "action": "URI", "uri": "https://example.com"},
                {"page": 1, "rect": null, "to": {"page": 2, "view": "Fit", "at": null}},
                {"page": 1, "rect": null, "to": {"page": 2, "view": "FitH", "at": [700.0]}},
                {"page": 1, "rect": null, "action": "GoTo", "to": {"page": 1, "view": "XYZ", "at": [54.1, 784.5]}},
                {"page": 1, "rect": null, "action": "GoToR", "file": "other.pdf"},
                {"page": 1, "rect": null, "action": "Launch", "file": "run.sh"},
                {"page": 1, "rect": null, "action": "JavaScript"},
                {"page": 1, "rect": null, "action": "GoToR", "file": "5"},
            ])
        );
        assert_eq!(
            got["annotations"],
            json!([
                {"page": 1, "type": "Text", "rect": [0.0, 0.0, 1.0, 1.0], "contents": "Note", "author": "Ana"},
                {"page": 1, "type": "Square", "rect": null, "contents": null, "author": null},
            ])
        );
        // A link with neither action nor destination says nothing more; a destination that is not an array is none.
        let (got, _) = read(
            "",
            "/Annots [10 0 R 11 0 R]",
            &[(10, "<< /Subtype /Link >>"), (11, "<< /Subtype /Link /Dest 5 >>")],
        );
        assert_eq!(
            got["links"],
            json!([{"page": 1, "rect": null}, {"page": 1, "rect": null, "to": null}])
        );
    }

    #[test]
    fn form_fields() {
        let catalog = "/AcroForm << /DA (/Helv 9 Tf 0 g) /Fields [20 0 R 21 0 R 24 0 R 25 0 R 26 0 R 27 0 R 28 0 R 5 30 0 R 31 0 R] >>";
        let more = [
            (
                20,
                "<< /T (name) /FT /Tx /V (Ana) /DV /Default /Ff 3 /MaxLen 10 /Q 1 /DA (0 g /Cour 12.5 Tf) \
                 /TU (Your name) /Subtype /Widget /Rect [0 0 100 20] >>",
            ),
            (21, "<< /T (choice) /FT /Btn /Ff 32768 /V /On /Kids [22 0 R 23 0 R] >>"),
            (22, "<< /Subtype /Widget /Rect [0 0 1 1] >>"),
            (23, "<< /Subtype /Widget >>"),
            (
                24,
                "<< /T (size) /FT /Ch /Ff 131072 /Opt [(S) [(m) (Medium)]] /V [1 2] /Q 5 >>",
            ),
            (
                25,
                "<< /T (sig) /FT /Sig /V << /Name (A. Signer) /M (D:2026) /ByteRange [0 10 20 30] >> >>",
            ),
            (26, "<< /T (group) /Kids [29 0 R 26 0 R] /FT /Btn /Ff 65536 >>"),
            (27, "<< /T (odd) /FT /Zz /DA (none) >>"),
            (28, "<< /T (bare) /V << >> /DV 3 >>"),
            (29, "<< /T (push) /Kids [26 0 R] >>"),
            (30, "<< /T (unsigned) /FT /Sig /Q 2 /MaxLen 7.9 /DA (/Helv 1.2.3 Tf) >>"),
            (
                31,
                "<< /T (odd numbers) /FT /Tx /Ff (x) /Q 32 0 R /V << /ByteRange [0 2.5 (x) 32 0 R] >> /FT /Sig >>",
            ),
            (32, "[1"),
        ];
        let (got, _) = read(catalog, "/Annots [20 0 R 22 0 R << /Subtype /Widget >>]", &more);
        let fields = got["fields"].as_array().unwrap();
        let summary: Vec<Json> = fields
            .iter()
            .map(|f| {
                json!([
                    f["name"],
                    f["type"],
                    f["page"],
                    f["value"],
                    f["font"],
                    f["font_size_pt"],
                    f["alignment"]
                ])
            })
            .collect();
        assert_eq!(
            summary,
            [
                json!(["name", "text", 1, "Ana", "Cour", 12.5, "center"]),
                json!(["choice", "radio", 1, "On", "Helv", 9.0, "left"]),
                json!(["choice", "radio", null, "On", "Helv", 9.0, "left"]),
                json!(["size", "dropdown", null, null, "Helv", 9.0, null]),
                json!(["sig", "signature", null, {"signer": "A. Signer", "date": "D:2026", "reason": null,
                    "location": null, "filter": null, "sub_filter": null, "byte_range": [0, 10, 20, 30]},
                    "Helv", 9.0, "left"]),
                json!(["odd", "Zz", null, null, null, null, "left"]),
                json!(["bare", "unknown", null, null, "Helv", 9.0, "left"]),
                json!(["unsigned", "signature", null, null, null, null, "right"]),
                json!(["odd numbers", "signature", null, {"signer": null, "date": null, "reason": null,
                    "location": null, "filter": null, "sub_filter": null, "byte_range": [0, 2]},
                    "Helv", 9.0, "left"]),
            ]
        );
        let name = &fields[0];
        assert_eq!(
            [
                &name["default"],
                &name["max_length"],
                &name["tooltip"],
                &name["required"],
                &name["read_only"],
                &name["rect"]
            ],
            [
                &json!("Default"),
                &json!(10),
                &json!("Your name"),
                &json!(true),
                &json!(true),
                &json!([0.0, 0.0, 100.0, 20.0])
            ]
        );
        assert_eq!(fields[3]["options"], json!(["S", "Medium"]));
        assert_eq!(fields[7]["max_length"], json!(7));
        assert_eq!(
            (&fields[6]["default"], &fields[6]["options"]),
            (&json!("3"), &json!(null))
        );
        // A list without combo is a list; a push button and a checkbox by their flags.
        let kinds = |dict: &str| {
            let (got, _) = read("/AcroForm << /Fields [20 0 R] >>", "", &[(20, dict)]);
            got["fields"][0]["type"].clone()
        };
        assert_eq!(kinds("<< /FT /Ch >>"), json!("list"));
        assert_eq!(kinds("<< /FT /Btn /Ff 65536 >>"), json!("button"));
        assert_eq!(kinds("<< /FT /Btn >>"), json!("checkbox"));
        assert_eq!(read("/AcroForm 5", "", &[]).0["fields"], json!([]));
    }

    #[test]
    fn outline_labels_layers_and_attachments() {
        let catalog = "/Outlines << /First 40 0 R >> /PageLabels << /Kids [<< /Nums [2 << /S /D /P (A-) /St 5.7 >> 9 7 (x) << >> 4 << >>] >> \
            << /Nums [0 << /S /a >> 0 << /S /r >>] >> 5 7 0 R] >> /OCProperties << /OCGs [50 0 R 51 0 R 5] /D << /OFF [51 0 R] >> >> \
            /Names << /EmbeddedFiles << /Names [(a) << /UF (dir/notes.txt) /EF << /F 60 0 R >> /Desc (Notes) >> \
            (b) << /F (raw.bin) >> (c) << /EF << /F << >> >> >> (d) 7 (e) << /EF << /UF 60 0 R >> >> (f) null null 7] >> >>";
        let more = [
            (40, "<< /Title (One) /Dest [3 0 R /Fit] /First 41 0 R /Next 42 0 R >>"),
            (41, "<< /Title (One.a) /A << /S /URI /URI (u) >> >>"),
            (42, "<< /Title (Two) /Next 43 0 R >>"),
            (43, "7"),
            (50, "<< /Name (Shown) >>"),
            (51, "<< /Name (Hidden) >>"),
            (60, "<< /Length 5 >>\nstream\nhello\nendstream"),
        ];
        let (got, out) = read(catalog, "", &more);
        assert_eq!(
            got["outline"],
            json!([
                {"level": 1, "title": "One", "to": {"page": 1, "view": "Fit", "at": null}},
                {"level": 2, "title": "One.a", "action": "URI", "uri": "u"},
                {"level": 1, "title": "Two", "to": null},
            ])
        );
        assert_eq!(
            got["page_labels"],
            json!([
                {"from_page": 1, "style": "r", "prefix": "", "start": 1},
                {"from_page": 3, "style": "D", "prefix": "A-", "start": 5},
                {"from_page": 5, "style": null, "prefix": "", "start": 1},
            ])
        );
        assert_eq!(
            got["layers"],
            json!([{"name": "Shown", "visible": true}, {"name": "Hidden", "visible": false}])
        );
        assert_eq!(
            got["attachments"],
            json!([
                {"name": "dir/notes.txt", "data": {"data": 0}, "description": "Notes"},
                {"name": "c", "data": {"error": "attachment c cannot be read"}, "description": null},
                {"name": "e", "data": {"data": 1}, "description": null},
            ])
        );
        assert_eq!(out.blobs, [b"hello".to_vec(), b"hello".to_vec()]);
        // Nothing of it in an empty file; an outline past its budget stops there.
        let (empty, _) = read("", "", &[]);
        assert_eq!(
            empty,
            json!({"fields": [], "links": [], "annotations": [], "outline": [], "page_labels": [],
            "layers": [], "attachments": []})
        );
        let looping = [(40, "<< /Title (Again) /Next 40 0 R >>")];
        assert_eq!(
            read("/Outlines << /First 40 0 R >>", "", &looping).0["outline"]
                .as_array()
                .unwrap()
                .len(),
            MAX_OUTLINE
        );
    }
}
