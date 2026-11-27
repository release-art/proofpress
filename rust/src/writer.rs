//! The writer: a full rewrite on the object model, with a classic cross-reference table and no object streams
//! (ISO 32000-2 7.5). Objects copied from a document keep their values, and streams keep their bytes and filters,
//! never re-encoded; references are followed through a graft map with an explicit queue, and one to an object the file
//! does not have is written as `null`.

pub mod crop;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::document::{Dict, Document, Object, Value};

/// A new file being written.
pub(crate) struct Writer<'a> {
    /// The document objects are copied from.
    doc: &'a Document,
    /// Each object's body, by its number less one.
    bodies: Vec<Option<Vec<u8>>>,
    /// Each copied object's number in the new file.
    map: BTreeMap<u32, u32>,
    /// Objects mapped and still to copy: their number in the document, their new number, and the object.
    queue: Vec<(u32, u32, &'a Object)>,
    /// Objects a copy refers to that the document could not read, so the copy lacks them.
    pub(crate) damaged: Vec<u32>,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            bodies: Vec::new(),
            map: BTreeMap::new(),
            queue: Vec::new(),
            damaged: Vec::new(),
        }
    }

    /// A new object number, its body set later with [`Self::set`].
    pub(crate) fn reserve(&mut self) -> u32 {
        self.bodies.push(None);
        u32::try_from(self.bodies.len()).unwrap_or(u32::MAX)
    }

    /// The body of object `num`, as written between `obj` and `endobj`.
    pub(crate) fn set(&mut self, num: u32, body: Vec<u8>) {
        if let Some(slot) = num.checked_sub(1).and_then(|i| self.bodies.get_mut(i as usize)) {
            *slot = Some(body);
        }
    }

    /// The new number of the document's object `old`, which is copied when the file is finished; `None` when the
    /// document does not have it.
    fn graft(&mut self, old: u32) -> Option<u32> {
        if let Some(new) = self.map.get(&old) {
            return Some(*new);
        }
        let Some(object) = self.doc.object(old) else {
            if self.doc.is_damaged(old) && !self.damaged.contains(&old) {
                self.damaged.push(old);
            }
            return None;
        };
        let new = self.reserve();
        self.map.insert(old, new);
        self.queue.push((old, new, object));
        Some(new)
    }

    /// `object` as PDF syntax, its references grafted.
    pub(crate) fn value(&mut self, object: &Object) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&object.value, &mut out);
        out
    }

    /// `dict` as PDF syntax, its references grafted.
    pub(crate) fn dict(&mut self, dict: &Dict) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_dict(dict.entries().iter().map(|(k, v)| (k.as_slice(), v)), &mut out);
        out
    }

    fn write(&mut self, value: &Value, out: &mut Vec<u8>) {
        match value {
            Value::Null => out.extend(b"null"),
            Value::Bool(b) => out.extend(if *b { b"true".as_slice() } else { b"false" }),
            Value::Integer(n) => out.extend(n.to_string().as_bytes()),
            Value::Real(r) => out.extend(real(*r).as_bytes()),
            Value::Name(name) => out.extend(self::name(name)),
            Value::String(bytes, _) => out.extend(string(bytes)),
            Value::Array(items) => {
                out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(b' ');
                    }
                    self.write(&item.value, out);
                }
                out.push(b']');
            }
            Value::Dict(dict) => self.write_dict(dict.entries().iter().map(|(k, v)| (k.as_slice(), v)), out),
            Value::Reference(num, _) => match self.graft(*num) {
                Some(new) => out.extend(format!("{new} 0 R").as_bytes()),
                None => out.extend(b"null"),
            },
        }
    }

    fn write_dict<'b>(&mut self, entries: impl Iterator<Item = (&'b [u8], &'b Object)>, out: &mut Vec<u8>) {
        out.extend(b"<<");
        for (key, value) in entries {
            out.extend(name(key));
            out.push(b' ');
            self.write(&value.value, out);
        }
        out.extend(b">>");
    }

    /// Copy the document's object `old`, `object`, into `new`: a stream's dictionary with its length, and its bytes as
    /// they are.
    fn copy(&mut self, old: u32, object: &Object, new: u32) {
        let body = match (self.doc.raw_stream(old), &object.value) {
            (Some(data), Value::Dict(dict)) => {
                let dict = self.without_identity_crypt(dict);
                let entries = dict.entries().iter().filter(|(k, _)| k != b"Length");
                let mut out = Vec::new();
                self.write_dict(entries.map(|(k, v)| (k.as_slice(), v)), &mut out);
                let length = format!("<</Length {}", data.len());
                out.splice(0..2, length.bytes());
                out.extend(b"\nstream\n");
                out.extend(data);
                out.extend(b"\nendstream");
                out
            }
            (_, value) => {
                let mut out = Vec::new();
                self.write(value, &mut out);
                out
            }
        };
        self.set(new, body);
    }

    /// `dict` without its `Identity` `/Crypt` filters (7.4.10), which leave the data as it is. The file is not
    /// encrypted (an encrypted one is refused), and readers do not apply a `/Crypt` filter outside encryption.
    fn without_identity_crypt(&self, dict: &Dict) -> Dict {
        let resolved = |key: &[u8]| dict.get(key).and_then(|v| self.doc.resolve(v)).map(|v| &v.value);
        let one = |o: &Object| vec![o.clone()];
        let (filters, parms) = match (resolved(b"Filter"), resolved(b"DecodeParms")) {
            (Some(Value::Array(f)), Some(Value::Array(p))) => (f.clone(), p.clone()),
            (Some(Value::Array(f)), _) => (f.clone(), Vec::new()),
            (Some(Value::Name(_)), _) => (
                dict.get(b"Filter").map_or_default(one),
                dict.get(b"DecodeParms").map_or_default(one),
            ),
            _ => return dict.clone(),
        };
        let identity = |i: usize| {
            let name = filters.get(i).and_then(|f| self.doc.resolve(f)).and_then(Object::name);
            let parm = parms.get(i).and_then(|p| self.doc.resolve(p)).and_then(Object::dict);
            let crypt_name = parm
                .and_then(|p| p.get(b"Name"))
                .and_then(|n| self.doc.resolve(n))
                .and_then(Object::name);
            name == Some(b"Crypt") && crypt_name.is_none_or(|n| n == b"Identity")
        };
        let kept: Vec<usize> = (0..filters.len()).filter(|i| !identity(*i)).collect();
        if kept.len() == filters.len() {
            return dict.clone();
        }
        let object = |value: Value| Object {
            value,
            start: 0,
            end: 0,
        };
        let pick = |list: &[Object]| -> Vec<Object> { kept.iter().filter_map(|i| list.get(*i).cloned()).collect() };
        let mut out = dict.clone();
        out.retain(|(k, _)| k != b"Filter" && k != b"DecodeParms");
        if !kept.is_empty() {
            out.push(b"Filter".to_vec(), object(Value::Array(pick(&filters))));
            if !parms.is_empty() {
                out.push(b"DecodeParms".to_vec(), object(Value::Array(pick(&parms))));
            }
        }
        out
    }

    /// Copy every grafted object, and those they refer to in turn.
    pub(crate) fn copy_all(&mut self) {
        while let Some((old, new, object)) = self.queue.pop() {
            self.copy(old, object, new);
        }
    }

    /// The finished file: every grafted object copied, then the header, objects, cross-reference table and trailer.
    /// The version is the document's.
    pub(crate) fn finish(mut self, root: u32) -> Vec<u8> {
        self.copy_all();
        let mut out = b"%PDF-".to_vec();
        out.extend(self.doc.version());
        out.extend(b"\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::new();
        for (i, body) in self.bodies.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend(body.as_deref().unwrap_or(b"null"));
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
        for offset in offsets {
            let _ = writeln!(table, "{offset:010} 00000 n ");
        }
        let size = self.bodies.len() + 1;
        let _ = write!(
            table,
            "trailer\n<</Size {size} /Root {root} 0 R>>\nstartxref\n{xref}\n%%EOF\n"
        );
        out.extend(table.as_bytes());
        out
    }
}

