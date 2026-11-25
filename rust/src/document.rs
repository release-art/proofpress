//! Tier 5: objects on request, filled at open and then frozen, and the page tree (ISO 32000-2, 7.7).
//!
//! [`Document::open`] reads every object reachable from the trailer, once, in a fixed order, into a table that is not
//! changed afterwards. A damaged object is a gap the rest of the walk goes around; a reference to an object that does not
//! exist reads as null (7.3.10), and the walk records why.

use std::collections::{BTreeMap, BTreeSet};

use crate::file::{self, Entry, ObjectDamage, Refusal, Xref};
use crate::filters::Decoded;
use crate::filters::Filter;
use crate::findings::{Finding, Location, OBJECT_DANGLING_REFERENCE, STREAM_FILTER};
use crate::pipeline::Limits;
use crate::syntax::{self, Mode, Note, Parser, Read};
pub(crate) use crate::syntax::{Dict, Object, Value};

/// Hand-built PDFs for the tests of the tiers above.
#[cfg(test)]
pub(crate) use crate::file::test_pdf;

/// The buffer a value's span is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buffer {
    /// The file itself.
    File,
    /// The decoded data of this object stream.
    ObjectStream(u32),
}

/// One object as the walk read it.
#[derive(Debug, PartialEq)]
pub(crate) enum Slot {
    /// Read: its value, its stream data's range for a stream, and the buffer both are in.
    Loaded {
        object: Object,
        stream: Option<(usize, usize)>,
        buffer: Buffer,
    },
    /// Damaged: a gap.
    Damaged(ObjectDamage),
}

/// Why a reference reads as null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dangling {
    /// No cross-reference section lists the object.
    NotListed,
    /// The object number is beyond the trailer's `/Size`.
    BeyondSize,
    /// The object is listed as free.
    Free,
    /// The object exists with another generation.
    Generation(u16),
}

/// An object stream's decoded data, and its `/N` and `/First`.
struct ObjectStream {
    data: Vec<u8>,
    count: usize,
    first: usize,
}

/// An open document: the file's cross-reference and every object reachable from its trailer.
pub struct Document {
    file: Vec<u8>,
    xref: Xref,
    slots: BTreeMap<u32, Slot>,
    object_streams: BTreeMap<u32, Result<ObjectStream, ObjectDamage>>,
    notes: Vec<(Buffer, Note)>,
    dangling: Vec<(u32, u16, Dangling)>,
}

impl Document {
    /// Open `bytes`, reading everything reachable.
    ///
    /// # Errors
    ///
    /// The finding that says why the file cannot be read at all: file-level damage or encryption.
    pub fn open(bytes: &[u8], limits: &Limits) -> Result<Self, Box<Finding>> {
        let xref = file::open(bytes, limits).map_err(|r: Refusal| Box::new(file::refusal_finding(&r)))?;
        let mut doc = Self {
            file: bytes.to_vec(),
            xref,
            slots: BTreeMap::new(),
            object_streams: BTreeMap::new(),
            notes: Vec::new(),
            dangling: Vec::new(),
        };
        let mut stack: Vec<(u32, u16)> = Vec::new();
        for (_, value) in doc.xref.trailer.entries.iter().rev() {
            references(value, &mut stack);
        }
        let mut seen = BTreeSet::new();
        while let Some((num, generation)) = stack.pop() {
            if !seen.insert(num) {
                continue;
            }
            if let Some(slot) = doc.load(num, generation, limits) {
                if let Slot::Loaded { object, .. } = &slot {
                    let before = stack.len();
                    references(object, &mut stack);
                    stack.get_mut(before..).unwrap_or_default().reverse();
                }
                doc.slots.insert(num, slot);
            }
        }
        Ok(doc)
    }

    /// The document catalog, when `/Root` names a dictionary that could be read.
    pub(crate) fn catalog(&self) -> Option<&Dict> {
        self.xref
            .trailer
            .get(b"Root")
            .and_then(|root| self.resolve(root))
            .and_then(Object::dict)
    }

