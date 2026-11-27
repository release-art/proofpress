//! Tier 4: file structure (ISO 32000-2, 7.5) and the refusal of encryption (7.6).
//!
//! [`open`] finds the header and `startxref`, follows every cross-reference section through `/Prev` and `/XRefStm`, and
//! keeps the newest trailer. Only file-level damage refuses a file: no header, no `startxref`, an unreadable section,
//! encryption, no `/Root`. [`read_object`] then reads one object for tier 5; damage there is a gap, not a refusal.
//! Offsets in the file count from the `%PDF-` header, which 7.5.2 lets other data precede.

use std::collections::{BTreeMap, BTreeSet};

use crate::filters::{self, Decoded, Filter, Params};
use crate::findings::{
    FILE_ENCRYPTED, FILE_EOF, FILE_HEADER, FILE_HEADER_OFFSET, FILE_LIMIT, FILE_STARTXREF, FILE_TRAILER, FILE_XREF,
    Finding, Location, OBJECT_DAMAGED,
};
use crate::pipeline::Limits;
use crate::syntax::{self, Dict, Mode, Note, Object, Parser, Raw, Read, Value};

/// Where an object is, by the cross-reference sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Listed as free: the object does not exist.
    Free,
    /// At this offset from the header, with this generation.
    Direct { offset: usize, generation: u16 },
    /// The `index`-th object of object stream `stream` (generation 0).
    Compressed { stream: u32, index: usize },
    /// Listed as in use with a generation above 65535, which no reference can name: a gap if anything refers to it.
    Invalid,
}

/// A file that opened: where its objects are, and its trailer.
#[derive(Debug, PartialEq)]
pub(crate) struct Xref {
    /// Where `%PDF-` starts; every offset counts from here.
    pub(crate) header: usize,
    /// The version the header names, such as `1.7`.
    pub(crate) version: Vec<u8>,
    pub(crate) entries: BTreeMap<u32, Entry>,
    /// The newest trailer, or the newest cross-reference stream's dictionary.
    pub(crate) trailer: Dict,
    /// What was irregular but did not refuse the file.
    pub(crate) notes: Vec<FileNote>,
}

/// Something irregular at file level that still lets the file open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileNote {
    /// `%PDF-` starts at this offset, not at 0 (allowed by 7.5.2; PDF/A forbids it).
    HeaderOffset(usize),
    /// No `%%EOF` after `startxref`.
    NoEof,
    /// Bytes other than white space after `%%EOF`, starting here.
    DataAfterEof(usize),
    /// A byte-level note in a trailer or cross-reference dictionary.
    Syntax(Note),
}

/// Why a file cannot be read at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// No `%PDF-` in the first 1024 bytes.
    NoHeader,
    /// No `startxref` with an offset in the last 1024 bytes.
    NoStartxref,
    /// A cross-reference section that cannot be read, at this offset, and why.
    BadXref(usize, XrefProblem),
    /// `/Prev` or `/XRefStm` leads back to a section already read, at this offset.
    XrefLoop(usize),
    /// More sections than [`Limits::xref_sections`].
    TooManySections,
    /// More objects than [`Limits::objects`].
    TooManyObjects,
    /// The trailer has `/Encrypt`.
    Encrypted,
    /// The trailer has no `/Root` reference.
    NoRoot,
}

/// What is wrong with a cross-reference section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum XrefProblem {
    /// Neither `xref` nor an object starts at the offset.
    NotASection,
    /// A subsection header that is not two integers.
    Subsection,
    /// An entry that is not 20 bytes in the form of 7.5.4.
    Entry,
    /// No trailer dictionary after the table.
    Trailer,
    /// The object at the offset is not a valid cross-reference stream.
    Stream,
    /// A cross-reference stream whose `/Index` lists more entries than [`Limits::objects`]: refused before its data
    /// is decoded, which for millions of entries takes a while.
    TooMany,
}

const WINDOW: usize = 1024;

/// Open `buf`: everything tier 4 needs before any object is read.
pub(crate) fn open(buf: &[u8], limits: &Limits) -> Result<Xref, Refusal> {
    let header = buf
        .windows(5)
        .take(WINDOW)
        .position(|w| w == b"%PDF-")
        .ok_or(Refusal::NoHeader)?;
    let version_start = header + 5;
    let version_len = buf
        .iter()
        .skip(version_start)
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    let version = buf
        .get(version_start..version_start + version_len)
        .unwrap_or_default()
        .to_vec();
    let mut notes = Vec::new();
    if header > 0 {
        notes.push(FileNote::HeaderOffset(header));
    }
    let start = tail(buf, &mut notes)?;
    let mut entries = BTreeMap::new();
    let mut trailer = None;
    let mut queue = vec![start];
    let mut seen = BTreeSet::new();
    while let Some(offset) = queue.pop() {
        let mut read = |offset: usize, seen: &mut BTreeSet<usize>| {
            if !seen.insert(offset) {
                return Err(Refusal::XrefLoop(offset));
            }
            if seen.len() > limits.xref_sections {
                return Err(Refusal::TooManySections);
            }
            let at = header.saturating_add(offset);
            section(buf, at, limits, &mut notes).map_err(|problem| match problem {
                XrefProblem::TooMany => Refusal::TooManyObjects,
                problem => Refusal::BadXref(at, problem),
            })
        };
        let section = read(offset, &mut seen)?;
        // A hybrid file's /XRefStm is read before its own table, whose entries for compressed objects are free
        // placeholders (7.5.8.4). Then newest first: an entry already known wins.
        // Microsoft Word names one /XRefStm from several trailers: a stream already read adds nothing new.
        let hidden = match offset_of(&section.trailer, b"XRefStm") {
            Some(stm) if !seen.contains(&stm) => read(stm, &mut seen)?.entries,
            _ => Vec::new(),
        };
        for (num, entry) in hidden.into_iter().chain(section.entries) {
            entries.entry(num).or_insert(entry);
        }
        if entries.len() > limits.objects {
            return Err(Refusal::TooManyObjects);
        }
        queue.extend(offset_of(&section.trailer, b"Prev"));
        trailer.get_or_insert(section.trailer);
    }
    // The queue started with one section, so a trailer was kept unless a section failed, which returned above.
    let trailer = trailer.unwrap_or_default();
    if trailer.get(b"Encrypt").is_some() {
        return Err(Refusal::Encrypted);
    }
    if !matches!(trailer.get(b"Root").map(|o| &o.value), Some(Value::Reference(..))) {
        return Err(Refusal::NoRoot);
    }
    Ok(Xref {
        header,
        version,
        entries,
        trailer,
        notes,
    })
}

fn offset_of(dict: &Dict, key: &[u8]) -> Option<usize> {
    dict.get(key)
        .and_then(Object::integer)
        .and_then(|i| usize::try_from(i).ok())
}