/// A real as the shortest text that reads back as the same number, never with an exponent (PDF has none).
pub(crate) fn real(r: f64) -> String {
    let text = format!("{r}");
    if text.contains('.') || text.contains("inf") || text.contains("NaN") {
        text
    } else {
        format!("{text}.0")
    }
}

/// A name, its bytes outside the regular printable range, `#`, and delimiters escaped as `#xx` (7.3.5).
pub(crate) fn name(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'/'];
    for &b in bytes {
        if (b'!'..=b'~').contains(&b) && !b"#()<>[]{}/%".contains(&b) {
            out.push(b);
        } else {
            out.extend(format!("#{b:02X}").as_bytes());
        }
    }
    out
}

/// A string in hexadecimal form, which holds any bytes as they are (7.3.4.3).
fn string(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'<'];
    for b in bytes {
        out.extend(format!("{b:02X}").as_bytes());
    }
    out.push(b'>');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    #[test]
    fn values_are_written_as_pdf_syntax() {
        assert_eq!(
            (real(0.5), real(-3.0), real(1e-7), real(1e20)),
            (
                "0.5".into(),
                "-3.0".into(),
                "0.0000001".into(),
                "100000000000000000000.0".into()
            )
        );
        assert_eq!(name(b"A b#/\x80"), b"/A#20b#23#2F#80");
        assert_eq!(string(b"(\x00)"), b"<280029>");
    }

    #[test]
    fn a_copy_reads_back_the_same() {
        let content = b"<< /Length 5 0 R /Filter /FlateDecode >>\nstream\nxyz\nendstream";
        let objects: [(u32, &[u8]); 5] = [
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /Extra [1 2.5 true false null (s) /N#20m 9 0 R 3 0 R] >>",
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>",
            ),
            (4, content),
            (5, b"3"),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let mut writer = Writer::new(&doc);
        let catalog = doc.catalog().unwrap().clone();
        let root = writer.reserve();
        let body = writer.dict(&catalog);
        writer.set(root, body);
        let spare = writer.reserve();
        // A number that was never reserved is not set.
        writer.set(0, b"x".to_vec());
        writer.set(99, b"x".to_vec());
        let file = writer.finish(root);
        let copy = Document::open(&file, &Limits::default()).unwrap();
        assert!(copy.findings().is_empty(), "{:?}", copy.findings());
        let extra = copy.catalog().unwrap().get(b"Extra").unwrap().array().unwrap();
        let values: Vec<&Value> = extra.iter().map(|o| &o.value).collect();
        assert_eq!(
            &values[..7],
            [
                &Value::Integer(1),
                &Value::Real(2.5),
                &Value::Bool(true),
                &Value::Bool(false),
                &Value::Null,
                &values[5].clone(),
                &Value::Name(b"N m".to_vec())
            ]
        );
        assert_eq!(values[7], &Value::Null, "the missing object 9 is written as null");
        assert_eq!(copy.pages().len(), 1);
        let page_contents = copy.pages()[0].get(b"Contents").map(|c| c.value.clone());
        // The catalog is 1, the page tree 2, the page 3, the spare 4; the contents are grafted while copying, as 5.
        assert_eq!(page_contents, Some(Value::Reference(5, 0)));
        assert_eq!(copy.raw_stream(5), Some(b"xyz".as_slice()));
        assert_eq!(copy.version(), b"1.7");
        let _ = spare;
    }

    #[test]
    fn identity_crypt_filters_are_dropped() {
        let objects: [(u32, &[u8]); 6] = [
            (1, b"<< /Type /Catalog /Pages 2 0 R /S [3 0 R 4 0 R 5 0 R 6 0 R] >>"),
            (2, b"<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, b"<< /Length 1 /Filter /Crypt >>\nstream\na\nendstream"),
            (4, b"<< /Length 1 /Filter [/Crypt /AHx] /DecodeParms [<< /Name /Identity >> null] >>\nstream\na\nendstream"),
            (5, b"<< /Length 1 /Filter [/Crypt] /DecodeParms [<< /Name /Other >>] >>\nstream\na\nendstream"),
            (6, b"<< /Length 1 /Filter [/AHx /Crypt] >>\nstream\na\nendstream"),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let mut writer = Writer::new(&doc);
        let root = writer.reserve();
        let body = writer.dict(&doc.catalog().unwrap().clone());
        writer.set(root, body);
        let file = writer.finish(root);
        let copy = Document::open(&file, &Limits::default()).unwrap();
        let filters: Vec<Option<Value>> = (3..7)
            .map(|n| {
                copy.stream_dict(n)
                    .and_then(|d| d.get(b"Filter"))
                    .map(|f| f.value.clone())
            })
            .collect();
        let name = |n: &[u8]| Object {
            value: Value::Name(n.to_vec()),
            start: 0,
            end: 0,
        };
        assert_eq!(filters[0], None);
        assert!(matches!(&filters[1], Some(Value::Array(a)) if a.len() == 1 && a[0].value == name(b"AHx").value));
        assert!(matches!(&filters[2], Some(Value::Array(a)) if a.len() == 1));
        assert!(matches!(&filters[3], Some(Value::Array(a)) if a.len() == 1 && a[0].value == name(b"AHx").value));
    }
}