    /// `object`, or what it refers to when it is a reference to an object the walk read.
    pub(crate) fn resolve<'a>(&'a self, object: &'a Object) -> Option<&'a Object> {
        match object.value {
            Value::Reference(num, _) => match self.slots.get(&num) {
                Some(Slot::Loaded { object, .. }) => Some(object),
                _ => None,
            },
            _ => Some(object),
        }
    }

    /// Every object the walk read: its number, value, stream data's range and buffer.
    pub(crate) fn objects(&self) -> impl Iterator<Item = (u32, &Object, Option<(usize, usize)>, Buffer)> {
        self.slots.iter().filter_map(|(num, slot)| match slot {
            Slot::Loaded { object, stream, buffer } => Some((*num, object, *stream, *buffer)),
            Slot::Damaged(_) => None,
        })
    }

    /// The bytes `start..end` of `buffer`, as written: a name's raw spelling, a real's text.
    pub(crate) fn raw(&self, buffer: Buffer, start: usize, end: usize) -> &[u8] {
        let data = match buffer {
            Buffer::File => self.file.as_slice(),
            Buffer::ObjectStream(num) => {
                let stream = self.object_streams.get(&num).and_then(|s| s.as_ref().ok());
                stream.map_or(&[][..], |s| s.data.as_slice())
            }
        };
        data.get(start..end).unwrap_or_default()
    }

    /// How many objects the cross-reference sections list as in use, reachable or not.
    pub(crate) fn listed_objects(&self) -> usize {
        self.xref.entries.values().filter(|e| !matches!(e, Entry::Free)).count()
    }

    /// Whether any object reachable from the trailer is a gap.
    #[must_use]
    pub fn damaged(&self) -> bool {
        self.slots.values().any(|s| matches!(s, Slot::Damaged(_)))
    }