/// The offset `startxref` names, from the last 1024 bytes, noting what follows `%%EOF`.
fn tail(buf: &[u8], notes: &mut Vec<FileNote>) -> Result<usize, Refusal> {
    let from = buf.len().saturating_sub(WINDOW);
    let window = buf.get(from..).unwrap_or_default();
    let keyword = window
        .windows(9)
        .rposition(|w| w == b"startxref")
        .ok_or(Refusal::NoStartxref)?
        + from;
    let mut parser = Parser::new(buf, keyword + 9, Mode::Objects, 1);
    let number = parser.next().ok().flatten().ok_or(Refusal::NoStartxref)?;
    let syntax::Event::Integer(offset) = number.event else {
        return Err(Refusal::NoStartxref);
    };
    let offset = usize::try_from(offset).map_err(|_| Refusal::NoStartxref)?;
    let rest = buf.get(number.end..).unwrap_or_default();
    let skip = rest.iter().take_while(|b| syntax::is_white(**b)).count();
    match rest.get(skip..).unwrap_or_default().strip_prefix(b"%%EOF") {
        None => notes.push(FileNote::NoEof),
        Some(after) => {
            if let Some(junk) = after.iter().position(|b| !syntax::is_white(*b)) {
                notes.push(FileNote::DataAfterEof(buf.len() - after.len() + junk));
            }
        }
    }
    Ok(offset)
}

struct Section {
    entries: Vec<(u32, Entry)>,
    trailer: Dict,
}

fn section(buf: &[u8], at: usize, limits: &Limits, notes: &mut Vec<FileNote>) -> Result<Section, XrefProblem> {
    if buf.get(at..).is_some_and(|rest| rest.starts_with(b"xref")) {
        table(buf, at + 4, limits, notes)
    } else {
        stream(buf, at, limits, notes)
    }
}

/// A classic table (7.5.4): subsections of 20-byte entries, then `trailer` and its dictionary.
fn table(buf: &[u8], mut at: usize, limits: &Limits, notes: &mut Vec<FileNote>) -> Result<Section, XrefProblem> {
    let mut entries = Vec::new();
    loop {
        at += buf.iter().skip(at).take_while(|b| syntax::is_white(**b)).count();
        let rest = buf.get(at..).unwrap_or_default();
        if let Some(after) = rest.strip_prefix(b"trailer") {
            let mut parser = Parser::new(buf, buf.len() - after.len(), Mode::Objects, limits.nesting);
            let mut found = Vec::new();
            let Ok(Read::Value(Object {
                value: Value::Dict(trailer),
                ..
            })) = syntax::read(&mut parser, &mut found)
            else {
                return Err(XrefProblem::Trailer);
            };
            notes.extend(found.into_iter().map(FileNote::Syntax));
            return Ok(Section { entries, trailer });
        }
        let (first, count, after) = subsection(rest).ok_or(XrefProblem::Subsection)?;
        at = buf.len() - after.len();
        for i in 0..count {
            let raw: &[u8; 20] = buf
                .get(at..at + 20)
                .and_then(|r| r.try_into().ok())
                .ok_or(XrefProblem::Entry)?;
            let num = first.checked_add(i).ok_or(XrefProblem::Entry)?;
            entries.push((num, entry(raw).ok_or(XrefProblem::Entry)?));
            at += 20;
        }
    }
}

/// `first count` and an end of line, then what follows.
fn subsection(rest: &[u8]) -> Option<(u32, u32, &[u8])> {
    let (first, rest) = digits(rest)?;
    let (count, rest) = digits(rest.strip_prefix(b" ")?)?;
    let eol = rest.iter().take_while(|b| matches!(b, b' ' | b'\r' | b'\n')).count();
    let first = u32::try_from(first).ok()?;
    (eol > 0).then(|| (first, u32::try_from(count).unwrap_or(u32::MAX), rest.split_at(eol).1))
}

/// The decimal number at the start of `text`, at least one digit, and what follows; `None` for no digits or overflow.
fn digits(text: &[u8]) -> Option<(u64, &[u8])> {
    let n = text.iter().take_while(|b| b.is_ascii_digit()).count();
    let (number, rest) = text.split_at(n);
    let value = number
        .iter()
        .try_fold(0_u64, |acc, d| acc.checked_mul(10)?.checked_add(u64::from(d - b'0')))?;
    (n > 0).then_some((value, rest))
}

/// One entry: ten digits, a space, five digits, a space, `n` or `f`, and a two-byte end of line.
fn entry(raw: &[u8; 20]) -> Option<Entry> {
    let (offset, rest) = raw.split_at(10);
    let (space, rest) = rest.split_at(1);
    let (generation, rest) = rest.split_at(5);
    let (space2, rest) = rest.split_at(1);
    let (kind, eol) = rest.split_at(1);
    let whole = |field: &[u8]| digits(field).filter(|(_, left)| left.is_empty()).map(|(v, _)| v);
    let (offset, generation) = (
        usize::try_from(whole(offset)?).unwrap_or(usize::MAX),
        whole(generation)?,
    );
    if space != b" " || space2 != b" " || !matches!(eol, b" \r" | b" \n" | b"\r\n") {
        return None;
    }
    // A free entry's generation is never used, and writers put 65536 in the head of the free list. An object in use
    // needs one that fits; one that does not is a damaged entry, not an unreadable table.
    match kind {
        b"n" => {
            Some(u16::try_from(generation).map_or(Entry::Invalid, |generation| Entry::Direct { offset, generation }))
        }
        b"f" => Some(Entry::Free),
        _ => None,
    }
}

/// A cross-reference stream (7.5.8): an indirect object whose dictionary is `/Type /XRef`.
fn stream(buf: &[u8], at: usize, limits: &Limits, notes: &mut Vec<FileNote>) -> Result<Section, XrefProblem> {
    let bad = XrefProblem::Stream;
    let mut found = Vec::new();
    let read = framed(buf, at, limits, &mut found, &|_| None).map_err(|damage| match damage {
        ObjectDamage::Header(_) => XrefProblem::NotASection,
        _ => bad,
    })?;
    notes.extend(found.into_iter().map(FileNote::Syntax));
    let (Some(range), Value::Dict(dict)) = (read.stream, read.object.value) else {
        return Err(bad);
    };
    if dict.get(b"Type").and_then(Object::name) != Some(b"XRef") {
        return Err(bad);
    }
    let widths: Vec<usize> = ints(dict.get(b"W")).ok_or(bad)?;
    let [w0, w1, w2] = widths.as_slice() else {
        return Err(bad);
    };
    let size: usize = offset_of(&dict, b"Size").ok_or(bad)?;
    let index = match dict.get(b"Index") {
        None => vec![0, size],
        some => ints(some)
            .filter(|i: &Vec<usize>| i.len().is_multiple_of(2))
            .ok_or(bad)?,
    };
    let declared = index
        .iter()
        .skip(1)
        .step_by(2)
        .fold(0usize, |n, c| n.saturating_add(*c));
    if declared > limits.objects {
        return Err(XrefProblem::TooMany);
    }
    let Ok(Decoded::Plain(data)) = decode(buf, &dict, range, limits, &|_| None) else {
        return Err(bad);
    };
    let row = w0 + w1 + w2;
    if row == 0 || [*w0, *w1, *w2].iter().any(|w| *w > 8) {
        return Err(bad);
    }
    let mut entries = Vec::new();
    let mut rows = data.chunks(row);
    let pairs = index.iter().step_by(2).zip(index.iter().skip(1).step_by(2));
    for (first, count) in pairs {
        for i in 0..*count {
            let row = rows.next().filter(|r| r.len() == row).ok_or(bad)?;
            let (kind, rest) = row.split_at(*w0);
            let (one, two) = rest.split_at(*w1);
            let kind = if *w0 == 0 { 1 } else { number(kind) };
            let num = u32::try_from(first + i).map_err(|_| bad)?;
            let entry = match kind {
                1 => Entry::Direct {
                    offset: number(one),
                    generation: u16::try_from(number(two)).map_err(|_| bad)?,
                },
                2 => Entry::Compressed {
                    stream: u32::try_from(number(one)).map_err(|_| bad)?,
                    index: number(two),
                },
                // Type 0 is a free entry; 7.5.8.3 makes any other type a reference to the null object.
                _ => Entry::Free,
            };
            entries.push((num, entry));
        }
    }
    Ok(Section { entries, trailer: dict })
}

