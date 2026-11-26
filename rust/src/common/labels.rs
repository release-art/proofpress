//! Page labels (ISO 32000-2 12.4.2): the catalog's `/PageLabels` number tree of ranges, each with a numbering style
//! (`/S`), a prefix (`/P`) and a first number (`/St`).

use crate::common::text;
use crate::document::{Document, Object, Value};

/// The deepest number tree followed; a deeper or looping one is cut off there.
const DEPTH: usize = 32;

/// Each of `count` pages' label, from the first; a page before the first range, or in a file without labels, has
/// none.
pub(crate) fn labels(doc: &Document, count: usize) -> Vec<Option<String>> {
    let mut ranges = Vec::new();
    if let Some(root) = doc.catalog().and_then(|c| c.get(b"PageLabels")) {
        collect(doc, root, 0, &mut ranges);
    }
    ranges.sort_by_key(|(start, _)| *start);
    (0..count)
        .map(|page| {
            let (start, range) = ranges.iter().rev().find(|(start, _)| *start <= page)?;
            Some(label(doc, range, page - start))
        })
        .collect()
}

/// Every `(first page, range dictionary)` of the number tree at `node`.
fn collect<'a>(doc: &'a Document, node: &'a Object, depth: usize, out: &mut Vec<(usize, &'a Object)>) {
    let Some(dict) = doc.resolve(node).and_then(Object::dict).filter(|_| depth < DEPTH) else {
        return;
    };
    let array = |key: &[u8]| {
        dict.get(key)
            .and_then(|a| doc.resolve(a))
            .and_then(Object::array)
            .unwrap_or_default()
    };
    for pair in array(b"Nums").as_chunks::<2>().0 {
        if let [key, range] = pair
            && let Some(Value::Integer(start)) = doc.resolve(key).map(|k| &k.value)
            && let Ok(start) = usize::try_from(*start)
        {
            out.push((start, range));
        }
    }
    for kid in array(b"Kids") {
        collect(doc, kid, depth + 1, out);
    }
}

/// The label of the page `offset` pages into `range`.
fn label(doc: &Document, range: &Object, offset: usize) -> String {
    let dict = doc.resolve(range).and_then(Object::dict);
    let entry = |key: &[u8]| {
        dict.and_then(|d| d.get(key))
            .and_then(|v| doc.resolve(v))
            .map(|v| &v.value)
    };
    let prefix = match entry(b"P") {
        Some(Value::String(bytes, _)) => text::decode(bytes).unwrap_or_default(),
        _ => String::new(),
    };
    let first = match entry(b"St") {
        Some(Value::Integer(n)) => usize::try_from(*n).unwrap_or(1),
        _ => 1,
    };
    let n = first + offset;
    let number = match entry(b"S").and_then(|s| match s {
        Value::Name(name) => Some(name.as_slice()),
        _ => None,
    }) {
        Some(b"D") => n.to_string(),
        Some(b"R") => roman(n),
        Some(b"r") => roman(n).to_lowercase(),
        Some(b"A") => letters(n),
        Some(b"a") => letters(n).to_lowercase(),
        _ => String::new(),
    };
    prefix + &number
}

/// `n` in upper-case Roman numerals.
fn roman(mut n: usize) -> String {
    const NUMERALS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

/// `n` as 12.4.2's letters: A to Z, then AA to ZZ, then AAA, and so on.
fn letters(n: usize) -> String {
    let Some(index) = n.checked_sub(1) else {
        return String::new();
    };
    let letter = char::from(b'A' + u8::try_from(index % 26).unwrap_or(0));
    letter.to_string().repeat(index / 26 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    #[test]
    fn ranges_number_their_pages() {
        let catalog = b"<< /Pages 2 0 R /PageLabels << /Kids [5 0 R 6 0 R] >> >>";
        let objects: [(u32, &[u8]); 4] = [
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [] /Count 0 >>"),
            (5, b"<< /Nums [0 << /S /r >> 2 << /S /D /P (A-) /St 8 >>] >>"),
            (
                6,
                b"<< /Nums [4 << /S /A /St 26 >> (x) 5 6 << /P (End) >> -1 << >>] /Kids [6 0 R] >>",
            ),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let got: Vec<Option<String>> = labels(&doc, 8);
        let want = ["i", "ii", "A-8", "A-9", "Z", "AA", "End", "End"].map(|l| Some(l.to_owned()));
        assert_eq!(got, want);
        let objects: [(u32, &[u8]); 3] = [
            (1, b"<< /Pages 2 0 R /PageLabels 7 0 R >>"),
            (2, b"<< /Type /Pages /Kids [] /Count 0 >>"),
            (
                7,
                b"<< /Nums [0 << /S /R >> 1 << /S /a /St 2 >> 2 << /S 5 /P (x) >>] >>",
            ),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        assert_eq!(labels(&doc, 3), ["I", "b", "x"].map(|l| Some(l.to_owned())));
        assert_eq!(
            (roman(1994), letters(53), letters(0)),
            ("MCMXCIV".to_owned(), "AAA".to_owned(), String::new())
        );
    }

    #[test]
    fn without_labels_no_page_has_one() {
        let objects: [(u32, &[u8]); 2] = [(1, b"<< /Pages 2 0 R >>"), (2, b"<< /Type /Pages /Kids [] /Count 0 >>")];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        assert_eq!(labels(&doc, 2), [None, None]);
    }
}