    /// What the file's structure and objects reported: refusals are not here, since an open document has none.
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let mut out: Vec<Finding> = self.xref.notes.iter().map(file::note_finding).collect();
        for (buffer, note) in &self.notes {
            out.push(syntax::note_finding(note, location(*buffer)));
        }
        for (num, slot) in &self.slots {
            if let Slot::Damaged(damage) = slot {
                out.push(file::damage_finding(*num, damage));
            }
        }
        for (num, name) in self.unknown_filters() {
            let name = String::from_utf8_lossy(&name);
            out.push(
                Finding::new(
                    STREAM_FILTER,
                    format!("object {num} names the filter /{name}, which no standard defines"),
                )
                .citing("7.4.1")
                .at(vec![Location::Object(num, 0)]),
            );
        }
        for (num, generation, why) in &self.dangling {
            let reason = match why {
                Dangling::NotListed => "no cross-reference section lists it".to_owned(),
                Dangling::BeyondSize => "it is beyond the trailer's /Size".to_owned(),
                Dangling::Free => "it is listed as free".to_owned(),
                Dangling::Generation(g) => format!("the object has generation {g}"),
            };
            out.push(
                Finding::new(
                    OBJECT_DANGLING_REFERENCE,
                    format!("a reference to {num} {generation} R reads as null: {reason}"),
                )
                .citing("7.3.10")
                .at(vec![Location::Object(*num, *generation)]),
            );
        }
        out
    }

    /// Every filter name a stream gives that no standard defines, wherever the stream is; decoding it waits until
    /// something reads it.
    fn unknown_filters(&self) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        for (num, object, stream, _) in self.objects() {
            let (Some(dict), Some(_)) = (object.dict(), stream) else {
                continue;
            };
            let names: Vec<&Object> = match dict.get(b"Filter").and_then(|f| self.resolve(f)) {
                Some(Object {
                    value: Value::Array(items),
                    ..
                }) => items.iter().filter_map(|i| self.resolve(i)).collect(),
                other => other.into_iter().collect(),
            };
            let unknown = names
                .into_iter()
                .filter_map(Object::name)
                .filter(|n| Filter::from_name(n).is_none());
            out.extend(unknown.map(|n| (num, n.to_vec())));
        }
        out
    }

    fn load(&mut self, num: u32, generation: u16, limits: &Limits) -> Option<Slot> {
        let beyond = offset_size(&self.xref).is_some_and(|size| num >= size);
        let why = match (beyond, self.xref.entries.get(&num).copied()) {
            (true, _) => Dangling::BeyondSize,
            (false, None) => Dangling::NotListed,
            (false, Some(Entry::Free)) => Dangling::Free,
            (false, Some(Entry::Direct { offset, generation: g })) if g == generation => {
                return Some(self.direct(num, generation, offset, limits));
            }
            (false, Some(Entry::Direct { generation: g, .. })) => Dangling::Generation(g),
            (false, Some(Entry::Compressed { stream, index })) if generation == 0 => {
                return Some(self.compressed(num, stream, index, limits));
            }
            (false, Some(Entry::Compressed { .. })) => Dangling::Generation(0),
            (false, Some(Entry::Invalid)) => return Some(Slot::Damaged(ObjectDamage::Entry)),
        };
        self.dangling.push((num, generation, why));
        None
    }

    fn direct(&mut self, num: u32, generation: u16, offset: usize, limits: &Limits) -> Slot {
        let at = self.xref.header.saturating_add(offset);
        let mut notes = Vec::new();
        let read = {
            let (buf, xref) = (&self.file, &self.xref);
            file::framed(buf, at, limits, &mut notes, &|o| integer_at(buf, xref, o, limits))
        };
        self.notes.extend(notes.into_iter().map(|n| (Buffer::File, n)));
        match read {
            Ok(framed) if framed.number == num && framed.generation == generation => Slot::Loaded {
                object: framed.object,
                stream: framed.stream,
                buffer: Buffer::File,
            },
            Ok(_) => Slot::Damaged(ObjectDamage::Header(at)),
            Err(damage) => Slot::Damaged(damage),
        }
    }

    fn compressed(&mut self, num: u32, stream: u32, index: usize, limits: &Limits) -> Slot {
        let damaged = Slot::Damaged(ObjectDamage::ObjectStream(stream));
        if !self.object_streams.contains_key(&stream) {
            let decoded = self.decode_object_stream(stream, limits);
            self.object_streams.insert(stream, decoded);
        }
        let Some(Ok(ObjectStream { data, count, first })) = self.object_streams.get(&stream) else {
            return damaged;
        };
        let mut header = Parser::new(data.get(..*first).unwrap_or_default(), 0, Mode::Objects, 1);
        let mut pairs = Vec::new();
        while let Ok(Some(syntax::Located {
            event: syntax::Event::Integer(i),
            ..
        })) = header.next()
        {
            pairs.push(i);
        }
        let listed = pairs.get(2 * index).copied();
        let offset = pairs.get(2 * index + 1).and_then(|o| usize::try_from(*o).ok());
        let (Some(offset), true) = (offset, index < *count && listed == Some(i64::from(num))) else {
            return damaged;
        };
        let mut parser = Parser::new(data, first.saturating_add(offset), Mode::Objects, limits.nesting);
        let mut notes = Vec::new();
        let read = syntax::read(&mut parser, &mut notes);
        self.notes
            .extend(notes.into_iter().map(|n| (Buffer::ObjectStream(stream), n)));
        match read {
            Ok(Read::Value(object)) => Slot::Loaded {
                object,
                stream: None,
                buffer: Buffer::ObjectStream(stream),
            },
            _ => damaged,
        }
    }

    /// Object stream `stream`, decoded, reading the stream object first if the walk has not.
    fn decode_object_stream(&mut self, stream: u32, limits: &Limits) -> Result<ObjectStream, ObjectDamage> {
        let damaged = ObjectDamage::ObjectStream(stream);
        if let (None, Some(Entry::Direct { offset, generation: 0 })) =
            (self.slots.get(&stream), self.xref.entries.get(&stream).copied())
        {
            let slot = self.direct(stream, 0, offset, limits);
            self.slots.insert(stream, slot);
        }
        let Some(Slot::Loaded {
            object,
            stream: Some(range),
            ..
        }) = self.slots.get(&stream)
        else {
            return Err(damaged);
        };
        let dict = object
            .dict()
            .filter(|d| d.get(b"Type").and_then(Object::name) == Some(b"ObjStm"))
            .ok_or(damaged)?;
        let (count, first) = (
            dict_int(dict, b"N").ok_or(damaged)?,
            dict_int(dict, b"First").ok_or(damaged)?,
        );
        let (buf, xref) = (&self.file, &self.xref);
        match file::decode(buf, dict, *range, limits, &|o| resolve_direct(buf, xref, o, limits)) {
            Ok(Decoded::Plain(data)) => Ok(ObjectStream { data, count, first }),
            _ => Err(damaged),
        }
    }
}