fn ints<T: TryFrom<i64>>(object: Option<&Object>) -> Option<Vec<T>> {
    object?
        .array()?
        .iter()
        .map(|o| T::try_from(o.integer()?).ok())
        .collect()
}

fn number(bytes: &[u8]) -> usize {
    bytes.iter().fold(0, |acc, b| acc << 8 | usize::from(*b))
}

/// One indirect object as read from a buffer: its value, and for a stream, its data's range in that buffer.
#[derive(Debug, PartialEq)]
pub(crate) struct Framed {
    pub(crate) number: u32,
    pub(crate) generation: u16,
    pub(crate) object: Object,
    pub(crate) stream: Option<(usize, usize)>,
}

/// Why one object could not be read. The object is a gap; the rest of the file is still read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObjectDamage {
    /// Tier 2 stopped here.
    Syntax(syntax::Damage),
    /// The object does not start with `n g obj`, or names another number or generation, at this offset.
    Header(usize),
    /// No `endobj`, or something else, after the value or the stream.
    NoEndobj(usize),
    /// A stream's `/Length` that is missing, not a non-negative integer, or wrong (no `endstream` after the data).
    Length(usize),
    /// `stream` followed by a bare carriage return, which 7.3.8.1 forbids.
    BareCr(usize),
    /// A compressed object whose object stream is damaged, missing, or has no such index.
    ObjectStream(u32),
    /// The cross-reference entry for the object is not a valid in-use entry.
    Entry,
}

/// Read the indirect object at `at`: `n g obj`, a value, an optional stream, `endobj`. `length` resolves an indirect
/// `/Length`. Notes collect byte-level defects and a missing end of line before `endstream`.
pub(crate) fn framed(
    buf: &[u8],
    at: usize,
    limits: &Limits,
    notes: &mut Vec<Note>,
    length: &dyn Fn(&Object) -> Option<i64>,
) -> Result<Framed, ObjectDamage> {
    let mut parser = Parser::new(buf, at, Mode::Objects, limits.nesting);
    let mut header = || parser.next().ok().flatten().map(|l| l.event);
    let (Some(syntax::Event::Integer(n)), Some(syntax::Event::Integer(g)), Some(syntax::Event::Keyword(b"obj"))) =
        (header(), header(), header())
    else {
        return Err(ObjectDamage::Header(at));
    };
    let (Ok(number), Ok(generation)) = (u32::try_from(n), u16::try_from(g)) else {
        return Err(ObjectDamage::Header(at));
    };
    let Read::Value(object) = syntax::read(&mut parser, notes).map_err(ObjectDamage::Syntax)? else {
        return Err(ObjectDamage::Header(at));
    };
    let after = syntax::read(&mut parser, notes).map_err(ObjectDamage::Syntax)?;
    let stream = match (&after, object.dict()) {
        (Read::Keyword { word, .. }, _) if word == b"endobj" => {
            return Ok(Framed {
                number,
                generation,
                object,
                stream: None,
            });
        }
        (Read::Keyword { word, end, .. }, Some(dict)) if word == b"stream" => {
            let len = dict.get(b"Length").and_then(|o| o.integer().or_else(|| length(o)));
            let len = len
                .and_then(|l| usize::try_from(l).ok())
                .ok_or(ObjectDamage::Length(*end))?;
            let raw: Raw = parser.raw(len).map_err(|_| ObjectDamage::Length(*end))?;
            if raw.bare_cr {
                return Err(ObjectDamage::BareCr(*end));
            }
            match syntax::read(&mut parser, notes) {
                Ok(Read::Keyword { word, .. }) if word == b"endstream" => {}
                _ => return Err(ObjectDamage::Length(*end)),
            }
            if raw.no_eol_after {
                notes.push(Note::NoEolBeforeEndstream(raw.end));
            }
            Some((raw.start, raw.end))
        }
        _ => return Err(ObjectDamage::NoEndobj(object.end)),
    };
    match syntax::read(&mut parser, notes) {
        Ok(Read::Keyword { word, .. }) if word == b"endobj" => Ok(Framed {
            number,
            generation,
            object,
            stream,
        }),
        _ => Err(ObjectDamage::NoEndobj(object.end)),
    }
}

/// The filters a stream dictionary names, with their parameters; `resolve` follows references among them.
pub(crate) fn chain(dict: &Dict, resolve: &dyn Fn(&Object) -> Option<Object>) -> Option<Vec<(Filter, Params)>> {
    let follow = |o: &Object| match o.value {
        Value::Reference(..) => resolve(o),
        _ => Some(o.clone()),
    };
    let listed = |key: &[u8]| -> Option<Vec<Object>> {
        match dict.get(key).map(follow) {
            None => Some(Vec::new()),
            Some(Some(Object {
                value: Value::Array(items),
                ..
            })) => items.iter().map(follow).collect(),
            Some(Some(o)) => Some(vec![o]),
            Some(None) => None,
        }
    };
    let names = listed(b"Filter")?;
    let decode_parms = listed(b"DecodeParms")?;
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let filter = Filter::from_name(name.name()?)?;
            let params = match decode_parms.get(i).map(|p| &p.value) {
                None | Some(Value::Null) => Params::default(),
                Some(Value::Dict(d)) => params(d)?,
                Some(_) => return None,
            };
            Some((filter, params))
        })
        .collect()
}

fn params(d: &Dict) -> Option<Params> {
    let get = |key: &[u8], default: i64| match d.get(key) {
        None => Some(default),
        Some(o) => o.integer(),
    };
    Some(Params {
        predictor: get(b"Predictor", 1)?,
        colors: get(b"Colors", 1)?,
        bits_per_component: get(b"BitsPerComponent", 8)?,
        columns: get(b"Columns", 1)?,
        early_change: get(b"EarlyChange", 1)?,
    })
}