/// The trailer's `/Size`, when it is a non-negative integer that fits.
fn offset_size(xref: &Xref) -> Option<u32> {
    xref.trailer
        .get(b"Size")
        .and_then(Object::integer)
        .and_then(|i| u32::try_from(i).ok())
}

fn dict_int(dict: &Dict, key: &[u8]) -> Option<usize> {
    dict.get(key)?.integer().and_then(|i| usize::try_from(i).ok())
}

/// Push every reference in `object`, depth first, onto `stack`.
fn references(object: &Object, stack: &mut Vec<(u32, u16)>) {
    match &object.value {
        Value::Reference(num, generation) => stack.push((*num, *generation)),
        Value::Array(items) => items.iter().for_each(|o| references(o, stack)),
        Value::Dict(dict) => dict.entries.iter().for_each(|(_, o)| references(o, stack)),
        _ => {}
    }
}

/// A referenced object read straight from the file, for an indirect `/Length` or a filter named by reference. Only a
/// directly stored object without a stream qualifies, so these reads cannot recurse.
fn resolve_direct(buf: &[u8], xref: &Xref, reference: &Object, limits: &Limits) -> Option<Object> {
    let Value::Reference(num, generation) = reference.value else {
        return None;
    };
    let Some(Entry::Direct { offset, generation: g }) = xref.entries.get(&num).copied() else {
        return None;
    };
    let framed = file::framed(
        buf,
        xref.header.saturating_add(offset),
        limits,
        &mut Vec::new(),
        &|_| None,
    )
    .ok()?;
    (framed.number == num && framed.generation == generation && g == generation && framed.stream.is_none())
        .then_some(framed.object)
}

fn integer_at(buf: &[u8], xref: &Xref, reference: &Object, limits: &Limits) -> Option<i64> {
    resolve_direct(buf, xref, reference, limits)?.integer()
}