/// Decode a stream's data with the filters its dictionary names. `None` when the filters cannot be named.
pub(crate) fn decode(
    buf: &[u8],
    dict: &Dict,
    (start, end): (usize, usize),
    limits: &Limits,
    resolve: &dyn Fn(&Object) -> Option<Object>,
) -> Result<Decoded, Option<filters::Damage>> {
    let chain = chain(dict, resolve).ok_or(None)?;
    filters::decode(buf.get(start..end).unwrap_or_default(), &chain, limits.decoded_bytes).map_err(Some)
}

/// The finding for a refused file.
pub(crate) fn refusal_finding(refusal: &Refusal) -> Finding {
    match refusal {
        Refusal::NoHeader => Finding::new(FILE_HEADER, "no %PDF- header in the first 1024 bytes").citing("7.5.2"),
        Refusal::NoStartxref => {
            Finding::new(FILE_STARTXREF, "no startxref and offset in the last 1024 bytes").citing("7.5.5")
        }
        Refusal::BadXref(at, problem) => {
            let what = match problem {
                XrefProblem::NotASection => "neither a cross-reference table nor a stream",
                XrefProblem::Subsection => "a subsection header that is not two integers",
                XrefProblem::Entry => "an entry that is not 20 bytes in the form of 7.5.4",
                XrefProblem::Trailer => "no trailer dictionary after the table",
                XrefProblem::Stream => "not a valid cross-reference stream",
                XrefProblem::TooMany => "more entries than the object limit",
            };
            let clause = if *problem == XrefProblem::Stream {
                "7.5.8"
            } else {
                "7.5.4"
            };
            Finding::new(
                FILE_XREF,
                format!("the cross-reference section at offset {at} has {what}"),
            )
            .citing(clause)
            .at(vec![Location::Offset(*at)])
        }
        Refusal::XrefLoop(at) => Finding::new(FILE_XREF, format!("/Prev or /XRefStm leads back to offset {at}"))
            .citing("7.5.4")
            .at(vec![Location::Offset(*at)]),
        Refusal::TooManySections => Finding::new(FILE_LIMIT, "more cross-reference sections than the limit"),
        Refusal::TooManyObjects => Finding::new(FILE_LIMIT, "more objects than the limit"),
        Refusal::Encrypted => Finding::new(FILE_ENCRYPTED, "the file is encrypted").citing("7.6"),
        Refusal::NoRoot => Finding::new(FILE_TRAILER, "the trailer has no /Root reference").citing("7.5.5"),
    }
}

/// The finding for a file-level note.
pub(crate) fn note_finding(note: &FileNote) -> Finding {
    match note {
        FileNote::HeaderOffset(at) => Finding::new(
            FILE_HEADER_OFFSET,
            format!("%PDF- starts at offset {at}, not 0: PDF/A forbids it"),
        )
        .citing("7.5.2")
        .at(vec![Location::Offset(*at)]),
        FileNote::NoEof => Finding::new(FILE_EOF, "no %%EOF after startxref").citing("7.5.5"),
        FileNote::DataAfterEof(at) => Finding::new(FILE_EOF, format!("data after %%EOF, from offset {at}"))
            .citing("7.5.5")
            .at(vec![Location::Offset(*at)]),
        FileNote::Syntax(note) => syntax::note_finding(note, Vec::new()),
    }
}

/// The finding for a damaged object.
pub(crate) fn damage_finding(num: u32, damage: &ObjectDamage) -> Finding {
    let (what, at, clause) = match damage {
        ObjectDamage::Syntax(d) => (
            format!("cannot be read: {}", syntax::describe(d.reason)),
            Some(d.at),
            "7.3",
        ),
        ObjectDamage::Header(at) => ("does not start with its own `n g obj`".to_owned(), Some(*at), "7.3.10"),
        ObjectDamage::NoEndobj(at) => ("is not followed by endobj".to_owned(), Some(*at), "7.3.10"),
        ObjectDamage::Length(at) => (
            "has a stream /Length that does not reach endstream".to_owned(),
            Some(*at),
            "7.3.8.1",
        ),
        ObjectDamage::BareCr(at) => (
            "has `stream` followed by a bare carriage return".to_owned(),
            Some(*at),
            "7.3.8.1",
        ),
        ObjectDamage::ObjectStream(s) => (format!("is in object stream {s}, which cannot be read"), None, "7.5.7"),
        ObjectDamage::Entry => (
            "has a cross-reference entry with a generation above 65535".to_owned(),
            None,
            "7.3.10",
        ),
    };
    let mut chain = vec![Location::Object(num, 0)];
    chain.extend(at.map(Location::Offset));
    Finding::new(OBJECT_DAMAGED, format!("object {num} {what}"))
        .citing(clause)
        .at(chain)
}

/// Hand-built PDFs for the tests of tiers 4 and 5.
#[cfg(test)]
pub(crate) mod test_pdf {
    use std::fmt::Write as _;

    /// A classic file: `%PDF-1.7`, each `(number, body)` as `n 0 obj body endobj`, a table, then `trailer` and `tail`.
    /// The table lists every number up to the highest; unlisted ones are free.
    pub(crate) fn classic(objects: &[(u32, &[u8])], trailer: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (num, body) in objects {
            offsets.insert(*num, out.len());
            out.extend(format!("{num} 0 obj\n").as_bytes());
            out.extend(*body);
            out.extend(b"\nendobj\n");
        }
        let size = offsets.keys().max().map_or(1, |m| m + 1);
        let xref = out.len();
        let mut table = format!("xref\n0 {size}\n");
        for num in 0..size {
            match offsets.get(&num) {
                Some(at) => write!(table, "{at:010} 00000 n\r\n"),
                None => write!(table, "0000000000 65535 f\r\n"),
            }
            .unwrap();
        }
        out.extend(table.as_bytes());
        out.extend(
            format!(
                "trailer\n{}\nstartxref\n{xref}\n%%EOF\n",
                trailer.replace("SIZE", &size.to_string())
            )
            .as_bytes(),
        );
        out
    }