fn location(buffer: Buffer) -> Vec<Location> {
    match buffer {
        Buffer::File => Vec::new(),
        Buffer::ObjectStream(num) => vec![Location::Object(num, 0)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::test_pdf::{classic, object_stream, with_xref_stream};

    fn open(buf: &[u8]) -> Document {
        Document::open(buf, &Limits::default()).unwrap()
    }

    fn ids(doc: &Document) -> Vec<&'static str> {
        doc.findings().iter().map(|f| f.check.id).collect()
    }

    #[test]
    fn the_walk_reads_everything_reachable_once() {
        let buf = classic(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R /Again 2 0 R >>"),
                (2, b"<< /Kids [3 0 R] >>"),
                (3, b"<< /Parent 2 0 R >>"),
                (4, b"(unreachable)"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = open(&buf);
        assert_eq!(doc.slots.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);
        assert!(!doc.damaged());
        assert_eq!(doc.findings(), []);
        assert!(matches!(
            doc.slots.get(&1),
            Some(Slot::Loaded {
                buffer: Buffer::File,
                stream: None,
                ..
            })
        ));
    }

    #[test]
    fn a_refused_file_says_why() {
        let refusal = Document::open(b"%PDF-1.7", &Limits::default()).err().unwrap();
        assert_eq!(refusal.check.id, "file.startxref");
    }

    #[test]
    fn dangling_references_read_as_null_with_a_reason() {
        let buf = classic(
            &[(1, b"<< /A 9 0 R /B 2 0 R /C 3 5 R /D 50 0 R >>"), (3, b"1")],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = open(&buf);
        let mut found = doc.dangling.clone();
        found.sort_unstable_by_key(|(num, generation, _)| (*num, *generation));
        assert_eq!(
            found,
            [
                (2, 0, Dangling::Free),
                (3, 5, Dangling::Generation(0)),
                (9, 0, Dangling::BeyondSize),
                (50, 0, Dangling::BeyondSize)
            ]
        );
        assert_eq!(ids(&doc), ["object.dangling-reference"; 4]);
        // Without a usable /Size, a number the sections do not list is "not listed".
        let doc = open(&classic(&[(1, b"<< /A 9 0 R >>")], "<< /Root 1 0 R >>"));
        assert_eq!(doc.dangling, [(9, 0, Dangling::NotListed)]);
        let messages: Vec<String> = doc.findings().into_iter().map(|f| f.message).collect();
        assert!(
            messages[0].contains("no cross-reference section lists it"),
            "{messages:?}"
        );
    }

    #[test]
    fn every_dangling_reason_has_words() {
        let mut doc = open(&classic(&[(1, b"<< >>")], "<< /Size SIZE /Root 1 0 R >>"));
        doc.dangling = vec![
            (1, 0, Dangling::NotListed),
            (1, 0, Dangling::BeyondSize),
            (1, 0, Dangling::Free),
            (1, 0, Dangling::Generation(2)),
        ];
        let messages: Vec<String> = doc.findings().into_iter().map(|f| f.message).collect();
        assert!(messages[3].ends_with("the object has generation 2"), "{messages:?}");
    }

    #[test]
    fn a_damaged_object_is_a_gap_the_walk_goes_around() {
        let buf = classic(
            &[(1, b"<< /A 2 0 R /B 3 0 R >>"), (2, b"[1"), (3, b"(fine)")],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = open(&buf);
        assert!(doc.damaged());
        assert!(matches!(
            doc.slots.get(&2),
            Some(Slot::Damaged(ObjectDamage::Syntax(_)))
        ));
        assert!(matches!(doc.slots.get(&3), Some(Slot::Loaded { .. })));
        assert_eq!(ids(&doc), ["object.damaged"]);
        // An entry pointing at another object's number is damage too.
        let swapped = String::from_utf8(buf).unwrap().replacen("3 0 obj", "7 0 obj", 1);
        assert!(matches!(
            open(swapped.as_bytes()).slots.get(&3),
            Some(Slot::Damaged(ObjectDamage::Header(_)))
        ));
    }

    #[test]
    fn syntax_notes_are_findings_with_their_buffer() {
        let buf = classic(&[(1, b"<< /A <414> /A 1 >>")], "<< /Size SIZE /Root 1 0 R >>");
        assert_eq!(ids(&open(&buf)), ["syntax.hex-digits", "syntax.duplicate-key"]);
        assert_eq!(location(Buffer::ObjectStream(4)), [Location::Object(4, 0)]);
    }

    #[test]
    fn an_indirect_length_is_followed() {
        let buf = classic(
            &[
                (1, b"<< /S 2 0 R >>"),
                (2, b"<< /Length 3 0 R >>\nstream\nabc\nendstream"),
                (3, b"3"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = open(&buf);
        assert!(
            matches!(doc.slots.get(&2), Some(Slot::Loaded { stream: Some(_), .. })),
            "{:?}",
            doc.slots.get(&2)
        );
        // A /Length that names a stream, a missing object or a compressed one cannot be followed.
        for target in [&b"<< /Length 1 >>\nstream\nx\nendstream"[..], b"(not a number)"] {
            let buf = classic(
                &[
                    (1, b"<< /S 2 0 R >>"),
                    (2, b"<< /Length 3 0 R >>\nstream\nabc\nendstream"),
                    (3, target),
                ],
                "<< /Size SIZE /Root 1 0 R >>",
            );
            assert!(matches!(
                open(&buf).slots.get(&2),
                Some(Slot::Damaged(ObjectDamage::Length(_)))
            ));
        }
        let buf = classic(
            &[
                (1, b"<< /S 2 0 R >>"),
                (2, b"<< /Length 9 0 R >>\nstream\nabc\nendstream"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        assert!(matches!(
            open(&buf).slots.get(&2),
            Some(Slot::Damaged(ObjectDamage::Length(_)))
        ));
        let buf = classic(
            &[
                (1, b"<< /S 2 0 R >>"),
                (2, b"<< /Length 3 1 R >>\nstream\nabc\nendstream"),
                (3, b"3"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        assert!(matches!(
            open(&buf).slots.get(&2),
            Some(Slot::Damaged(ObjectDamage::Length(_)))
        ));
    }

    #[test]
    fn compressed_objects() {
        let stm = object_stream(&[(2, "<< /In 3 0 R >>"), (3, "(three)")]);
        let buf = with_xref_stream(
            &[(1, b"<< /Type /Catalog /P 2 0 R >>"), (5, &stm)],
            &[(2, 5, 0), (3, 5, 1)],
            6,
            "/Root 1 0 R",
        );
        let doc = open(&buf);
        assert!(!doc.damaged(), "{:?}", doc.slots);
        assert!(matches!(
            doc.slots.get(&3),
            Some(Slot::Loaded {
                buffer: Buffer::ObjectStream(5),
                ..
            })
        ));
        assert_eq!(doc.findings(), []);
        let wrong_generation = with_xref_stream(&[(1, b"<< /P 2 1 R >>"), (5, &stm)], &[(2, 5, 0)], 6, "/Root 1 0 R");
        assert_eq!(open(&wrong_generation).dangling, [(2, 1, Dangling::Generation(0))]);
    }

    fn object_stream_damage(stream: &[u8], compressed: &[(u32, u32, u16)]) -> Option<ObjectDamage> {
        let buf = with_xref_stream(&[(1, b"<< /P 2 0 R >>"), (5, stream)], compressed, 6, "/Root 1 0 R");
        match open(&buf).slots.get(&2) {
            Some(Slot::Damaged(d)) => Some(*d),
            _ => None,
        }
    }

    #[test]
    fn a_broken_object_stream_leaves_its_objects_as_gaps() {
        let good = object_stream(&[(2, "1")]);
        let damaged = Some(ObjectDamage::ObjectStream(5));
        assert_eq!(object_stream_damage(&good, &[(2, 5, 0)]), None);
        assert_eq!(object_stream_damage(&good, &[(2, 5, 1)]), damaged);
        assert_eq!(
            object_stream_damage(&good, &[(2, 7, 0)]),
            Some(ObjectDamage::ObjectStream(7))
        );
        let text = String::from_utf8(good.clone()).unwrap();
        for broken in [
            text.replace("/Type /ObjStm", "/Type /Other"),
            text.replace("/N 1", "/N x"),
            text.replace("/N 1", "/N 0"),
            text.replace("/N 1", "/N -1"),
            text.replace("2 0 1", "4 0 1"),
            text.replace("2 0 1", "2 -1 1"),
            text.replace("/First 4", "/First 2"),
            text.replace("/Length", "/Filter /FlateDecode /Length"),
            text.replace("2 0 1", "2 0 ]"),
        ] {
            assert_eq!(
                object_stream_damage(broken.as_bytes(), &[(2, 5, 0)]),
                damaged,
                "{broken}"
            );
        }
        assert_eq!(object_stream_damage(b"(not a stream)", &[(2, 5, 0)]), damaged);
        // The object stream itself stored with another generation, or compressed, cannot hold objects.
        let buf = with_xref_stream(&[(1, b"<< /P 2 0 R >>")], &[(2, 5, 0), (5, 7, 0)], 6, "/Root 1 0 R");
        assert!(matches!(
            open(&buf).slots.get(&2),
            Some(Slot::Damaged(ObjectDamage::ObjectStream(5)))
        ));
        let second = with_xref_stream(
            &[
                (1, b"<< /P 2 0 R /Q 3 0 R >>"),
                (5, &object_stream(&[(2, "1"), (3, "2")])),
            ],
            &[(2, 5, 0), (3, 5, 1)],
            6,
            "/Root 1 0 R",
        );
        assert!(!open(&second).damaged());
    }

    #[test]
    fn an_object_stream_may_name_its_filter_by_reference_and_carry_notes() {
        let data = b"2 0 <414>";
        let zipped = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
        let mut stm = format!(
            "<< /Type /ObjStm /N 1 /First 4 /Filter 8 0 R /Length {} >>\nstream\n",
            zipped.len()
        )
        .into_bytes();
        stm.extend(&zipped);
        stm.extend(b"\nendstream");
        let buf = with_xref_stream(
            &[(1, b"<< /P 2 0 R >>"), (5, &stm), (8, b"/FlateDecode")],
            &[(2, 5, 0)],
            6,
            "/Root 1 0 R",
        );
        let doc = open(&buf);
        assert!(matches!(
            doc.slots.get(&2),
            Some(Slot::Loaded {
                buffer: Buffer::ObjectStream(5),
                ..
            })
        ));
        let findings = doc.findings();
        assert_eq!(
            findings.iter().map(|f| f.check.id).collect::<Vec<_>>(),
            ["syntax.hex-digits"]
        );
        assert_eq!(findings[0].chain, [Location::Object(5, 0), Location::Offset(4)]);
    }

    #[test]
    fn a_damaged_object_stream_is_reported_once_per_object() {
        let buf = with_xref_stream(
            &[
                (1, b"<< /P 2 0 R >>"),
                (5, b"<< /Type /ObjStm /N 1 /First 4 /Length 99 >>\nstream\nx\nendstream"),
            ],
            &[(2, 5, 0)],
            6,
            "/Root 1 0 R",
        );
        let doc = open(&buf);
        assert!(doc.damaged());
        assert_eq!(ids(&doc), ["object.damaged", "object.damaged"]);
    }

    #[test]
    fn more_ways_a_length_or_an_object_stream_fails() {
        let length = |target: &[u8], length: &[u8]| {
            let body = [&b"<< /Length "[..], length, b" >>\nstream\nabc\nendstream"].concat();
            let buf = classic(
                &[(1, b"<< /S 2 0 R >>"), (2, &body), (3, target)],
                "<< /Size SIZE /Root 1 0 R >>",
            );
            matches!(open(&buf).slots.get(&2), Some(Slot::Damaged(ObjectDamage::Length(_))))
        };
        assert!(length(b"3", b"(3)"));
        assert!(length(b"[3", b"3 0 R"));
        assert!(!length(b"3", b"3 0 R"));
        let text = String::from_utf8(object_stream(&[(2, "1")])).unwrap();
        assert_eq!(
            object_stream_damage(text.replace("/First 4", "").as_bytes(), &[(2, 5, 0)]),
            Some(ObjectDamage::ObjectStream(5))
        );
    }

    #[test]
    fn an_unknown_filter_is_a_finding_wherever_it_is() {
        let doc = open(&classic(
            &[
                (1, b"<< /A 2 0 R /B 3 0 R >>"),
                (2, b"<< /Filter /Rot13 /Length 1 >>\nstream\nx\nendstream"),
                (3, b"<< /Filter [/FlateDecode 4 0 R] /Length 1 >>\nstream\nx\nendstream"),
                (4, b"/Nope"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        ));
        assert_eq!(ids(&doc), ["stream.filter", "stream.filter"]);
    }

    #[test]
    fn an_invalid_entry_is_a_gap_when_referred_to() {
        let text = String::from_utf8(classic(
            &[(1, b"<< /A 2 0 R >>"), (2, b"1")],
            "<< /Size SIZE /Root 1 0 R >>",
        ))
        .unwrap();
        let at = text.find("2 0 obj").unwrap();
        let broken = text.replacen(&format!("{at:010} 00000 n"), &format!("{at:010} 65536 n"), 1);
        let doc = open(broken.as_bytes());
        assert!(matches!(doc.slots.get(&2), Some(Slot::Damaged(ObjectDamage::Entry))));
    }
}