    /// A file whose cross-reference is a stream (object `xref_num`, `/W [1 4 2]`, no filter) listing `objects` and,
    /// as compressed entries, `compressed`: `(number, container stream, index)`.
    pub(crate) fn with_xref_stream(
        objects: &[(u32, &[u8])],
        compressed: &[(u32, u32, u16)],
        xref_num: u32,
        extra: &str,
    ) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut rows = std::collections::BTreeMap::new();
        for (num, body) in objects {
            rows.insert(
                *num,
                [
                    vec![1],
                    u32::try_from(out.len()).unwrap().to_be_bytes().to_vec(),
                    vec![0, 0],
                ]
                .concat(),
            );
            out.extend(format!("{num} 0 obj\n").as_bytes());
            out.extend(*body);
            out.extend(b"\nendobj\n");
        }
        for (num, stream, index) in compressed {
            rows.insert(
                *num,
                [vec![2], stream.to_be_bytes().to_vec(), index.to_be_bytes().to_vec()].concat(),
            );
        }
        let xref = out.len();
        rows.insert(
            xref_num,
            [vec![1], u32::try_from(xref).unwrap().to_be_bytes().to_vec(), vec![0, 0]].concat(),
        );
        let size = rows.keys().max().unwrap() + 1;
        let mut data = Vec::new();
        for num in 0..size {
            data.extend(rows.get(&num).cloned().unwrap_or_else(|| vec![0; 7]));
        }
        out.extend(
            format!(
                "{xref_num} 0 obj\n<< /Type /XRef /W [1 4 2] /Size {size} /Length {} {extra} >>\nstream\n",
                data.len()
            )
            .as_bytes(),
        );
        out.extend(&data);
        out.extend(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    /// The body of an object stream holding `members`, each `(number, value text)`.
    pub(crate) fn object_stream(members: &[(u32, &str)]) -> Vec<u8> {
        let mut header = String::new();
        let mut body = String::new();
        for (num, text) in members {
            write!(header, "{num} {} ", body.len()).unwrap();
            body.push_str(text);
            body.push(' ');
        }
        let data = format!("{header}{body}");
        format!(
            "<< /Type /ObjStm /N {} /First {} /Length {} >>\nstream\n{data}\nendstream",
            members.len(),
            header.len(),
            data.len()
        )
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::test_pdf::{classic, with_xref_stream};
    use super::*;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    const CATALOG: &[u8] = b"<< /Type /Catalog >>";
    const TRAILER: &str = "<< /Size SIZE /Root 1 0 R >>";

    fn open_it(buf: &[u8]) -> Result<Xref, Refusal> {
        open(buf, &Limits::default())
    }

    #[test]
    fn a_classic_file() {
        let buf = classic(&[(1, CATALOG), (3, b"42")], TRAILER);
        let xref = open_it(&buf).unwrap();
        assert_eq!(
            (xref.header, xref.version.as_slice(), xref.notes.as_slice()),
            (0, &b"1.7"[..], &[][..])
        );
        assert_eq!(
            xref.entries.get(&1),
            Some(&Entry::Direct {
                offset: 9,
                generation: 0
            })
        );
        assert_eq!(xref.entries.get(&2), Some(&Entry::Free));
        assert_eq!(xref.trailer.get(b"Size").and_then(Object::integer), Some(4));
        let framed = framed(&buf, 9, &Limits::default(), &mut Vec::new(), &|_| None).unwrap();
        assert_eq!((framed.number, framed.generation, framed.stream), (1, 0, None));
    }

    #[test]
    fn header_and_tail_notes() {
        let mut buf = b"junk".to_vec();
        buf.extend(classic(&[(1, CATALOG)], TRAILER));
        buf.extend(b"\0 \r\n");
        let xref = open_it(&buf).unwrap();
        assert_eq!(
            (xref.header, xref.notes.as_slice()),
            (4, &[FileNote::HeaderOffset(4)][..])
        );
        let mut junk = classic(&[(1, CATALOG)], TRAILER);
        let len = junk.len();
        junk.extend(b"<html>");
        assert_eq!(open_it(&junk).unwrap().notes, [FileNote::DataAfterEof(len)]);
        let no_eof = String::from_utf8(classic(&[(1, CATALOG)], TRAILER))
            .unwrap()
            .replace("%%EOF\n", "");
        assert_eq!(open_it(no_eof.as_bytes()).unwrap().notes, [FileNote::NoEof]);
    }

    #[test]
    fn refusals() {
        let refused = |buf: &[u8]| open_it(buf).unwrap_err();
        assert_eq!(refused(b"hello"), Refusal::NoHeader);
        assert_eq!(refused(b"%PDF-1.7\nno tail"), Refusal::NoStartxref);
        assert_eq!(refused(b"%PDF-1.7\nstartxref\nx"), Refusal::NoStartxref);
        assert_eq!(refused(b"%PDF-1.7\nstartxref\n-4\n%%EOF"), Refusal::NoStartxref);
        assert_eq!(refused(b"%PDF-1.7\nstartxref\n"), Refusal::NoStartxref);
        assert_eq!(
            refused(b"%PDF-1.7\nstartxref\n2\n%%EOF"),
            Refusal::BadXref(2, XrefProblem::NotASection)
        );
        let buf = classic(&[(1, CATALOG)], "<< /Size SIZE /Root 1 0 R /Encrypt 5 0 R >>");
        assert_eq!(refused(&buf), Refusal::Encrypted);
        assert_eq!(refused(&classic(&[(1, CATALOG)], "<< /Size SIZE >>")), Refusal::NoRoot);
        assert_eq!(
            refused(&classic(&[(1, CATALOG)], "<< /Size SIZE /Root 1 >>")),
            Refusal::NoRoot
        );
    }

    /// The refusal for the classic file with `from` replaced by `to`, as a problem at its table's offset.
    fn broken_table(from: &str, to: &str) -> Refusal {
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        let at = text.find("xref").unwrap();
        let refusal = open_it(text.replacen(from, to, 1).as_bytes()).unwrap_err();
        assert!(
            matches!(refusal, Refusal::BadXref(found, _) if found == at),
            "{refusal:?}"
        );
        refusal
    }

    #[test]
    fn a_table_must_be_exact() {
        let at = String::from_utf8(classic(&[(1, CATALOG)], TRAILER))
            .unwrap()
            .find("xref")
            .unwrap();
        let bad = |p| Refusal::BadXref(at, p);
        assert_eq!(broken_table("0 2\n", "0 x\n"), bad(XrefProblem::Subsection));
        assert_eq!(broken_table("0 2\n", "0\n"), bad(XrefProblem::Subsection));
        assert_eq!(broken_table("0 2\n", "0 2x"), bad(XrefProblem::Subsection));
        assert_eq!(broken_table("0 2\n", "x 2\n"), bad(XrefProblem::Subsection));
        assert_eq!(broken_table("0 2\n", "0 3\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table(" 00000 n\r\n", " 00000 n\n\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table(" 00000 n\r\n", " 00000 x\r\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table(" 00000 n\r\n", "-00000 n\r\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table(" 00000 n\r\n", " 0000x n\r\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table("0 2\n", "4294967295 2\n"), bad(XrefProblem::Entry));
        assert_eq!(broken_table("trailer\n<<", "trailer\n[ <<"), bad(XrefProblem::Trailer));
        assert_eq!(
            broken_table("0 2\n", "0 99999999999999999999\n"),
            bad(XrefProblem::Subsection)
        );
        assert_eq!(broken_table("0 2\n", "0 4294967296\n"), bad(XrefProblem::Entry));
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        let cut = &text[..text.find(" n\r\n").unwrap()];
        let truncated = format!("{cut}\nstartxref\n{}\n%%EOF", text.find("xref").unwrap());
        assert!(matches!(
            open_it(truncated.as_bytes()),
            Err(Refusal::BadXref(_, XrefProblem::Entry))
        ));
        // Other end-of-line forms an entry may end with.
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        assert!(open_it(text.replace(" n\r\n", " n \n").as_bytes()).is_ok());
        assert!(open_it(text.replace(" n\r\n", " n \r").as_bytes()).is_ok());
    }

    #[test]
    fn prev_chains_and_their_limits() {
        // An update appended after the original: object 2 is new, object 1 is replaced, and the newest trailer wins.
        let base = classic(&[(1, CATALOG)], TRAILER);
        let first = String::from_utf8(base.clone()).unwrap().find("xref").unwrap();
        let mut buf = base;
        let one = buf.len();
        buf.extend(b"1 0 obj\n<< /Type /Catalog /New true >>\nendobj\n2 0 obj\n7\nendobj\n");
        let two = one + 46;
        let xref = buf.len();
        buf.extend(format!("xref\n1 2\n{one:010} 00000 n\r\n{two:010} 00000 n\r\ntrailer\n<< /Size 3 /Root 1 0 R /Prev {first} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let opened = open_it(&buf).unwrap();
        assert_eq!(
            opened.entries.get(&1),
            Some(&Entry::Direct {
                offset: one,
                generation: 0
            })
        );
        assert_eq!(opened.trailer.get(b"Size").and_then(Object::integer), Some(3));
        assert_eq!(
            open(
                &buf,
                &Limits {
                    xref_sections: 1,
                    ..Limits::default()
                }
            ),
            Err(Refusal::TooManySections)
        );
        assert_eq!(
            open(
                &buf,
                &Limits {
                    objects: 1,
                    ..Limits::default()
                }
            ),
            Err(Refusal::TooManyObjects)
        );
        let looped = String::from_utf8(buf)
            .unwrap()
            .replace(&format!("/Prev {first}"), &format!("/Prev {xref}"));
        assert_eq!(open_it(looped.as_bytes()), Err(Refusal::XrefLoop(xref)));
    }

    #[test]
    fn a_cross_reference_stream() {
        let buf = with_xref_stream(&[(1, CATALOG)], &[(2, 5, 0)], 3, "/Root 1 0 R");
        let xref = open_it(&buf).unwrap();
        assert_eq!(xref.entries.get(&2), Some(&Entry::Compressed { stream: 5, index: 0 }));
        assert_eq!(xref.entries.get(&0), Some(&Entry::Free));
        assert_eq!(xref.trailer.get(b"Type").and_then(Object::name), Some(&b"XRef"[..]));
    }

    fn problem(refusal: Refusal) -> Option<XrefProblem> {
        match refusal {
            Refusal::BadXref(_, problem) => Some(problem),
            _ => None,
        }
    }

    #[test]
    fn problem_reads_only_bad_sections() {
        assert_eq!(problem(Refusal::NoRoot), None);
    }

    /// The problem with the cross-reference stream of a small file after `change`.
    fn stream_refusal(change: impl Fn(String) -> String) -> Refusal {
        let text = change(String::from_utf8(with_xref_stream(&[(1, CATALOG)], &[], 2, "/Root 1 0 R")).unwrap());
        let at = text.find("2 0 obj").unwrap();
        let refusal = open_it(text.as_bytes()).unwrap_err();
        assert!(
            matches!(refusal, Refusal::BadXref(found, _) if found == at),
            "{refusal:?}"
        );
        Refusal::BadXref(0, problem(refusal).unwrap())
    }

    #[test]
    fn a_cross_reference_stream_must_be_valid() {
        let bad = Refusal::BadXref(0, XrefProblem::Stream);
        assert_eq!(stream_refusal(|t| t.replace("/Type /XRef", "/Type /Other")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W [1 4]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W [0 0 0]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W [9 4 2]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W 3")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/Size 3", "/Size x")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/Size 3", "")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W [1 (x) 2]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/Size 3", "/Size 3 /Index [0]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/Size 3", "/Size 3 /Index [0 9]")), bad);
        assert_eq!(
            stream_refusal(|t| t.replace("/Size 3", "/Size 3 /Index [4294967295 3]")),
            bad
        );
        assert_eq!(
            stream_refusal(|t| t.replace("/Size 3", "/Size 3 /Filter /FlateDecode")),
            bad
        );
        assert_eq!(stream_refusal(|t| t.replace(" >>\nstream", " >>\nendobj")), bad);
        assert_eq!(
            stream_refusal(|t| t
                .replace("2 0 obj\n<<", "2 0 obj\n[ <<")
                .replace(">>\nstream", ">> ]\nendobj")),
            bad
        );
    }

    #[test]
    fn stream_rows_with_unusual_types_and_widths() {
        // Type 7 reads as null; a generation too big for 16 bits and a stream number too big for 32 refuse.
        let text = String::from_utf8(with_xref_stream(&[(1, CATALOG)], &[], 2, "/Root 1 0 R")).unwrap();
        let typed = text.replacen("\0\0\0\0\0\0\0", "\x07\0\0\0\0\0\0", 1);
        assert_eq!(open_it(typed.as_bytes()).unwrap().entries.get(&0), Some(&Entry::Free));
        // /W [0 4 2]: no type field, so every row is type 1.
        let mut buf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
        let xref = buf.len();
        let rows: Vec<u8> = [9_u32, u32::try_from(xref).unwrap()]
            .iter()
            .flat_map(|o| [o.to_be_bytes().to_vec(), vec![0, 0]].concat())
            .collect();
        buf.extend(
            format!(
                "2 0 obj\n<< /Type /XRef /W [0 4 2] /Size 3 /Index [1 2] /Root 1 0 R /Length {} >>\nstream\n",
                rows.len()
            )
            .as_bytes(),
        );
        buf.extend(&rows);
        buf.extend(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        assert_eq!(
            open_it(&buf).unwrap().entries.get(&1),
            Some(&Entry::Direct {
                offset: 9,
                generation: 0
            })
        );
        let wide = |w: &str, row: Vec<u8>| {
            let mut buf = b"%PDF-1.7\n".to_vec();
            buf.extend(
                format!(
                    "1 0 obj\n<< /Type /XRef /W {w} /Size 1 /Root 1 0 R /Length {} >>\nstream\n",
                    row.len()
                )
                .as_bytes(),
            );
            buf.extend(&row);
            buf.extend(b"\nendstream\nendobj\nstartxref\n9\n%%EOF\n");
            open_it(&buf)
        };
        assert_eq!(
            wide("[1 4 3]", vec![1, 0, 0, 0, 9, 1, 0, 0]),
            Err(Refusal::BadXref(9, XrefProblem::Stream))
        );
        assert_eq!(
            wide("[1 5 2]", vec![2, 1, 0, 0, 0, 0, 0, 0]),
            Err(Refusal::BadXref(9, XrefProblem::Stream))
        );
    }

    #[test]
    fn a_hybrid_file_prefers_its_stream_over_its_table() {
        // Object 2 sits in object stream 3; the table lists it as free, the /XRefStm stream as compressed.
        let mut buf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
        let stm = buf.len();
        let rows = [
            vec![0; 7],
            vec![0; 7],
            [vec![2], 3_u32.to_be_bytes().to_vec(), vec![0, 0]].concat(),
        ]
        .concat();
        buf.extend(
            format!(
                "4 0 obj\n<< /Type /XRef /W [1 4 2] /Size 3 /Length {} >>\nstream\n",
                rows.len()
            )
            .as_bytes(),
        );
        buf.extend(&rows);
        buf.extend(b"\nendstream\nendobj\n");
        let xref = buf.len();
        buf.extend(format!("xref\n0 3\n0000000000 65535 f\r\n0000000009 00000 n\r\n0000000000 65535 f\r\ntrailer\n<< /Size 5 /Root 1 0 R /XRefStm {stm} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let opened = open_it(&buf).unwrap();
        assert_eq!(opened.entries.get(&2), Some(&Entry::Compressed { stream: 3, index: 0 }));
        let elsewhere = String::from_utf8(buf)
            .unwrap()
            .replace(&format!("/XRefStm {stm}"), "/XRefStm 3");
        assert!(matches!(open_it(elsewhere.as_bytes()), Err(Refusal::BadXref(3, _))));
    }

    fn framed_at(buf: &[u8], length: &dyn Fn(&Object) -> Option<i64>) -> Result<Framed, ObjectDamage> {
        framed(buf, 0, &Limits::default(), &mut Vec::new(), length)
    }

    #[test]
    fn objects_and_streams() {
        let read = framed_at(b"4 0 obj << /Length 3 >> stream\nabc\nendstream endobj", &|_| None).unwrap();
        assert_eq!((read.number, read.stream), (4, Some((31, 34))));
        let indirect = framed_at(b"4 0 obj << /Length 9 0 R >> stream\nabc\nendstream endobj", &|_| {
            Some(3)
        })
        .unwrap();
        assert_eq!(indirect.stream, Some((35, 38)));
        let mut notes = Vec::new();
        framed(
            b"4 0 obj << /Length 3 >> stream\nabcendstream endobj",
            0,
            &Limits::default(),
            &mut notes,
            &|_| None,
        )
        .unwrap();
        assert_eq!(notes, [Note::NoEolBeforeEndstream(34)]);
    }

    #[test]
    fn object_damage() {
        let damage = |buf: &[u8]| framed_at(buf, &|_| None).unwrap_err();
        assert_eq!(damage(b"4 0 R"), ObjectDamage::Header(0));
        assert_eq!(damage(b"4 0 obj"), ObjectDamage::Header(0));
        assert_eq!(damage(b"-4 0 obj 1 endobj"), ObjectDamage::Header(0));
        assert_eq!(damage(b"4 0 obj endobj"), ObjectDamage::Header(0));
        assert!(matches!(damage(b"4 0 obj [1 endobj"), ObjectDamage::Syntax(_)));
        assert!(matches!(damage(b"4 0 obj 1 ]"), ObjectDamage::Syntax(_)));
        assert_eq!(damage(b"4 0 obj 1 2"), ObjectDamage::NoEndobj(9));
        assert_eq!(damage(b"4 0 obj 1 stream"), ObjectDamage::NoEndobj(9));
        assert_eq!(
            damage(b"4 0 obj << /Length 1 >> stream\nx\nendstream"),
            ObjectDamage::NoEndobj(23)
        );
        assert_eq!(
            damage(b"4 0 obj << /Length 1 >> stream\nx\nendstream junk"),
            ObjectDamage::NoEndobj(23)
        );
        assert_eq!(damage(b"4 0 obj << >> stream\nx"), ObjectDamage::Length(20));
        assert_eq!(damage(b"4 0 obj << /Length -1 >> stream\nx"), ObjectDamage::Length(31));
        assert_eq!(damage(b"4 0 obj << /Length 9 >> stream\nx"), ObjectDamage::Length(30));
        assert_eq!(
            damage(b"4 0 obj << /Length 1 >> stream\nxx endstream endobj"),
            ObjectDamage::Length(30)
        );
        assert_eq!(
            damage(b"4 0 obj << /Length 1 >> stream\rx\nendstream endobj"),
            ObjectDamage::BareCr(30)
        );
    }

    fn dict_of(read: Read) -> Dict {
        match read {
            Read::Value(Object {
                value: Value::Dict(d), ..
            }) => d,
            _ => Dict::default(),
        }
    }

    #[test]
    fn filter_chains() {
        let dict = |text: &[u8]| {
            let read = syntax::read(&mut Parser::new(text, 0, Mode::Objects, 8), &mut Vec::new());
            dict_of(read.unwrap())
        };
        let none = |_: &Object| None;
        assert_eq!(chain(&dict(b"<< >>"), &none), Some(vec![]));
        let flate_png = Params {
            predictor: 12,
            columns: 4,
            ..Params::default()
        };
        assert_eq!(
            chain(
                &dict(b"<< /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 >> >>"),
                &none
            ),
            Some(vec![(Filter::Flate, flate_png)])
        );
        assert_eq!(
            chain(&dict(b"<< /Filter [/AHx /Fl] /DecodeParms [null << >>] >>"), &none),
            Some(vec![
                (Filter::AsciiHex, Params::default()),
                (Filter::Flate, Params::default())
            ])
        );
        let target = Object {
            value: Value::Name(b"FlateDecode".to_vec()),
            start: 0,
            end: 0,
        };
        assert_eq!(
            chain(&dict(b"<< /Filter 7 0 R >>"), &|_| Some(target.clone())),
            Some(vec![(Filter::Flate, Params::default())])
        );
        assert_eq!(chain(&dict(b"<< /Filter 7 0 R >>"), &none), None);
        assert_eq!(chain(&dict(b"<< /Filter /Nope >>"), &none), None);
        assert_eq!(chain(&dict(b"<< /Filter 5 >>"), &none), None);
        assert_eq!(chain(&dict(b"<< /Filter /Fl /DecodeParms 5 >>"), &none), None);
        assert_eq!(
            chain(&dict(b"<< /Filter /Fl /DecodeParms << /Predictor /x >> >>"), &none),
            None
        );
        let zipped = compress_to_vec_zlib(b"data", 6);
        let d = dict(b"<< /Filter /FlateDecode >>");
        assert_eq!(
            decode(&zipped, &d, (0, zipped.len()), &Limits::default(), &none),
            Ok(Decoded::Plain(b"data".to_vec()))
        );
        assert_eq!(
            decode(b"x", &dict(b"<< /Filter /Nope >>"), (0, 1), &Limits::default(), &none),
            Err(None)
        );
        assert_eq!(
            decode(b"x", &d, (0, 1), &Limits::default(), &none),
            Err(Some(filters::Damage::Corrupt(Filter::Flate)))
        );
    }

    #[test]
    fn a_stream_listing_too_many_objects_is_refused_before_it_is_decoded() {
        let buf = with_xref_stream(&[(1, b"<< >>")], &[], 2, "/Root 1 0 R");
        let few = Limits {
            objects: 2,
            ..Limits::default()
        };
        assert_eq!(open(&buf, &few), Err(Refusal::TooManyObjects));
        assert!(open(&buf, &Limits::default()).is_ok());
    }

    #[test]
    fn every_refusal_note_and_damage_has_a_finding() {
        let refusals = [
            Refusal::NoHeader,
            Refusal::NoStartxref,
            Refusal::BadXref(5, XrefProblem::NotASection),
            Refusal::BadXref(5, XrefProblem::Subsection),
            Refusal::BadXref(5, XrefProblem::Entry),
            Refusal::BadXref(5, XrefProblem::Trailer),
            Refusal::BadXref(5, XrefProblem::Stream),
            Refusal::BadXref(5, XrefProblem::TooMany),
            Refusal::XrefLoop(5),
            Refusal::TooManySections,
            Refusal::TooManyObjects,
            Refusal::Encrypted,
            Refusal::NoRoot,
        ];
        let ids: Vec<&str> = refusals.iter().map(|r| refusal_finding(r).check.id).collect();
        assert_eq!(
            ids,
            [
                "file.header",
                "file.startxref",
                "file.xref",
                "file.xref",
                "file.xref",
                "file.xref",
                "file.xref",
                "file.xref",
                "file.xref",
                "file.limit",
                "file.limit",
                "file.encrypted",
                "file.trailer"
            ]
        );
        assert_eq!(
            refusal_finding(&Refusal::BadXref(5, XrefProblem::Stream))
                .clause
                .map(|c| c.number),
            Some("7.5.8")
        );
        let notes = [
            FileNote::HeaderOffset(3),
            FileNote::NoEof,
            FileNote::DataAfterEof(9),
            FileNote::Syntax(Note::DuplicateKey(4)),
        ];
        let ids: Vec<&str> = notes.iter().map(|n| note_finding(n).check.id).collect();
        assert_eq!(
            ids,
            ["file.header-offset", "file.eof", "file.eof", "syntax.duplicate-key"]
        );
        let damages = [
            ObjectDamage::Syntax(syntax::Damage {
                at: 1,
                reason: syntax::Reason::Brace,
            }),
            ObjectDamage::Header(1),
            ObjectDamage::NoEndobj(1),
            ObjectDamage::Length(1),
            ObjectDamage::BareCr(1),
            ObjectDamage::ObjectStream(9),
            ObjectDamage::Entry,
        ];
        for d in &damages {
            let f = damage_finding(7, d);
            assert_eq!(
                (f.check.id, f.chain.first()),
                ("object.damaged", Some(&Location::Object(7, 0)))
            );
        }
    }

    #[test]
    fn filter_parameters_must_be_integers() {
        let dict = |text: &str| {
            dict_of(syntax::read(&mut Parser::new(text.as_bytes(), 0, Mode::Objects, 8), &mut Vec::new()).unwrap())
        };
        for key in ["Predictor", "Colors", "BitsPerComponent", "Columns", "EarlyChange"] {
            let text = format!("<< /Filter /Fl /DecodeParms << /{key} /x >> >>");
            assert_eq!(chain(&dict(&text), &|_| None), None, "{key}");
        }
        assert_eq!(chain(&dict("<< /Filter /Fl /DecodeParms 7 0 R >>"), &|_| None), None);
        assert_eq!(dict_of(Read::End), Dict::default());
    }

    #[test]
    fn more_broken_sections() {
        let at = String::from_utf8(classic(&[(1, CATALOG)], TRAILER))
            .unwrap()
            .find("xref")
            .unwrap();
        assert_eq!(
            broken_table("0 2\n", "0 99999999999999999999\n"),
            Refusal::BadXref(at, XrefProblem::Subsection)
        );
        assert_eq!(
            broken_table("0 2\n", "4294967296 2\n"),
            Refusal::BadXref(at, XrefProblem::Subsection)
        );
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        let cut = &text[..text.find(" n\r\n").unwrap()];
        let truncated = format!("{cut}\nstartxref\n{at}\n%%EOF");
        assert_eq!(
            open_it(truncated.as_bytes()),
            Err(Refusal::BadXref(at, XrefProblem::Entry))
        );
        let bad = Refusal::BadXref(0, XrefProblem::Stream);
        assert_eq!(stream_refusal(|t| t.replace("/Size 3", "")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "/W [1 (x) 2]")), bad);
        assert_eq!(stream_refusal(|t| t.replace("/W [1 4 2]", "")), bad);
    }

    #[test]
    fn a_table_longer_than_the_file() {
        let at = String::from_utf8(classic(&[(1, CATALOG)], TRAILER))
            .unwrap()
            .find("xref")
            .unwrap();
        assert_eq!(
            broken_table("0 2\n", "0 999\n"),
            Refusal::BadXref(at, XrefProblem::Entry)
        );
    }

    #[test]
    fn a_table_cut_off_by_the_end_of_the_file() {
        // startxref may sit anywhere in the last 1024 bytes; here it precedes a table that runs off the end.
        let buf = b"%PDF-1.7\nstartxref\n28\n%%EOF\nxref\n0 3\n0000000000 65535 f\r\n";
        assert_eq!(open_it(buf), Err(Refusal::BadXref(28, XrefProblem::Entry)));
    }

    #[test]
    fn a_free_entry_may_have_generation_65536() {
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        let opened = open_it(text.replacen("0000000000 65535 f", "0000000000 65536 f", 1).as_bytes()).unwrap();
        assert_eq!(opened.entries.get(&0), Some(&Entry::Free));
    }

    #[test]
    fn one_xrefstm_named_by_two_trailers_is_read_once() {
        // Two tables, the newer with /Prev to the older, both naming the same /XRefStm, as Microsoft Word writes them.
        let mut buf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
        let stm = buf.len();
        let rows = [
            vec![0; 7],
            vec![0; 7],
            [vec![2], 3_u32.to_be_bytes().to_vec(), vec![0, 0]].concat(),
        ]
        .concat();
        buf.extend(
            format!(
                "4 0 obj\n<< /Type /XRef /W [1 4 2] /Size 3 /Length {} >>\nstream\n",
                rows.len()
            )
            .as_bytes(),
        );
        buf.extend(&rows);
        buf.extend(b"\nendstream\nendobj\n");
        let older = buf.len();
        buf.extend(format!("xref\n0 2\n0000000000 65535 f\r\n0000000009 00000 n\r\ntrailer\n<< /Size 5 /Root 1 0 R /XRefStm {stm} >>\n").as_bytes());
        let newer = buf.len();
        buf.extend(format!("xref\n0 1\n0000000000 65535 f\r\ntrailer\n<< /Size 5 /Root 1 0 R /XRefStm {stm} /Prev {older} >>\nstartxref\n{newer}\n%%EOF\n").as_bytes());
        let opened = open_it(&buf).unwrap();
        assert_eq!(opened.entries.get(&2), Some(&Entry::Compressed { stream: 3, index: 0 }));
    }

    #[test]
    fn an_in_use_entry_with_a_generation_too_big_is_invalid_not_a_refusal() {
        let text = String::from_utf8(classic(&[(1, CATALOG)], TRAILER)).unwrap();
        let opened = open_it(text.replacen("0000000009 00000 n", "0000000009 65536 n", 1).as_bytes()).unwrap();
        assert_eq!(opened.entries.get(&1), Some(&Entry::Invalid));
    }
}
